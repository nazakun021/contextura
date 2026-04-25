use crossbeam_channel::{Receiver, Sender, bounded};
use screencapturekit::prelude::*;
use screencapturekit::shareable_content::{SCShareableContentOptions, SCWindow};

/// A copied pixel buffer from `ScreenCaptureKit`.
#[derive(Clone)]
pub struct PixelBuffer {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone)]
pub struct CaptureFrame {
    pub buffer: PixelBuffer,
    #[allow(dead_code)] // used by future multi-display routing (Phase 8)
    pub display_id: u32,
    pub scale_factor: f32,
}
struct OutputHandler {
    tx: Sender<CaptureFrame>,
    display_id: u32,
    scale_factor: f32,
}

impl SCStreamOutputTrait for OutputHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _type: SCStreamOutputType) {
        log::debug!("[Capture] Raw frame received");
        if let Some(pixel_buffer) = sample.image_buffer()
            && let Ok(guard) = pixel_buffer.lock_read_only()
        {
            let width = guard.width();
            let height = guard.height();
            let bytes_per_row = guard.bytes_per_row();

            let mut data = Vec::with_capacity(width * height * 4);
            let slice = guard.as_slice();

            if bytes_per_row == width * 4 {
                data = slice.to_vec();
            } else {
                for row in slice.chunks(bytes_per_row) {
                    if row.len() >= width * 4 {
                        data.extend_from_slice(&row[..width * 4]);
                    }
                }
            }

            let frame = CaptureFrame {
                buffer: PixelBuffer {
                    data,
                    width,
                    height,
                },
                display_id: self.display_id,
                scale_factor: self.scale_factor,
            };

            // Drop frame if channel is full
            let _ = self.tx.try_send(frame);
        }
    }
}

pub struct DisplayManager {
    stream: Option<SCStream>,
}

impl DisplayManager {
    pub fn new() -> Self {
        Self { stream: None }
    }

    pub fn start_capture(
        &mut self,
        display_id: u32,
        excluded_bundle_ids: &[&str],
        excluded_process_ids: &[i32],
        excluded_name_hints: &[&str],
    ) -> Receiver<CaptureFrame> {
        let (tx, rx) = bounded::<CaptureFrame>(2);

        if let Some(stream) = self.stream.take() {
            let _ = stream.stop_capture();
        }

        let content = SCShareableContentOptions::default()
            .with_exclude_desktop_windows(true)
            .with_on_screen_windows_only(true)
            .get()
            .expect("Failed to get shareable content");

        let display = content
            .displays()
            .into_iter()
            .find(|d: &SCDisplay| d.display_id() == display_id)
            .or_else(|| content.displays().into_iter().next())
            .expect("No displays found");

        let excluded_apps = content
            .applications()
            .into_iter()
            .filter(|app: &SCRunningApplication| {
                excluded_bundle_ids
                    .iter()
                    .any(|bundle_id| app.bundle_identifier() == *bundle_id)
                    || excluded_process_ids.contains(&app.process_id())
                    || name_matches(&app.application_name(), excluded_name_hints)
            })
            .collect::<Vec<_>>();
        let excluded_app_refs = excluded_apps.iter().collect::<Vec<_>>();
        let excluded_windows = content
            .windows()
            .into_iter()
            .filter(|window| {
                window_matches(
                    window,
                    excluded_bundle_ids,
                    excluded_process_ids,
                    excluded_name_hints,
                )
            })
            .collect::<Vec<_>>();
        let excluded_window_refs = excluded_windows.iter().collect::<Vec<_>>();
        let filter_builder = SCContentFilter::create().with_display(&display);
        let filter = if !excluded_window_refs.is_empty() {
            filter_builder
                .with_excluding_windows(&excluded_window_refs)
                .build()
        } else if excluded_app_refs.is_empty() {
            filter_builder.build()
        } else {
            filter_builder
                .with_excluding_applications(&excluded_app_refs, &[])
                .build()
        };
        let display_frame = display.frame();
        let scale_factor = if display_frame.width > 0.0 {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            {
                (f64::from(display.width()) / display_frame.width) as f32
            }
        } else {
            1.0
        };
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let actual_display_id = display.display_id();

        let config = SCStreamConfiguration::new()
            .with_width(display.width())
            .with_height(display.height())
            .with_pixel_format(PixelFormat::BGRA);

        let mut stream = SCStream::new(&filter, &config);
        let handler = OutputHandler {
            tx,
            display_id: actual_display_id,
            scale_factor,
        };
        stream.add_output_handler(handler, SCStreamOutputType::Screen);
        log::info!(
            "[Capture] Starting stream for display {} at scale factor {:.2} ({:?}); excluded apps: {:?}; excluded windows: {:?}",
            actual_display_id,
            scale_factor,
            PixelFormat::BGRA,
            excluded_bundle_ids,
            excluded_windows
                .iter()
                .map(|window| format!("{}:{:?}", window.window_id(), window.title()))
                .collect::<Vec<_>>()
        );
        stream.start_capture().expect("Failed to start capture");
        log::info!("[Capture] Stream started successfully");

        self.stream = Some(stream);

        rx
    }
}

fn name_matches(candidate: &str, hints: &[&str]) -> bool {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return false;
    }

    let candidate_lower = candidate.to_ascii_lowercase();
    hints.iter().any(|hint| {
        let hint = hint.trim();
        !hint.is_empty() && candidate_lower.contains(&hint.to_ascii_lowercase())
    })
}

fn window_matches(
    window: &SCWindow,
    excluded_bundle_ids: &[&str],
    excluded_process_ids: &[i32],
    excluded_name_hints: &[&str],
) -> bool {
    let title = window.title().unwrap_or_default();
    if name_matches(&title, excluded_name_hints) {
        return true;
    }

    let Some(app) = window.owning_application() else {
        return false;
    };

    excluded_bundle_ids
        .iter()
        .any(|bundle_id| app.bundle_identifier() == *bundle_id)
        || excluded_process_ids.contains(&app.process_id())
        || name_matches(&app.application_name(), excluded_name_hints)
}
