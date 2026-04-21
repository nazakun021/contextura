#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

pub struct ThermalMonitor {
    pub current_state: ThermalState,
    pub on_battery: bool,
}

impl ThermalMonitor {
    pub fn new() -> Self {
        Self {
            current_state: ThermalState::Nominal,
            on_battery: false,
        }
    }

    pub fn should_throttle(&self) -> bool {
        self.on_battery && matches!(self.current_state, ThermalState::Serious | ThermalState::Critical)
    }
}

impl Default for ThermalMonitor {
    fn default() -> Self {
        Self::new()
    }
}
