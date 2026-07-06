// src-tauri/src/cli.rs

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::models::{ModelManifest, ModelStatus};
use crate::ocr::OcrEngine;
use crate::path_resolver::{
    find_available_local_port, resolve_binary_path,
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
pub(crate) struct ExpectedOcrBox {
    text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Deserialize)]
pub(crate) struct CorpusExpectation {
    #[serde(default)]
    pub(crate) ocr_must_contain: Vec<String>,
    #[serde(default)]
    pub(crate) translation_must_contain: Vec<String>,
    /// Optional per-box coordinate assertions with ±5px tolerance.
    #[serde(default)]
    pub(crate) ocr_boxes: Vec<ExpectedOcrBox>,
}

/// Result of evaluating one corpus test case against its fixture.
#[derive(Debug, PartialEq)]
pub(crate) struct CaseResult {
    /// All `ocr_must_contain` fragments were found in the joined OCR text.
    pub ocr_text_ok: bool,
    /// All expected bounding boxes matched a detected box within ±tolerance.
    pub coord_ok: bool,
    /// All `translation_must_contain` fragments were found (case-insensitive) in the joined translation text.
    pub translation_ok: bool,
}

impl CaseResult {
    /// Returns `true` only when every assertion passed.
    pub fn passed(&self) -> bool {
        self.ocr_text_ok && self.coord_ok && self.translation_ok
    }
}

/// Evaluates a corpus test case against its fixture purely from text/box data.
/// This is the core assertion seam; it is independent of IO and process management.
pub(crate) fn evaluate_corpus_case(
    ocr_text: &str,
    detected_boxes: &[crate::ocr::OcrResult],
    translation_text: &str,
    expectation: &CorpusExpectation,
) -> CaseResult {
    let ocr_text_ok = expectation
        .ocr_must_contain
        .iter()
        .all(|fragment| ocr_text.contains(fragment.as_str()));
    let coord_ok = ocr_boxes_match(detected_boxes, &expectation.ocr_boxes, OCR_COORD_TOLERANCE);
    let translation_ok = expectation.translation_must_contain.iter().all(|fragment| {
        translation_text
            .to_ascii_lowercase()
            .contains(&fragment.to_ascii_lowercase())
    });
    CaseResult {
        ocr_text_ok,
        coord_ok,
        translation_ok,
    }
}

const OCR_COORD_TOLERANCE: f32 = 5.0;

/// Returns true when every expected OCR box finds a matching detected box
/// whose text is a substring match and whose bounding coordinates are all
/// within `tolerance` pixels of the expected values.
pub fn ocr_boxes_match(
    detected: &[crate::ocr::OcrResult],
    expected: &[ExpectedOcrBox],
    tolerance: f32,
) -> bool {
    expected.iter().all(|exp| {
        detected.iter().any(|det| {
            det.text.contains(&exp.text)
                && (det.bounding_box.x - exp.x).abs() <= tolerance
                && (det.bounding_box.y - exp.y).abs() <= tolerance
                && (det.bounding_box.width - exp.width).abs() <= tolerance
                && (det.bounding_box.height - exp.height).abs() <= tolerance
        })
    })
}

fn resolve_active_model_for_cli() -> anyhow::Result<(Settings, ModelStatus)> {
    let app_dir = Settings::dir()?;
    let settings = Settings::load(&app_dir)?;
    let model = crate::models::active_model_status(&app_dir, &settings)?;
    Ok((settings, model))
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
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client = TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_headless(&active_model.path, &active_model.entry.id, active_model.entry.strategy.as_deref())?;
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
    
    translation_client.shutdown_sidecar();

    Ok(())
}

async fn run_test_suite(dir: &Path) -> anyhow::Result<()> {
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
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client = TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_headless(&active_model.path, &active_model.entry.id, active_model.entry.strategy.as_deref())?;
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

        let result = evaluate_corpus_case(&ocr_text, &ocr_results, &translation_text, &expected);
        failed |= !result.passed();

        println!(
            "[{}] {}",
            if result.passed() { "PASS" } else { "FAIL" },
            png.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>")
        );
        if !result.passed() {
            if !result.ocr_text_ok {
                println!("  OCR text mismatch. Got: {ocr_text}");
            }
            if !result.coord_ok {
                println!("  OCR coordinate mismatch (tolerance ±{OCR_COORD_TOLERANCE}px).");
            }
            if !result.translation_ok {
                println!("  Translation mismatch. Got: {translation_text}");
            }
        }
    }

    translation_client.shutdown_sidecar();

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

