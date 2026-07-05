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
mod path_resolver;
mod scheduler;
mod snapshot;
mod tray;

pub use scheduler::{PipelineCommand, emit_runtime_notice, request_model_switch};

use crossbeam_channel::Sender;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindow, NSWindowSharingType};
use std::sync::Arc;

use crate::ipc::WizardStatusPayload;
use crate::path_resolver::resolve_vision_helper_path;
use clap::Parser;
use cli::CliArgs;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn complete_wizard(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    pipeline_tx: tauri::State<'_, Sender<PipelineCommand>>,
) -> Result<(), String> {
    use tauri::Manager;
    let app_dir = crate::settings::Settings::dir().map_err(|e| e.to_string())?;
    let mut settings = crate::settings::Settings::load(&app_dir).map_err(|e| e.to_string())?;
    settings.wizard_completed = true;
    settings.save(&app_dir).map_err(|e| e.to_string())?;

    let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
        reason: "Wizard completed".to_string(),
    });

    if let Some(overlay) = app.get_webview_window("overlay-main") {
        let _ = overlay.show();
    }

    window.close().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn reload_runtime(
    pipeline_tx: tauri::State<'_, Sender<PipelineCommand>>,
) {
    let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
        reason: "UI requested reload".to_string(),
    });
}

#[tauri::command]
fn wizard_status() -> Result<WizardStatusPayload, String> {
    scheduler::load_wizard_status()
}

#[tauri::command]
fn open_models_folder_command() -> Result<(), String> {
    scheduler::open_models_folder()
}

#[tauri::command]
fn open_screen_recording_settings() -> Result<(), String> {
    scheduler::open_screen_recording_settings_impl()
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
        cli::run_cli(&args);
        return;
    }



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
            open_screen_recording_settings,
            reload_runtime
        ])
        .setup(move |app| {
            use tauri::Manager;

            let cache_dir = app.path().app_cache_dir().expect("Failed to get cache dir");
            let _ = std::fs::create_dir_all(&cache_dir);
            snapshot::cleanup_stale_temp_frames(&cache_dir);

            let app_dir = crate::settings::Settings::dir().expect("Failed to get app directory");
            let startup_settings = crate::settings::Settings::load(&app_dir)
                .expect("Failed to load settings at startup");
            let vision_helper_path =
                resolve_vision_helper_path(app).expect("Failed to resolve vision-helper path");
            let app_bundle_id = app.config().identifier.clone();
            let app_process_id = i32::try_from(std::process::id()).unwrap_or_default();
            let app_name_hint = app.package_info().name.clone();

            // --- Subsystem Initialization ---
            let (window_tracker, invalidation_rx) = context::AppWindowTracker::new();
            let ocr_engine = Arc::new(ocr::OcrEngine::new(
                startup_settings.furigana_suppression,
                vision_helper_path,
            ));
            let display_manager = capture::DisplayManager::new();
            let (pipeline_tx, pipeline_rx) = crossbeam_channel::bounded(16);
            app.manage(pipeline_tx.clone());
            *pipeline_tx_setup
                .lock()
                .expect("pipeline exit handle lock poisoned") = Some(pipeline_tx.clone());

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
                snapshot::cleanup_stale_temp_frames(&cache_dir);
                default_hook(panic_info);
            }));

            // --- Pipeline Orchestration ---
            scheduler::start_scheduler(scheduler::SchedulerConfig {
                app_handle: app.handle().clone(),
                app_bundle_id,
                app_process_id,
                app_name_hint,
                initial_memory_size: startup_settings.context_memory_size,
                window_tracker,
                invalidation_rx,
                ocr_engine,
                display_manager,
                pipeline_tx,
                pipeline_rx,
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) && let Some(tx) = pipeline_tx_for_exit
                .lock()
                .expect("pipeline exit handle lock poisoned")
                .as_ref()
                .cloned()
            {
                let _ = tx.try_send(PipelineCommand::Shutdown);
            }
        });
}
