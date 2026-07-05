// src-tauri/src/path_resolver.rs

use std::net::TcpListener;
use std::path::PathBuf;

/// Resolves path to a local binary.
pub fn resolve_binary_path(binary_name: &str) -> anyhow::Result<PathBuf> {
    let mut candidates = Vec::new();

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

    candidates.push(PathBuf::from(format!("src-tauri/binaries/{binary_name}")));
    candidates.push(PathBuf::from(format!(
        "src-tauri/binaries/{binary_name}-aarch64-apple-darwin"
    )));

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| anyhow::anyhow!("Could not locate {binary_name} binary"))
}

/// Resolves vision-helper path using `tauri::App` resource paths.
pub fn resolve_vision_helper_path(app: &tauri::App) -> anyhow::Result<PathBuf> {
    use tauri::Manager;

    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("binaries").join("vision-helper"));
        candidates.push(
            resource_dir
                .join("binaries")
                .join("vision-helper-aarch64-apple-darwin"),
        );
        candidates.push(resource_dir.join("vision-helper"));
        candidates.push(resource_dir.join("vision-helper-aarch64-apple-darwin"));
    }

    candidates.extend([
        resolve_binary_path("vision-helper")?,
        PathBuf::from("src-tauri/binaries/vision-helper-aarch64-apple-darwin"),
    ]);

    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| anyhow::anyhow!("Could not locate vision-helper binary"))
}

/// Resolves path to llama-server.
pub fn resolve_llama_server_path() -> anyhow::Result<PathBuf> {
    resolve_binary_path("llama-server")
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
}
