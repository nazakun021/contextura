use std::process;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Registers the 5 global shortcuts defined in Phase 6.1.
///
/// # Errors
/// Returns an error if the shortcuts cannot be parsed or registered.
pub fn register_shortcuts(app: &tauri::App) -> anyhow::Result<()> {
    let toggle_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyT);
    let quit_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyQ);
    let force_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyR);
    let reset_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyM);
    let model_shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyG);

    app.global_shortcut()
        .on_shortcut(toggle_shortcut, |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::info!("Shortcut: Toggle overlay visibility");
            }
        })?;

    app.global_shortcut()
        .on_shortcut(quit_shortcut, |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::info!("Shortcut: Quit application");
                process::exit(0);
            }
        })?;

    app.global_shortcut()
        .on_shortcut(force_shortcut, |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::info!("Shortcut: Force OCR");
            }
        })?;

    app.global_shortcut()
        .on_shortcut(reset_shortcut, |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::info!("Shortcut: Manual reset memory");
            }
        })?;

    app.global_shortcut()
        .on_shortcut(model_shortcut, |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                log::info!("Shortcut: Trigger model switch");
            }
        })?;

    Ok(())
}
