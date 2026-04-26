// src-tauri/src/lib.rs

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

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use clap::Parser;
use cli::CliArgs;
use crossbeam_channel::{Receiver, Sender};
use image::ColorType;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindow, NSWindowSharingType};
use rayon::prelude::*;
use tauri::Emitter;

use crate::ipc::{
    TranslationBox, TranslationErrorPayload, TranslationPayload, TranslationStartedPayload,
    WizardStatusPayload,
};
use crate::models::ModelManifest;
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::styling::StylingEngine;

const SETTINGS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum PipelineCommand {
    ForceScan,
    ReloadRuntime { reason: String },
    Shutdown,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    settings: settings::Settings,
    active_model: models::ModelStatus,
    loaded_at: Instant,
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

fn find_available_local_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn swap_bgra_to_rgba(buffer: &[u8]) -> Vec<u8> {
    let mut rgba_data = buffer.to_vec();
    for pixel in rgba_data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    rgba_data
}

/// Encodes an RGBA pixel buffer to a temporary PNG file.
fn save_frame_as_png(
    rgba_data: &[u8],
    width: usize,
    height: usize,
    frame_id: u64,
) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/contextura-frame-{frame_id}.png"));
    let latest_path = PathBuf::from("/tmp/contextura-frame-latest.png");

    image::save_buffer(
        &path,
        rgba_data,
        u32::try_from(width)?,
        u32::try_from(height)?,
        ColorType::Rgba8,
    )?;
    // Keep the latest captured frame on disk for manual inspection/debugging.
    let _ = image::save_buffer(
        &latest_path,
        rgba_data,
        u32::try_from(width)?,
        u32::try_from(height)?,
        ColorType::Rgba8,
    );

    Ok(path)
}

fn cleanup_stale_temp_frames() {
    let Ok(entries) = std::fs::read_dir("/tmp") else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with("contextura-frame-") && file_name.ends_with(".png") {
            let _ = std::fs::remove_file(path);
        }
    }
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

