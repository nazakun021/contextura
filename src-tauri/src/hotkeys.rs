// src-tauri/src/hotkeys.rs

use crossbeam_channel::Sender;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::PipelineCommand;
use crate::context::AppWindowTracker;

/// Registers all global keyboard shortcuts for the application.
///
/// # Shortcut Map
///
/// | Shortcut        | Action                                         | Status  |
/// |-----------------|------------------------------------------------|---------|
/// | Cmd+Shift+T     | Toggle overlay visibility                      | ✅ Live  |
/// | Cmd+Shift+R     | Force immediate OCR scan (bypass debounce)     | ✅ Live  |
/// | Cmd+Shift+M     | Clear translation memory (manual reset)        | ✅ Live  |
/// | Cmd+Shift+Q     | Quit application                               | ✅ Live  |
/// | Cmd+Shift+G     | Switch to the next installed model             | ✅ Live  |
///
/// # Errors
/// Returns an error if any shortcut cannot be registered with the OS.
pub fn register_shortcuts(
    app: &tauri::App,
    window_tracker: AppWindowTracker,
    pipeline_tx: Sender<PipelineCommand>,
) -> anyhow::Result<()> {
    let toggle_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyT);
    let quit_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyQ);
    let force_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyR);
    let reset_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyM);
    let model_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyG);

    // Cmd+Shift+T — toggle overlay visibility
    {
        let app_handle = app.handle().clone();
        app.global_shortcut()
            .on_shortcut(toggle_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed
                    && let Some(overlay) = app_handle.get_webview_window("overlay-main")
                {
                    let visible = overlay.is_visible().unwrap_or(false);
                    if visible {
                        let _ = overlay.hide();
                        log::info!("[Hotkey] Overlay hidden (Cmd+Shift+T)");
                    } else {
                        let _ = overlay.show();
                        log::info!("[Hotkey] Overlay shown (Cmd+Shift+T)");
                    }
                }
            })?;
    }

    // Cmd+Shift+Q — quit immediately
    {
        let app_handle = app.handle().clone();
        app.global_shortcut()
            .on_shortcut(quit_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    log::info!("[Hotkey] Quit (Cmd+Shift+Q)");
                    app_handle.exit(0);
                }
            })?;
    }

    // Cmd+Shift+R — force immediate OCR scan (bypasses debounce)
    {
        let tx = pipeline_tx.clone();
        app.global_shortcut()
            .on_shortcut(force_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    log::info!("[Hotkey] Force OCR scan (Cmd+Shift+R)");
                    let _ = tx.try_send(PipelineCommand::ForceScan);
                }
            })?;
    }

    // Cmd+Shift+M — clear translation memory (manual reset)
    {
        let tracker = window_tracker;
        app.global_shortcut()
            .on_shortcut(reset_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    log::info!("[Hotkey] Manual memory reset (Cmd+Shift+M)");
                    tracker.trigger_manual_reset();
                }
            })?;
    }

    // Cmd+Shift+G — model switch (stub until v1.1 Quality Mode)
    {
        let app_handle = app.handle().clone();
        let tx = pipeline_tx;
        app.global_shortcut()
            .on_shortcut(model_shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    match crate::request_model_switch(&app_handle, &tx) {
                        Ok(()) => log::info!("[Hotkey] Switched model (Cmd+Shift+G)"),
                        Err(error) => {
                            log::warn!("[Hotkey] Model switch unavailable: {error}");
                            crate::emit_runtime_notice(
                                &app_handle,
                                "Model Switch Unavailable",
                                "No alternate installed model was found.",
                                error.to_string(),
                                "warning",
                                5000,
                            );
                        }
                    }
                }
            })?;
    }

    Ok(())
}
