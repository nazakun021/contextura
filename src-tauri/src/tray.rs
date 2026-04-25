// src-tauri/src/tray.rs

use crossbeam_channel::Sender;
use std::process;
use tauri::{
    App, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::PipelineCommand;
use crate::context::AppWindowTracker;

/// Sets up the system tray icon, menu, and event handler.
///
/// The tray needs a pipeline channel and a `window_tracker` to provide real
/// behaviour for translation, model switching, and context clearing actions.
///
/// # Errors
/// Returns an error if the menu or tray icon cannot be constructed.
pub fn setup_tray(
    app: &App,
    pipeline_tx: Sender<PipelineCommand>,
    window_tracker: AppWindowTracker,
) -> anyhow::Result<()> {
    let toggle_i = MenuItem::with_id(
        app,
        "toggle",
        "Enable / Disable Overlay",
        true,
        None::<&str>,
    )?;
    let force_i = MenuItem::with_id(app, "force", "Translate Now", true, None::<&str>)?;
    let clear_ctx_i =
        MenuItem::with_id(app, "clear_ctx", "Clear Context Memory", true, None::<&str>)?;
    let switch_model_i =
        MenuItem::with_id(app, "switch_model", "Switch Model", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(
        app,
        "settings",
        "Open Settings Folder...",
        true,
        None::<&str>,
    )?;
    let help_i = MenuItem::with_id(app, "help", "Help", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit Contextura", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle_i,
            &force_i,
            &clear_ctx_i,
            &switch_model_i,
            &settings_i,
            &help_i,
            &quit_i,
        ],
    )?;

    TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(move |app_handle, event| match event.id().as_ref() {
            "toggle" => {
                if let Some(overlay) = app_handle.get_webview_window("overlay-main") {
                    let visible = overlay.is_visible().unwrap_or(false);
                    if visible {
                        let _ = overlay.hide();
                        log::info!("[Tray] Overlay hidden");
                    } else {
                        let _ = overlay.show();
                        log::info!("[Tray] Overlay shown");
                    }
                }
            }
            "force" => {
                log::info!("[Tray] Force translate triggered");
                let _ = pipeline_tx.try_send(PipelineCommand::ForceScan);
            }
            "clear_ctx" => {
                log::info!("[Tray] Clear context triggered");
                window_tracker.trigger_manual_reset();
            }
            "switch_model" => match crate::request_model_switch(app_handle, &pipeline_tx) {
                Ok(()) => log::info!("[Tray] Model switched"),
                Err(error) => crate::emit_runtime_notice(
                    app_handle,
                    "Model Switch Unavailable",
                    "No alternate installed model was found.",
                    error.to_string(),
                    "warning",
                    5000,
                ),
            },
            "settings" => {
                if let Ok(dir) = crate::settings::Settings::dir() {
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }
            }
            "help" => {
                if app_handle.get_webview_window("help").is_none() {
                    let _ = tauri::WebviewWindowBuilder::new(
                        app_handle,
                        "help",
                        tauri::WebviewUrl::App("help.html".into()),
                    )
                    .title("Contextura Help")
                    .inner_size(800.0, 600.0)
                    .resizable(true)
                    .build();
                }
            }
            "quit" => {
                process::exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
