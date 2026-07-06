// src-tauri/src/path_resolver.rs

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;

#[derive(Clone)]
pub struct AppConfig {
    pub bundle_id: String,
    pub process_id: i32,
    pub name_hint: String,
    pub path_resolver: Arc<PathResolver>,
}

impl AppConfig {
    pub fn new(
        bundle_id: String,
        process_id: i32,
        name_hint: String,
        path_resolver: Arc<PathResolver>,
    ) -> Self {
        Self {
            bundle_id,
            process_id,
            name_hint,
            path_resolver,
        }
    }
}

pub struct PathResolver {
    is_headless: bool,
    custom_root: Option<PathBuf>,
}

impl PathResolver {
    pub fn new(is_headless: bool, custom_root: Option<PathBuf>) -> Self {
        Self {
            is_headless,
            custom_root,
        }
    }

    pub fn models_dir(&self, app_handle: Option<&tauri::AppHandle>) -> anyhow::Result<PathBuf> {
        if self.is_headless {
            let root = self
                .custom_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            return Ok(root.join("models"));
        }

        if let Some(app) = app_handle {
            return Ok(app.path().app_local_data_dir()?.join("models"));
        }

        // Fallback
        let mut path =
            dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("No data local dir available"))?;
        path.push("contextura");
        path.push("models");
        Ok(path)
    }

    pub fn cache_dir(&self, app_handle: Option<&tauri::AppHandle>) -> anyhow::Result<PathBuf> {
        if self.is_headless {
            let root = self
                .custom_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            return Ok(root.join("cache"));
        }

        if let Some(app) = app_handle {
            return Ok(app.path().app_cache_dir()?);
        }

        let mut path =
            dirs::cache_dir().ok_or_else(|| anyhow::anyhow!("No cache dir available"))?;
        path.push("contextura");
        Ok(path)
    }

    pub fn settings_dir(&self, app_handle: Option<&tauri::AppHandle>) -> anyhow::Result<PathBuf> {
        if self.is_headless {
            let root = self
                .custom_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            return Ok(root);
        }

        if let Some(app) = app_handle {
            return Ok(app.path().app_local_data_dir()?);
        }

        let mut path =
            dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("No data local dir available"))?;
        path.push("contextura");
        Ok(path)
    }

    #[allow(clippy::unused_self)]
    pub fn resolve_binary(
        &self,
        binary_name: &str,
        app_handle: Option<&tauri::AppHandle>,
    ) -> anyhow::Result<PathBuf> {
        let mut candidates = Vec::new();

        if let Some(app) = app_handle
            && let Ok(resource_dir) = app.path().resource_dir()
        {
            candidates.push(resource_dir.join("binaries").join(binary_name));
            candidates.push(
                resource_dir
                    .join("binaries")
                    .join(format!("{binary_name}-aarch64-apple-darwin")),
            );
            candidates.push(resource_dir.join(binary_name));
            candidates.push(resource_dir.join(format!("{binary_name}-aarch64-apple-darwin")));
        }

        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            candidates.push(exe_dir.join(binary_name));
            candidates.push(exe_dir.join(format!("{binary_name}-aarch64-apple-darwin")));
            candidates.push(exe_dir.join("binaries").join(binary_name));
            candidates.push(
                exe_dir
                    .join("binaries")
                    .join(format!("{binary_name}-aarch64-apple-darwin")),
            );
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.push(manifest_dir.join("binaries").join(binary_name));
        candidates.push(
            manifest_dir
                .join("binaries")
                .join(format!("{binary_name}-aarch64-apple-darwin")),
        );
        candidates.push(PathBuf::from(format!("src-tauri/binaries/{binary_name}")));
        candidates.push(PathBuf::from(format!(
            "src-tauri/binaries/{binary_name}-aarch64-apple-darwin"
        )));

        candidates
            .into_iter()
            .find(|path| path.exists())
            .ok_or_else(|| anyhow::anyhow!("Could not locate {binary_name} binary"))
    }
}

/// Binds to a random port on 127.0.0.1 to find an available local port.
pub fn find_available_local_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_available_local_port() {
        let port = find_available_local_port();
        assert!(port.is_ok());
        let port_val = port.unwrap();
        assert!(port_val > 0);
    }

    #[test]
    fn test_path_resolver_headless_resolution() {
        let temp_root = std::env::temp_dir().join("contextura-test-resolver");
        let resolver = PathResolver::new(true, Some(temp_root.clone()));

        let cache = resolver.cache_dir(None).unwrap();
        assert_eq!(cache, temp_root.join("cache"));

        let settings = resolver.settings_dir(None).unwrap();
        assert_eq!(settings, temp_root);

        let models = resolver.models_dir(None).unwrap();
        assert_eq!(models, temp_root.join("models"));
    }

    #[test]
    fn test_app_config_creation() {
        let temp_root = std::env::temp_dir().join("contextura-test-appconfig");
        let resolver = Arc::new(PathResolver::new(true, Some(temp_root)));
        let app_config = AppConfig::new(
            "com.test.app".to_string(),
            999,
            "TestApp".to_string(),
            resolver,
        );

        assert_eq!(app_config.bundle_id, "com.test.app");
    }
}
