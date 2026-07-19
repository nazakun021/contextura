use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub settings: crate::settings::Settings,
    pub active_model: crate::models::ModelStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyMode {
    Normal,
    Retry,
}

pub trait RuntimeCoordinator {
    fn load_runtime_state(&self, app_dir: &Path) -> anyhow::Result<RuntimeState>;

    fn should_restart_sidecar_for_model_change(
        &self,
        current: Option<&RuntimeState>,
        next: &RuntimeState,
    ) -> bool;

    fn apply_runtime_settings(
        &self,
        processor: &mut crate::pipeline::PipelineProcessor,
        settings: &crate::settings::Settings,
        on_battery: bool,
    );

    fn ready_mode(&self, failure_count: u32) -> ReadyMode;

    fn should_halt_startup(&self, failure_count: u32) -> bool;

    fn handle_halt_command(
        &self,
        command: crate::scheduler::PipelineCommand,
        failure_count: &mut u32,
        runtime_reload_requested: &mut bool,
        sidecar_started: &mut bool,
    ) -> bool;
}

pub struct RuntimeLoopState {
    pub failure_count: u32,
    pub sidecar_started: bool,
    pub warned_missing_model: bool,
    pub active_model_id: String,
    pub runtime_state: Option<RuntimeState>,
    pub runtime_reload_requested: bool,
    pub last_thermal_check: Instant,
    pub thermal_monitor: crate::thermal::ThermalMonitor,
}

impl RuntimeLoopState {
    pub fn new() -> Self {
        Self {
            failure_count: 0,
            sidecar_started: false,
            warned_missing_model: false,
            active_model_id: String::new(),
            runtime_state: None,
            runtime_reload_requested: true,
            last_thermal_check: Instant::now()
                .checked_sub(Duration::from_secs(31))
                .unwrap_or_else(Instant::now),
            thermal_monitor: crate::thermal::ThermalMonitor::new(),
        }
    }

    pub fn should_refresh_runtime(&self) -> bool {
        self.runtime_reload_requested || self.runtime_state.is_none()
    }

    pub fn apply_loaded_runtime_state<C: RuntimeCoordinator>(
        &mut self,
        coordinator: &C,
        state: RuntimeState,
        processor: &mut crate::pipeline::PipelineProcessor,
    ) {
        if coordinator.should_restart_sidecar_for_model_change(self.runtime_state.as_ref(), &state)
        {
            self.sidecar_started = false;
        }
        coordinator.apply_runtime_settings(
            processor,
            &state.settings,
            self.thermal_monitor.on_battery,
        );
        self.runtime_state = Some(state);
        self.runtime_reload_requested = false;
    }

    pub fn note_model_ready(&mut self, model_id: &str) {
        if self.active_model_id != model_id {
            self.sidecar_started = false;
            self.active_model_id = model_id.to_string();
        }
        self.warned_missing_model = false;
    }

    pub fn note_missing_model_warning(&mut self) {
        self.warned_missing_model = true;
    }

    pub fn note_sidecar_started(&mut self) {
        self.sidecar_started = true;
    }

    pub fn note_sidecar_failure(&mut self) {
        self.failure_count += 1;
        self.sidecar_started = false;
    }

    pub fn note_sidecar_ready(&mut self) {
        self.failure_count = 0;
    }
}

pub struct DefaultRuntimeCoordinator;

impl RuntimeCoordinator for DefaultRuntimeCoordinator {
    fn load_runtime_state(&self, app_dir: &Path) -> anyhow::Result<RuntimeState> {
        let loaded_settings = crate::settings::Settings::load(app_dir)?;
        let active_model = crate::models::active_model_status(app_dir, &loaded_settings)?;
        Ok(RuntimeState {
            settings: loaded_settings,
            active_model,
        })
    }

    fn should_restart_sidecar_for_model_change(
        &self,
        current: Option<&RuntimeState>,
        next: &RuntimeState,
    ) -> bool {
        current.is_none_or(|state| state.active_model.entry.id != next.active_model.entry.id)
    }

    fn apply_runtime_settings(
        &self,
        processor: &mut crate::pipeline::PipelineProcessor,
        settings: &crate::settings::Settings,
        on_battery: bool,
    ) {
        processor.update_settings(
            if on_battery {
                1200
            } else {
                settings.debounce_ms
            },
            settings.motion_threshold,
            settings.pixel_diff_threshold,
            settings.edge_inset_percent,
        );
    }

    fn ready_mode(&self, failure_count: u32) -> ReadyMode {
        if failure_count == 0 {
            ReadyMode::Normal
        } else {
            ReadyMode::Retry
        }
    }

    fn should_halt_startup(&self, failure_count: u32) -> bool {
        failure_count > 5
    }

