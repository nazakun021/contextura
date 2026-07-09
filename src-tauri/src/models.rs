// src-tauri/src/models.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::settings::Settings;

#[cfg_attr(not(test), allow(dead_code))]
const DEFAULT_MODEL_FILENAME: &str = "translategemma-4b-it.Q4_K_M.gguf";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: String,
    pub filename: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelManifest {
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub entry: ModelEntry,
    pub path: PathBuf,
    pub installed: bool,
}

#[derive(Debug, Clone)]
pub struct ModelSwitchResult {
    pub previous: ModelStatus,
    pub current: ModelStatus,
}

impl ModelEntry {
    #[must_use]
    pub fn display_label(&self) -> &str {
        if self.label.is_empty() {
            &self.id
        } else {
            &self.label
        }
    }
}

impl ModelManifest {
    pub fn load(app_dir: &Path, settings: &Settings) -> anyhow::Result<Self> {
        let models_dir = models_dir(app_dir);
        if !models_dir.exists() {
            fs::create_dir_all(&models_dir)?;
        }

        let manifest_path = manifest_path(app_dir);
        let mut manifest = if manifest_path.exists() {
            serde_json::from_str::<Self>(&fs::read_to_string(manifest_path)?)?
        } else {
            Self::default()
        };

        for path in gguf_files(&models_dir)? {
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let id = file_stem_id(file_name);
            let already_known = manifest
                .models
                .iter()
                .any(|model| model.id == id || model.filename == file_name);
            if already_known {
                continue;
            }

            manifest.models.push(ModelEntry {
                label: prettify_label(&id),
                tier: infer_tier(&id),
                active: false,
                filename: file_name.to_string(),
                id,
                strategy: None,
            });
        }

        if manifest.models.is_empty() {
            manifest.models.push(ModelEntry {
                id: settings.active_model.clone(),
                filename: format!("{}.gguf", settings.active_model),
                label: prettify_label(&settings.active_model),
                tier: infer_tier(&settings.active_model),
                active: true,
                strategy: None,
            });
        }

        Ok(manifest.normalized(settings, &models_dir))
    }

    fn normalized(mut self, settings: &Settings, models_dir: &Path) -> Self {
        if self.models.is_empty() {
            return self;
        }

        for model in &mut self.models {
            if model.label.is_empty() {
                model.label = prettify_label(&model.id);
            }
            if model.tier.is_empty() {
                model.tier = infer_tier(&model.id);
            }
            if model.filename.is_empty() {
                model.filename = format!("{}.gguf", model.id);
            }
            if model.strategy.is_none() {
                model.strategy = Some(
                    if model.id.to_ascii_lowercase().contains("translategemma") {
                        "gemma".to_string()
                    } else if model.id.to_ascii_lowercase().contains("lfm") || model.id.to_ascii_lowercase().contains("350m") {
                        "lfm".to_string()
                    } else {
                        "qwen".to_string()
                    },
                );
            }
        }

        let active_id = self
            .models
            .iter()
            .find(|model| model.active)
            .map(|model| model.id.clone())
            .or_else(|| {
                self.models
                    .iter()
                    .find(|model| model.id == settings.active_model)
                    .map(|model| model.id.clone())
            })
            .or_else(|| {
                self.models
                    .iter()
                    .find(|model| models_dir.join(&model.filename).exists())
                    .map(|model| model.id.clone())
            })
            .unwrap_or_else(|| self.models[0].id.clone());

        for model in &mut self.models {
            model.active = model.id == active_id;
        }

        self
    }

    #[must_use]
    pub fn statuses(&self, app_dir: &Path) -> Vec<ModelStatus> {
        self.models
            .iter()
            .cloned()
            .map(|entry| {
                let path = models_dir(app_dir).join(&entry.filename);
                let installed = path.exists();
                ModelStatus {
                    entry,
                    path,
                    installed,
                }
            })
            .collect()
    }

