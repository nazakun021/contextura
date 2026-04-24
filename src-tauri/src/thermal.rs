//! Thermal and power source monitoring.
//!
//! Uses `NSProcessInfo` for thermal state (real data) and `pmset -g batt`
//! for battery state (simpler than `IOKit` FFI, sufficient for throttle decisions).

use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

pub struct ThermalMonitor {
    pub current_state: ThermalState,
    /// `true` if running on battery (no AC adapter), `false` if plugged in.
    pub on_battery: bool,
}

impl ThermalMonitor {
    pub fn new() -> Self {
        let mut monitor = Self {
            current_state: ThermalState::Nominal,
            on_battery: false,
        };
        monitor.update();
        monitor
    }

    /// Refreshes both thermal state and battery status.
    ///
    /// Thermal state comes from `NSProcessInfo.thermalState` (fast, no I/O).
    /// Battery state is sampled via `pmset -g batt` (subprocess, ~5ms).
    /// Call this at most once per second to avoid hot-polling.
    pub fn update(&mut self) {
        // --- Thermal State (NSProcessInfo, always fast) ---
        let info = NSProcessInfo::processInfo();
        let state = info.thermalState();
        self.current_state = match state {
            NSProcessInfoThermalState::Fair => ThermalState::Fair,
            NSProcessInfoThermalState::Serious => ThermalState::Serious,
            NSProcessInfoThermalState::Critical => ThermalState::Critical,
            _ => ThermalState::Nominal,
        };

        // --- Battery State (`pmset -g batt`) ---
        // Example output:
        //   "Now drawing from 'Battery Power'\n..."  → on battery
        //   "Now drawing from 'AC Power'\n..."       → plugged in
        self.on_battery = Self::check_on_battery();
    }

    /// Returns `true` if macOS reports the machine is drawing from battery.
    ///
    /// Uses `pmset -g batt` which is always available on macOS (part of the OS).
    /// Falls back to `false` (assume plugged in) on any parse failure — the safe
    /// direction for throttle decisions since we only throttle when on battery.
    fn check_on_battery() -> bool {
        let output = Command::new("pmset").args(["-g", "batt"]).output();

        match output {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout);
                // The first line is always: "Now drawing from 'Battery Power'" or
                // "Now drawing from 'AC Power'"
                text.lines()
                    .next()
                    .is_some_and(|l| l.contains("Battery Power"))
            }
            Err(e) => {
                log::warn!("[Thermal] pmset failed: {e} — assuming AC power");
                false
            }
        }
    }

    /// Returns `true` when the pipeline should throttle (skip frames, add delays).
    ///
    /// Throttle conditions:
    /// - `Serious` or `Critical` thermal state (regardless of power source)
    /// - `Fair` thermal state AND on battery (prevents drain under load)
    pub fn should_throttle(&self) -> bool {
        matches!(
            self.current_state,
            ThermalState::Serious | ThermalState::Critical
        ) || (self.on_battery && self.current_state == ThermalState::Fair)
    }
}

impl Default for ThermalMonitor {
    fn default() -> Self {
        Self::new()
    }
}
