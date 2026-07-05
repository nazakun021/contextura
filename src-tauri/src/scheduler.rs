// src-tauri/src/scheduler.rs

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use crossbeam_channel::{Receiver, Sender};
use rayon::prelude::*;
use tauri::{Emitter, Manager};

use crate::ipc::{
    TranslationBox, TranslationErrorPayload, TranslationPayload, TranslationStartedPayload,
    WizardStatusPayload,
};
use crate::models::ModelManifest;
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::path_resolver::find_available_local_port;
use crate::snapshot::save_frame_as_png;
use crate::styling::StylingEngine;

const SETTINGS_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PipelineCommand {
    ForceScan,
    ReloadRuntime { reason: String },
    Shutdown,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    settings: crate::settings::Settings,
    active_model: crate::models::ModelStatus,
    loaded_at: Instant,
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
    client: &Arc<AsyncMutex<crate::translation::TranslationClient>>,
    invalidation_rx: &Receiver<crate::context::InvalidationReason>,
) {
    while let Ok(reason) = invalidation_rx.try_recv() {
        log::info!("[Context] Invalidation: {reason:?}");
        match reason {
            crate::context::InvalidationReason::AppSwitch { from, to } => {
                log::info!("[Context] App switch: {from} -> {to} — clearing memory");
                client.lock().await.memory.clear();
                let _ = app_handle.emit("translation-clear", ());
            }

            crate::context::InvalidationReason::ManualReset => {
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
    client: &Arc<AsyncMutex<crate::translation::TranslationClient>>,
    ocr_engine: &crate::ocr::OcrEngine,
    invalidation_rx: &Receiver<crate::context::InvalidationReason>,
    pipeline_tx: &Sender<PipelineCommand>,
    frame: &crate::capture::CaptureFrame,
    frame_id: u64,
) -> Option<TranslationPayload> {

    drain_invalidations(app_handle, client, invalidation_rx).await;

    let cache_dir = app_handle.path().app_cache_dir().expect("Failed to get cache dir");
    let rgba_data = &frame.buffer.data;
    let png_path = match save_frame_as_png(
        rgba_data,
        frame.buffer.width,
        frame.buffer.height,
        frame_id,
        &cache_dir,
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
    let app_dir = crate::settings::Settings::dir()?;
    let loaded_settings = crate::settings::Settings::load(&app_dir)?;
    let active_model = crate::models::active_model_status(&app_dir, &loaded_settings)?;
    Ok(RuntimeState {
        settings: loaded_settings,
        active_model,
        loaded_at: Instant::now(),
    })
}

pub fn open_models_folder() -> Result<(), String> {
    let app_dir = crate::settings::Settings::dir().map_err(|e| e.to_string())?;
    let models_dir = crate::models::models_dir(&app_dir);
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(&models_dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn open_screen_recording_settings_impl() -> Result<(), String> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_wizard_status() -> Result<WizardStatusPayload, String> {
    let app_dir = crate::settings::Settings::dir().map_err(|e| e.to_string())?;
    let settings = crate::settings::Settings::load(&app_dir).map_err(|e| e.to_string())?;
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
        models_dir: crate::models::models_dir(&app_dir).display().to_string(),
    })
}

pub fn request_model_switch(
    app: &tauri::AppHandle,
    pipeline_tx: &Sender<PipelineCommand>,
) -> anyhow::Result<()> {
    let app_dir = crate::settings::Settings::dir()?;
    let mut settings = crate::settings::Settings::load(&app_dir)?;
    let switch = crate::models::cycle_active_model(&app_dir, &mut settings)?;
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

pub struct SchedulerConfig {
    pub app_handle: tauri::AppHandle,
    pub app_bundle_id: String,
    pub app_process_id: i32,
    pub app_name_hint: String,
    pub initial_memory_size: usize,
    pub window_tracker: crate::context::AppWindowTracker,
    pub invalidation_rx: Receiver<crate::context::InvalidationReason>,
    pub ocr_engine: Arc<crate::ocr::OcrEngine>,
    pub display_manager: crate::capture::DisplayManager,
    pub pipeline_tx: Sender<PipelineCommand>,
    pub pipeline_rx: Receiver<PipelineCommand>,
}

#[allow(clippy::too_many_lines)]
pub fn start_scheduler(mut config: SchedulerConfig) {
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Tokio runtime should initialize");
        let sidecar_port = find_available_local_port().expect("localhost port should be available");
        let client = Arc::new(AsyncMutex::new(crate::translation::TranslationClient::new(
            config.initial_memory_size,
            sidecar_port,
        )));
        let client_clone = Arc::clone(&client);
        let app_handle_sidecar = config.app_handle.clone();
        let active_model_path = Arc::new(AsyncMutex::new(PathBuf::new()));
        let active_model_key = Arc::new(AsyncMutex::new(String::new()));

        // Start Window Tracker on its own thread
        let mut window_tracker_task = config.window_tracker;
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
            let mut last_thermal_check = Instant::now().checked_sub(Duration::from_secs(31)).unwrap_or_else(Instant::now);
            let mut runtime_state: Option<RuntimeState> = None;
            let mut runtime_reload_requested = true;
            let mut thermal_monitor = crate::thermal::ThermalMonitor::new();

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
                        log::info!("[Pipeline] Translation sidecar is ready (active: {sidecar_started})");
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

                let ocr_engine_loop = Arc::clone(&config.ocr_engine);
                let frame_rx = config.display_manager.get_or_start_capture(
                    0,
                    &[&config.app_bundle_id],
                    &[config.app_process_id],
                    &[&config.app_name_hint],
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
                let mut latest_frame: Option<crate::capture::CaptureFrame> = None;
                let mut last_frame_at = Instant::now();
                let mut was_scrolling = false;
                let mut last_processed_hash: Option<u64> = None;
                let mut last_payload: Option<TranslationPayload> = None;

                log::info!("[Pipeline] Entering capture loop");

                'capture: loop {
                    sleep(Duration::from_millis(10)).await;

                    while let Ok(command) = config.pipeline_rx.try_recv() {
                        match command {
                            PipelineCommand::ForceScan => {
                                if let Some(frame) = latest_frame.clone() {
                                    log::info!(
                                        "[Pipeline] Force scan requested on cached frame"
                                    );
                                    let cached_rgba = &frame.buffer.data;
                                    let cached_thumbnail = motion_detector.downsample(
                                        cached_rgba,
                                        frame.buffer.width,
                                        frame.buffer.height,
                                    );
                                    let frame_hash = crate::motion::compute_thumbnail_hash(&cached_thumbnail);
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
                                            &config.invalidation_rx,
                                            &config.pipeline_tx,
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
                                break 'capture;
                            }
                            PipelineCommand::Shutdown => {
                                log::info!("[Pipeline] Shutdown requested");
                                config.display_manager.stop();
                                client_clone.lock().await.shutdown_sidecar();

                                if let Ok(cache_dir) = config.app_handle.path().app_cache_dir() {
                                    crate::snapshot::cleanup_stale_temp_frames(&cache_dir);
                                }
                                return;
                            }
                        }
                    }

                    let frame_res = frame_rx.try_recv();
                    if frame_res.is_err() {
                        if last_frame_at.elapsed() > Duration::from_secs(60) {
                            log::warn!("[Pipeline] Frame stream silent for 60s - checking health");
                            last_frame_at = Instant::now();
                        }

                        if matches!(debounce.state, crate::motion::DebounceState::Settling(_))
                            && matches!(debounce.update(0.0), DebounceEvent::Triggered)
                            && let Some(frame) = latest_frame.as_ref()
                        {
                            log::info!("[Pipeline] Debounce triggered on static screen");
                            let current_frame_id = frame_id;
                            frame_id += 1;
                            if let Some(payload) = process_capture_frame(
                                &app_handle_sidecar,
                                &client_clone,
                                &ocr_engine_loop,
                                &config.invalidation_rx,
                                &config.pipeline_tx,
                                frame,
                                current_frame_id,
                            )
                            .await
                            {
                                last_payload = Some(payload);
                            }
                        }
                        continue;
                    }
                    let frame = frame_res.unwrap();
                    last_frame_at = Instant::now();
                    latest_frame = Some(frame.clone());

                    let is_forced = std::mem::take(&mut pending_force_scan);
                    let rgba_data = &frame.buffer.data;
                    let thumbnail = motion_detector.downsample(
                        rgba_data,
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
                            let frame_hash = crate::motion::compute_thumbnail_hash(&thumbnail);
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
                                &config.invalidation_rx,
                                &config.pipeline_tx,
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
                            if !matches!(debounce.state, crate::motion::DebounceState::Scrolling) {
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
}
