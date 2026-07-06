// src-tauri/src/scheduler.rs

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use crossbeam_channel::{Receiver, Sender};
use tauri::{Emitter, Manager};

use crate::ipc::{
    TranslationErrorPayload, TranslationPayload, TranslationStartedPayload, WizardStatusPayload,
};
use crate::models::ModelManifest;
use crate::motion::DebounceEvent;
use crate::path_resolver::find_available_local_port;

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

async fn run_pipeline_frame(
    app_handle: &tauri::AppHandle,
    processor: &crate::pipeline::PipelineProcessor,
    frame: &crate::capture::CaptureFrame,
    frame_id: u64,
    invalidation_rx: &Receiver<crate::context::InvalidationReason>,
    pipeline_tx: &Sender<PipelineCommand>,
) -> Option<TranslationPayload> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .expect("Failed to get cache dir");

    let _ = app_handle.emit(
        "translation-started",
        TranslationStartedPayload {
            display_id: frame.display_id,
        },
    );

    let result = processor
        .process_frame(&cache_dir, frame, frame_id, invalidation_rx, pipeline_tx)
        .await;

    if result.clear_context {
        let _ = app_handle.emit("translation-clear", ());
    }
    if result.manual_reset {
        emit_runtime_notice(
            app_handle,
            "Context Cleared",
            "Translation memory was cleared.",
            "New translations will start without prior context.",
            "info",
            2500,
        );
    }

    if let Some(ref payload) = result.payload
        && let Err(error) = app_handle.emit("translation-update", payload)
    {
        log::error!("[IPC] Failed to emit translation-update: {error}");
    }

    result.payload
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

fn load_runtime_state(app_dir: &std::path::Path) -> anyhow::Result<RuntimeState> {
    let loaded_settings = crate::settings::Settings::load(app_dir)?;
    let active_model = crate::models::active_model_status(app_dir, &loaded_settings)?;
    Ok(RuntimeState {
        settings: loaded_settings,
        active_model,
    })
}

