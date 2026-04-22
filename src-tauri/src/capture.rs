use crossbeam_channel::{Receiver, Sender, bounded};
use screencapturekit::prelude::*;
use std::thread;

/// A stub representation of a pixel buffer from `ScreenCaptureKit`
#[derive(Clone)]
pub struct PixelBuffer {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

pub struct CaptureFrame {
    pub buffer: PixelBuffer,
    #[allow(dead_code)] // used by future multi-display routing (Phase 8)
    pub display_id: u32,
    pub scale_factor: f32,
}

pub struct DisplayManager {}

struct OutputHandler {
    tx: Sender<CaptureFrame>,
    display_id: u32,
    scale_factor: f32,
}

impl SCStreamOutputTrait for OutputHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _type: SCStreamOutputType) {
        if let Some(pixel_buffer) = sample.image_buffer()
            && let Ok(guard) = pixel_buffer.lock_read_only()
        {
            let data = guard.as_slice().to_vec();
            let width = guard.width();
            let height = guard.height();
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

impl DisplayManager {
    pub fn new() -> Self {
        Self {}
    }

    #[allow(clippy::unused_self)]
    pub fn start_capture(&self, display_id: u32) -> Receiver<CaptureFrame> {
        let (tx, rx) = bounded::<CaptureFrame>(2);

        thread::spawn(move || {
            let content = SCShareableContent::get().expect("Failed to get shareable content");

            let display = content
                .displays()
                .into_iter()
                .find(|d| d.display_id() == display_id)
                .or_else(|| content.displays().into_iter().next())
                .expect("No displays found");

            let filter = SCContentFilter::create().with_display(&display).build();

            let config = SCStreamConfiguration::new()
                .with_width(display.width())
                .with_height(display.height());

            let mut stream = SCStream::new(&filter, &config);
            let handler = OutputHandler {
                tx,
                display_id,
                scale_factor: 2.0, // TODO: query actual backingScaleFactor from main thread (Phase 7)
            };
            stream.add_output_handler(handler, SCStreamOutputType::Screen);
            stream.start_capture().expect("Failed to start capture");

            // Keep the thread alive
            loop {
                thread::sleep(std::time::Duration::from_secs(1));
            }
        });

        rx
    }
}
