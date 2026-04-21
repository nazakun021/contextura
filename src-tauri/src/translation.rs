use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;
use tauri_plugin_shell::{ShellExt, process::CommandEvent};

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
    pub model_id: String,
    port: u16,
    client: Client,
}

impl TranslationClient {
    pub fn new(max_memory_size: usize, model_id: String, port: u16) -> anyhow::Result<Self> {
        Ok(Self {
            memory: TranslationMemory::new(max_memory_size),
            model_id,
            port,
            client: Client::new(),
        })
    }

    pub async fn wait_for_ready(&self) -> anyhow::Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let mut attempts = 0;
        loop {
            if let Ok(res) = self.client.get(&url).send().await {
                if let Ok(json) = res.json::<Value>().await {
                    if json.get("status").and_then(|s| s.as_str()) == Some("ok") {
                        return Ok(());
                    }
                }
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

        let mut prompt = String::new();
        let context = self.memory.as_context_slice();
        if !context.is_empty() {
            prompt.push_str("Previous context (do not retranslate, for reference only):\n");
            for (ja, en) in context {
                prompt.push_str(&format!("- {} -> \"{}\"\n", ja, en));
            }
            prompt.push_str("\n");
        }

        prompt.push_str("Translate each numbered Japanese string to English.\n");
        prompt.push_str("Output only translations, one per line, same numbered format.\n\n");
        
        for (i, s) in strings.iter().enumerate() {
            prompt.push_str(&format!("{}: {}\n", i + 1, s));
        }

        let payload = json!({
            "model": "local",
            "messages": [
                { "role": "system", "content": "You are a Japanese-to-English translator. Do not include any explanations, just translate." },
                { "role": "user", "content": prompt }
            ],
            "temperature": 0.1,
            "max_tokens": 512
        });

        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        let res: Value = self.client.post(&url).json(&payload).send().await?.json().await?;
        
        let content = res["choices"][0]["message"]["content"].as_str().unwrap_or("");
        
        let mut results = vec![String::new(); strings.len()];
        for line in content.lines() {
            if let Some((num, text)) = line.split_once(':') {
                if let Ok(idx) = num.trim().parse::<usize>() {
                    if idx > 0 && idx <= strings.len() {
                        results[idx - 1] = text.trim().to_string();
                    }
                }
            }
        }

        for (i, s) in strings.iter().enumerate() {
            if !results[i].is_empty() {
                self.memory.push(s.clone(), results[i].clone());
            }
        }

        Ok(results)
    }
}
