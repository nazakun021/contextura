use crossbeam_channel::Receiver;
use std::time::Instant;

use crate::capture::CaptureFrame;
use crate::motion::DebounceEvent;

/// `CaptureLoopDriver` provides the seam between sync crossbeam receivers and the async capture loop.
pub struct CaptureLoopDriver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLoopAction {
    ClearForMotion,
    RunPipeline { is_forced: bool },
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLoopAction {
    RunForcedScan { has_cached_frame: bool },
    ReloadRuntime { reason: String },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebounceLoopAction {
    RunPipeline,
    Noop,
}

pub struct CaptureLoopState {
    pub pending_force_scan: bool,
    pub latest_frame: Option<CaptureFrame>,
    pub last_frame_at: Instant,
}

impl CaptureLoopState {
    pub fn new() -> Self {
        Self {
            pending_force_scan: false,
            latest_frame: None,
            last_frame_at: Instant::now(),
        }
    }

    pub fn queue_force_scan(&mut self) {
        self.pending_force_scan = true;
    }

    pub fn record_frame(&mut self, frame: CaptureFrame) {
        self.last_frame_at = Instant::now();
        self.latest_frame = Some(frame);
    }

    pub fn take_forced_scan(&mut self) -> bool {
        std::mem::take(&mut self.pending_force_scan)
    }

    pub fn force_scan_target(&mut self) -> Option<CaptureFrame> {
        if let Some(frame) = self.latest_frame.clone() {
            self.pending_force_scan = false;
            Some(frame)
        } else {
            self.queue_force_scan();
            None
        }
    }

    pub fn note_stream_idle(&mut self, warn_after: std::time::Duration) -> bool {
        if self.last_frame_at.elapsed() > warn_after {
            self.last_frame_at = Instant::now();
            true
        } else {
            false
        }
    }
}

impl CaptureLoopDriver {
    pub fn bridge_receiver<T: Send + 'static>(
        rx_sync: Receiver<T>,
        capacity: usize,
    ) -> tokio::sync::mpsc::Receiver<T> {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        crate::async_bridge::spawn_bridge(rx_sync, tx);
        rx
    }

    pub fn handle_frame_event(
        processor: &mut crate::pipeline::PipelineProcessor,
        capture_loop_state: &mut CaptureLoopState,
        frame: CaptureFrame,
    ) -> (CaptureFrame, FrameLoopAction) {
        capture_loop_state.record_frame(frame.clone());

        let is_forced = capture_loop_state.take_forced_scan();
        let debounce_event = processor.process_motion(&frame, is_forced);

        let action = match debounce_event {
            DebounceEvent::MotionDetected => {
                if processor.was_scrolling {
                    FrameLoopAction::Noop
                } else {
                    processor.was_scrolling = true;
                    FrameLoopAction::ClearForMotion
                }
            }
            DebounceEvent::Triggered => {
                processor.was_scrolling = false;
                FrameLoopAction::RunPipeline { is_forced }
            }
            DebounceEvent::None => {
                if !matches!(
                    processor.debounce.state,
                    crate::motion::DebounceState::Scrolling
                ) {
                    processor.was_scrolling = false;
                }
                FrameLoopAction::Noop
            }
        };

        (frame, action)
    }

    pub fn handle_command_event(
        capture_loop_state: &mut CaptureLoopState,
        command: crate::scheduler::PipelineCommand,
    ) -> CommandLoopAction {
        match command {
            crate::scheduler::PipelineCommand::ForceScan => {
                let has_cached_frame = capture_loop_state.force_scan_target().is_some();
                CommandLoopAction::RunForcedScan { has_cached_frame }
            }
            crate::scheduler::PipelineCommand::ReloadRuntime { reason } => {
                CommandLoopAction::ReloadRuntime { reason }
            }
            crate::scheduler::PipelineCommand::Shutdown => CommandLoopAction::Shutdown,
        }
    }

    pub fn handle_debounce_event(
        processor: &mut crate::pipeline::PipelineProcessor,
        capture_loop_state: &CaptureLoopState,
        debounce_event: DebounceEvent,
    ) -> DebounceLoopAction {
        if matches!(debounce_event, DebounceEvent::Triggered)
            && capture_loop_state.latest_frame.is_some()
        {
            processor.was_scrolling = false;
            DebounceLoopAction::RunPipeline
        } else {
            DebounceLoopAction::Noop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureLoopDriver, CaptureLoopState, CommandLoopAction, DebounceLoopAction, FrameLoopAction,
    };
    use crate::capture::{CaptureFrame, PixelBuffer};
    use crate::motion::DebounceEvent;
    use crate::ocr::OcrEngine;
    use crate::pipeline::PipelineProcessor;
    use crate::translation::TranslationClient;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;
    use tokio::time::{Duration, timeout};

    fn sample_frame(byte: u8) -> CaptureFrame {
        CaptureFrame {
            buffer: PixelBuffer {
                data: vec![byte; 4 * 4 * 4],
                width: 4,
                height: 4,
            },
            display_id: 1,
            scale_factor: 2.0,
        }
    }

    fn sample_processor() -> PipelineProcessor {
        PipelineProcessor::new(
            10,
            0,
            50,
            0.05,
            Arc::new(OcrEngine::new(false, PathBuf::from("mock-vision"))),
            Arc::new(AsyncMutex::new(TranslationClient::new(1, 8765))),
        )
    }

    #[tokio::test]
    async fn bridge_receiver_forwards_messages() {
        let (sync_tx, sync_rx) = crossbeam_channel::bounded(2);
        let mut async_rx = CaptureLoopDriver::bridge_receiver(sync_rx, 2);

        sync_tx.send(7).expect("send should succeed");
        sync_tx.send(9).expect("second send should succeed");
        drop(sync_tx);

        let first = timeout(Duration::from_millis(200), async_rx.recv())
            .await
            .expect("first recv should not timeout");
        let second = timeout(Duration::from_millis(200), async_rx.recv())
            .await
            .expect("second recv should not timeout");

        assert_eq!(first, Some(7));
        assert_eq!(second, Some(9));
    }

    #[tokio::test]
    async fn bridge_receiver_ends_when_sync_sender_drops() {
        let (sync_tx, sync_rx) = crossbeam_channel::bounded::<u8>(1);
        let mut async_rx = CaptureLoopDriver::bridge_receiver(sync_rx, 1);

        drop(sync_tx);

        // Sender is dropped, so receiver should close.
        let msg = timeout(Duration::from_millis(200), async_rx.recv())
            .await
            .expect("recv should complete");
        assert_eq!(msg, None);
    }

    #[test]
    fn capture_loop_state_queues_and_consumes_force_scan() {
        let mut state = CaptureLoopState::new();
        assert!(!state.pending_force_scan);

        state.queue_force_scan();
        assert!(state.take_forced_scan());
        assert!(!state.pending_force_scan);
    }

    #[test]
    fn capture_loop_state_records_latest_frame() {
        let mut state = CaptureLoopState::new();
        let frame = sample_frame(9);

        state.record_frame(frame);

        assert_eq!(state.latest_frame.as_ref().map(|f| f.display_id), Some(1));
        assert_eq!(
            state.latest_frame.as_ref().map(|f| f.scale_factor),
            Some(2.0)
        );
        assert_eq!(
            state.latest_frame.as_ref().map(|f| f.buffer.data[0]),
            Some(9)
        );
    }

    #[test]
    fn capture_loop_state_force_scan_target_queues_when_no_frame() {
        let mut state = CaptureLoopState::new();

        let target = state.force_scan_target();

        assert!(target.is_none());
        assert!(state.pending_force_scan);
    }

    #[test]
    fn capture_loop_state_force_scan_target_returns_latest_frame() {
        let mut state = CaptureLoopState::new();
        state.record_frame(sample_frame(4));
        state.queue_force_scan();

        let target = state.force_scan_target();

        assert_eq!(target.as_ref().map(|f| f.buffer.data[0]), Some(4));
        assert!(!state.pending_force_scan);
    }

    #[test]
    fn capture_loop_state_warns_only_after_idle_threshold() {
        let mut state = CaptureLoopState::new();
        assert!(!state.note_stream_idle(Duration::from_secs(60)));

        state.last_frame_at = std::time::Instant::now()
            .checked_sub(Duration::from_secs(61))
            .expect("checked subtraction should succeed");
        assert!(state.note_stream_idle(Duration::from_secs(60)));
    }

    #[test]
    fn handle_frame_event_clears_on_first_motion_detection() {
        let mut processor = sample_processor();
        let mut state = CaptureLoopState::new();

        let (_frame, action) =
            CaptureLoopDriver::handle_frame_event(&mut processor, &mut state, sample_frame(255));

        assert_eq!(action, FrameLoopAction::ClearForMotion);
        assert!(processor.was_scrolling);
    }

    #[test]
    fn handle_frame_event_runs_pipeline_when_forced() {
        let mut processor = sample_processor();
        let mut state = CaptureLoopState::new();
        state.queue_force_scan();

        let (_frame, action) =
            CaptureLoopDriver::handle_frame_event(&mut processor, &mut state, sample_frame(0));

        assert_eq!(action, FrameLoopAction::RunPipeline { is_forced: true });
        assert!(!processor.was_scrolling);
    }

    #[test]
    fn handle_command_event_force_scan_reports_cached_frame_presence() {
        let mut state = CaptureLoopState::new();
        state.record_frame(sample_frame(3));

        let action = CaptureLoopDriver::handle_command_event(
            &mut state,
            crate::scheduler::PipelineCommand::ForceScan,
        );

        assert_eq!(
            action,
            CommandLoopAction::RunForcedScan {
                has_cached_frame: true
            }
        );
    }

    #[test]
    fn handle_command_event_force_scan_queues_without_frame() {
        let mut state = CaptureLoopState::new();

        let action = CaptureLoopDriver::handle_command_event(
            &mut state,
            crate::scheduler::PipelineCommand::ForceScan,
        );

        assert_eq!(
            action,
            CommandLoopAction::RunForcedScan {
                has_cached_frame: false
            }
        );
        assert!(state.pending_force_scan);
    }

    #[test]
    fn handle_command_event_preserves_reload_reason() {
        let mut state = CaptureLoopState::new();

        let action = CaptureLoopDriver::handle_command_event(
            &mut state,
            crate::scheduler::PipelineCommand::ReloadRuntime {
                reason: "manual retry".to_string(),
            },
        );

        assert_eq!(
            action,
            CommandLoopAction::ReloadRuntime {
                reason: "manual retry".to_string()
            }
        );
    }

    #[test]
    fn handle_debounce_event_runs_pipeline_when_triggered_and_frame_exists() {
        let mut processor = sample_processor();
        processor.was_scrolling = true;
        let mut state = CaptureLoopState::new();
        state.record_frame(sample_frame(8));

        let action = CaptureLoopDriver::handle_debounce_event(
            &mut processor,
            &state,
            DebounceEvent::Triggered,
        );

        assert_eq!(action, DebounceLoopAction::RunPipeline);
        assert!(!processor.was_scrolling);
    }

    #[test]
    fn handle_debounce_event_noops_without_frame() {
        let mut processor = sample_processor();
        let state = CaptureLoopState::new();

        let action = CaptureLoopDriver::handle_debounce_event(
            &mut processor,
            &state,
            DebounceEvent::Triggered,
        );

        assert_eq!(action, DebounceLoopAction::Noop);
    }
}
