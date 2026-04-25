mod cli;
mod downloader;
mod models;
mod settings;

mod capture;
mod context;
mod ipc;
mod motion;
mod ocr;
mod styling;
mod thermal;
mod translation;

mod hotkeys;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use clap::Parser;
use cli::CliArgs;
use crossbeam_channel::Sender;
use image::{ImageBuffer, RgbaImage};
use rayon::prelude::*;
use tauri::Emitter;

use crate::ipc::{
    TranslationBox, TranslationErrorPayload, TranslationPayload, TranslationStartedPayload,
    WizardStatusPayload,
};
use crate::models::ModelManifest;
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::styling::StylingEngine;

#[derive(Debug, Clone)]
pub enum PipelineCommand {
    ForceScan,
    ReloadRuntime { reason: String },
}

fn resolve_binary_path(binary_name: &str) -> anyhow::Result<PathBuf> {
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

fn resolve_vision_helper_path(app: &tauri::App) -> anyhow::Result<PathBuf> {
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

fn resolve_llama_server_path() -> anyhow::Result<PathBuf> {
    resolve_binary_path("llama-server")
}

/// Encodes a BGRA `CaptureFrame` pixel buffer to a temporary PNG file.
///
/// `ScreenCaptureKit` delivers frames in BGRA order. The `image` crate expects
/// RGBA, so we swap the B and R channels in-place before encoding.
fn save_frame_as_png(frame: &capture::CaptureFrame, frame_id: u64) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/contextura-frame-{frame_id}.png"));
    let latest_path = PathBuf::from("/tmp/contextura-frame-latest.png");

    // BGRA → RGBA: swap index 0 (B) with index 2 (R) for each pixel
    let mut rgba_data = frame.buffer.data.clone();
    for pixel in rgba_data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let img: RgbaImage = ImageBuffer::from_raw(
        u32::try_from(frame.buffer.width)?,
        u32::try_from(frame.buffer.height)?,
        rgba_data,
    )
    .ok_or_else(|| anyhow::anyhow!("Failed to construct image buffer from frame data"))?;

    img.save(&path)?;
    // Keep the latest captured frame on disk for manual inspection/debugging.
    let _ = img.save(&latest_path);

    Ok(path)
}

pub fn emit_runtime_notice<S1: Into<String>, S2: Into<String>, S3: Into<String>>(
    app: &tauri::AppHandle,
    title: S1,
    message: S2,
    detail: S3,
    level: &str,
    dismiss_ms: u64,
) {
    let message = message.into();
    let _ = app.emit(
        "translation-error",
        TranslationErrorPayload {
            title: title.into(),
            detail: detail.into(),
            level: level.to_string(),
            dismiss_ms,
            message,
        },
    );
}

fn open_models_folder() -> Result<(), String> {
    let app_dir = settings::Settings::dir().map_err(|e| e.to_string())?;
    let models_dir = models::models_dir(&app_dir);
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&models_dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn open_screen_recording_settings_impl() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_wizard_status() -> Result<WizardStatusPayload, String> {
    let app_dir = settings::Settings::dir().map_err(|e| e.to_string())?;
    let settings = settings::Settings::load(&app_dir).map_err(|e| e.to_string())?;
    let manifest = ModelManifest::load(&app_dir, &settings).map_err(|e| e.to_string())?;
    let active = manifest.active_status(&app_dir);

    Ok(WizardStatusPayload {
        has_model: active.as_ref().is_some_and(|status| status.installed),
        active_model_label: active.as_ref().map_or_else(
            || "No model detected".to_string(),
            |status| status.entry.display_label().to_string(),
        ),
        active_model_tier: active
            .as_ref()
            .map_or_else(String::new, |status| status.entry.tier.clone()),
        models_dir: models::models_dir(&app_dir).display().to_string(),
    })
}

pub fn request_model_switch(
    app: &tauri::AppHandle,
    pipeline_tx: &Sender<PipelineCommand>,
) -> anyhow::Result<()> {
    let app_dir = settings::Settings::dir()?;
    let mut settings = settings::Settings::load(&app_dir)?;
    let switch = models::cycle_active_model(&app_dir, &mut settings)?;
    emit_runtime_notice(
        app,
        "Model Switched",
        format!("Now using {}", switch.current.entry.display_label()),
        format!(
            "{} -> {}",
            switch.previous.entry.display_label(),
            switch.current.entry.display_label()
        ),
        "info",
        5000,
    );
    let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
        reason: format!("Switched to {}", switch.current.entry.display_label()),
    });
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn complete_wizard(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    use tauri::Manager;
    let app_dir = settings::Settings::dir().map_err(|e| e.to_string())?;
    let mut settings = settings::Settings::load(&app_dir).map_err(|e| e.to_string())?;
    settings.wizard_completed = true;
    settings.save(&app_dir).map_err(|e| e.to_string())?;

    if let Some(overlay) = app.get_webview_window("overlay-main") {
        let _ = overlay.show();
    }

    window.close().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn wizard_status() -> Result<WizardStatusPayload, String> {
    load_wizard_status()
}

#[tauri::command]
fn open_models_folder_command() -> Result<(), String> {
    open_models_folder()
}

#[tauri::command]
fn open_screen_recording_settings() -> Result<(), String> {
    open_screen_recording_settings_impl()
}

#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
pub fn run() {
    // Initialize logging so log::info!/error! actually emit output.
    env_logger::init();
    let _sentry = std::env::var("CONTEXTURA_SENTRY_DSN").ok().map(|dsn| {
        log::info!("[Sentry] Crash reporting enabled via CONTEXTURA_SENTRY_DSN");
        sentry::init(dsn)
    });

    let args = CliArgs::parse();

    if args.is_cli_mode() {
        run_cli(&args);
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            complete_wizard,
            wizard_status,
            open_models_folder_command,
            open_screen_recording_settings
        ])
        .setup(|app| {
            use tauri::Manager;

            let app_dir = settings::Settings::dir().expect("Failed to get app directory");
            let settings = settings::Settings::load(&app_dir).expect("Failed to load settings at startup");
            let vision_helper_path =
                resolve_vision_helper_path(app).expect("Failed to resolve vision-helper path");
            let app_bundle_id = app.config().identifier.clone();

            // --- Subsystem Initialization ---
            let (window_tracker, invalidation_rx) = context::AppWindowTracker::new();
            let mut thermal_monitor = thermal::ThermalMonitor::new();
            let ocr_engine = Arc::new(ocr::OcrEngine::new(
                settings.furigana_suppression,
                vision_helper_path,
            ));
            let mut display_manager = capture::DisplayManager::new();
            let (pipeline_tx, pipeline_rx) = crossbeam_channel::bounded(16);

            // Register Hotkeys
            hotkeys::register_shortcuts(app, window_tracker.clone(), pipeline_tx.clone())
                .expect("Failed to register shortcuts");

            // Make the overlay window background truly transparent on macOS and enable click-through.
            if let Some(overlay) = app.get_webview_window("overlay-main") {
                let _ = overlay.set_ignore_cursor_events(true);
                let _ = overlay.with_webview(|wv| {
                    #[cfg(target_os = "macos")]
                    {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        use objc2_foundation::{NSNumber, NSString};
                        unsafe {
                            let webview_obj: *mut AnyObject = wv.inner().cast();
                            let value = NSNumber::new_bool(false);
                            let key = NSString::from_str("drawsBackground");
                            let _: () = msg_send![webview_obj, setValue: &*value, forKey: &*key];
                        }
                    }
                });

                // Only show overlay if wizard is completed
                if settings.wizard_completed {
                    let _ = overlay.show();
                }
            }

            // Show wizard if not completed
            if !settings.wizard_completed {
                if let Some(wizard) = app.get_webview_window("wizard") {
                    let _ = wizard.show();
                } else {
                    // Fallback: if no wizard window defined in tauri.conf.json, create it
                    let _ = tauri::WebviewWindowBuilder::new(
                        app,
                        "wizard",
                        tauri::WebviewUrl::App("wizard.html".into()),
                    )
                    .title("Contextura Setup")
                    .inner_size(500.0, 400.0)
                    .build();
                }
            }

            // Initialize Tray
            tray::setup_tray(app, pipeline_tx, window_tracker.clone())
                .expect("Failed to setup tray");

            // --- Panic Hook (Cleanup Temp Files) ---
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg("rm -f /tmp/contextura-frame-*.png")
                    .output();
                default_hook(panic_info);
            }));

            // --- Pipeline Orchestration ---
            let app_handle = app.handle().clone();
            let app_bundle_id_sidecar = app_bundle_id;
            let initial_memory_size = settings.context_memory_size;

            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Tokio runtime should initialize");
                let client = Arc::new(AsyncMutex::new(translation::TranslationClient::new(
                    initial_memory_size,
                    8765,
                )));
                let client_clone = Arc::clone(&client);
                let app_handle_sidecar = app_handle.clone();
                let active_model_path = Arc::new(AsyncMutex::new(PathBuf::new()));

                // Start Window Tracker on its own thread
                let mut window_tracker_task = window_tracker;
                thread::spawn(move || {
                    window_tracker_task.start_polling();
                });

                rt.block_on(async move {
                    let watchdog_client = Arc::clone(&client);
                    let watchdog_app_handle = app_handle_sidecar.clone();
                    let watchdog_model_path = Arc::clone(&active_model_path);
                    tokio::spawn(async move {
                        let mut consecutive_failures = 0u8;
                        loop {
                            sleep(Duration::from_secs(5)).await;
                            let model_path = watchdog_model_path.lock().await.clone();
                            if model_path.as_os_str().is_empty() {
                                continue;
                            }

                            let mut guard = watchdog_client.lock().await;
                            if guard.wait_for_ready().await.is_err() {
                                consecutive_failures += 1;
                                log::warn!(
                                    "[Watchdog] Sidecar health check failed ({consecutive_failures}/3)"
                                );
                                if consecutive_failures >= 3 {
                                    emit_runtime_notice(
                                        &watchdog_app_handle,
                                        "Translation Engine Restarted",
                                        "The local translation engine became unresponsive and was restarted.",
                                        model_path.display().to_string(),
                                        "warning",
                                        5000,
                                    );
                                    let _ = guard.start_sidecar(&watchdog_app_handle, &model_path);
                                    consecutive_failures = 0;
                                }
                            } else {
                                consecutive_failures = 0;
                            }
                        }
                    });

                    let mut failure_count = 0u32;
                    let mut sidecar_started = false;
                    let mut warned_missing_model = false;
                    let mut active_model_id = String::new();
                    let mut frame_id: u64 = 0;

                    loop {
                        let app_dir = settings::Settings::dir().expect("Failed to get app directory");
                        let runtime_settings =
                            settings::Settings::load(&app_dir).expect("Failed to load settings");
                        let active_model = match models::active_model_status(&app_dir, &runtime_settings)
                        {
                            Ok(model) => model,
                            Err(error) => {
                                if !warned_missing_model {
                                    emit_runtime_notice(
                                        &app_handle_sidecar,
                                        "No Model Available",
                                        "Add a GGUF model to the Contextura models folder to enable translation.",
                                        error.to_string(),
                                        "warning",
                                        6000,
                                    );
                                    warned_missing_model = true;
                                }
                                sleep(Duration::from_secs(5)).await;
                                thermal_monitor.update();
                                continue;
                            }
                        };

                        *active_model_path.lock().await = active_model.path.clone();

                        if !active_model.installed {
                            if !warned_missing_model {
                                emit_runtime_notice(
                                    &app_handle_sidecar,
                                    "Model Missing",
                                    format!(
                                        "The active model {} is not installed.",
                                        active_model.entry.display_label()
                                    ),
                                    active_model.path.display().to_string(),
                                    "warning",
                                    6000,
                                );
                                warned_missing_model = true;
                            }
                            sleep(Duration::from_secs(5)).await;
                            thermal_monitor.update();
                            continue;
                        }
                        warned_missing_model = false;

                        if active_model_id != active_model.entry.id {
                            sidecar_started = false;
                            active_model_id = active_model.entry.id.clone();
                        }

                        if !sidecar_started {
                            match client_clone
                                .lock()
                                .await
                                .start_sidecar(&app_handle_sidecar, &active_model.path)
                            {
                                Ok(()) => {
                                    log::info!(
                                        "[Pipeline] Sidecar started with model {}",
                                        active_model.path.display()
                                    );
                                    sidecar_started = true;
                                }
                                Err(error) => {
                                    log::error!("[Pipeline] Failed to start sidecar: {error}");
                                    emit_runtime_notice(
                                        &app_handle_sidecar,
                                        "Translation Engine Failed",
                                        "Contextura could not start the local translation sidecar.",
                                        error.to_string(),
                                        "error",
                                        6000,
                                    );
                                    sleep(Duration::from_secs(5)).await;
                                    continue;
                                }
                            }
                        }

                        match client_clone.lock().await.wait_for_ready().await {
                            Ok(()) => {
                                failure_count = 0;
                                log::info!("[Pipeline] Translation sidecar is ready");
                            }
                            Err(error) => {
                                failure_count += 1;
                                log::error!(
                                    "[Pipeline] Sidecar not ready (attempt {failure_count}): {error}"
                                );
                                if failure_count > 30 {
                                    emit_runtime_notice(
                                        &app_handle_sidecar,
                                        "Translation Engine Unavailable",
                                        "The sidecar never became ready. Restart the app after verifying the model file.",
                                        error.to_string(),
                                        "error",
                                        8000,
                                    );
                                    break;
                                }
                                sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                        }

                        let ocr_engine_loop = Arc::clone(&ocr_engine);
                        let frame_rx =
                            display_manager.start_capture(0, &[app_bundle_id_sidecar.as_str()]);
                        let mut motion_detector = MotionDetector::new(
                            runtime_settings.pixel_diff_threshold,
                            runtime_settings.edge_inset_percent,
                        );
                        let mut debounce = DebounceStateMachine::new(
                            runtime_settings.debounce_ms,
                            runtime_settings.motion_threshold,
                        );
                        let mut pending_force_scan = false;
                        let mut last_frame_at = Instant::now();

                        log::info!("[Pipeline] Entering capture loop");

                        'capture: loop {
                            sleep(Duration::from_millis(10)).await;

                            while let Ok(command) = pipeline_rx.try_recv() {
                                match command {
                                    PipelineCommand::ForceScan => pending_force_scan = true,
                                    PipelineCommand::ReloadRuntime { reason } => {
                                        log::info!("[Pipeline] Reload requested: {reason}");
                                        sidecar_started = false;
                                        break 'capture;
                                    }
                                }
                            }

                            let Ok(frame) = frame_rx.try_recv() else {
                                if last_frame_at.elapsed() > Duration::from_secs(10) {
                                    emit_runtime_notice(
                                        &app_handle_sidecar,
                                        "Capture Restarting",
                                        "Screen capture stalled, so Contextura is restarting the capture stream.",
                                        "This usually happens after display sleep, wake, or a capture permission reset."
                                            .to_string(),
                                        "warning",
                                        5000,
                                    );
                                    break 'capture;
                                }
                                continue;
                            };
                            last_frame_at = Instant::now();

                            let is_forced = std::mem::take(&mut pending_force_scan);
                            let thumbnail = motion_detector.downsample(
                                &frame.buffer.data,
                                frame.buffer.width,
                                frame.buffer.height,
                            );
                            let motion_ratio = motion_detector.process_thumbnail(&thumbnail);
                            let debounce_event = if is_forced {
                                DebounceEvent::Triggered
                            } else {
                                debounce.update(motion_ratio)
                            };

                            match debounce_event {
                                DebounceEvent::MotionDetected => {
                                    let _ = app_handle_sidecar.emit("translation-clear", ());
                                }
                                DebounceEvent::Triggered => {
                                    let current_frame_id = frame_id;
                                    frame_id += 1;

                                    while let Ok(reason) = invalidation_rx.try_recv() {
                                        log::info!("[Context] Invalidation: {reason:?}");
                                        match reason {
                                            context::InvalidationReason::AppSwitch { from, to } => {
                                                log::info!(
                                                    "[Context] App switch: {from} -> {to} — clearing memory"
                                                );
                                                client_clone.lock().await.memory.clear();
                                                let _ = app_handle_sidecar.emit("translation-clear", ());
                                            }
                                            context::InvalidationReason::ManualReset => {
                                                client_clone.lock().await.memory.clear();
                                            }
                                        }
                                    }

                                    let png_path = match save_frame_as_png(&frame, current_frame_id) {
                                        Ok(path) => path,
                                        Err(error) => {
                                            log::error!("[OCR] PNG save failed: {error}");
                                            continue;
                                        }
                                    };

                                    let _ = app_handle_sidecar.emit(
                                        "translation-started",
                                        TranslationStartedPayload {
                                            display_id: frame.display_id,
                                        },
                                    );

                                    let ocr_results = ocr_engine_loop.recognize(
                                        &png_path,
                                        frame.buffer.width as f32,
                                        frame.buffer.height as f32,
                                        frame.scale_factor,
                                    );
                                    let _ = std::fs::remove_file(&png_path);

                                    let ocr_results = match ocr_results {
                                        Ok(results) => results,
                                        Err(error) => {
                                            log::error!("[OCR] Recognition failed: {error}");
                                            emit_runtime_notice(
                                                &app_handle_sidecar,
                                                "OCR Failed",
                                                "The Vision helper could not read the current frame.",
                                                error.to_string(),
                                                "warning",
                                                4000,
                                            );
                                            continue;
                                        }
                                    };

                                    if ocr_results.is_empty() {
                                        log::debug!("[OCR] No CJK text found in frame {current_frame_id}");
                                        continue;
                                    }

                                    let texts = ocr_results
                                        .iter()
                                        .map(|result| result.text.clone())
                                        .collect::<Vec<_>>();

                                    let translations = {
                                        let mut guard = client_clone.lock().await;
                                        guard.translate_batch(&texts).await
                                    };

                                    let translations = match translations {
                                        Ok(translations) => translations,
                                        Err(error) => {
                                            log::error!("[Translation] Batch failed: {error}");
                                            emit_runtime_notice(
                                                &app_handle_sidecar,
                                                "Translation Failed",
                                                "The local model did not return a valid translation batch.",
                                                error.to_string(),
                                                "warning",
                                                5000,
                                            );
                                            continue;
                                        }
                                    };

                                    let raw_data = &frame.buffer.data;
                                    let buf_width = frame.buffer.width;
                                    let buf_height = frame.buffer.height;
                                    let scale = frame.scale_factor;
                                    let styled_boxes = ocr_results
                                        .par_iter()
                                        .zip(translations.par_iter())
                                        .enumerate()
                                        .map(|(index, (ocr, translation))| {
                                            let bg = StylingEngine::sample_rect_ring(
                                                raw_data,
                                                buf_width,
                                                buf_height,
                                                ocr.bounding_box.x,
                                                ocr.bounding_box.y,
                                                ocr.bounding_box.width,
                                                ocr.bounding_box.height,
                                                scale,
                                            );
                                            let fg_color =
                                                StylingEngine::get_fg_color(bg.r, bg.g, bg.b);
                                            TranslationBox {
                                                id: format!("{current_frame_id}-{index}"),
                                                translated: translation.clone(),
                                                original: ocr.text.clone(),
                                                x: ocr.bounding_box.x,
                                                y: ocr.bounding_box.y,
                                                width: ocr.bounding_box.width,
                                                height: ocr.bounding_box.height,
                                                is_vertical: ocr.is_vertical,
                                                bg_color: bg.to_css_color(),
                                                fg_color: fg_color.to_string(),
                                                confidence: ocr.confidence,
                                            }
                                        })
                                        .collect::<Vec<_>>();

                                    let payload = TranslationPayload {
                                        boxes: styled_boxes,
                                        scale_factor: scale,
                                        display_id: frame.display_id,
                                        frame_id: current_frame_id,
                                    };

                                    if let Err(error) =
                                        app_handle_sidecar.emit("translation-update", &payload)
                                    {
                                        log::error!(
                                            "[IPC] Failed to emit translation-update: {error}"
                                        );
                                    }
                                }
                                DebounceEvent::None => {}
                            }

                            thermal_monitor.update();
                            if thermal_monitor.should_throttle() {
                                sleep(Duration::from_millis(500)).await;
                            }
                        }
                    }
                });
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[derive(serde::Serialize)]
struct CliDebugOutput {
    input: String,
    ocr: Vec<String>,
    translations: Vec<String>,
}

#[derive(serde::Deserialize)]
struct CorpusExpectation {
    #[serde(default)]
    ocr_must_contain: Vec<String>,
    #[serde(default)]
    translation_must_contain: Vec<String>,
}

fn resolve_active_model_for_cli() -> anyhow::Result<(settings::Settings, models::ModelStatus)> {
    let app_dir = settings::Settings::dir()?;
    let settings = settings::Settings::load(&app_dir)?;
    let model = models::active_model_status(&app_dir, &settings)?;
    Ok((settings, model))
}

fn spawn_cli_sidecar(
    model_path: &std::path::Path,
    port: u16,
) -> anyhow::Result<std::process::Child> {
    use std::process::{Command, Stdio};

    let llama_path = resolve_llama_server_path()?;
    let binaries_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");

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

async fn run_debug_cli_once(args: &CliArgs, input: &std::path::Path) -> anyhow::Result<()> {
    let (settings, active_model) = resolve_active_model_for_cli()?;
    if !active_model.installed {
        anyhow::bail!(
            "Active model {} is missing at {}",
            active_model.entry.display_label(),
            active_model.path.display()
        );
    }

    let mut sidecar = spawn_cli_sidecar(&active_model.path, 8765)?;
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = ocr::OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client =
        translation::TranslationClient::new(settings.context_memory_size, 8765);
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

async fn run_test_suite(dir: &std::path::Path) -> anyhow::Result<()> {
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

    let mut sidecar = spawn_cli_sidecar(&active_model.path, 8765)?;
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = ocr::OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client =
        translation::TranslationClient::new(settings.context_memory_size, 8765);
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

    let _ = sidecar.kill();
    let _ = sidecar.wait();

    if failed {
        anyhow::bail!("One or more corpus checks failed");
    }

    Ok(())
}

fn run_cli(args: &CliArgs) {
    if args.list_models {
        match resolve_active_model_for_cli() {
            Ok((settings, active_model)) => {
                let app_dir = settings::Settings::dir().expect("app dir should resolve");
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
