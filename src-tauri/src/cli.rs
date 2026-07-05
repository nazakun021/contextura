// src-tauri/src/cli.rs

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::models::{ModelManifest, ModelStatus};
use crate::ocr::OcrEngine;
use crate::path_resolver::{
    find_available_local_port, resolve_binary_path, resolve_llama_server_path,
};
use crate::settings::Settings;
use crate::translation::TranslationClient;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
pub struct CliArgs {
    /// Headless mode, JSON to stdout
    #[arg(long)]
    pub debug_cli: bool,

    /// Pretty-printed JSON output in debug-cli mode
    #[arg(long, requires = "debug_cli")]
    pub pretty: bool,

    /// Trigger exactly one OCR cycle then exit
    #[arg(long, requires = "debug_cli")]
    pub once: bool,

    /// PNG input for debug-cli OCR/translation runs
    #[arg(long, value_name = "PNG", requires = "debug_cli")]
    pub input: Option<PathBuf>,

    /// Run E2E test suite against directory of PNGs + expected JSON
    #[arg(long, value_name = "DIR", requires = "debug_cli")]
    pub test_suite: Option<PathBuf>,

    /// Print manifest table and exit
    #[arg(long)]
    pub list_models: bool,

    /// Interactive model cleanup wizard
    #[arg(long)]
    pub prune_models: bool,
}

impl CliArgs {
    pub fn is_cli_mode(&self) -> bool {
        self.debug_cli || self.list_models || self.prune_models
    }
}

#[derive(Serialize)]
struct CliDebugOutput {
    input: String,
    ocr: Vec<String>,
    translations: Vec<String>,
}

#[derive(Deserialize)]
struct CorpusExpectation {
    #[serde(default)]
    ocr_must_contain: Vec<String>,
    #[serde(default)]
    translation_must_contain: Vec<String>,
}

fn resolve_active_model_for_cli() -> anyhow::Result<(Settings, ModelStatus)> {
    let app_dir = Settings::dir()?;
    let settings = Settings::load(&app_dir)?;
    let model = crate::models::active_model_status(&app_dir, &settings)?;
    Ok((settings, model))
}

fn spawn_cli_sidecar(model_path: &Path, port: u16) -> anyhow::Result<std::process::Child> {
    use std::process::{Command, Stdio};

    let llama_path = resolve_llama_server_path()?;
    let binaries_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");

    let child = Command::new(&llama_path)
        .env("DYLD_FALLBACK_LIBRARY_PATH", binaries_dir)
        .arg("--model")
        .arg(model_path)
        .arg("--port")
        .arg(port.to_string())
        .arg("--n-gpu-layers")
        .arg("99")
        .arg("--ctx-size")
        .arg("1024")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--jinja")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    Ok(child)
}

async fn run_debug_cli_once(args: &CliArgs, input: &Path) -> anyhow::Result<()> {
    let (settings, active_model) = resolve_active_model_for_cli()?;
    if !active_model.installed {
        anyhow::bail!(
            "Active model {} is missing at {}",
            active_model.entry.display_label(),
            active_model.path.display()
        );
    }

    let sidecar_port = find_available_local_port()?;
    let mut sidecar = spawn_cli_sidecar(&active_model.path, sidecar_port)?;
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client = TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_mode_for_cli(&active_model.entry.id, active_model.entry.strategy.as_deref());
    translation_client.wait_for_ready().await?;

    let (width, height) = image::image_dimensions(input)?;
    #[allow(clippy::cast_precision_loss)]
    let ocr_results = ocr_engine.recognize(input, width as f32, height as f32, 1.0)?;
    let texts = ocr_results
        .iter()
        .map(|result| result.text.clone())
        .collect::<Vec<_>>();
    let translations = translation_client.translate_batch(&texts).await?;
    let output = CliDebugOutput {
        input: input.display().to_string(),
        ocr: texts,
        translations,
    };
    let json = if args.pretty {
        serde_json::to_string_pretty(&output)?
    } else {
        serde_json::to_string(&output)?
    };
    println!("{json}");
    let _ = sidecar.kill();
    let _ = sidecar.wait();

    Ok(())
}

