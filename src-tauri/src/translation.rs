// src-tauri/src/translation.rs

use futures::future::join_all;
use reqwest::Client;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fmt::Write;
use std::time::Duration;
use tokio::time::sleep;

const TRANSLATION_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const TRANSLATEGEMMA_HISTORY_LIMIT: usize = 6;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationStrategy {
    Qwen,
    Gemma,
}

impl TranslationStrategy {
    pub async fn translate_chunk(
        self,
        client: &TranslationClient,
        history: &[(String, String)],
        chunk_strings: &[String],
    ) -> anyhow::Result<Vec<String>> {
        match self {
            Self::Qwen => {
                self.translate_qwen_chunk(client, history, chunk_strings)
                    .await
            }
            Self::Gemma => {
                self.translate_gemma_chunk(client, history, chunk_strings)
                    .await
            }
        }
    }

    fn build_qwen_batch_prompt(history: &[(String, String)], chunk_strings: &[String]) -> String {
        let mut prompt = String::new();
        if !history.is_empty() {
            prompt.push_str("Previous context (do not retranslate, for reference only):\n");
            for (ja, en) in history {
                let _ = writeln!(prompt, "- {ja} -> \"{en}\"");
            }
            prompt.push('\n');
        }

        prompt.push_str("Translate each numbered Japanese string to English.\n");
        prompt.push_str("Output only translations, one per line, same numbered format.\n\n");

        for (i, s) in chunk_strings.iter().enumerate() {
            let _ = writeln!(prompt, "{}: {}", i + 1, s);
        }

        prompt
    }

    fn build_qwen_batch_payload(history: &[(String, String)], chunk_strings: &[String]) -> Value {
        let prompt = Self::build_qwen_batch_prompt(history, chunk_strings);
        json!({
            "model": "local",
            "messages": [
                { "role": "system", "content": "You are a Japanese-to-English translator. /no_think" },
                { "role": "user", "content": prompt }
            ],
            "temperature": 0.1,
            "max_tokens": (chunk_strings.len() * 80).max(512)
        })
    }

    fn build_qwen_single_payload(input_text: &str) -> Value {
        json!({
            "model": "local",
            "messages": [
                { "role": "system", "content": "You are a Japanese-to-English translator. /no_think Output only the English translation, nothing else." },
                { "role": "user", "content": input_text }
            ],
            "temperature": 0.1,
            "max_tokens": 256
        })
    }

    async fn translate_single_qwen(
        self,
        client: &TranslationClient,
        input_text: &str,
    ) -> anyhow::Result<String> {
        let payload = Self::build_qwen_single_payload(input_text);
        let res = client.post_chat_completion(payload).await?;
        TranslationClient::response_text_from_completion(&res)
    }

    async fn translate_qwen_chunk(
        self,
        client: &TranslationClient,
        history: &[(String, String)],
        chunk_strings: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let chunk_translations = match self
            .do_translate_qwen_chunk(client, history, chunk_strings)
            .await
        {
            Ok(results) => results,
            Err(first_error) => {
                log::warn!("[Translation] Qwen batch failed, retrying once: {first_error}");
                sleep(Duration::from_millis(500)).await;
                self.do_translate_qwen_chunk(client, history, chunk_strings).await.map_err(|retry_error| {
                    anyhow::anyhow!(
                        "qwen batch failed after retry: first={first_error}; retry={retry_error}"
                    )
                })?
            }
        };
        Ok(chunk_translations)
    }

