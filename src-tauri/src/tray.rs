// src-tauri/src/tray.rs

use crossbeam_channel::Sender;
use tauri::{
    App, Manager,
    Emitter,
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::TrayIconBuilder,
};

use crate::PipelineCommand;
use crate::context::AppWindowTracker;
use crate::ipc::OverlaySettingsPayload;
use crate::models::ModelManifest;
use crate::settings::{OverlayPlacement, Settings};

fn tray_icon() -> tauri::image::Image<'static> {
    const SIZE: usize = 18;
    const IMAGE_SIZE: u32 = 18;
    let mut pixels = vec![0; SIZE * SIZE * 4];

    for y in 3..13 {
        for x in 3..15 {
            let rounded_corner = (x == 3 || x == 14) && (y == 3 || y == 12);
            if rounded_corner {
                continue;
            }
            let index = (y * SIZE + x) * 4;
            pixels[index + 3] = u8::MAX;
        }
    }

    for (x, y) in [(8, 13), (8, 14), (9, 13)] {
        let index = (y * SIZE + x) * 4;
        pixels[index + 3] = u8::MAX;
    }

    tauri::image::Image::new_owned(pixels, IMAGE_SIZE, IMAGE_SIZE)
}

fn build_model_menu<R: tauri::Runtime>(
    app: &App<R>,
    app_dir: &std::path::Path,
    settings: &Settings,
) -> anyhow::Result<Submenu<R>> {
    let manifest = ModelManifest::load(app_dir, settings)?;
    let model_items = manifest
        .statuses(app_dir)
        .into_iter()
        .filter(|status| status.installed)
        .map(|status| {
            CheckMenuItem::with_id(
                app,
                format!("model:{}", status.entry.id),
                status.entry.display_label(),
                true,
                status.entry.active,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let model_item_refs = model_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<_>)
        .collect::<Vec<_>>();
    Submenu::with_items(app, "Model", true, &model_item_refs).map_err(Into::into)
}

fn build_placement_menu<R: tauri::Runtime>(
    app: &App<R>,
    placement: OverlayPlacement,
) -> anyhow::Result<Submenu<R>> {
    let cover_i = CheckMenuItem::with_id(
        app,
        "placement:cover",
        "Cover original text",
        true,
        placement == OverlayPlacement::Cover,
        None::<&str>,
    )?;
    let above_i = CheckMenuItem::with_id(
        app,
        "placement:above",
        "Above original text",
        true,
        placement == OverlayPlacement::Above,
        None::<&str>,
    )?;
    let below_i = CheckMenuItem::with_id(
        app,
        "placement:below",
        "Below original text",
        true,
        placement == OverlayPlacement::Below,
        None::<&str>,
    )?;
    Submenu::with_items(app, "Translation placement", true, &[&cover_i, &above_i, &below_i])
        .map_err(Into::into)
}

fn select_model(app_handle: &tauri::AppHandle, pipeline_tx: &Sender<PipelineCommand>, model_id: &str) {
    let result = (|| -> anyhow::Result<()> {
        let app_dir = Settings::dir()?;
        let mut settings = Settings::load(&app_dir)?;
        let switch = crate::models::select_active_model(&app_dir, &mut settings, model_id)?;
        crate::emit_runtime_notice(
            app_handle,
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
            reason: format!("Selected {}", switch.current.entry.display_label()),
        });
        Ok(())
    })();
    if let Err(error) = result {
        crate::emit_runtime_notice(
            app_handle,
            "Model Selection Unavailable",
            "The selected model could not be activated.",
            error.to_string(),
            "warning",
            5000,
        );
    }
}

fn update_placement(app_handle: &tauri::AppHandle, placement: OverlayPlacement) {
    let result = (|| -> anyhow::Result<()> {
        let app_dir = Settings::dir()?;
        let mut settings = Settings::load(&app_dir)?;
        settings.overlay_placement = placement;
        settings.save(&app_dir)?;
        app_handle.emit(
            "overlay-settings-update",
            OverlaySettingsPayload { placement },
        )?;
        Ok(())
    })();
    if let Err(error) = result {
        crate::emit_runtime_notice(
            app_handle,
            "Placement Update Failed",
            "The translation placement could not be updated.",
            error.to_string(),
            "warning",
            5000,
        );
    }
}

fn handle_menu_event(
    app_handle: &tauri::AppHandle,
    menu_id: &str,
    pipeline_tx: &Sender<PipelineCommand>,
    window_tracker: &AppWindowTracker,
) {
    match menu_id {
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
        "switch_model" => match crate::request_model_switch(app_handle, pipeline_tx) {
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
        "placement:cover" => update_placement(app_handle, OverlayPlacement::Cover),
        "placement:above" => update_placement(app_handle, OverlayPlacement::Above),
        "placement:below" => update_placement(app_handle, OverlayPlacement::Below),
        "retry" => {
            log::info!("[Tray] Manual retry requested");
            let _ = pipeline_tx.try_send(PipelineCommand::ReloadRuntime {
                reason: "Tray retry click".to_string(),
            });
        }
        "settings" => {
            if let Ok(dir) = Settings::dir() {
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
        "quit" => app_handle.exit(0),
        id if id.starts_with("model:") => select_model(app_handle, pipeline_tx, id.trim_start_matches("model:")),
        _ => {}
    }
}

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
    let settings_dir = Settings::dir()?;
    let settings = Settings::load(&settings_dir)?;
    let model_menu = build_model_menu(app, &settings_dir, &settings)?;
    let placement_menu = build_placement_menu(app, settings.overlay_placement)?;

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
    let retry_i = MenuItem::with_id(app, "retry", "Retry Connecting Engine", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit Contextura", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle_i,
            &force_i,
            &clear_ctx_i,
            &switch_model_i,
            &model_menu,
            &placement_menu,
            &retry_i,
            &settings_i,
            &help_i,
            &quit_i,
        ],
    )?;

    TrayIconBuilder::new()
        .icon(tray_icon())
        .icon_as_template(true)
        .tooltip("Contextura")
        .menu(&menu)
        .on_menu_event(move |app_handle, event| {
            handle_menu_event(
                app_handle,
                event.id().as_ref(),
                &pipeline_tx,
                &window_tracker,
            );
        })
        .build(app)?;

    Ok(())
}