    pub fn active_status(&self, app_dir: &Path) -> Option<ModelStatus> {
        self.statuses(app_dir)
            .into_iter()
            .find(|status| status.entry.active)
    }

    pub fn save(&self, app_dir: &Path) -> anyhow::Result<()> {
        let path = manifest_path(app_dir);
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

pub fn active_model_status(app_dir: &Path, settings: &Settings) -> anyhow::Result<ModelStatus> {
    let manifest = ModelManifest::load(app_dir, settings)?;
    manifest
        .active_status(app_dir)
        .ok_or_else(|| anyhow::anyhow!("No active model is configured"))
}

pub fn cycle_active_model(
    app_dir: &Path,
    settings: &mut Settings,
) -> anyhow::Result<ModelSwitchResult> {
    let mut manifest = ModelManifest::load(app_dir, settings)?;
    let statuses = manifest.statuses(app_dir);
    let installed_indices: Vec<usize> = statuses
        .iter()
        .enumerate()
        .filter_map(|(index, status)| status.installed.then_some(index))
        .collect();

    if installed_indices.len() < 2 {
        return Err(anyhow::anyhow!(
            "Model switching needs at least two installed GGUF files in the models directory"
        ));
    }

    let current_index = installed_indices
        .iter()
        .find(|index| statuses[**index].entry.active)
        .copied()
        .unwrap_or(installed_indices[0]);
    let current_pos = installed_indices
        .iter()
        .position(|index| *index == current_index)
        .unwrap_or(0);
    let next_index = installed_indices[(current_pos + 1) % installed_indices.len()];

    let previous = statuses[current_index].clone();
    let current = statuses[next_index].clone();

    for model in &mut manifest.models {
        model.active = model.id == current.entry.id;
    }
    manifest.save(app_dir)?;

    settings.active_model.clone_from(&current.entry.id);
    settings.save(app_dir)?;

    Ok(ModelSwitchResult { previous, current })
}

#[must_use]
pub fn manifest_path(app_dir: &Path) -> PathBuf {
    models_dir(app_dir).join("manifest.json")
}

#[must_use]
pub fn models_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("models")
}

fn gguf_files(models_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(models_dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("gguf"))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn file_stem_id(file_name: &str) -> String {
    file_name
        .strip_suffix(".gguf")
        .unwrap_or(file_name)
        .to_string()
}

fn prettify_label(id: &str) -> String {
    id.replace(['_', '-', '.'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_ascii_uppercase().to_string() + chars.as_str()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn infer_tier(id: &str) -> String {
    let lowercase = id.to_ascii_lowercase();
    if lowercase.contains("0.6b")
        || lowercase.contains("1b")
        || lowercase.contains("350m")
        || lowercase.contains("lfm")
        || lowercase.contains("small")
    {
        "Standard".to_string()
    } else if lowercase.contains("quality")
        || lowercase.contains("4b")
        || lowercase.contains("7b")
        || lowercase.contains("8b")
        || lowercase.contains("14b")
        || lowercase.contains("gemma")
    {
        "Quality".to_string()
    } else {
        "Custom".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MODEL_FILENAME, ModelManifest, cycle_active_model};
    use crate::settings::Settings;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_app_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("contextura-{label}-{unique}"));
        fs::create_dir_all(dir.join("models")).expect("temp model dir should be created");
        dir
    }

    #[test]
    fn manifest_load_marks_settings_model_as_active_when_manifest_is_missing() {
        let app_dir = temp_app_dir("manifest-load");
        let settings = Settings::default();
        fs::write(
            app_dir.join("models").join(DEFAULT_MODEL_FILENAME),
            b"model-bytes",
        )
        .expect("default model file should be written");

        let manifest = ModelManifest::load(&app_dir, &settings).expect("manifest should load");

        assert!(
            manifest
                .models
                .iter()
                .any(|model| model.id == settings.active_model && model.active)
        );
    }

    #[test]
    fn manifest_load_should_merge_new_disk_models_into_existing_manifest() {
        let app_dir = temp_app_dir("manifest-merge");
        let settings = Settings {
            active_model: "qwen3-0.6b-q4".to_string(),
            ..Settings::default()
        };

        fs::write(
            app_dir.join("models").join("manifest.json"),
            r#"{
  "models": [
    {
      "id": "qwen3-0.6b-q4",
      "filename": "qwen3-0.6b-q4_k_m.gguf",
      "active": true
    }
  ]
}"#,
        )
        .expect("manifest should be written");
        fs::write(
            app_dir.join("models").join("qwen3-0.6b-q4_k_m.gguf"),
            b"qwen",
        )
        .expect("qwen model should be written");
        fs::write(
            app_dir
                .join("models")
                .join("translategemma-4b-it.Q4_K_M.gguf"),
            b"gemma",
        )
        .expect("translategemma model should be written");

        let manifest = ModelManifest::load(&app_dir, &settings).expect("manifest should load");

        assert!(
            manifest
                .models
                .iter()
                .any(|model| model.id == "qwen3-0.6b-q4" && model.active)
        );
        assert!(
            manifest
                .models
                .iter()
                .any(|model| model.id == "translategemma-4b-it.Q4_K_M")
        );
    }

    #[test]
    fn cycle_active_model_should_switch_to_the_next_installed_model() {
        let app_dir = temp_app_dir("manifest-cycle");
        let mut settings = Settings::default();
        fs::write(
            app_dir.join("models").join(DEFAULT_MODEL_FILENAME),
            b"standard-model",
        )
        .expect("standard model file should be written");
        fs::write(
            app_dir.join("models").join("qwen3-4b-q4.gguf"),
            b"quality-model",
        )
        .expect("quality model file should be written");

        let result =
            cycle_active_model(&app_dir, &mut settings).expect("model switch should succeed");

        assert_eq!(result.previous.entry.id, "translategemma-4b-it.Q4_K_M");
        assert_eq!(result.current.entry.id, "qwen3-4b-q4");
        assert_eq!(settings.active_model, "qwen3-4b-q4");
    }

    #[test]
    fn test_manifest_strategy_normalization() {
        let app_dir = temp_app_dir("manifest-strategy");
        let settings = Settings {
            active_model: "qwen3-0.6b-q4".to_string(),
            ..Settings::default()
        };
        fs::write(
            app_dir
                .join("models")
                .join("translategemma-4b-it.Q4_K_M.gguf"),
            b"gemma",
        )
        .expect("gemma model should be written");
        fs::write(app_dir.join("models").join("qwen3-0.6b-q4.gguf"), b"qwen")
            .expect("qwen model should be written");

        let manifest = ModelManifest::load(&app_dir, &settings).expect("manifest should load");

        let gemma_entry = manifest
            .models
            .iter()
            .find(|m| m.id.contains("translategemma"))
            .unwrap();
        let qwen_entry = manifest
            .models
            .iter()
            .find(|m| m.id.contains("qwen"))
            .unwrap();

        assert_eq!(gemma_entry.strategy.as_deref(), Some("gemma"));
        assert_eq!(qwen_entry.strategy.as_deref(), Some("qwen"));
    }

    #[test]
    fn test_lfm2_model_tier_and_strategy() {
        let app_dir = temp_app_dir("lfm2-test");
        let settings = Settings {
            active_model: "LFM2-350M-ENJP-MT-Q8_0".to_string(),
            ..Settings::default()
        };
        fs::write(
            app_dir.join("models").join("LFM2-350M-ENJP-MT-Q8_0.gguf"),
            b"lfm2-bytes",
        )
        .expect("lfm2 model file should be written");

        let manifest = ModelManifest::load(&app_dir, &settings).expect("manifest should load");
        let lfm_entry = manifest
            .models
            .iter()
            .find(|m| m.id.contains("LFM2-350M"))
            .unwrap();

        assert_eq!(lfm_entry.tier, "Standard");
        assert_eq!(lfm_entry.strategy.as_deref(), Some("lfm"));
    }
}