    async fn do_translate_qwen_chunk(
        self,
        client: &TranslationClient,
        history: &[(String, String)],
        chunk_strings: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let payload = Self::build_qwen_batch_payload(history, chunk_strings);
        let res = client.post_chat_completion(payload).await?;
        let response_content = TranslationClient::response_text_from_completion(&res)?;
        let mut results = vec![String::new(); chunk_strings.len()];

        for line in response_content.lines() {
            if let Some((idx, text)) = TranslationClient::parse_numbered_translation_line(line)
                && idx > 0
                && idx <= chunk_strings.len()
            {
                results[idx - 1] = text.trim().to_string();
            }
        }

        if chunk_strings.len() == 1 && results[0].is_empty() {
            results[0] = response_content.trim().to_string();
        }

        let empty_indices = results
            .iter()
            .enumerate()
            .filter_map(|(idx, text)| text.is_empty().then_some(idx))
            .collect::<Vec<_>>();

        if !empty_indices.is_empty() {
            log::warn!(
                "[Translation] {} slots empty after batch parse, retrying individually",
                empty_indices.len()
            );
            for idx in empty_indices {
                if let Ok(single) = self
                    .translate_single_qwen(client, &chunk_strings[idx])
                    .await
                {
                    results[idx] = single;
                }
            }
        }

        let unresolved = results
            .iter()
            .enumerate()
            .filter_map(|(idx, text)| {
                if text.is_empty() {
                    log::warn!(
                        "[Translation] Empty numbered translation slot {} after retries",
                        idx + 1
                    );
                    Some(idx + 1)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if unresolved.is_empty() {
            Ok(results)
        } else {
            anyhow::bail!("missing translations for slots {unresolved:?}");
        }
    }

    async fn translate_gemma_chunk(
        self,
        client: &TranslationClient,
        history: &[(String, String)],
        chunk_strings: &[String],
    ) -> anyhow::Result<Vec<String>> {
        let mut final_results = vec![String::new(); chunk_strings.len()];
        let mut request_futures = Vec::with_capacity(chunk_strings.len());

        for input_text in chunk_strings {
            let payload = json!({
                "model": "local",
                "messages": Self::build_translategemma_messages(
                    history,
                    input_text,
                ),
                "temperature": 0.1,
                "max_tokens": 256
            });
            request_futures.push(client.post_chat_completion(payload));
        }

        // Process all requests in the chunk in parallel
        let chunk_responses = join_all(request_futures).await;

        for (chunk_offset, res_result) in chunk_responses.into_iter().enumerate() {
            if let Ok(res) = res_result
                && let Ok(translation) = TranslationClient::response_text_from_completion(&res)
            {
                final_results[chunk_offset] = translation;
            }
        }

        // Check if any translations in this chunk are missing and retry individually
        let missing_indices: Vec<usize> = chunk_strings
            .iter()
            .enumerate()
            .filter_map(|(i, _)| {
                if final_results[i].is_empty() {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if !missing_indices.is_empty() {
            log::warn!(
                "[Translation] {} Gemma slots empty after parallel batch, retrying individually",
                missing_indices.len()
            );
            for i in missing_indices {
                let input_text = &chunk_strings[i];
                let payload = json!({
                    "model": "local",
                    "messages": Self::build_translategemma_messages(
                        history,
                        input_text,
                    ),
                    "temperature": 0.1,
                    "max_tokens": 256
                });
                if let Ok(res) = client.post_chat_completion(payload).await
                    && let Ok(translation) = TranslationClient::response_text_from_completion(&res)
                {
                    final_results[i] = translation;
                }
            }
        }

        let still_missing = chunk_strings
            .iter()
            .enumerate()
            .filter(|(i, _)| final_results[*i].is_empty())
            .map(|(i, _)| i + 1)
            .collect::<Vec<_>>();

        if !still_missing.is_empty() {
            anyhow::bail!("Gemma parallel batch failed for slots {still_missing:?}");
        }

        Ok(final_results)
    }

    pub fn build_translategemma_conversation(
        history: &[(String, String)],
        input_text: &str,
    ) -> Vec<Value> {
        let keep = history.len().min(TRANSLATEGEMMA_HISTORY_LIMIT);
        let history_slice = &history[history.len().saturating_sub(keep)..];

        let mut conversation = Vec::with_capacity(history_slice.len() * 2 + 1);
        for (source_text, translated_text) in history_slice {
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

    pub fn build_translategemma_messages(
        history: &[(String, String)],
        input_text: &str,
    ) -> Vec<Value> {
        let system_prompt = "You are a professional Japanese-to-English translator. Translate the user's Japanese screen-text observations into natural, concise English. Output only the English translation of the observed text. Do not provide notes, explanations, or alternate translations.";
        let mut messages = vec![json!({
            "role": "system",
            "content": system_prompt
        })];
        messages.extend(Self::build_translategemma_conversation(history, input_text));
        messages
    }
}

pub struct TranslationClient {
    pub memory: TranslationMemory,
    pub(crate) port: u16,
    pub(crate) client: Client,
    pub(crate) sidecar: crate::sidecar::SidecarManager,
    pub strategy: TranslationStrategy,
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
            sidecar: crate::sidecar::SidecarManager::new(port),
            strategy: TranslationStrategy::Qwen,
        }
    }

    pub fn set_strategy(&mut self, strategy_name: &str) {
        self.strategy = match strategy_name.to_ascii_lowercase().as_str() {
            "gemma" | "translategemma" => TranslationStrategy::Gemma,
            _ => TranslationStrategy::Qwen,
        };
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

    pub(crate) async fn post_chat_completion(&self, payload: Value) -> anyhow::Result<Value> {
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        let mut last_error = String::from("unknown error");

        for attempt in 1..=2 {
            let response = self.client.post(&url).json(&payload).send().await;
            match response {
                Ok(response) => {
                    let status = response.status();
                    let body = response.text().await?;
                    if status.is_success() {
                        return serde_json::from_str(&body).map_err(|error| {
                            anyhow::anyhow!(
                                "llama-server returned invalid JSON: {error}; body: {}",
                                body.trim()
                            )
                        });
                    }
                    last_error = format!("llama-server returned HTTP {status}: {}", body.trim());
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

    pub(crate) fn response_text_from_completion(res: &Value) -> anyhow::Result<String> {
        let content = &res["choices"][0]["message"]["content"];
        let text = Self::response_text(content);
        if text.is_empty() {
            anyhow::bail!("llama-server returned an empty translation");
        }
        Ok(text)
    }

    pub fn start_sidecar(
        &mut self,
        app: &tauri::AppHandle,
        model_path: &std::path::Path,
        model_id: &str,
        strategy: Option<&str>,
    ) -> anyhow::Result<()> {
        let strategy_name = strategy.unwrap_or_else(|| {
            if model_id.to_ascii_lowercase().contains("translategemma") {
                "gemma"
            } else {
                "qwen"
            }
        });
        self.set_strategy(strategy_name);
        self.sidecar.start(app, model_path, model_id, strategy)
    }

    pub fn start_sidecar_headless(
        &mut self,
        model_path: &std::path::Path,
        model_id: &str,
        strategy: Option<&str>,
    ) -> anyhow::Result<()> {
        let strategy_name = strategy.unwrap_or_else(|| {
            if model_id.to_ascii_lowercase().contains("translategemma") {
                "gemma"
            } else {
                "qwen"
            }
        });
        self.set_strategy(strategy_name);
        self.sidecar.start_headless(model_path, model_id, strategy)
    }

    pub async fn wait_for_ready(&self) -> anyhow::Result<()> {
        self.sidecar.wait_for_ready().await
    }

    pub async fn wait_for_ready_retry(&self) -> anyhow::Result<()> {
        self.sidecar.wait_for_ready_retry().await
    }

    pub async fn wait_for_runtime_ready(&self) -> anyhow::Result<()> {
        self.sidecar.wait_for_runtime_ready().await
    }

    pub async fn quick_health_check(&self) -> anyhow::Result<()> {
        self.sidecar.quick_health_check().await
    }

    pub fn shutdown_sidecar(&mut self) {
        self.sidecar.stop();
    }

    pub(crate) fn parse_numbered_translation_line(line: &str) -> Option<(usize, String)> {
        let trimmed = line.trim();
        let trimmed = trimmed
            .trim_start_matches('*')
            .trim_end_matches('*')
            .trim_start();
        let number_end = trimmed.find(|c: char| !c.is_ascii_digit())?;
        if number_end == 0 {
            return None;
        }

        let idx = trimmed[..number_end].parse::<usize>().ok()?;
        let remainder = trimmed[number_end..].trim_start();
        let remainder = remainder
            .strip_prefix(':')
            .or_else(|| remainder.strip_prefix('.'))
            .or_else(|| remainder.strip_prefix(')'))?
            .trim_start()
            .trim_start_matches('*')
            .trim_start();
        Some((idx, remainder.to_string()))
    }

    pub async fn translate_batch(&mut self, strings: &[String]) -> anyhow::Result<Vec<String>> {
        if strings.is_empty() {
            return Ok(vec![]);
        }

        let mut final_results = vec![String::new(); strings.len()];
        let history = self.memory.as_context_slice().to_vec();

        // Sub-batch at 15 strings to avoid hitting token limits or context window issues
        for (chunk_idx, chunk_strings) in strings.chunks(15).enumerate() {
            let offset = chunk_idx * 15;
            let chunk_translations = self
                .strategy
                .translate_chunk(self, &history, chunk_strings)
                .await?;
            for (chunk_offset, translation) in chunk_translations.into_iter().enumerate() {
                final_results[offset + chunk_offset] = translation;
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
        let conversation =
            super::TranslationStrategy::build_translategemma_conversation(&[], "はじめに");

        assert_eq!(conversation.len(), 1);
        assert_eq!(conversation[0]["role"], "user");
        assert_eq!(conversation[0]["content"][0]["type"], "text");
        assert_eq!(conversation[0]["content"][0]["source_lang_code"], "ja");
        assert_eq!(conversation[0]["content"][0]["target_lang_code"], "en");
        assert_eq!(conversation[0]["content"][0]["text"], "はじめに");
    }

    #[test]
    fn translategemma_conversation_preserves_prior_pairs() {
        let conversation = super::TranslationStrategy::build_translategemma_conversation(
            &[("猫".to_string(), "cat".to_string())],
            "犬",
        );

        assert_eq!(conversation.len(), 3);
        assert_eq!(conversation[0]["content"][0]["text"], "猫");
        assert_eq!(conversation[1]["role"], "assistant");
        assert_eq!(conversation[1]["content"], "cat");
        assert_eq!(conversation[2]["content"][0]["text"], "犬");
    }

    #[test]
    fn translategemma_messages_include_system_prompt() {
        let messages = super::TranslationStrategy::build_translategemma_messages(&[], "犬");

        assert_eq!(messages[0]["role"], "system");
        assert!(
            messages[0]["content"]
                .as_str()
                .is_some_and(|content: &str| {
                    content.contains("Output only the English translation")
                })
        );
        assert_eq!(messages[1]["role"], "user");
    }

    #[test]
    fn parse_numbered_translation_line_accepts_multiple_formats() {
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("1: hello"),
            Some((1, "hello".to_string()))
        );
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("2. world"),
            Some((2, "world".to_string()))
        );
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("**3:** test"),
            Some((3, "test".to_string()))
        );
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("4) sample"),
            Some((4, "sample".to_string()))
        );
    }

    #[test]
    fn parse_numbered_translation_line_rejects_malformed() {
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("hello"),
            None
        );
        assert_eq!(TranslationClient::parse_numbered_translation_line(""), None);
        assert_eq!(
            TranslationClient::parse_numbered_translation_line("1234"),
            None
        );
        assert_eq!(
            TranslationClient::parse_numbered_translation_line(
                "9999999999999999999999999999: hello"
            ),
            None
        );
    }

    #[test]
    fn test_strategy_selection() {
        let mut client = TranslationClient::new(6, 8765);

        client.set_strategy("qwen");
        assert_eq!(client.strategy, super::TranslationStrategy::Qwen);

        client.set_strategy("gemma");
        assert_eq!(client.strategy, super::TranslationStrategy::Gemma);
    }

    #[test]
    fn test_translation_memory_eviction() {
        let mut memory = super::TranslationMemory::new(3);
        memory.push("1".to_string(), "one".to_string());
        memory.push("2".to_string(), "two".to_string());
        memory.push("3".to_string(), "three".to_string());

        assert_eq!(memory.as_context_slice().len(), 3);
        assert_eq!(memory.as_context_slice()[0].0, "1");

        // Exceed capacity -> should evict the oldest ("1")
        memory.push("4".to_string(), "four".to_string());
        let slice = memory.as_context_slice();
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0].0, "2");
        assert_eq!(slice[1].0, "3");
        assert_eq!(slice[2].0, "4");
    }

    #[test]
    fn test_translation_memory_zero_capacity() {
        let mut memory = super::TranslationMemory::new(0);
        memory.push("1".to_string(), "one".to_string());
        assert!(memory.as_context_slice().is_empty());
    }

    #[test]
    fn test_translation_memory_clear() {
        let mut memory = super::TranslationMemory::new(5);
        memory.push("1".to_string(), "one".to_string());
        memory.push("2".to_string(), "two".to_string());
        assert_eq!(memory.as_context_slice().len(), 2);

        memory.clear();
        assert!(memory.as_context_slice().is_empty());
    }
}