async fn drain_invalidations(
    app_handle: &tauri::AppHandle,
    client: &Arc<AsyncMutex<translation::TranslationClient>>,
    invalidation_rx: &Receiver<context::InvalidationReason>,
) {
    while let Ok(reason) = invalidation_rx.try_recv() {
        log::info!("[Context] Invalidation: {reason:?}");
        match reason {
            context::InvalidationReason::AppSwitch { from, to } => {
                log::info!("[Context] App switch: {from} -> {to} — clearing memory");
                client.lock().await.memory.clear();
                let _ = app_handle.emit("translation-clear", ());
            }

            context::InvalidationReason::ManualReset => {
                client.lock().await.memory.clear();
                emit_runtime_notice(
                    app_handle,
                    "Context Cleared",
                    "Translation memory was cleared.",
                    "New translations will start without prior context.",
                    "info",
                    2500,
                );
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn process_capture_frame(
    app_handle: &tauri::AppHandle,
    client: &Arc<AsyncMutex<translation::TranslationClient>>,
    ocr_engine: &ocr::OcrEngine,
    invalidation_rx: &Receiver<context::InvalidationReason>,
    pipeline_tx: &Sender<PipelineCommand>,
    frame: &capture::CaptureFrame,
    frame_id: u64,
) -> Option<TranslationPayload> {
    drain_invalidations(app_handle, client, invalidation_rx).await;

    let rgba_data = swap_bgra_to_rgba(&frame.buffer.data);
    let png_path = match save_frame_as_png(
        &rgba_data,
        frame.buffer.width,
        frame.buffer.height,
        frame_id,
    ) {
        Ok(path) => path,
        Err(error) => {
            log::error!("[OCR] PNG save failed: {error}");
            return None;
        }
    };

    let _ = app_handle.emit(
        "translation-started",
        TranslationStartedPayload {
            display_id: frame.display_id,
        },
    );

    if client.lock().await.quick_health_check().await.is_err() {
        log::warn!(
            "[Translation] Pre-flight health check failed — skipping frame and requesting runtime reload"
        );
        let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
            reason: "health check failed before batch".to_string(),
        });
        return None;
    }

    #[allow(clippy::cast_precision_loss)]
    let ocr_results = ocr_engine.recognize(
        &png_path,
        frame.buffer.width as f32,
        frame.buffer.height as f32,
        frame.scale_factor,
    );
    // Styling uses the in-memory RGBA buffer, so the temp OCR PNG can be deleted now while
    // /tmp/contextura-frame-latest.png remains available for post-mortem debugging.
    let _ = std::fs::remove_file(&png_path);

    let ocr_results = match ocr_results {
        Ok(results) => results,
        Err(error) => {
            log::error!("[OCR] Recognition failed: {error}");
            emit_runtime_notice(
                app_handle,
                "OCR Failed",
                "The Vision helper could not read the current frame.",
                error.to_string(),
                "warning",
                4000,
            );
            return None;
        }
    };

    if ocr_results.is_empty() {
        log::debug!("[OCR] No CJK text found in frame {frame_id}");
        return None;
    }

    let texts = ocr_results
        .iter()
        .map(|result| result.text.clone())
        .collect::<Vec<_>>();

    let translations = {
        let mut guard = client.lock().await;
        guard.translate_batch(&texts).await
    };

    let translations = match translations {
        Ok(translations) => translations,
        Err(error) => {
            log::error!("[Translation] Batch failed: {error}");
            emit_runtime_notice(
                app_handle,
                "Translation Failed",
                "The local model did not return a valid translation batch.",
                error.to_string(),
                "warning",
                5000,
            );
            return None;
        }
    };

    let raw_data = &rgba_data;
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
            let fg_color = StylingEngine::get_fg_color(bg.r, bg.g, bg.b);
            TranslationBox {
                id: format!("{frame_id}-{index}"),
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
        frame_id,
    };

    if let Err(error) = app_handle.emit("translation-update", &payload) {
        log::error!("[IPC] Failed to emit translation-update: {error}");
    }

    Some(payload)
}

fn emit_cached_translation_payload(
    app_handle: &tauri::AppHandle,
    payload: &TranslationPayload,
) -> bool {
    if let Err(error) = app_handle.emit("translation-update", payload) {
        log::error!("[IPC] Failed to emit cached translation-update: {error}");
        false
    } else {
        true
    }
}

fn load_runtime_state() -> anyhow::Result<RuntimeState> {
    let app_dir = settings::Settings::dir()?;
    let loaded_settings = settings::Settings::load(&app_dir)?;
    let active_model = models::active_model_status(&app_dir, &loaded_settings)?;
    Ok(RuntimeState {
        settings: loaded_settings,
        active_model,
        loaded_at: Instant::now(),
    })
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

    cleanup_stale_temp_frames();

    let pipeline_tx_for_exit = Arc::new(std::sync::Mutex::new(None::<Sender<PipelineCommand>>));
    let pipeline_tx_setup = Arc::clone(&pipeline_tx_for_exit);

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
        .setup(move |app| {
            use tauri::Manager;

            let app_dir = settings::Settings::dir().expect("Failed to get app directory");
            let startup_settings =
                settings::Settings::load(&app_dir).expect("Failed to load settings at startup");
            let vision_helper_path =
                resolve_vision_helper_path(app).expect("Failed to resolve vision-helper path");
            let app_bundle_id = app.config().identifier.clone();
            let app_process_id = i32::try_from(std::process::id()).unwrap_or_default();
            let app_name_hint = app.package_info().name.clone();

            // --- Subsystem Initialization ---
            let (window_tracker, invalidation_rx) = context::AppWindowTracker::new();
            let mut thermal_monitor = thermal::ThermalMonitor::new();
            let ocr_engine = Arc::new(ocr::OcrEngine::new(
                startup_settings.furigana_suppression,
                vision_helper_path,
            ));
            let mut display_manager = capture::DisplayManager::new();
            let (pipeline_tx, pipeline_rx) = crossbeam_channel::bounded(16);
            *pipeline_tx_setup.lock().expect("pipeline exit handle lock poisoned") =
                Some(pipeline_tx.clone());

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
                #[cfg(target_os = "macos")]
                if let Ok(ns_window) = overlay.ns_window() {
                    let ns_window: &NSWindow = unsafe { &*ns_window.cast() };
                    ns_window.setSharingType(NSWindowSharingType::None);
                }

                // Only show overlay if wizard is completed
                if startup_settings.wizard_completed {
                    let _ = overlay.show();
                }
            }

            // Show wizard if not completed
            if !startup_settings.wizard_completed {
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
            tray::setup_tray(app, pipeline_tx.clone(), window_tracker.clone())
                .expect("Failed to setup tray");

            // --- Panic Hook (Cleanup Temp Files) ---
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                cleanup_stale_temp_frames();
                default_hook(panic_info);
            }));

            // --- Pipeline Orchestration ---
            let app_handle = app.handle().clone();
            let app_bundle_id_sidecar = app_bundle_id;
            let initial_memory_size = startup_settings.context_memory_size;

            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Tokio runtime should initialize");
                let sidecar_port =
                    find_available_local_port().expect("localhost port should be available");
                let client = Arc::new(AsyncMutex::new(translation::TranslationClient::new(
                    initial_memory_size,
                    sidecar_port,
                )));
                let client_clone = Arc::clone(&client);
                let app_handle_sidecar = app_handle.clone();
                let active_model_path = Arc::new(AsyncMutex::new(PathBuf::new()));
                let active_model_key = Arc::new(AsyncMutex::new(String::new()));

                // Start Window Tracker on its own thread
                let mut window_tracker_task = window_tracker;
                thread::spawn(move || {
                    window_tracker_task.start_polling();
                });

                rt.block_on(async move {
                    let watchdog_client = Arc::clone(&client);
                    let watchdog_app_handle = app_handle_sidecar.clone();
                    let watchdog_model_path = Arc::clone(&active_model_path);
                    let watchdog_model_key = Arc::clone(&active_model_key);
                    tokio::spawn(async move {
                        let mut consecutive_failures = 0u8;
                        loop {
                            sleep(Duration::from_secs(5)).await;
                            let model_path = watchdog_model_path.lock().await.clone();
                            let model_key = watchdog_model_key.lock().await.clone();
                            if model_path.as_os_str().is_empty() {
                                continue;
                            }

                            let mut guard = watchdog_client.lock().await;
                            if guard.wait_for_runtime_ready().await.is_err() {
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
                                    let _ = guard.start_sidecar(
                                        &watchdog_app_handle,
                                        &model_path,
                                        &model_key,
                                    );
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
                    let mut last_thermal_check = Instant::now() - Duration::from_secs(31);
                    let mut runtime_state: Option<RuntimeState> = None;
                    let mut runtime_reload_requested = true;

                    loop {
                        let should_refresh_runtime = runtime_reload_requested
                            || runtime_state.as_ref().is_none_or(|state| {
                                state.loaded_at.elapsed() >= SETTINGS_REFRESH_INTERVAL
                            });

                        if should_refresh_runtime {
                            match load_runtime_state() {
                                Ok(state) => {
                                    if runtime_state.as_ref().is_none_or(|current| {
                                        current.active_model.entry.id != state.active_model.entry.id
                                    }) {
                                        sidecar_started = false;
                                    }
                                    runtime_state = Some(state);
                                    runtime_reload_requested = false;
                                }
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
                                    if last_thermal_check.elapsed() > Duration::from_secs(30) {
                                        thermal_monitor.update();
                                        last_thermal_check = Instant::now();
                                    }
                                    continue;
                                }
                            }
                        }

                        let Some(state) = runtime_state.as_ref() else {
                            sleep(Duration::from_secs(1)).await;
                            continue;
                        };
                        let runtime_settings = state.settings.clone();
                        let active_model = state.active_model.clone();

                        *active_model_path.lock().await = active_model.path.clone();
                        *active_model_key.lock().await = active_model.entry.id.clone();

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
                            if last_thermal_check.elapsed() > Duration::from_secs(30) {
                                thermal_monitor.update();
                                last_thermal_check = Instant::now();
                            }
                            continue;
                        }
                        warned_missing_model = false;

                        if active_model_id != active_model.entry.id {
                            sidecar_started = false;
                            active_model_id = active_model.entry.id.clone();
                        }

                        if !sidecar_started {
                            log::info!("[Pipeline] Starting sidecar on port {sidecar_port}");
                            match client_clone
                                .lock()
                                .await
                                .start_sidecar(
                                    &app_handle_sidecar,
                                    &active_model.path,
                                    &active_model.entry.id,
                                )
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

                        let ready_result = if failure_count == 0 {
                            client_clone.lock().await.wait_for_ready().await
                        } else {
                            client_clone.lock().await.wait_for_ready_retry().await
                        };

                        match ready_result {
                            Ok(()) => {
                                failure_count = 0;
                                log::info!("[Pipeline] Translation sidecar is ready (active: {})", sidecar_started);
                            }
                            Err(error) => {
                                failure_count += 1;
                                sidecar_started = false;
                                log::error!(
                                    "[Pipeline] Sidecar not ready (attempt {failure_count}): {error}"
                                );
                                if failure_count > 5 {
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
                                sleep(Duration::from_secs(2)).await;
                                continue;
                            }
                        }

                        let ocr_engine_loop = Arc::clone(&ocr_engine);
                        let frame_rx = display_manager.get_or_start_capture(
                            0,
                            &[app_bundle_id_sidecar.as_str()],
                            &[app_process_id],
                            &[app_name_hint.as_str()],
                        );
                        let mut motion_detector = MotionDetector::new(
                            runtime_settings.pixel_diff_threshold,
                            runtime_settings.edge_inset_percent,
                        );
                        let mut debounce = DebounceStateMachine::new(
                            runtime_settings.debounce_ms,
                            runtime_settings.motion_threshold,
                        );
                        let mut pending_force_scan = false;
                        let mut latest_frame: Option<capture::CaptureFrame> = None;
                        let mut last_frame_at = Instant::now();
                        let mut was_scrolling = false;
                        let mut last_processed_hash: Option<u64> = None;
                        let mut last_payload: Option<TranslationPayload> = None;

                        log::info!("[Pipeline] Entering capture loop");

                        'capture: loop {
                            sleep(Duration::from_millis(10)).await;

                            while let Ok(command) = pipeline_rx.try_recv() {
                                match command {
                                    PipelineCommand::ForceScan => {
                                        if let Some(frame) = latest_frame.clone() {
                                            log::info!(
                                                "[Pipeline] Force scan requested on cached frame"
                                            );
                                            let cached_rgba = swap_bgra_to_rgba(&frame.buffer.data);
                                            let cached_thumbnail = motion_detector.downsample(
                                                &cached_rgba,
                                                frame.buffer.width,
                                                frame.buffer.height,
                                            );
                                            let frame_hash = cached_thumbnail
                                                .iter()
                                                .map(|&pixel| u64::from(pixel))
                                                .sum::<u64>();
                                            if last_processed_hash == Some(frame_hash)
                                                && let Some(payload) = last_payload.as_ref()
                                            {
                                                log::debug!(
                                                    "[Pipeline] Force scan frame identical to last processed, reusing cached payload"
                                                );
                                                let _ = emit_cached_translation_payload(
                                                    &app_handle_sidecar,
                                                    payload,
                                                );
                                            } else {
                                                let current_frame_id = frame_id;
                                                frame_id += 1;
                                                if let Some(payload) = process_capture_frame(
                                                    &app_handle_sidecar,
                                                    &client_clone,
                                                    &ocr_engine_loop,
                                                    &invalidation_rx,
                                                    &pipeline_tx,
                                                    &frame,
                                                    current_frame_id,
                                                )
                                                .await
                                                {
                                                    last_processed_hash = Some(frame_hash);
                                                    last_payload = Some(payload);
                                                }
                                            }
                                            pending_force_scan = false;
                                        } else {
                                            log::info!(
                                                "[Pipeline] Force scan queued until the first frame arrives"
                                            );
                                            pending_force_scan = true;
                                        }
                                    }
                                    PipelineCommand::ReloadRuntime { reason } => {
                                        log::info!("[Pipeline] Reload requested: {reason}");
                                        runtime_reload_requested = true;
                                        sidecar_started = false;
                                        // We don't necessarily need to break 'capture anymore if just settings changed,
                                        // but if the model changed, we should break to restart sidecar.
                                        break 'capture;
                                    }
                                    PipelineCommand::Shutdown => {
                                        log::info!("[Pipeline] Shutdown requested");
                                        display_manager.stop();
                                        client_clone.lock().await.shutdown_sidecar();
                                        return;
                                    }
                                }
                            }

                            let frame_res = frame_rx.try_recv();
                            if frame_res.is_err() {
                                if last_frame_at.elapsed() > Duration::from_secs(60) {
                                    log::warn!("[Pipeline] Frame stream silent for 60s - checking health");
                                    // Passive check: only restart if we suspect a real crash
                                    // Display manager already keeps it sticky, so just wait
                                    last_frame_at = Instant::now();
                                }

                                // BUG FIX: If we are settling but no frames are arriving (screen is static),
                                // we must still tick the debounce machine so it can eventually Trigger.
                                if matches!(debounce.state, motion::DebounceState::Settling(_)) {
                                    if let DebounceEvent::Triggered = debounce.update(0.0) {
                                        if let Some(frame) = latest_frame.as_ref() {
                                            log::info!("[Pipeline] Debounce triggered on static screen");
                                            let current_frame_id = frame_id;
                                            frame_id += 1;
                                            if let Some(payload) = process_capture_frame(
                                                &app_handle_sidecar,
                                                &client_clone,
                                                &ocr_engine_loop,
                                                &invalidation_rx,
                                                &pipeline_tx,
                                                &frame,
                                                current_frame_id,
                                            )
                                            .await
                                            {
                                                last_payload = Some(payload);
                                            }
                                        }
                                    }
                                }
                                continue;
                            }
                            let frame = frame_res.unwrap();
                            last_frame_at = Instant::now();
                            latest_frame = Some(frame.clone());

                            let is_forced = std::mem::take(&mut pending_force_scan);
                            let rgba_data = swap_bgra_to_rgba(&frame.buffer.data);
                            let thumbnail = motion_detector.downsample(
                                &rgba_data,
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
                                    if !was_scrolling {
                                        let _ = app_handle_sidecar.emit("translation-clear", ());
                                        was_scrolling = true;
                                    }
                                }
                                DebounceEvent::Triggered => {
                                    was_scrolling = false;
                                    let frame_hash =
                                        thumbnail.iter().map(|&pixel| u64::from(pixel)).sum::<u64>();
                                    if last_processed_hash == Some(frame_hash)
                                        && let Some(payload) = last_payload.as_ref()
                                    {
                                        log::debug!(
                                            "[Pipeline] Frame identical to last processed, reusing cached payload"
                                        );
                                        let _ = emit_cached_translation_payload(
                                            &app_handle_sidecar,
                                            payload,
                                        );
                                        continue;
                                    }
                                    let current_frame_id = frame_id;
                                    frame_id += 1;
                                    if let Some(payload) = process_capture_frame(
                                        &app_handle_sidecar,
                                        &client_clone,
                                        &ocr_engine_loop,
                                        &invalidation_rx,
                                        &pipeline_tx,
                                        &frame,
                                        current_frame_id,
                                    )
                                    .await
                                    {
                                        last_processed_hash = Some(frame_hash);
                                        last_payload = Some(payload);
                                    }
                                }
                                DebounceEvent::None => {
                                    if !matches!(debounce.state, motion::DebounceState::Scrolling) {
                                        was_scrolling = false;
                                    }
                                }
                            }

                            if last_thermal_check.elapsed() > Duration::from_secs(30) {
                                thermal_monitor.update();
                                last_thermal_check = Instant::now();
                            }
                            if thermal_monitor.should_throttle() {
                                sleep(Duration::from_millis(500)).await;
                            }
                        }
                    }
                });
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit)
                && let Some(tx) = pipeline_tx_for_exit
                    .lock()
                    .expect("pipeline exit handle lock poisoned")
                    .as_ref()
                    .cloned()
            {
                let _ = tx.try_send(PipelineCommand::Shutdown);
            }
        });
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

    let sidecar_port = find_available_local_port()?;
    let mut sidecar = spawn_cli_sidecar(&active_model.path, sidecar_port)?;
    let vision_helper_path = resolve_binary_path("vision-helper")?;
    let ocr_engine = ocr::OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client =
        translation::TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_mode_for_cli(&active_model.entry.id);
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
    let ocr_engine = ocr::OcrEngine::new(settings.furigana_suppression, vision_helper_path);
    let mut translation_client =
        translation::TranslationClient::new(settings.context_memory_size, sidecar_port);
    translation_client.start_sidecar_mode_for_cli(&active_model.entry.id);
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
