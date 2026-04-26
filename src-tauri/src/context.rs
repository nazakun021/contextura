// src-tauri/src/context.rs

use crossbeam_channel::{Receiver, Sender, bounded};
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSWorkspace;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidationReason {
    AppSwitch { from: String, to: String },
    ManualReset,
}

#[derive(Clone)]
pub struct AppWindowTracker {
    invalidation_tx: Sender<InvalidationReason>,
}

impl AppWindowTracker {
    pub fn new() -> (Self, Receiver<InvalidationReason>) {
        let (tx, rx) = bounded(10);
        let tracker = Self {
            invalidation_tx: tx,
        };
        (tracker, rx)
    }

    pub fn start_polling(&mut self) {
        let tx = self.invalidation_tx.clone();

        thread::spawn(move || {
            let mut last_bundle: Option<String> = None;

            loop {
                // Polling at 200ms is snappier for tab/window switches
                thread::sleep(Duration::from_millis(200));

                autoreleasepool(|_| {
                    let workspace = NSWorkspace::sharedWorkspace();
                    let front_app = workspace.frontmostApplication();

                    let active_bundle =
                        front_app.as_ref().and_then(|app| app.bundleIdentifier().map(|id| id.to_string()));

                    if active_bundle != last_bundle {
                        if let (Some(from), Some(to)) = (&last_bundle, &active_bundle) {
                            let _ = tx.send(InvalidationReason::AppSwitch {
                                from: from.clone(),
                                to: to.clone(),
                            });
                        }
                        last_bundle = active_bundle;
                    }

                    // For tab-switch detection within the same app, we'd ideally poll the window title.
                    // Since full CGWindowList/Accessibility polling is heavy, we rely on the 
                    // ScreenCaptureKit motion detector for sub-app switches, but we still emit 
                    // the switch event if the bundle ID changed to ensure memory isolation.
                });
            }
        });
    }

    pub fn trigger_manual_reset(&self) {
        let _ = self.invalidation_tx.send(InvalidationReason::ManualReset);
    }
}
