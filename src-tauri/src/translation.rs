// src-tauri/src/translation.rs

use reqwest::Client;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fmt::Write;
use std::time::Duration;
use tokio::time::sleep;

const STARTUP_READY_TIMEOUT: Duration = Duration::from_secs(180);
const RUNTIME_READY_TIMEOUT: Duration = Duration::from_secs(15);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TRANSLATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const TRANSLATEGEMMA_HISTORY_LIMIT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslationMode {
    NumberedBatchQwen,
    StructuredTranslateGemma,
}

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
    mode: TranslationMode,
}

impl TranslationClient {
    pub fn new(max_memory_size: usize, port: u16) -> Self {
        Self {
            memory: TranslationMemory::new(max_memory_size),
            port,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(TRANSLATION_REQUEST_TIMEOUT)
                .build()
                .expect("translation HTTP client should build"),
            sidecar_child: None,
            mode: TranslationMode::NumberedBatchQwen,
        }
    }

    fn mode_for_model(model_id: &str) -> TranslationMode {
        if model_id.to_ascii_lowercase().contains("translategemma") {
            TranslationMode::StructuredTranslateGemma
        } else {
            TranslationMode::NumberedBatchQwen
        }
    }

    pub fn start_sidecar_mode_for_cli(&mut self, model_id: &str) {
        self.mode = Self::mode_for_model(model_id);
    }

    fn response_text(content: &Value) -> String {
        if let Some(text) = content.as_str() {
            return text.trim().to_string();
        }

        content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string()
    }

    fn build_translategemma_conversation(
        history: &[(String, String)],
        input_text: &str,
    ) -> Vec<Value> {
        let mut conversation = Vec::with_capacity(history.len() * 2 + 1);
        for (source_text, translated_text) in history {
            conversation.push(json!({
                "role": "user",
                "content": [{
                    "type": "text",
                    "source_lang_code": "ja",
                    "target_lang_code": "en",
                    "text": source_text
                }]
            }));
            conversation.push(json!({
                "role": "assistant",
                "content": translated_text
            }));
        }

        conversation.push(json!({
            "role": "user",
            "content": [{
                "type": "text",
                "source_lang_code": "ja",
                "target_lang_code": "en",
                "text": input_text
            }]
        }));

        conversation
    }

    async fn post_chat_completion(&self, payload: &Value) -> anyhow::Result<Value> {
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        let mut last_error = String::from("unknown error");

        for attempt in 1..=2 {
            let response = self.client.post(&url).json(payload).send().await;
            match response {
                Ok(response) => {
                    let status = response.status();
                    let body = response.text().await?;
                    if !status.is_success() {
                        last_error =
                            format!("llama-server returned HTTP {status}: {}", body.trim());
                    } else {
                        return serde_json::from_str(&body).map_err(|error| {
                            anyhow::anyhow!(
                                "llama-server returned invalid JSON: {error}; body: {}",
                                body.trim()
                            )
                        });
                    }
                }
                Err(error) => {
                    last_error = error.to_string();
                }
            }

            if attempt < 2 {
                sleep(Duration::from_millis(250)).await;
            }
        }

        Err(anyhow::anyhow!(
            "translation request failed after retry: {last_error}"
        ))
    }

    fn response_text_from_completion(res: &Value) -> anyhow::Result<String> {
        let content = &res["choices"][0]["message"]["content"];
        let text = Self::response_text(content);
        if text.is_empty() {
            anyhow::bail!("llama-server returned an empty translation");
        }
        Ok(text)
    }

    fn translategemma_seed_history(&mut self, chunk_strings: &[String]) -> Vec<(String, String)> {
        let history = self.memory.as_context_slice();
        let keep = history
            .len()
            .min(TRANSLATEGEMMA_HISTORY_LIMIT)
            .min(chunk_strings.len());
        history[history.len().saturating_sub(keep)..].to_vec()
    }

