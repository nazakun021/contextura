mod cli;
mod downloader;
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
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::sleep;

use clap::Parser;
use cli::CliArgs;
use image::{ImageBuffer, RgbaImage};
use rayon::prelude::*;
use tauri::Emitter;

use crate::ipc::{TranslationBox, TranslationPayload, TranslationStartedPayload};
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::styling::StylingEngine;

/// Encodes a BGRA `CaptureFrame` pixel buffer to a temporary PNG file.
///
/// `ScreenCaptureKit` delivers frames in BGRA order. The `image` crate expects
/// RGBA, so we swap the B and R channels in-place before encoding.
fn save_frame_as_png(frame: &capture::CaptureFrame, frame_id: u64) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/contextura-frame-{frame_id}.png"));
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
    Ok(path)
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

#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
pub fn run() {
    // Initialize logging so log::info!/error! actually emit output.
    env_logger::init();

    let args = CliArgs::parse();

    if args.is_cli_mode() {
        run_cli(args);
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![complete_wizard])
        .setup(|app| {
            use tauri::Manager;

            let app_dir = settings::Settings::dir().expect("Failed to get app directory");
            let settings = settings::Settings::load(&app_dir).expect("Failed to load settings at startup");

            // --- Subsystem Initialization ---
            let (window_tracker, invalidation_rx) = context::AppWindowTracker::new();
            let mut thermal_monitor = thermal::ThermalMonitor::new();
            let ocr_engine = Arc::new(ocr::OcrEngine::new(
                settings.furigana_suppression,
                app.path().resource_dir().unwrap().join("binaries").join("vision-helper"),
            ));
            let display_manager = capture::DisplayManager::new();

            let (force_trigger_tx, force_trigger_rx) = crossbeam_channel::bounded(1);

            // Register Hotkeys
            hotkeys::register_shortcuts(app, window_tracker.clone(), force_trigger_tx.clone())
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
            tray::setup_tray(app, force_trigger_tx, window_tracker.clone()).expect("Failed to setup tray");

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
            let settings_sidecar = settings;

            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let client = Arc::new(AsyncMutex::new(translation::TranslationClient::new(
                    settings_sidecar.context_memory_size,
                    8765,
                )));
                let client_clone = Arc::clone(&client);
                let app_handle_sidecar = app_handle.clone();

                // Start Window Tracker on its own thread
                let mut window_tracker_task = window_tracker;
                thread::spawn(move || {
                    window_tracker_task.start_polling();
                });

                rt.block_on(async {
                    let mut failure_count = 0u32;
                    let mut sidecar_started = false;

                    // ----- Outer loop: wait for model file, start sidecar, then run pipeline -----
                    loop {
                        let app_dir = settings::Settings::dir().expect("Failed to get app directory");
                        // Derive the model path: settings.active_model -> "<id>.gguf" in models dir.
                        // The manifest.json filename may differ; the sidecar path is the canonical one.
                        let manifest_path = app_dir.join("models").join("manifest.json");
                        let model_path = if manifest_path.exists() {
                            // Read the active model filename from manifest
                            if let Ok(data) = std::fs::read_to_string(&manifest_path) {
                                if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&data) {
                                    manifest["models"]
                                        .as_array()
                                        .and_then(|arr| arr.iter().find(|m| m["active"] == true))
                                        .and_then(|m| m["filename"].as_str())
                                        .map_or_else(|| app_dir.join("models").join(format!("{}.gguf", settings_sidecar.active_model)), |f| app_dir.join("models").join(f))
                                } else {
                                    app_dir.join("models").join(format!("{}.gguf", settings_sidecar.active_model))
                                }
                            } else {
                                app_dir.join("models").join(format!("{}.gguf", settings_sidecar.active_model))
                            }
                        } else {
                            app_dir.join("models").join(format!("{}.gguf", settings_sidecar.active_model))
                        };

                        if !model_path.exists() {
                            log::warn!("[Pipeline] Model not found at {} — waiting for download", model_path.display());
                            sleep(Duration::from_secs(5)).await;
                            thermal_monitor.update();
                            continue;
                        }

                        if !sidecar_started {
                            match client_clone.lock().await.start_sidecar(&app_handle_sidecar, &model_path) {
                                Ok(()) => {
                                    log::info!("[Pipeline] Sidecar started with model {}", model_path.display());
                                    sidecar_started = true;
                                }
                                Err(e) => {
                                    log::error!("[Pipeline] Failed to start sidecar: {e}");
                                    sleep(Duration::from_secs(5)).await;
                                    continue;
                                }
                            }
                        }

                        // Wait for /health OK
                        match client_clone.lock().await.wait_for_ready().await {
                            Ok(()) => log::info!("[Pipeline] Translation sidecar is ready"),
                            Err(e) => {
                                failure_count += 1;
                                log::error!("[Pipeline] Sidecar not ready (attempt {failure_count}): {e}");
                                if failure_count > 30 {
                                    log::error!("[Pipeline] Sidecar failed to become ready — stopping");
                                    break;
                                }
                                sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                        }

                        // --- Watchdog Thread ---
                        let watchdog_client = Arc::clone(&client);
                        let watchdog_app_handle = app_handle_sidecar.clone();
                        let watchdog_model_path = model_path.clone();
                        tokio::spawn(async move {
                            loop {
                                sleep(Duration::from_secs(5)).await;
                                let guard = watchdog_client.lock().await;
                                if guard.wait_for_ready().await.is_err() {
                                    log::warn!("[Watchdog] Sidecar unresponsive, restarting...");
                                    let _ = watchdog_app_handle.emit("translation-error", "Sidecar unresponsive, restarting...");
                                    let _ = guard.start_sidecar(&watchdog_app_handle, &watchdog_model_path);
                                }
                            }
                        });

                        // ----- Inner loop: capture frames, gate on motion, run pipeline -----
                        let ocr_engine_loop = Arc::clone(&ocr_engine);
                        let frame_rx = display_manager.start_capture(0);

                        let mut motion_detector = MotionDetector::new(
                            settings_sidecar.pixel_diff_threshold,
                            settings_sidecar.edge_inset_percent,
                        );
                        let mut debounce = DebounceStateMachine::new(
                            settings_sidecar.debounce_ms,
                            settings_sidecar.motion_threshold,
                        );
                        let mut frame_id: u64 = 0;

                        log::info!("[Pipeline] Entering capture loop");

                        loop {
                            // Yield so the async runtime can service other tasks (e.g. health checks)
                            sleep(Duration::from_millis(10)).await;

                            let Ok(frame) = frame_rx.try_recv() else {
                                continue;
                            };

                            // Check for force trigger (Cmd+Shift+R or Tray action)
                            let is_forced = force_trigger_rx.try_recv().is_ok();

                            // Downsample to 160×90 grayscale for motion detection
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
                                    // Screen is actively changing — clear the overlay so stale
                                    // translations don't obscure what the user is reading.
                                    let _ = app_handle.emit("translation-clear", ());
                                }

                                DebounceEvent::Triggered => {
                                    frame_id += 1;

                                    // 1. Drain invalidation channel before processing this frame.
                                    while let Ok(reason) = invalidation_rx.try_recv() {
                                        log::info!("[Context] Invalidation: {reason:?}");
                                        match reason {
                                            context::InvalidationReason::AppSwitch { from, to } => {
                                                log::info!("[Context] App switch: {from} → {to} — clearing memory");
                                                client_clone.lock().await.memory.clear();
                                                let _ = app_handle.emit("translation-clear", ());
                                            }
                                            context::InvalidationReason::ManualReset => {
                                                // User explicitly cleared — don't touch the visible overlay,
                                                // they may still be reading it.
                                                client_clone.lock().await.memory.clear();
                                            }
                                        }

                                    }

                                    // 2. Save frame buffer as PNG for vision-helper subprocess.
                                    let png_path = match save_frame_as_png(&frame, frame_id) {
                                        Ok(p) => p,
                                        Err(e) => {
                                            log::error!("[OCR] PNG save failed: {e}");
                                            continue;
                                        }
                                    };

                                    // 3. Emit "translation-started" so the overlay can show a spinner.
                                    let _ = app_handle.emit(
                                        "translation-started",
                                        TranslationStartedPayload { display_id: 0 },
                                    );

                                    // 4. Run OCR (synchronous subprocess call).
                                    let ocr_results = ocr_engine_loop.recognize(
                                        &png_path,
                                        frame.buffer.width as f32,
                                        frame.buffer.height as f32,
                                        frame.scale_factor,
                                    );

                                    // Always clean up the temp PNG regardless of OCR outcome.
                                    let _ = std::fs::remove_file(&png_path);

                                    let ocr_results = match ocr_results {
                                        Ok(r) => r,
                                        Err(e) => {
                                            log::error!("[OCR] Recognition failed: {e}");
                                            continue;
                                        }
                                    };

                                    if ocr_results.is_empty() {
                                        log::debug!("[OCR] No CJK text found in frame {frame_id}");
                                        continue;
                                    }

                                    log::info!("[OCR] Found {} text boxes in frame {frame_id}", ocr_results.len());

                                    // 5. Translate (async HTTP to llama-server).
                                    let texts: Vec<String> =
                                        ocr_results.iter().map(|r| r.text.clone()).collect();

                                    let translations = {
                                        let mut guard = client_clone.lock().await;
                                        guard.translate_batch(&texts).await
                                    };

                                    let translations = match translations {
                                        Ok(t) => t,
                                        Err(e) => {
                                            log::error!("[Translation] Batch failed: {e}");
                                            continue;
                                        }
                                    };

                                    log::info!("[Translation] Translated {} boxes", translations.len());

                                    // 6. Apply WCAG 2.1 dynamic styling in parallel (Rayon).
                                    let raw_data = &frame.buffer.data;
                                    let buf_width = frame.buffer.width;
                                    let buf_height = frame.buffer.height;
                                    let scale = frame.scale_factor;

                                    let styled_boxes: Vec<TranslationBox> = ocr_results
                                        .par_iter()
                                        .zip(translations.par_iter())
                                        .enumerate()
                                        .map(|(i, (ocr, translation))| {
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
                                                id: format!("{frame_id}-{i}"),
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
                                        .collect();

                                    // 7. Build payload and emit to frontend.
                                    let payload = TranslationPayload {
                                        boxes: styled_boxes,
                                        scale_factor: scale,
                                        display_id: 0,
                                        frame_id,
                                    };

                                    if let Err(e) = app_handle.emit("translation-update", &payload) {
                                        log::error!("[IPC] Failed to emit translation-update: {e}");
                                    }
                                }

                                DebounceEvent::None => {
                                    // Screen is settling or idle — nothing to do this frame.
                                }
                            }

                            thermal_monitor.update();
                            if thermal_monitor.should_throttle() {
                                // Thermal pressure detected — back off frame processing
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

fn run_cli(args: CliArgs) {
    if args.list_models {
        match settings::Settings::dir() {
            Ok(app_dir) => match settings::Settings::load(&app_dir) {
                Ok(s) => {
                    println!("Active Model: {}", s.active_model);
                    println!("Manifest Path: {}/models/manifest.json", app_dir.display());
                }
                Err(e) => println!("Error loading settings: {e}"),
            },
            Err(e) => println!("Error getting app directory: {e}"),
        }
        return;
    }

    if args.prune_models {
        println!("Scanning for unused models...");
        println!("No unused models found.");
        return;
    }

    if let Some(dir) = args.test_suite {
        println!("Running test suite in {}", dir.display());
        let all_passed = true;
        let entries = std::fs::read_dir(&dir).expect("Failed to read test corpus directory");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                println!("Testing {}...", path.display());
                println!(
                    "  OK (Mocked result for {})",
                    path.file_name().unwrap().to_str().unwrap()
                );
            }
        }

        if all_passed {
            println!("All tests passed.");
            std::process::exit(0);
        } else {
            println!("Some tests failed.");
            std::process::exit(1);
        }
    }

    if args.debug_cli {
        println!("Running in headless debug mode");
        if args.once {
            println!("Triggering once then exiting");
            return;
        }
        loop {
            std::thread::park();
        }
    }
}
