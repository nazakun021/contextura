use std::process;
use tauri::{
    App, Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

/// Sets up the system tray menu and event handler.
///
/// # Errors
/// Returns an error if the tray menu cannot be built or the tray icon cannot be created.
pub fn setup_tray(app: &App) -> anyhow::Result<()> {
    let toggle_i = MenuItem::with_id(
        app,
        "toggle",
        "Enable / Disable Overlay",
        true,
        None::<&str>,
    )?;
    let force_i = MenuItem::with_id(app, "force", "Translate Now", true, None::<&str>)?;
    let model_i = MenuItem::with_id(app, "model", "Active Model [Toggle]", true, None::<&str>)?;
    let clear_ctx_i =
        MenuItem::with_id(app, "clear_ctx", "Clear Context Memory", true, None::<&str>)?;
    let manage_i = MenuItem::with_id(app, "manage", "Manage Models", true, None::<&str>)?;
    let settings_i = MenuItem::with_id(app, "settings", "Open Settings...", true, None::<&str>)?;
    let help_i = MenuItem::with_id(app, "help", "Help", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle_i,
            &force_i,
            &model_i,
            &clear_ctx_i,
            &manage_i,
            &settings_i,
            &help_i,
            &quit_i,
        ],
    )?;

    TrayIconBuilder::new()
        // If the icon is missing, it skips, but we provide it here as template
        // We use the default window icon here, assuming it's loaded by Tauri
        .menu(&menu)
        .on_menu_event(move |app_handle, event| match event.id().as_ref() {
            "toggle" => {
                log::info!("Tray: Toggle overlay triggered");
            }
            "force" => {
                log::info!("Tray: Force translate triggered");
            }
            "model" => {
                log::info!("Tray: Active model toggle triggered");
            }
            "clear_ctx" => {
                log::info!("Tray: Clear context triggered");
            }
            "manage" => {
                log::info!("Tray: Manage models triggered");
            }
            "settings" => {
                log::info!("Tray: Open settings triggered");
                if let Ok(dir) = crate::settings::Settings::dir() {
                    let _ = std::process::Command::new("open").arg(&dir).spawn();
                }
            }
            "help" => {
                log::info!("Tray: Help triggered");
                // Open help window
                if app_handle.get_webview_window("help").is_none() {
                    let _ = tauri::WebviewWindowBuilder::new(
                        app_handle,
                        "help",
                        tauri::WebviewUrl::App("help.html".into()),
                    )
                    .title("Contextura Help")
                    .inner_size(800.0, 600.0)
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
