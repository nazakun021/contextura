use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
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
    pub wizard_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            debounce_ms: 300,
            motion_threshold: 0.05,
            pixel_diff_threshold: 15,
            capture_fps: 30,
            edge_inset_percent: 5,
            furigana_suppression: true,
            show_original_text: false,
            context_memory_size: 6,
            active_model: "nllb-600m-q4".to_string(),
            wizard_completed: false,
        }
    }
}

impl Settings {
    /// Loads settings from disk, creating defaults if missing.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created or file cannot be written/read.
    pub fn load() -> anyhow::Result<Self> {
        let app_dir = Self::dir()?;
        let settings_path = app_dir.join("settings.json");

        if !settings_path.exists() {
            let default_settings = Self::default();
            let json = serde_json::to_string_pretty(&default_settings)?;
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
    // Used when wizard completion is persisted; called from wizard close handler (not yet wired).
    #[expect(dead_code, reason = "Will be called from the wizard completion handler in the next integration step")]
    pub fn save(&self) -> anyhow::Result<()> {
        let app_dir = Self::dir()?;
        let settings_path = app_dir.join("settings.json");
        let json = serde_json::to_string_pretty(self)?;
        fs::write(settings_path, json)?;
        Ok(())
    }

    /// Helper to get the standard application support directory.
    pub fn dir() -> anyhow::Result<PathBuf> {
        let mut path = dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("No data local dir"))?;
        path.push("jp-translate");
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }
        Ok(path)
    }
}
