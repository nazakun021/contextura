// src-tauri/src/sidecar.rs

use anyhow::Context;
use reqwest::Client;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

const STARTUP_READY_TIMEOUT: Duration = Duration::from_secs(180);
const RETRY_READY_TIMEOUT: Duration = Duration::from_secs(30);
const RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(15);
const QUICK_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub enum SidecarChild {
    Tauri(tauri_plugin_shell::process::CommandChild),
    Headless(std::process::Child),
}

impl SidecarChild {
    pub fn kill(self) -> std::io::Result<()> {
        match self {
            Self::Tauri(child) => child
                .kill()
                .map_err(|e| std::io::Error::other(e.to_string())),
            Self::Headless(mut child) => child.kill(),
        }
    }
}

pub struct SidecarManager {
    pub(crate) port: u16,
    pub(crate) sidecar_child: Option<SidecarChild>,
    pub(crate) client: Client,
}

impl SidecarManager {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            sidecar_child: None,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .build()
                .expect("sidecar manager HTTP client should build"),
        }
    }

    pub fn build_launch_args(&self, model_path: &Path, strategy: &str) -> Vec<String> {
        let mut args = vec![
            "--model".to_string(),
            model_path.to_string_lossy().into_owned(),
            "--port".to_string(),
            self.port.to_string(),
            "--n-gpu-layers".to_string(),
            "99".to_string(),
            "-c".to_string(),
            "8192".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
        ];

        let strategy_lower = strategy.to_ascii_lowercase();
        if strategy_lower.contains("gemma") {
            args.push("--no-jinja".to_string());
            args.push("--parallel".to_string());
            args.push("4".to_string());
        } else if strategy_lower.contains("lfm") {
            args.push("--jinja".to_string());
            args.push("--parallel".to_string());
            args.push("4".to_string());
        } else {
            args.push("--jinja".to_string());
        }

        args
    }

    pub fn start<R: tauri::Runtime>(
        &mut self,
        app: &tauri::AppHandle<R>,
        model_path: &Path,
        model_id: &str,
        strategy: Option<&str>,
    ) -> anyhow::Result<()> {
        use tauri::Manager;
        use tauri_plugin_shell::ShellExt;

        if let Some(child) = self.sidecar_child.take() {
            let _ = child.kill();
        }

        let strategy_name = strategy.unwrap_or_else(|| {
            crate::translation::TranslationClient::select_strategy_for_model(model_id)
        });

        let resource_dir = app
            .path()
            .resource_dir()
            .context("resource_dir unavailable")?;
        let binaries_dir = resource_dir.join("binaries");
        let binaries_dir_str = binaries_dir
            .to_str()
            .ok_or_else(|| {
                anyhow::anyhow!("binaries dir path is not UTF-8: {}", binaries_dir.display())
            })?
            .to_string();

        let launch_args = self.build_launch_args(model_path, strategy_name);

        let command = app
            .shell()
            .sidecar("llama-server")?
            .env("DYLD_FALLBACK_LIBRARY_PATH", binaries_dir_str)
            .args(&launch_args);

        let (mut rx, child) = command.spawn()?;

        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    tauri_plugin_shell::process::CommandEvent::Stdout(line) => {
                        log::info!("SIDECAR OUT: {}", String::from_utf8_lossy(&line));
                    }
                    tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                        log::info!("SIDECAR ERR: {}", String::from_utf8_lossy(&line));
                    }
                    tauri_plugin_shell::process::CommandEvent::Error(err) => {
                        log::error!("SIDECAR ERROR EVENT: {err}");
                    }
                    tauri_plugin_shell::process::CommandEvent::Terminated(payload) => {
                        log::info!("SIDECAR TERMINATED: {:?}", payload.code);
                    }
                    _ => {}
                }
            }
        });

        self.sidecar_child = Some(SidecarChild::Tauri(child));
        log::info!("Sidecar started via Tauri");
        Ok(())
    }

    pub fn start_headless(
        &mut self,
        model_path: &Path,
        model_id: &str,
        strategy: Option<&str>,
    ) -> anyhow::Result<()> {
        use std::process::{Command, Stdio};

        if let Some(child) = self.sidecar_child.take() {
            let _ = child.kill();
        }

        let strategy_name = strategy.unwrap_or_else(|| {
            if model_id.to_ascii_lowercase().contains("translategemma") {
                "gemma"
            } else {
                "qwen"
            }
        });

        let path_resolver = crate::path_resolver::PathResolver::new(true, None);
        let llama_path = path_resolver.resolve_binary("llama-server", None)?;
        // CARGO_MANIFEST_DIR is reliable since the CLI runs in dev/test-suite environments
        let binaries_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");

        let launch_args = self.build_launch_args(model_path, strategy_name);

        let child = Command::new(&llama_path)
            .env("DYLD_FALLBACK_LIBRARY_PATH", binaries_dir)
            .args(&launch_args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        self.sidecar_child = Some(SidecarChild::Headless(child));
        log::info!("Sidecar started headlessly");
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(child) = self.sidecar_child.take() {
            let _ = child.kill();
        }
    }

    async fn wait_for_ready_with_timeout(&self, timeout: Duration) -> anyhow::Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let started_at = tokio::time::Instant::now();
        let mut last_error = None::<String>;

        loop {
            if started_at.elapsed() >= timeout {
                let detail =
                    last_error.unwrap_or_else(|| "unknown health check failure".to_string());
                return Err(anyhow::anyhow!(
                    "Llama-server health check timed out after {}s: {detail}",
                    timeout.as_secs()
                ));
            }

            match self.client.get(&url).send().await {
                Ok(res) => {
                    let status = res.status();
                    let body = res.text().await.unwrap_or_default();

                    if status.is_success() {
                        match serde_json::from_str::<Value>(&body) {
                            Ok(json)
                                if json.get("status").and_then(|value| value.as_str())
                                    == Some("ok") =>
                            {
                                return Ok(());
                            }
                            Ok(json) => {
                                last_error = Some(format!(
                                    "health endpoint returned unexpected payload: {json}"
                                ));
                            }
                            Err(error) => {
                                last_error = Some(format!(
                                    "health endpoint returned invalid JSON: {error}; body: {}",
                                    body.trim()
                                ));
                            }
                        }
                    } else {
                        last_error = Some(format!(
                            "health endpoint returned HTTP {status}: {}",
                            body.trim()
                        ));
                    }
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                }
            }

            sleep(HEALTH_POLL_INTERVAL).await;
        }
    }

    pub async fn wait_for_ready(&self) -> anyhow::Result<()> {
        self.wait_for_ready_with_timeout(STARTUP_READY_TIMEOUT)
            .await
    }

    pub async fn wait_for_ready_retry(&self) -> anyhow::Result<()> {
        self.wait_for_ready_with_timeout(RETRY_READY_TIMEOUT).await
    }

    pub async fn wait_for_runtime_ready(&self) -> anyhow::Result<()> {
        self.wait_for_ready_with_timeout(RUNTIME_READY_TIMEOUT)
            .await
    }

    pub async fn quick_health_check(&self) -> anyhow::Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let response = tokio::time::timeout(QUICK_HEALTH_TIMEOUT, self.client.get(&url).send())
            .await
            .map_err(|_| anyhow::anyhow!("health check timed out after 2s"))??;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("health endpoint returned HTTP {status}: {}", body.trim());
        }

        let json: Value = serde_json::from_str(&body).map_err(|error| {
            anyhow::anyhow!(
                "health endpoint returned invalid JSON: {error}; body: {}",
                body.trim()
            )
        })?;

        if json.get("status").and_then(Value::as_str) == Some("ok") {
            Ok(())
        } else {
            anyhow::bail!("health endpoint returned unexpected payload: {json}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qwen_launch_arguments() {
        let manager = SidecarManager::new(8765);
        let model_path = Path::new("/path/to/qwen-model.gguf");
        let args = manager.build_launch_args(model_path, "qwen");

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"/path/to/qwen-model.gguf".to_string()));
        assert!(args.contains(&"--port".to_string()));
        assert!(args.contains(&"8765".to_string()));
        assert!(args.contains(&"--jinja".to_string()));
        assert!(!args.contains(&"--no-jinja".to_string()));
    }

    #[test]
    fn test_gemma_launch_arguments() {
        let manager = SidecarManager::new(9999);
        let model_path = Path::new("/path/to/gemma-model.gguf");
        let args = manager.build_launch_args(model_path, "gemma");

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"/path/to/gemma-model.gguf".to_string()));
        assert!(args.contains(&"--port".to_string()));
        assert!(args.contains(&"9999".to_string()));
        assert!(args.contains(&"--no-jinja".to_string()));
        assert!(!args.contains(&"--jinja".to_string()));
        assert!(args.contains(&"--parallel".to_string()));
        assert!(args.contains(&"4".to_string()));
    }

    #[test]
    fn test_lfm_launch_arguments() {
        let manager = SidecarManager::new(1234);
        let model_path = Path::new("/path/to/lfm-model.gguf");
        let args = manager.build_launch_args(model_path, "lfm");

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"/path/to/lfm-model.gguf".to_string()));
        assert!(args.contains(&"--port".to_string()));
        assert!(args.contains(&"1234".to_string()));
        assert!(args.contains(&"--jinja".to_string()));
        assert!(!args.contains(&"--no-jinja".to_string()));
        assert!(args.contains(&"--parallel".to_string()));
        assert!(args.contains(&"4".to_string()));
    }
}