#[cfg(test)]
mod tests {
    use super::{
        evaluate_corpus_case, ocr_boxes_match, CaseResult, CorpusExpectation, ExpectedOcrBox,
        OCR_COORD_TOLERANCE,
    };
    use crate::ocr::{OcrResult, Rect};

    fn make_result(text: &str, x: f32, y: f32, width: f32, height: f32) -> OcrResult {
        OcrResult {
            text: text.to_string(),
            confidence: 0.95,
            bounding_box: Rect::new(x, y, width, height),
            text_angle: 0.0,
            is_vertical: false,
            is_furigana: false,
        }
    }

    fn make_expected(text: &str, x: f32, y: f32, width: f32, height: f32) -> ExpectedOcrBox {
        ExpectedOcrBox {
            text: text.to_string(),
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn ocr_boxes_match_exact_coordinates_passes() {
        let detected = vec![make_result("日本語", 10.0, 20.0, 100.0, 40.0)];
        let expected = vec![make_expected("日本語", 10.0, 20.0, 100.0, 40.0)];
        assert!(ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_within_tolerance_passes() {
        let detected = vec![make_result("テスト", 12.0, 18.0, 103.0, 37.0)];
        // 2px off on each coord — well inside ±5px
        let expected = vec![make_expected("テスト", 10.0, 20.0, 100.0, 40.0)];
        assert!(ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_at_exact_tolerance_boundary_passes() {
        let detected = vec![make_result("境界", 15.0, 25.0, 105.0, 45.0)];
        // Exactly 5px off on every coordinate — should pass (tolerance is inclusive)
        let expected = vec![make_expected("境界", 10.0, 20.0, 100.0, 40.0)];
        assert!(ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_beyond_tolerance_fails() {
        let detected = vec![make_result("違反", 20.0, 20.0, 100.0, 40.0)];
        // x is 10px off — exceeds ±5px
        let expected = vec![make_expected("違反", 10.0, 20.0, 100.0, 40.0)];
        assert!(!ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_text_mismatch_fails() {
        let detected = vec![make_result("猫", 10.0, 20.0, 100.0, 40.0)];
        let expected = vec![make_expected("犬", 10.0, 20.0, 100.0, 40.0)];
        assert!(!ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_empty_expected_always_passes() {
        let detected = vec![make_result("何でも", 10.0, 20.0, 100.0, 40.0)];
        assert!(ocr_boxes_match(&detected, &[], OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_partial_text_substring_passes() {
        // detected text contains the expected fragment
        let detected = vec![make_result("東京都渋谷区", 5.0, 5.0, 200.0, 30.0)];
        let expected = vec![make_expected("渋谷", 5.0, 5.0, 200.0, 30.0)];
        assert!(ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    #[test]
    fn ocr_boxes_match_multiple_boxes_all_must_match() {
        let detected = vec![
            make_result("日本語", 10.0, 10.0, 100.0, 30.0),
            make_result("テスト", 10.0, 50.0, 100.0, 30.0),
        ];
        let expected = vec![
            make_expected("日本語", 10.0, 10.0, 100.0, 30.0),
            make_expected("テスト", 10.0, 50.0, 100.0, 30.0),
        ];
        assert!(ocr_boxes_match(&detected, &expected, OCR_COORD_TOLERANCE));
    }

    // ── CorpusExpectation deserialization ────────────────────────────────────

    #[test]
    fn corpus_expectation_deserializes_full_fixture() {
        let json = r#"{
            "description": "test",
            "ocr_must_contain": ["日本語", "テスト"],
            "translation_must_contain": ["japanese"],
            "ocr_boxes": [
                {"text": "日本語", "x": 10.0, "y": 20.0, "width": 100.0, "height": 40.0}
            ]
        }"#;
        let exp: CorpusExpectation = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(exp.ocr_must_contain, ["日本語", "テスト"]);
        assert_eq!(exp.translation_must_contain, ["japanese"]);
        assert_eq!(exp.ocr_boxes.len(), 1);
        assert_eq!(exp.ocr_boxes[0].text, "日本語");
        assert!((exp.ocr_boxes[0].x - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn corpus_expectation_defaults_all_fields_when_absent() {
        let json = r"{}";
        let exp: CorpusExpectation = serde_json::from_str(json).expect("should deserialize empty");
        assert!(exp.ocr_must_contain.is_empty());
        assert!(exp.translation_must_contain.is_empty());
        assert!(exp.ocr_boxes.is_empty());
    }

    // ── evaluate_corpus_case ─────────────────────────────────────────────────

    #[test]
    fn evaluate_corpus_case_all_empty_expectations_passes() {
        // A clean-pass fixture: no assertions → always passes regardless of output.
        let exp = CorpusExpectation {
            ocr_must_contain: vec![],
            translation_must_contain: vec![],
            ocr_boxes: vec![],
        };
        let result = evaluate_corpus_case("", &[], "", &exp);
        assert_eq!(result, CaseResult { ocr_text_ok: true, coord_ok: true, translation_ok: true });
        assert!(result.passed());
    }

    #[test]
    fn evaluate_corpus_case_ocr_fragment_present_passes() {
        let exp = CorpusExpectation {
            ocr_must_contain: vec!["勇者".to_string()],
            translation_must_contain: vec![],
            ocr_boxes: vec![],
        };
        let result = evaluate_corpus_case("勇者よ、魔王を倒せ！", &[], "", &exp);
        assert!(result.ocr_text_ok);
        assert!(result.passed());
    }

    #[test]
    fn evaluate_corpus_case_ocr_fragment_missing_fails() {
        let exp = CorpusExpectation {
            ocr_must_contain: vec!["不在テキスト".to_string()],
            translation_must_contain: vec![],
            ocr_boxes: vec![],
        };
        let result = evaluate_corpus_case("勇者よ、魔王を倒せ！", &[], "", &exp);
        assert!(!result.ocr_text_ok);
        assert!(!result.passed());
    }

    #[test]
    fn evaluate_corpus_case_translation_fragment_case_insensitive_passes() {
        let exp = CorpusExpectation {
            ocr_must_contain: vec![],
            translation_must_contain: vec!["Hero".to_string()],
            ocr_boxes: vec![],
        };
        // Translation output is lowercase — fragment matching must be case-insensitive
        let result = evaluate_corpus_case("", &[], "defeat the hero and the demon king", &exp);
        assert!(result.translation_ok);
        assert!(result.passed());
    }

    #[test]
    fn evaluate_corpus_case_translation_fragment_missing_fails() {
        let exp = CorpusExpectation {
            ocr_must_contain: vec![],
            translation_must_contain: vec!["dragon".to_string()],
            ocr_boxes: vec![],
        };
        let result = evaluate_corpus_case("", &[], "defeat the demon king", &exp);
        assert!(!result.translation_ok);
        assert!(!result.passed());
    }

    #[test]
    fn evaluate_corpus_case_coord_mismatch_fails_but_text_passes() {
        // OCR text is fine but a box coordinate is out of tolerance → overall fail
        let exp = CorpusExpectation {
            ocr_must_contain: vec!["魔王".to_string()],
            translation_must_contain: vec![],
            ocr_boxes: vec![make_expected("魔王", 10.0, 20.0, 100.0, 40.0)],
        };
        // Detected box is way off (50px x deviation)
        let detected = vec![make_result("魔王", 60.0, 20.0, 100.0, 40.0)];
        let result = evaluate_corpus_case("魔王を倒せ！", &detected, "", &exp);
        assert!(result.ocr_text_ok, "ocr text should pass");
        assert!(!result.coord_ok, "coord should fail");
        assert!(!result.passed());
    }

    #[test]
    fn evaluate_corpus_case_all_checks_pass_together() {
        let exp = CorpusExpectation {
            ocr_must_contain: vec!["勇者".to_string()],
            translation_must_contain: vec!["brave".to_string()],
            ocr_boxes: vec![make_expected("勇者", 10.0, 20.0, 100.0, 40.0)],
        };
        let detected = vec![make_result("勇者よ", 10.0, 20.0, 100.0, 40.0)];
        let result = evaluate_corpus_case("勇者よ、魔王を倒せ！", &detected, "defeat the brave hero", &exp);
        assert_eq!(result, CaseResult { ocr_text_ok: true, coord_ok: true, translation_ok: true });
        assert!(result.passed());
    }
}
