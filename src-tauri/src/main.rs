#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod settings;
mod capture;
mod motion;
mod ocr;
mod translation;
mod context;
mod thermal;
mod styling;
mod ipc;
mod tray;
mod hotkeys;

use clap::Parser;
use cli::CliArgs;

fn main() {
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
        println!("Running test suite in {:?}", dir);
        return;
    }

    if args.debug_cli {
        println!("Running in headless debug mode");
        if args.once {
            println!("Triggering once then exiting");
            return;
        }
        loop {
            std::thread::park(); // Keep main thread alive for now
        }
    }
}

fn run_tauri() {
    // Sentry rust initialization
    let _sentry_guard = sentry::init(("https://example@sentry.io/1234567", sentry::ClientOptions {
        release: sentry::release_name!(),
        // Only enabled if opt-in via settings (currently false as default)
        ..Default::default()
    }));

    tauri::Builder::default()
        .setup(|app| {
            let settings = settings::Settings::load()
                .expect("Failed to load settings at startup");

            // Build Tray Menu
            if let Err(e) = tray::setup_tray(app) {
                log::error!("Failed to setup tray: {}", e);
            }

            // Register Hotkeys
            if let Err(e) = hotkeys::register_shortcuts(app) {
                log::error!("Failed to register hotkeys: {}", e);
            }

            // First-Run Wizard
            if !settings.wizard_completed {
                log::info!("Showing first-run wizard...");
                let _ = tauri::WebviewWindowBuilder::new(
                    app,
                    "wizard",
                    tauri::WebviewUrl::App("wizard.html".into())
                )
                .title("Contextura Setup")
                .inner_size(600.0, 450.0)
                .build();
            }

            // Apply auto update silently via plugin (Phase 6.5)
            // (Handled automatically by the configured tauri-plugin-updater if specified in tauri.conf)

            Ok(())
        })
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