async fn run_test_suite(dir: &Path) -> anyhow::Result<()> {
    struct SidecarGuard(std::process::Child);
    impl Drop for SidecarGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut entries = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    entries.sort();

    if entries.is_empty() {
        anyhow::bail!("No PNG files were found in {}", dir.display());
    }

    let (settings, active_model) = resolve_active_model_for_cli()?;
    if !active_model.installed {
        anyhow::bail!(
            "Active model {} is missing at {}",
            active_model.entry.display_label(),
            active_model.path.display()
        );
    }

    let sidecar_port = find_available_local_port()?;
    let sidecar = SidecarGuard(spawn_cli_sidecar(&active_model.path, sidecar_port)?);
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client = TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_mode_for_cli(&active_model.entry.id, active_model.entry.strategy.as_deref());
    translation_client.wait_for_ready().await?;

    let mut failed = false;
    for png in entries {
        let expected_path = png.with_extension("expected.json");
        let expected =
            serde_json::from_str::<CorpusExpectation>(&std::fs::read_to_string(&expected_path)?)?;
        let (width, height) = image::image_dimensions(&png)?;
        #[allow(clippy::cast_precision_loss)]
        let ocr_results = ocr_engine.recognize(&png, width as f32, height as f32, 1.0)?;
        let ocr_text = ocr_results
            .iter()
            .map(|result| result.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let translations = translation_client
            .translate_batch(
                &ocr_results
                    .iter()
                    .map(|result| result.text.clone())
                    .collect::<Vec<_>>(),
            )
            .await?;
        let translation_text = translations.join("\n");

        let ocr_ok = expected
            .ocr_must_contain
            .iter()
            .all(|fragment| ocr_text.contains(fragment));
        let translation_ok = expected.translation_must_contain.iter().all(|fragment| {
            translation_text
                .to_ascii_lowercase()
                .contains(&fragment.to_ascii_lowercase())
        });
        let passed = ocr_ok && translation_ok;
        failed |= !passed;

        println!(
            "[{}] {}",
            if passed { "PASS" } else { "FAIL" },
            png.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
        );
        if !passed {
            println!("  OCR: {ocr_text}");
            println!("  Translation: {translation_text}");
        }
    }

    // Guard will automatically kill the sidecar when dropped.
    drop(sidecar);

    if failed {
        anyhow::bail!("One or more corpus checks failed");
    }

    Ok(())
}

pub fn run_cli(args: &CliArgs) {
    if args.list_models {
        match resolve_active_model_for_cli() {
            Ok((settings, active_model)) => {
                let app_dir = Settings::dir().expect("app dir should resolve");
                let manifest =
                    ModelManifest::load(&app_dir, &settings).expect("manifest should load");
                println!("Models:");
                for status in manifest.statuses(&app_dir) {
                    println!(
                        "  {}  {:<10}  {:<9}  {}",
                        if status.entry.active { "*" } else { " " },
                        status.entry.tier,
                        if status.installed {
                            "installed"
                        } else {
                            "missing"
                        },
                        status.entry.display_label()
                    );
                }
                println!("Active: {}", active_model.entry.display_label());
            }
            Err(error) => {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.prune_models {
        println!("Scanning for unused models...");
        println!("No automated pruning policy is configured yet.");
        return;
    }

    let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime should initialize for CLI");

    if let Some(dir) = args.test_suite.as_deref() {
        match runtime.block_on(run_test_suite(dir)) {
            Ok(()) => println!("All corpus checks passed."),
            Err(error) => {
                eprintln!("Test suite failed: {error:?}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.debug_cli {
        let Some(input) = args.input.as_deref() else {
            eprintln!("debug-cli requires --input <PNG> for a real OCR/translation run");
            std::process::exit(1);
        };

        match runtime.block_on(run_debug_cli_once(args, input)) {
            Ok(()) => {}
            Err(error) => {
                eprintln!("debug-cli failed: {error}");
                std::process::exit(1);
            }
        }
    }
}
