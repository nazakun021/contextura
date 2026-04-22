use crossbeam_channel::{Receiver, Sender, bounded};
use std::thread;
use std::time::Duration;
use objc2_app_kit::NSWorkspace;

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
            // NSWorkspace must be accessed from the main thread marker or a thread that can safely interact with it.
            // Since we are just polling properties, we'll use a loop.
            loop {
                thread::sleep(Duration::from_millis(500));

                let workspace = NSWorkspace::sharedWorkspace();
                let front_app = workspace.frontmostApplication();
                
                let active_bundle = front_app.and_then(|app| { 
                    app.bundleIdentifier().map(|id| id.to_string()) 
                });

                if active_bundle != last_bundle {
                    if let (Some(from), Some(to)) = (&last_bundle, &active_bundle) {
                        let _ = tx.send(InvalidationReason::AppSwitch {
                            from: from.clone(),
                            to: to.clone(),
                        });
                    }
                    last_bundle = active_bundle;
                }
            }
        });
    }

    pub fn trigger_manual_reset(&self) {
        let _ = self.invalidation_tx.send(InvalidationReason::ManualReset);
    }
}
