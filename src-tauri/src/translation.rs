use std::collections::VecDeque;

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

pub struct TranslationEngine {
    // llama.cpp state would be here
    pub memory: TranslationMemory,
    pub model_id: String,
}

impl TranslationEngine {
    pub fn new(max_memory_size: usize, model_id: String) -> anyhow::Result<Self> {
        Ok(Self {
            memory: TranslationMemory::new(max_memory_size),
            model_id,
        })
    }

    pub fn switch_model(&mut self, new_model_id: &str) -> anyhow::Result<()> {
        log::info!("Switching model to {}", new_model_id);
        self.model_id = new_model_id.to_string();
        self.memory.clear();
        // llama engine reload goes here
        Ok(())
    }

    pub fn translate_batch(&mut self, strings: &[String]) -> anyhow::Result<Vec<String>> {
        // Build prompt with context if memory is not empty
        // MOCK: just returning uppercase for now
        let results = strings.iter().map(|s| format!("{} TRANSLATED", s)).collect::<Vec<_>>();
        
        // Push to memory
        for (original, translated) in strings.iter().zip(results.iter()) {
            self.memory.push(original.clone(), translated.clone());
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_should_enforce_max_size() {
        let mut memory = TranslationMemory::new(2);
        memory.push("1".to_string(), "one".to_string());
        memory.push("2".to_string(), "two".to_string());
        memory.push("3".to_string(), "three".to_string());

        let slice = memory.as_context_slice();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0].0, "2");
        assert_eq!(slice[1].0, "3");
    }
}