    fn handle_halt_command(
        &self,
        command: crate::scheduler::PipelineCommand,
        failure_count: &mut u32,
        runtime_reload_requested: &mut bool,
        sidecar_started: &mut bool,
    ) -> bool {
        match command {
            crate::scheduler::PipelineCommand::ReloadRuntime { .. } => {
                *failure_count = 0;
                *runtime_reload_requested = true;
                *sidecar_started = false;
                false
            }
            crate::scheduler::PipelineCommand::Shutdown => true,
            crate::scheduler::PipelineCommand::ForceScan => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DefaultRuntimeCoordinator, ReadyMode, RuntimeCoordinator};
    use super::{RuntimeLoopState, RuntimeState};
    use crate::models::{ModelEntry, ModelStatus};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex as AsyncMutex;

    fn fake_runtime_state(id: &str, debounce_ms: u64) -> RuntimeState {
        RuntimeState {
            settings: crate::settings::Settings {
                debounce_ms,
                ..Default::default()
            },
            active_model: ModelStatus {
                entry: ModelEntry {
                    id: id.to_string(),
                    filename: format!("{id}.gguf"),
                    label: id.to_string(),
                    tier: "Standard".to_string(),
                    active: true,
                    strategy: Some("qwen".to_string()),
                },
                path: PathBuf::from(format!("/tmp/{id}.gguf")),
                installed: true,
            },
        }
    }

    fn dummy_processor() -> crate::pipeline::PipelineProcessor {
        crate::pipeline::PipelineProcessor::new(
            0,
            0,
            0,
            0.0,
            Arc::new(crate::ocr::OcrEngine::new(
                false,
                PathBuf::from("mock-vision"),
            )),
            Arc::new(AsyncMutex::new(crate::translation::TranslationClient::new(
                1, 8765,
            ))),
        )
    }

    #[test]
    fn ready_mode_switches_after_first_failure() {
        let coordinator = DefaultRuntimeCoordinator;
        assert_eq!(coordinator.ready_mode(0), ReadyMode::Normal);
        assert_eq!(coordinator.ready_mode(1), ReadyMode::Retry);
    }

    #[test]
    fn startup_halt_only_after_threshold() {
        let coordinator = DefaultRuntimeCoordinator;
        assert!(!coordinator.should_halt_startup(5));
        assert!(coordinator.should_halt_startup(6));
    }

    #[test]
    fn loop_state_marks_refresh_complete_and_updates_processor_settings() {
        let coordinator = DefaultRuntimeCoordinator;
        let mut loop_state = RuntimeLoopState::new();
        let mut processor = dummy_processor();

        let state = fake_runtime_state("model-a", 275);
        loop_state.apply_loaded_runtime_state(&coordinator, state, &mut processor);

        assert!(!loop_state.runtime_reload_requested);
        assert!(loop_state.runtime_state.is_some());
        assert_eq!(
            processor.debounce.debounce_duration,
            Duration::from_millis(275)
        );
    }

    #[test]
    fn loop_state_tracks_model_and_failure_transitions() {
        let mut loop_state = RuntimeLoopState::new();

        loop_state.note_model_ready("model-a");
        assert_eq!(loop_state.active_model_id, "model-a");
        assert!(!loop_state.warned_missing_model);

        loop_state.note_missing_model_warning();
        assert!(loop_state.warned_missing_model);

        loop_state.note_sidecar_started();
        assert!(loop_state.sidecar_started);

        loop_state.note_sidecar_failure();
        assert_eq!(loop_state.failure_count, 1);
        assert!(!loop_state.sidecar_started);

        loop_state.note_sidecar_ready();
        assert_eq!(loop_state.failure_count, 0);
    }

    #[test]
    fn halt_command_reload_resets_runtime_for_retry() {
        let coordinator = DefaultRuntimeCoordinator;
        let mut failure_count = 3;
        let mut runtime_reload_requested = false;
        let mut sidecar_started = true;

        let should_exit = coordinator.handle_halt_command(
            crate::scheduler::PipelineCommand::ReloadRuntime {
                reason: "manual retry".to_string(),
            },
            &mut failure_count,
            &mut runtime_reload_requested,
            &mut sidecar_started,
        );

        assert!(!should_exit);
        assert_eq!(failure_count, 0);
        assert!(runtime_reload_requested);
        assert!(!sidecar_started);
    }

    #[test]
    fn halt_command_shutdown_requests_exit() {
        let coordinator = DefaultRuntimeCoordinator;
        let mut failure_count = 3;
        let mut runtime_reload_requested = false;
        let mut sidecar_started = true;

        let should_exit = coordinator.handle_halt_command(
            crate::scheduler::PipelineCommand::Shutdown,
            &mut failure_count,
            &mut runtime_reload_requested,
            &mut sidecar_started,
        );

        assert!(should_exit);
        assert_eq!(failure_count, 3);
        assert!(!runtime_reload_requested);
        assert!(sidecar_started);
    }
}
