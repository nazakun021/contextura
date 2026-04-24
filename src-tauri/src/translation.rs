use reqwest::Client;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fmt::Write;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug)]
pub struct TranslationMemory {
    entries: VecDeque<(String, String)>,
    max_size: usize,
}

impl TranslationMemory {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    pub fn push(&mut self, original: String, translated: String) {
        if self.max_size == 0 {
            return;
        }

        if self.entries.len() == self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back((original, translated));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn as_context_slice(&mut self) -> &[(String, String)] {
        self.entries.make_contiguous()
    }
}

pub struct TranslationClient {
    pub memory: TranslationMemory,
    port: u16,
    client: Client,
    sidecar_child: Option<tauri_plugin_shell::process::CommandChild>,
}

impl TranslationClient {
    pub fn new(max_memory_size: usize, port: u16) -> Self {
        Self {
            memory: TranslationMemory::new(max_memory_size),
            port,
            client: Client::new(),
            sidecar_child: None,
        }
    }

    pub fn start_sidecar(
        &mut self,
        app: &tauri::AppHandle,
        model_path: &std::path::Path,
    ) -> anyhow::Result<()> {
        use tauri::Manager;
        use tauri_plugin_shell::ShellExt;

        if let Some(child) = self.sidecar_child.take() {
            let _ = child.kill();
        }

        let binaries_dir = app.path().resource_dir().unwrap().join("binaries");

        let (mut rx, child) = app
            .shell()
            .sidecar("llama-server")?
            .env("DYLD_FALLBACK_LIBRARY_PATH", binaries_dir.to_str().unwrap())
            .args([
                "--model",
                model_path.to_str().unwrap(),
                "--port",
                &self.port.to_string(),
                "--n-gpu-layers",
                "99", // full Metal offload
                "--ctx-size",
                "1024",
                "--host",
                "127.0.0.1",
                // "--log-disable", // quiet; Rust handles logging
                "--jinja",       // required for Qwen3 chat template (Jinja2 format)
            ])
            .spawn()?;

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

        self.sidecar_child = Some(child);

        log::info!("Sidecar started");
        Ok(())
    }

    pub async fn wait_for_ready(&self) -> anyhow::Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let mut attempts = 0;
        loop {
            if let Ok(res) = self.client.get(&url).send().await
                && let Ok(json) = res.json::<Value>().await
                && json.get("status").and_then(|s| s.as_str()) == Some("ok")
            {
                return Ok(());
            }
            attempts += 1;
            if attempts > 30 {
                return Err(anyhow::anyhow!("Llama-server health check timed out"));
            }
            sleep(Duration::from_millis(500)).await;
        }
    }

    pub async fn translate_batch(&mut self, strings: &[String]) -> anyhow::Result<Vec<String>> {
        if strings.is_empty() {
            return Ok(vec![]);
        }

        let mut final_results = vec![String::new(); strings.len()];

        // Sub-batch at 15 strings to avoid hitting token limits or context window issues
        for (chunk_idx, chunk_strings) in strings.chunks(15).enumerate() {
            let offset = chunk_idx * 15;

            let mut prompt = String::new();
            let context = self.memory.as_context_slice();
            if !context.is_empty() {
                prompt.push_str("Previous context (do not retranslate, for reference only):\n");
                for (ja, en) in context {
                    let _ = writeln!(prompt, "- {ja} -> \"{en}\"");
                }
                prompt.push('\n');
            }

            prompt.push_str("Translate each numbered Japanese string to English.\n");
            prompt.push_str("Output only translations, one per line, same numbered format.\n\n");

            for (i, s) in chunk_strings.iter().enumerate() {
                let _ = writeln!(prompt, "{}: {}", i + 1, s);
            }

            let payload = json!({
                "model": "local",
                "messages": [
                    // /no_think disables Qwen3 thinking mode — without it, the model outputs
                    // <think>...</think> tokens before the response, breaking our ^(\d+): parser.
                    { "role": "system", "content": "You are a Japanese-to-English translator. /no_think" },
                    { "role": "user", "content": prompt }
                ],
                "temperature": 0.1,
                "max_tokens": 512
            });

            let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
            let res: Value = self
                .client
                .post(&url)
                .json(&payload)
                .send()
                .await?
                .json()
                .await?;

            let response_content = res["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("");

            for line in response_content.lines() {
                if let Some((num, text)) = line.split_once(':')
                    && let Ok(idx) = num.trim().parse::<usize>()
                    && idx > 0
                    && idx <= chunk_strings.len()
                {
                    final_results[offset + idx - 1] = text.trim().to_string();
                }
            }
        }

        for (i, s) in strings.iter().enumerate() {
            if !final_results[i].is_empty() {
                self.memory.push(s.clone(), final_results[i].clone());
            }
        }

        Ok(final_results)
    }
}
