// src-tauri/src/scheduler.rs

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crossbeam_channel::{Receiver, Sender};
use tauri::{Emitter, Manager};

use crate::ipc::{TranslationErrorPayload, TranslationPayload, WizardStatusPayload};
use crate::models::ModelManifest;
use crate::path_resolver::find_available_local_port;
use crate::runtime_coordinator::RuntimeCoordinator;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PipelineCommand {
    ForceScan,
    ReloadRuntime { reason: String },
    Shutdown,
}

pub fn emit_runtime_notice<
    R: tauri::Runtime,
    S1: Into<String>,
    S2: Into<String>,
    S3: Into<String>,
>(
    app: &tauri::AppHandle<R>,
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

async fn run_pipeline_frame<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    processor: &mut crate::pipeline::PipelineProcessor,
    frame: &crate::capture::CaptureFrame,
    is_forced: bool,
    invalidation_rx: &Receiver<crate::context::InvalidationReason>,
    pipeline_tx: &Sender<PipelineCommand>,
) -> Option<TranslationPayload> {
    processor
        .handle_frame(app_handle, frame, is_forced, invalidation_rx, pipeline_tx)
        .await
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

        let mut translation_manager =
            crate::translation::TranslationManager::new(config.initial_memory_size, sidecar_port);

        let client = Arc::clone(&translation_manager.client);
        let client_clone = Arc::clone(&client);
        let app_handle_sidecar = config.app_handle.clone();

        // Start Window Tracker on its own thread
        let mut window_tracker_task = config.window_tracker;
        thread::spawn(move || {
            window_tracker_task.start_polling();
        });

        rt.block_on(async move {
            // Start the watchdog internally once inside the Tokio runtime context
            translation_manager.start_watchdog(config.app_handle.clone());

            let app_dir = crate::settings::Settings::dir().expect("Failed to get app directory");

            let mut loop_state = crate::runtime_coordinator::RuntimeLoopState::new();
            let runtime_coordinator = crate::runtime_coordinator::DefaultRuntimeCoordinator;
            let runtime_executor = crate::runtime_executor::RuntimeExecutor;
            let mut processor = crate::pipeline::PipelineProcessor::new(
                0,
                0,
                0,
                0.0,
                Arc::clone(&config.ocr_engine),
                Arc::clone(&client_clone),
            );

            loop {
                let idle_sleep_duration = crate::runtime_executor::RuntimeExecutor::idle_sleep_duration(
                    loop_state.thermal_monitor.on_battery,
                );
                let should_refresh_runtime = loop_state.should_refresh_runtime();

                if should_refresh_runtime {
                    match runtime_coordinator.load_runtime_state(&app_dir) {
                        Ok(state) => {
                            loop_state.apply_loaded_runtime_state(
                                &runtime_coordinator,
                                state,
                                &mut processor,
                            );
                        }
                        Err(error) => {
                            if !loop_state.warned_missing_model {
                                emit_runtime_notice(
                                    &app_handle_sidecar,
                                    "No Model Available",
                                    "Add a GGUF model to the Contextura models folder to enable translation.",
                                    error.to_string(),
                                    "warning",
                                    6000,
                                );
                                loop_state.note_missing_model_warning();
                            }
                            sleep(Duration::from_secs(5)).await;
                            if loop_state.last_thermal_check.elapsed() > Duration::from_secs(30) {
                                loop_state.thermal_monitor.update();
                                loop_state.last_thermal_check = Instant::now();
                            }
                            continue;
                        }
                    }
                }

                let Some(state) = loop_state.runtime_state.as_ref() else {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                };
                let active_model = state.active_model.clone();

                translation_manager.update_model_state(
                    &active_model.path,
                    &active_model.entry.id,
                    active_model.entry.strategy.clone(),
                ).await;

                if !active_model.installed {
                    if !loop_state.warned_missing_model {
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
                        loop_state.note_missing_model_warning();
                    }
                    sleep(Duration::from_secs(5)).await;
                    if loop_state.last_thermal_check.elapsed() > Duration::from_secs(30) {
                        loop_state.thermal_monitor.update();
                        loop_state.last_thermal_check = Instant::now();
                    }
                    continue;
                }
                loop_state.note_model_ready(&active_model.entry.id);

                match runtime_executor
                    .ensure_sidecar_ready(crate::sidecar_runtime_adapter::SidecarEnsureRequest {
                        app_handle: &app_handle_sidecar,
                        client: &client_clone,
                        runtime_coordinator: &runtime_coordinator,
                        loop_state: &mut loop_state,
                        model_path: &active_model.path,
                        model_id: &active_model.entry.id,
                        strategy: active_model.entry.strategy.as_deref(),
                    })
                    .await
                {
                    crate::sidecar_runtime_adapter::SidecarEnsureResult::Ready => {
                        log::info!(
                            "[Pipeline] Translation sidecar is ready (active: {})",
                            loop_state.sidecar_started
                        );
                    }
                    crate::sidecar_runtime_adapter::SidecarEnsureResult::StartFailed { error } => {
                        log::error!(
                            "[Pipeline] Failed to start sidecar (attempt {}): {error}",
                            loop_state.failure_count
                        );
                        emit_runtime_notice(
                            &app_handle_sidecar,
                            "Translation Engine Startup Failed",
                            format!("Attempt {}/5 to start the translation engine failed.", loop_state.failure_count),
                            error,
                            "error",
                            3000,
                        );
                        if runtime_coordinator.should_halt_startup(loop_state.failure_count) {
                            emit_runtime_notice(
                                &app_handle_sidecar,
                                "Translation Engine Unavailable",
                                "The sidecar failed to start. Verify the active model file and click Retry.",
                                active_model.path.display().to_string(),
                                "error",
                                0,
                            );
                            if handle_startup_halt(&config.pipeline_rx, &mut loop_state.failure_count, &mut loop_state.runtime_reload_requested, &mut loop_state.sidecar_started).await {
                                return;
                            }
                            continue;
                        }
                        sleep(idle_sleep_duration).await;
                        continue;
                    }
                    crate::sidecar_runtime_adapter::SidecarEnsureResult::ReadyFailed { error } => {
                        log::error!(
                            "[Pipeline] Sidecar not ready (attempt {}): {error}",
                            loop_state.failure_count
                        );
                        emit_runtime_notice(
                            &app_handle_sidecar,
                            "Translation Engine Startup Failed",
                            format!("Attempt {}/5 to start the translation engine failed.", loop_state.failure_count),
                            error,
                            "error",
                            3000,
                        );
                        if runtime_coordinator.should_halt_startup(loop_state.failure_count) {
                            emit_runtime_notice(
                                &app_handle_sidecar,
                                "Translation Engine Unavailable",
                                "The sidecar never became ready. Verify the active model file and click Retry.",
                                active_model.path.display().to_string(),
                                "error",
                                0,
                            );
                            if handle_startup_halt(&config.pipeline_rx, &mut loop_state.failure_count, &mut loop_state.runtime_reload_requested, &mut loop_state.sidecar_started).await {
                                return;
                            }
                            continue;
                        }
                        sleep(idle_sleep_duration).await;
                        continue;
                    }
                }

                let frame_rx = config.display_manager.get_or_start_capture(
                    0,
                    &[&config.app_config.bundle_id],
                    &[config.app_config.process_id],
                    &[&config.app_config.name_hint],
                );
                let mut capture_loop_state = crate::capture_loop_driver::CaptureLoopState::new();

                let mut pipeline_tokio_rx =
                    crate::capture_loop_driver::CaptureLoopDriver::bridge_receiver(
                        config.pipeline_rx.clone(),
                        16,
                    );

                let mut frame_tokio_rx =
                    crate::capture_loop_driver::CaptureLoopDriver::bridge_receiver(frame_rx, 2);

                log::info!("[Pipeline] Entering capture loop");

                'capture: loop {
                    let debounce_sleep = compute_debounce_sleep(processor.debounce.state, processor.debounce.debounce_duration);
                    tokio::pin!(debounce_sleep);

                    tokio::select! {
                        cmd_opt = pipeline_tokio_rx.recv() => {
                            let Some(command) = cmd_opt else { break 'capture; };
                            match crate::capture_loop_driver::CaptureLoopDriver::handle_command_event(
                                &mut capture_loop_state,
                                command,
                            ) {
                                crate::capture_loop_driver::CommandLoopAction::RunForcedScan { has_cached_frame } => {
                                    if has_cached_frame {
                                        let frame = capture_loop_state
                                            .latest_frame
                                            .clone()
                                            .expect("cached frame should exist when command seam reports it");
                                        log::info!(
                                            "[Pipeline] Force scan requested on cached frame"
                                        );
                                        let _ = run_pipeline_frame(
                                            &app_handle_sidecar,
                                            &mut processor,
                                            &frame,
                                            true,
                                            &config.invalidation_rx,
                                            &config.pipeline_tx,
                                        )
                                        .await;
                                    } else {
                                        log::info!(
                                            "[Pipeline] Force scan queued until the first frame arrives"
                                        );
                                    }
                                }
                                crate::capture_loop_driver::CommandLoopAction::ReloadRuntime { reason } => {
                                    log::info!("[Pipeline] Reload requested: {reason}");
                                    loop_state.runtime_reload_requested = true;
                                    loop_state.sidecar_started = false;
                                    break 'capture;
                                }
                                crate::capture_loop_driver::CommandLoopAction::Shutdown => {
                                    log::info!("[Pipeline] Shutdown requested");
                                    config.display_manager.stop();
                                    translation_manager.shutdown().await;

                                    if let Ok(cache_dir) = config.app_handle.path().app_cache_dir() {
                                        crate::snapshot::cleanup_stale_temp_frames(&cache_dir);
                                    }
                                    return;
                                }
                            }
                        }
                        frame_opt = frame_tokio_rx.recv() => {
                            let Some(frame) = frame_opt else {
                                if capture_loop_state.note_stream_idle(Duration::from_secs(60)) {
                                    log::warn!("[Pipeline] Frame stream silent for 60s - checking health");
                                }
                                continue;
                            };
                            let (frame, action) = crate::capture_loop_driver::CaptureLoopDriver::handle_frame_event(
                                &mut processor,
                                &mut capture_loop_state,
                                frame,
                            );

                            match action {
                                crate::capture_loop_driver::FrameLoopAction::ClearForMotion => {
                                    let _ = app_handle_sidecar.emit("translation-clear", ());
                                }
                                crate::capture_loop_driver::FrameLoopAction::RunPipeline { is_forced } => {
                                    let _ = run_pipeline_frame(
                                        &app_handle_sidecar,
                                        &mut processor,
                                        &frame,
                                        is_forced,
                                        &config.invalidation_rx,
                                        &config.pipeline_tx,
                                    )
                                    .await;
                                }
                                crate::capture_loop_driver::FrameLoopAction::Noop => {}
                            }
                        }
                        () = &mut debounce_sleep => {
                            let debounce_event = processor.debounce.update(0.0);
                            match crate::capture_loop_driver::CaptureLoopDriver::handle_debounce_event(
                                &mut processor,
                                &capture_loop_state,
                                debounce_event,
                            ) {
                                crate::capture_loop_driver::DebounceLoopAction::RunPipeline => {
                                    let frame = capture_loop_state
                                        .latest_frame
                                        .as_ref()
                                        .expect("latest frame should exist when debounce seam reports RunPipeline");
                                    log::info!("[Pipeline] Debounce triggered on static screen");
                                    let _ = run_pipeline_frame(
                                        &app_handle_sidecar,
                                        &mut processor,
                                        frame,
                                        false,
                                        &config.invalidation_rx,
                                        &config.pipeline_tx,
                                    )
                                    .await;
                                }
                                crate::capture_loop_driver::DebounceLoopAction::Noop => {}
                            }
                        }
                    }

                    if loop_state.last_thermal_check.elapsed() > Duration::from_secs(30) {
                        loop_state.thermal_monitor.update();
                        loop_state.last_thermal_check = Instant::now();
                        if let Some(state) = &loop_state.runtime_state {
                            runtime_coordinator.apply_runtime_settings(
                                &mut processor,
                                &state.settings,
                                loop_state.thermal_monitor.on_battery,
                            );
                        }
                    }
                    if loop_state.thermal_monitor.should_throttle() {
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
    let runtime_coordinator = crate::runtime_coordinator::DefaultRuntimeCoordinator;
    let pipeline_rx_sync = pipeline_rx.clone();
    let result = tokio::task::spawn_blocking(move || {
        while let Ok(command) = pipeline_rx_sync.recv() {
            if !matches!(command, PipelineCommand::ForceScan) {
                return command;
            }
        }
        PipelineCommand::Shutdown
    })
    .await;

    let Ok(command) = result else {
        log::info!("[Pipeline] Shutdown requested while halted");
        return true;
    };

    if let PipelineCommand::ReloadRuntime { reason } = &command {
        log::info!("[Pipeline] Manual retry triggered: {reason}");
    }

    if runtime_coordinator.handle_halt_command(
        command,
        failure_count,
        runtime_reload_requested,
        sidecar_started,
    ) {
        log::info!("[Pipeline] Shutdown requested while halted");
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Mutex as AsyncMutex;

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

        let runtime_coordinator = crate::runtime_coordinator::DefaultRuntimeCoordinator;
        let state = runtime_coordinator
            .load_runtime_state(&app_dir)
            .expect("should load initial state");
        assert_eq!(state.settings.debounce_ms, 200);

        // Update settings on disk
        let mut updated_settings = settings;
        updated_settings.debounce_ms = 400;
        updated_settings.save(&app_dir).unwrap();

        // Reload state
        let reloaded_state = runtime_coordinator
            .load_runtime_state(&app_dir)
            .expect("should reload state");
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
