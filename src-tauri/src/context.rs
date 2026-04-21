use crossbeam_channel::{Receiver, Sender, bounded};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidationReason {
    AppSwitch { from: String, to: String },
    ManualReset,
    ModelSwitch,
}

pub struct AppWindowTracker {
    current_bundle_id: Option<String>,
    invalidation_tx: Sender<InvalidationReason>,
}

impl AppWindowTracker {
    pub fn new() -> (Self, Receiver<InvalidationReason>) {
        let (tx, rx) = bounded(10);
        let tracker = Self {
            current_bundle_id: None,
            invalidation_tx: tx,
        };
        (tracker, rx)
    }

    pub fn start_polling(&mut self) {
        let tx = self.invalidation_tx.clone();

        // Mock polling for active window
        thread::spawn(move || {
            let mut last_bundle: Option<String> = None;
            loop {
                // In a real implementation this would use NSWorkspace or similar
                // Here we just sleep to avoid spinning
                thread::sleep(Duration::from_secs(2));

                // MOCK implementation
                let active_bundle = Some("com.apple.Safari".to_string());

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

    pub fn trigger_model_switch(&self) {
        let _ = self.invalidation_tx.send(InvalidationReason::ModelSwitch);
    }
}
