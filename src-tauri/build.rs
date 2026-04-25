use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        println!("cargo:rerun-if-changed=src/bin/vision-helper.swift");
        compile_vision_helper();
    }

    tauri_build::build();
}

fn compile_vision_helper() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|error| panic!("CARGO_MANIFEST_DIR was not set: {error}")),
    );
    let helper_source = manifest_dir.join("src").join("bin").join("vision-helper.swift");
    let binaries_dir = manifest_dir.join("binaries");
    let helper_binary = binaries_dir.join("vision-helper-aarch64-apple-darwin");
    let helper_alias = binaries_dir.join("vision-helper");
    let out_dir = PathBuf::from(
        std::env::var("OUT_DIR").unwrap_or_else(|error| panic!("OUT_DIR was not set: {error}")),
    );
    let swift_module_cache = out_dir.join("swift-module-cache");
    let built_helper = out_dir.join("vision-helper-aarch64-apple-darwin");

    std::fs::create_dir_all(&binaries_dir)
        .unwrap_or_else(|error| panic!("Failed to create binaries directory: {error}"));
    std::fs::create_dir_all(&swift_module_cache)
        .unwrap_or_else(|error| panic!("Failed to create Swift module cache directory: {error}"));

    let output = Command::new("xcrun")
        .arg("swiftc")
        .arg("-parse-as-library")
        .arg(path_arg(&helper_source))
        .arg("-framework")
        .arg("Vision")
        .arg("-framework")
        .arg("AppKit")
        .arg("-o")
        .arg(path_arg(&built_helper))
        .env("SWIFT_MODULE_CACHE_PATH", path_arg(&swift_module_cache))
        .env("CLANG_MODULE_CACHE_PATH", path_arg(&swift_module_cache))
        .output()
        .unwrap_or_else(|error| panic!("Failed to invoke xcrun swiftc for vision-helper: {error}"));

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "Failed to compile vision-helper (status {}).\nstdout:\n{}\nstderr:\n{}",
            output.status, stdout, stderr
        );
    }

    sync_if_changed(&built_helper, &helper_binary);
    sync_if_changed(&built_helper, &helper_alias);
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn sync_if_changed(source: &Path, destination: &Path) {
    let source_bytes = std::fs::read(source)
        .unwrap_or_else(|error| panic!("Failed to read compiled vision-helper: {error}"));

    let destination_matches = std::fs::read(destination)
        .map(|existing| existing == source_bytes)
        .unwrap_or(false);

    if destination_matches {
        return;
    }

    std::fs::write(destination, source_bytes)
        .unwrap_or_else(|error| panic!("Failed to update {}: {error}", destination.display()));
}
