// src-tauri/src/settings.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPlacement {
    #[default]
    Cover,
    Above,
    Below,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub debounce_ms: u64,
    pub motion_threshold: f32,
    pub pixel_diff_threshold: u8,
    pub capture_fps: u32,
    pub edge_inset_percent: u32,
    pub furigana_suppression: bool,
    pub show_original_text: bool,
    pub context_memory_size: usize,
    pub active_model: String,
    #[serde(default)]
    pub overlay_placement: OverlayPlacement,
    #[serde(default)]
    pub wizard_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            debounce_ms: 150,
            motion_threshold: 0.01,
            pixel_diff_threshold: 15,
            capture_fps: 30,
            edge_inset_percent: 5,
            furigana_suppression: true,
            show_original_text: false,
            context_memory_size: 6,
            active_model: "translategemma-4b-it.Q4_K_M".to_string(),
            overlay_placement: OverlayPlacement::Cover,
            wizard_completed: false,
        }
    }
}

impl Settings {
    /// Loads settings from disk, creating defaults if missing.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created or file cannot be written/read.
    pub fn load(app_dir: &std::path::Path) -> anyhow::Result<Self> {
        let settings_path = app_dir.join("settings.json");

        if !settings_path.exists() {
            let default_settings = Self::default();
            let json = serde_json::to_string_pretty(&default_settings)?;
            if !app_dir.exists() {
                fs::create_dir_all(app_dir)?;
            }
            fs::write(&settings_path, json)?;
            return Ok(default_settings);
        }

        let json = fs::read_to_string(&settings_path)?;
        let settings = serde_json::from_str(&json)?;
        Ok(settings)
    }

    /// Saves the current settings to disk.
    ///
    /// # Errors
    /// Returns an error if the serialization fails or if the file cannot be written.
    pub fn save(&self, app_dir: &std::path::Path) -> anyhow::Result<()> {
        let settings_path = app_dir.join("settings.json");
        let json = serde_json::to_string_pretty(self)?;
        if !app_dir.exists() {
            fs::create_dir_all(app_dir)?;
        }
        fs::write(settings_path, json)?;
        Ok(())
    }

    /// Helper to get the standard application support directory.
    /// Note: In Tauri v2, prefer using `app.path().app_data_dir()` or similar.
    pub fn dir() -> anyhow::Result<PathBuf> {
        let path = match std::env::var_os("CONTEXTURA_DATA_DIR") {
            Some(value) if !value.is_empty() => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    anyhow::bail!("CONTEXTURA_DATA_DIR must be an absolute path");
                }
                path
            }
            _ => {
                let mut path =
                    dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("No data local dir"))?;
                path.push("contextura");
                path
            }
        };
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{OverlayPlacement, Settings};

    #[test]
    fn missing_overlay_placement_defaults_to_cover() {
        let settings = serde_json::from_str::<Settings>(
            r#"{
                "debounce_ms": 150,
                "motion_threshold": 0.01,
                "pixel_diff_threshold": 15,
                "capture_fps": 30,
                "edge_inset_percent": 5,
                "furigana_suppression": true,
                "show_original_text": false,
                "context_memory_size": 6,
                "active_model": "example"
            }"#,
        )
        .expect("legacy settings should deserialize");

        assert_eq!(settings.overlay_placement, OverlayPlacement::Cover);
    }

    #[test]
    fn overlay_placement_round_trips_through_settings_json() {
        let settings = Settings {
            overlay_placement: OverlayPlacement::Below,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).expect("settings should serialize");
        let restored =
            serde_json::from_str::<Settings>(&json).expect("settings should deserialize");

        assert!(json.contains(r#""overlay_placement":"below""#));
        assert_eq!(restored.overlay_placement, OverlayPlacement::Below);
    }
}