pub fn open_models_folder(models_dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
    std::process::Command::new("open")
        .arg(models_dir)
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
    pub app_config: crate::path_resolver::AppConfig,
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
        let active_model_strategy = Arc::new(AsyncMutex::new(Option::<String>::None));

        // Start Window Tracker on its own thread
        let mut window_tracker_task = config.window_tracker;
        thread::spawn(move || {
            window_tracker_task.start_polling();
        });

        rt.block_on(async move {
            let app_dir = crate::settings::Settings::dir().expect("Failed to get app directory");
            let watchdog_client = Arc::clone(&client);
            let watchdog_app_handle = app_handle_sidecar.clone();
            let watchdog_model_path = Arc::clone(&active_model_path);
            let watchdog_model_key = Arc::clone(&active_model_key);
            let watchdog_model_strategy = Arc::clone(&active_model_strategy);
            tokio::spawn(async move {
                let mut consecutive_failures = 0u8;
                loop {
                    sleep(Duration::from_secs(5)).await;
                    let model_path = watchdog_model_path.lock().await.clone();
                    let model_key = watchdog_model_key.lock().await.clone();
                    let model_strategy = watchdog_model_strategy.lock().await.clone();
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
                            log::warn!("[Watchdog] Restarting unresponsive sidecar...");
                            let restart_res = guard.start_sidecar(
                                &watchdog_app_handle,
                                &model_path,
                                &model_key,
                                model_strategy.as_deref(),
                            );
                            if let Err(error) = restart_res {
                                log::error!("[Watchdog] Failed to restart sidecar: {error}");
                                emit_runtime_notice(
                                    &watchdog_app_handle,
                                    "Translation Engine Restart Failed",
                                    "Watchdog failed to restart the translation engine.",
                                    error.to_string(),
                                    "error",
                                    0, // Persistent!
                                );
                            } else {
                                emit_runtime_notice(
                                    &watchdog_app_handle,
                                    "Translation Engine Restarted",
                                    "The local translation engine became unresponsive and was restarted.",
                                    model_path.display().to_string(),
                                    "warning",
                                    5000,
                                );
                            }
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
            let mut processor = crate::pipeline::PipelineProcessor::new(
                0,
                0,
                0,
                0.0,
                Arc::clone(&config.ocr_engine),
                Arc::clone(&client_clone),
            );

            loop {
                let should_refresh_runtime = runtime_reload_requested || runtime_state.is_none();

                if should_refresh_runtime {
                    match load_runtime_state(&app_dir) {
                        Ok(state) => {
                            if runtime_state.as_ref().is_none_or(|current| {
                                current.active_model.entry.id != state.active_model.entry.id
                            }) {
                                sidecar_started = false;
                            }
                            processor.update_settings(
                                state.settings.debounce_ms,
                                state.settings.motion_threshold,
                                state.settings.pixel_diff_threshold,
                                state.settings.edge_inset_percent,
                            );
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
                let active_model = state.active_model.clone();

                *active_model_path.lock().await = active_model.path.clone();
                *active_model_key.lock().await = active_model.entry.id.clone();
                *active_model_strategy.lock().await = active_model.entry.strategy.clone();

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
                            active_model.entry.strategy.as_deref(),
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
                            failure_count += 1;
                            log::error!("[Pipeline] Failed to start sidecar (attempt {failure_count}): {error}");
                            emit_runtime_notice(
                                &app_handle_sidecar,
                                "Translation Engine Startup Failed",
                                format!("Attempt {failure_count}/5 to start the translation engine failed."),
                                error.to_string(),
                                "error",
                                3000,
                            );
                            if failure_count > 5 {
                                emit_runtime_notice(
                                    &app_handle_sidecar,
                                    "Translation Engine Unavailable",
                                    "The sidecar failed to start. Verify the active model file and click Retry.",
                                    error.to_string(),
                                    "error",
                                    0, // Persistent
                                );
                                if handle_startup_halt(&config.pipeline_rx, &mut failure_count, &mut runtime_reload_requested, &mut sidecar_started).await {
                                    return;
                                }
                                continue;
                            }
                            sleep(Duration::from_secs(2)).await;
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
                        emit_runtime_notice(
                            &app_handle_sidecar,
                            "Translation Engine Startup Failed",
                            format!("Attempt {failure_count}/5 to start the translation engine failed."),
                            error.to_string(),
                            "error",
                            3000,
                        );
                        if failure_count > 5 {
                            emit_runtime_notice(
                                &app_handle_sidecar,
                                "Translation Engine Unavailable",
                                "The sidecar never became ready. Verify the active model file and click Retry.",
                                error.to_string(),
                                "error",
                                0, // Persistent
                            );
                            if handle_startup_halt(&config.pipeline_rx, &mut failure_count, &mut runtime_reload_requested, &mut sidecar_started).await {
                                return;
                            }
                            continue;
                        }
                        sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                }

                let frame_rx = config.display_manager.get_or_start_capture(
                    0,
                    &[&config.app_config.bundle_id],
                    &[config.app_config.process_id],
                    &[&config.app_config.name_hint],
                );
                let mut pending_force_scan = false;
                let mut latest_frame: Option<crate::capture::CaptureFrame> = None;
                let mut last_frame_at = Instant::now();
                let mut was_scrolling = false;
                let mut last_processed_hash: Option<u64> = None;
                let mut last_payload: Option<TranslationPayload> = None;

                let (pipeline_tokio_tx, mut pipeline_tokio_rx) = tokio::sync::mpsc::channel(16);
                let pipeline_rx_sync = config.pipeline_rx.clone();
                tokio::task::spawn_blocking(move || {
                    while let Ok(msg) = pipeline_rx_sync.recv() {
                        if pipeline_tokio_tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                });

                let (frame_tokio_tx, mut frame_tokio_rx) = tokio::sync::mpsc::channel(2);
                let frame_rx_sync = frame_rx.clone();
                tokio::task::spawn_blocking(move || {
                    while let Ok(frame) = frame_rx_sync.recv() {
                        if frame_tokio_tx.blocking_send(frame).is_err() {
                            break;
                        }
                    }
                });

                log::info!("[Pipeline] Entering capture loop");

                'capture: loop {
                    let debounce_sleep = compute_debounce_sleep(processor.debounce.state, processor.debounce.debounce_duration);
                    tokio::pin!(debounce_sleep);

                    tokio::select! {
                        cmd_opt = pipeline_tokio_rx.recv() => {
                            let Some(command) = cmd_opt else { break 'capture; };
                            match command {
                                PipelineCommand::ForceScan => {
                                    if let Some(frame) = latest_frame.clone() {
                                        log::info!(
                                            "[Pipeline] Force scan requested on cached frame"
                                        );
                                        let cached_rgba = &frame.buffer.data;
                                        let cached_thumbnail = processor.motion_detector.downsample(
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
                                            if let Some(payload) = run_pipeline_frame(
                                                &app_handle_sidecar,
                                                &processor,
                                                &frame,
                                                current_frame_id,
                                                &config.invalidation_rx,
                                                &config.pipeline_tx,
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
                        frame_opt = frame_tokio_rx.recv() => {
                            let Some(frame) = frame_opt else {
                                if last_frame_at.elapsed() > Duration::from_secs(60) {
                                    log::warn!("[Pipeline] Frame stream silent for 60s - checking health");
                                    last_frame_at = Instant::now();
                                }
                                continue;
                            };
                            last_frame_at = Instant::now();
                            latest_frame = Some(frame.clone());

                            let is_forced = std::mem::take(&mut pending_force_scan);
                            let debounce_event = processor.process_motion(&frame, is_forced);

                            match debounce_event {
                                DebounceEvent::MotionDetected => {
                                    if !was_scrolling {
                                        let _ = app_handle_sidecar.emit("translation-clear", ());
                                        was_scrolling = true;
                                    }
                                }
                                DebounceEvent::Triggered => {
                                    was_scrolling = false;
                                    let rgba_data = &frame.buffer.data;
                                    let thumbnail = processor.motion_detector.downsample(
                                        rgba_data,
                                        frame.buffer.width,
                                        frame.buffer.height,
                                    );
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
                                    if let Some(payload) = run_pipeline_frame(
                                        &app_handle_sidecar,
                                        &processor,
                                        &frame,
                                        current_frame_id,
                                        &config.invalidation_rx,
                                        &config.pipeline_tx,
                                    )
                                    .await
                                    {
                                        last_processed_hash = Some(frame_hash);
                                        last_payload = Some(payload);
                                    }
                                }
                                DebounceEvent::None => {
                                    if !matches!(processor.debounce.state, crate::motion::DebounceState::Scrolling) {
                                        was_scrolling = false;
                                    }
                                }
                            }
                        }
                        () = &mut debounce_sleep => {
                            let debounce_event = processor.debounce.update(0.0);
                            if matches!(debounce_event, DebounceEvent::Triggered) && let Some(frame) = latest_frame.as_ref() {
                                log::info!("[Pipeline] Debounce triggered on static screen");
                                was_scrolling = false;
                                let current_frame_id = frame_id;
                                frame_id += 1;
                                if let Some(payload) = run_pipeline_frame(
                                    &app_handle_sidecar,
                                    &processor,
                                    frame,
                                    current_frame_id,
                                    &config.invalidation_rx,
                                    &config.pipeline_tx,
                                )
                                .await
                                {
                                    last_payload = Some(payload);
                                }
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

async fn compute_debounce_sleep(state: crate::motion::DebounceState, duration: Duration) {
    if let crate::motion::DebounceState::Settling(start_time) = state {
        let elapsed = start_time.elapsed();
        if elapsed < duration {
            tokio::time::sleep(duration.saturating_sub(elapsed)).await;
        }
    } else {
        std::future::pending::<()>().await;
    }
}

async fn handle_startup_halt(
    pipeline_rx: &crossbeam_channel::Receiver<PipelineCommand>,
    failure_count: &mut u32,
    runtime_reload_requested: &mut bool,
    sidecar_started: &mut bool,
) -> bool {
    log::info!("[Pipeline] Halted due to startup failure. Waiting for manual retry...");
    let pipeline_rx_sync = pipeline_rx.clone();
    let result = tokio::task::spawn_blocking(move || {
        while let Ok(command) = pipeline_rx_sync.recv() {
            match command {
                PipelineCommand::ReloadRuntime { reason } => {
                    return Some(reason);
                }
                PipelineCommand::Shutdown => {
                    return None;
                }
                PipelineCommand::ForceScan => {}
            }
        }
        None
    })
    .await;

    if let Ok(Some(reason)) = result {
        log::info!("[Pipeline] Manual retry triggered: {reason}");
        *failure_count = 0;
        *runtime_reload_requested = true;
        *sidecar_started = false;
        false
    } else {
        log::info!("[Pipeline] Shutdown requested while halted");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_app_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("contextura-{label}-{unique}"));
        fs::create_dir_all(dir.join("models")).expect("temp model dir should be created");
        dir
    }

    #[test]
    fn test_load_runtime_state_reloads_immediately() {
        let app_dir = temp_app_dir("scheduler-reload");

        // Write initial settings
        let settings = Settings {
            debounce_ms: 200,
            ..Default::default()
        };
        settings.save(&app_dir).unwrap();

        // Write a mock active model
        let model_filename = "translategemma-4b-it.Q4_K_M.gguf";
        fs::write(app_dir.join("models").join(model_filename), b"model-bytes").unwrap();

        let state = load_runtime_state(&app_dir).expect("should load initial state");
        assert_eq!(state.settings.debounce_ms, 200);

        // Update settings on disk
        let mut updated_settings = settings;
        updated_settings.debounce_ms = 400;
        updated_settings.save(&app_dir).unwrap();

        // Reload state
        let reloaded_state = load_runtime_state(&app_dir).expect("should reload state");
        assert_eq!(reloaded_state.settings.debounce_ms, 400);
    }

    #[tokio::test]
    async fn test_compute_debounce_sleep_settling_resolves() {
        use crate::motion::DebounceState;
        use std::time::Instant;

        let start = Instant::now();
        let state = DebounceState::Settling(start);
        let duration = Duration::from_millis(50);

        let sleep_fut = compute_debounce_sleep(state, duration);
        let start_wait = Instant::now();
        sleep_fut.await;
        let elapsed = start_wait.elapsed();

        assert!(elapsed >= Duration::from_millis(45));
    }

    #[tokio::test]
    async fn test_compute_debounce_sleep_idle_pending() {
        use crate::motion::DebounceState;
        use tokio::time::timeout;

        let state = DebounceState::Idle;
        let duration = Duration::from_millis(50);

        let sleep_fut = compute_debounce_sleep(state, duration);
        let res = timeout(Duration::from_millis(20), sleep_fut).await;
        assert!(res.is_err()); // should time out because it's pending
    }

    #[tokio::test]
    async fn test_compute_debounce_sleep_scrolling_pending() {
        use crate::motion::DebounceState;
        use tokio::time::timeout;

        let state = DebounceState::Scrolling;
        let duration = Duration::from_millis(50);

        let sleep_fut = compute_debounce_sleep(state, duration);
        let res = timeout(Duration::from_millis(20), sleep_fut).await;
        assert!(res.is_err()); // should time out because it's pending
    }

    #[test]
    fn test_translation_error_payload_serialization() {
        let payload = crate::ipc::TranslationErrorPayload {
            message: "test_msg".to_string(),
            title: "test_title".to_string(),
            detail: "test_detail".to_string(),
            level: "error".to_string(),
            dismiss_ms: 0,
        };
        let serialized = serde_json::to_string(&payload).unwrap();

        assert!(serialized.contains("\"message\":\"test_msg\""));
        assert!(serialized.contains("\"title\":\"test_title\""));
        assert!(serialized.contains("\"detail\":\"test_detail\""));
        assert!(serialized.contains("\"level\":\"error\""));
        assert!(serialized.contains("\"dismiss_ms\":0"));
    }

    #[tokio::test]
    async fn test_process_concurrent_translation_and_styling() {
        use crate::ocr::{OcrResult, Rect};
        use crate::translation::TranslationClient;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Start a local mock server to mock the LLM sidecar
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                // Return a mock Qwen translation response with matching numbered index
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\n  \"choices\": [\n    {\n      \"message\": {\n        \"content\": \"1: English One\\n2: English Two\"\n      }\n    }\n  ]\n}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        // Initialize translation client pointing to the mock server port
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, port)));
        // Make sure it uses Qwen strategy (which outputs numbered formats)
        client.lock().await.set_strategy("qwen");

        // Prepare dummy OCR results
        let ocr_results = vec![
            OcrResult {
                text: "日本語一".to_string(),
                confidence: 0.9,
                bounding_box: Rect::new(10.0, 10.0, 100.0, 50.0),
                text_angle: 0.0,
                is_vertical: false,
                is_furigana: false,
            },
            OcrResult {
                text: "日本語二".to_string(),
                confidence: 0.8,
                bounding_box: Rect::new(20.0, 80.0, 100.0, 50.0),
                text_angle: 0.0,
                is_vertical: false,
                is_furigana: false,
            },
        ];

        // Prepare a mock 100x100 black pixel buffer (40000 bytes for RGBA)
        let rgba_data = vec![0u8; 40000];

        let processor = crate::pipeline::PipelineProcessor::new(
            10,
            0,
            50,
            0.05,
            Arc::new(crate::ocr::OcrEngine::new(
                false,
                PathBuf::from("mock-vision"),
            )),
            Arc::clone(&client),
        );

        // Run the concurrent logic
        let boxes = processor
            .process_concurrent_translation_and_styling(
                &ocr_results,
                &rgba_data,
                100,
                100,
                1.0,
                123,
            )
            .await
            .unwrap();

        // Verify length and alignment
        assert_eq!(boxes.len(), 2);

        assert_eq!(boxes[0].original, "日本語一");
        assert_eq!(boxes[0].translated, "English One");
        assert_eq!(boxes[0].id, "123-0");

        assert_eq!(boxes[1].original, "日本語二");
        assert_eq!(boxes[1].translated, "English Two");
        assert_eq!(boxes[1].id, "123-1");

        // Verify colors were sampled (black bg -> white fg)
        assert_eq!(boxes[0].bg_color, "rgba(0, 0, 0, 0.85)");
        assert_eq!(boxes[0].fg_color, "#FFFFFF");
    }
}