    pub fn start_sidecar(
        &mut self,
        app: &tauri::AppHandle,
        model_path: &std::path::Path,
        model_id: &str,
    ) -> anyhow::Result<()> {
        use tauri::Manager;
        use tauri_plugin_shell::ShellExt;

        if let Some(child) = self.sidecar_child.take() {
            let _ = child.kill();
        }

        self.mode = Self::mode_for_model(model_id);

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
                "--jinja", // required for Qwen3 chat template (Jinja2 format)
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

    pub async fn wait_for_runtime_ready(&self) -> anyhow::Result<()> {
        self.wait_for_ready_with_timeout(RUNTIME_READY_TIMEOUT)
            .await
    }

    pub async fn translate_batch(&mut self, strings: &[String]) -> anyhow::Result<Vec<String>> {
        if strings.is_empty() {
            return Ok(vec![]);
        }

        let mut final_results = vec![String::new(); strings.len()];

        // Sub-batch at 15 strings to avoid hitting token limits or context window issues
        for (chunk_idx, chunk_strings) in strings.chunks(15).enumerate() {
            let offset = chunk_idx * 15;
            match self.mode {
                TranslationMode::NumberedBatchQwen => {
                    let mut prompt = String::new();
                    let context = self.memory.as_context_slice();
                    if !context.is_empty() {
                        prompt.push_str(
                            "Previous context (do not retranslate, for reference only):\n",
                        );
                        for (ja, en) in context {
                            let _ = writeln!(prompt, "- {ja} -> \"{en}\"");
                        }
                        prompt.push('\n');
                    }

                    prompt.push_str("Translate each numbered Japanese string to English.\n");
                    prompt.push_str(
                        "Output only translations, one per line, same numbered format.\n\n",
                    );

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

                    let res = self.post_chat_completion(&payload).await?;
                    let response_content = Self::response_text_from_completion(&res)?;

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
                TranslationMode::StructuredTranslateGemma => {
                    let mut conversation_history = self.translategemma_seed_history(chunk_strings);

                    for (chunk_offset, input_text) in chunk_strings.iter().enumerate() {
                        let payload = json!({
                            "model": "local",
                            "messages": Self::build_translategemma_conversation(
                                &conversation_history,
                                input_text,
                            ),
                            "temperature": 0.1,
                            "max_tokens": 256
                        });

                        let res = self.post_chat_completion(&payload).await?;
                        let translation = Self::response_text_from_completion(&res)?;
                        final_results[offset + chunk_offset] = translation.clone();
                        conversation_history.push((input_text.clone(), translation));
                        if conversation_history.len() > TRANSLATEGEMMA_HISTORY_LIMIT {
                            conversation_history.remove(0);
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::TranslationClient;
    use serde_json::json;

    #[test]
    fn response_text_should_accept_plain_string_content() {
        assert_eq!(TranslationClient::response_text(&json!("Hello")), "Hello");
    }

    #[test]
    fn response_text_should_join_structured_text_segments() {
        let content = json!([
            { "type": "text", "text": "Hello" },
            { "type": "text", "text": " world" }
        ]);
        assert_eq!(TranslationClient::response_text(&content), "Hello world");
    }

    #[test]
    fn translategemma_conversation_matches_structured_user_only_format() {
        let conversation = TranslationClient::build_translategemma_conversation(&[], "はじめに");

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0]["role"], "user");
        assert_eq!(conversation[0]["content"][0]["type"], "text");
        assert_eq!(conversation[0]["content"][0]["source_lang_code"], "ja");
        assert_eq!(conversation[0]["content"][0]["target_lang_code"], "en");
        assert_eq!(conversation[0]["content"][0]["text"], "はじめに");
    }

    #[test]
    fn translategemma_conversation_preserves_prior_pairs() {
        let conversation = TranslationClient::build_translategemma_conversation(
            &[("猫".to_string(), "cat".to_string())],
            "犬",
        );

        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation[0]["content"][0]["text"], "猫");
        assert_eq!(conversation[1]["role"], "assistant");
        assert_eq!(conversation[1]["content"], "cat");
        assert_eq!(conversation[2]["content"][0]["text"], "犬");
    }
}
