use crossbeam_channel::Sender;
use std::process;
use tauri::{
    App, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::context::AppWindowTracker;

/// Sets up the system tray icon, menu, and event handler.
///
/// The tray needs a `force_trigger_tx` channel and a `window_tracker` to
/// provide real behaviour for "Translate Now" and "Clear Context Memory".
///
/// # Errors
/// Returns an error if the menu or tray icon cannot be constructed.
pub fn setup_tray(
    app: &App,
    force_trigger_tx: Sender<()>,
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
    let settings_i = MenuItem::with_id(app, "settings", "Open Settings Folder...", true, None::<&str>)?;
    let help_i = MenuItem::with_id(app, "help", "Help", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit Contextura", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle_i,
            &force_i,
            &clear_ctx_i,
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
                let _ = force_trigger_tx.try_send(());
            }
            "clear_ctx" => {
                log::info!("[Tray] Clear context triggered");
                window_tracker.trigger_manual_reset();
            }
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
