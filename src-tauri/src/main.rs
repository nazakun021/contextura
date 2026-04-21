#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod settings;

// Pipeline modules — scaffolded for the main event loop, not yet integrated.
// The #[expect] attribute (rust-best-practices Ch.2) is preferred over #[allow]
// because it will fire a *new* warning if the dead_code is ever resolved,
// reminding us to clean up the suppression.
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod capture;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod context;
mod ipc;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod motion;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod ocr;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod styling;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod thermal;
#[expect(
    dead_code,
    reason = "Scaffolded for pipeline integration, not yet wired"
)]
mod translation;

mod hotkeys;
mod tray;

use clap::Parser;
use cli::CliArgs;

fn main() {
    // Initialize logging so log::info!/error! actually emit output.
    env_logger::init();

    let args = CliArgs::parse();

    if args.is_cli_mode() {
        run_cli(args);
        return;
    }

    run_tauri();
}

fn run_cli(args: CliArgs) {
    if args.list_models {
        println!("Manifest table: (not implemented)");
        return;
    }

    if args.prune_models {
        println!("Interactive model cleanup: (not implemented)");
        return;
    }

    if let Some(dir) = args.test_suite {
        println!("Running test suite in {}", dir.display());
        let all_passed = true;

        let entries = std::fs::read_dir(&dir).expect("Failed to read test corpus directory");
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                println!("Testing {}...", path.display());
                // TODO(#1): Replace with real OCR + translation assertion logic
                println!("  OK");
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

fn run_tauri() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;

            let settings = settings::Settings::load().expect("Failed to load settings at startup");

            // Make the overlay window background truly transparent on macOS.
            // Tauri sets NSWindow.isOpaque = false, but WKWebView still draws a
            // white/black layer by default. We must clear it via KVC.
            // This requires macOSPrivateApi = true in tauri.conf.json.
            if let Some(overlay) = app.get_webview_window("overlay-main") {
                let _ = overlay.with_webview(|wv| {
                    #[cfg(target_os = "macos")]
                    {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        use objc2_foundation::{NSNumber, NSString};

                        // SAFETY: wv.inner() is a WKWebView*, which is an NSObject
                        // subclass. Casting *mut c_void → *mut AnyObject is sound
                        // because AnyObject represents any Objective-C id.
                        // setValue:forKey: with "drawsBackground" is a semi-private
                        // KVC key stable since macOS 10.10 (widely relied upon).
                        unsafe {
                            let webview_obj: *mut AnyObject = wv.inner().cast();
                            let value = NSNumber::new_bool(false);
                            let key = NSString::from_str("drawsBackground");
                            let _: () = msg_send![webview_obj, setValue: &*value, forKey: &*key];
                        }
                    }
                });
                // Window starts hidden (visible: false in tauri.conf.json).
                // Show it now that we've set it up correctly.
                let _ = overlay.show();
            }

            // Build system tray menu.
            if let Err(e) = tray::setup_tray(app) {
                log::error!("Failed to setup tray: {e}");
            }

            // Register global hotkeys.
            if let Err(e) = hotkeys::register_shortcuts(app) {
                log::error!("Failed to register hotkeys: {e}");
            }

            // First-run wizard — shown in a normal (non-transparent) window.
            if !settings.wizard_completed {
                log::info!("First launch: showing setup wizard");
                let _ = tauri::WebviewWindowBuilder::new(
                    app,
                    "wizard",
                    tauri::WebviewUrl::App("wizard.html".into()),
                )
                .title("Contextura Setup")
                .inner_size(600.0, 450.0)
                .resizable(false)
                .always_on_top(true)
                .build();
            }

            Ok(())
        })
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
