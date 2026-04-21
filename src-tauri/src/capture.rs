use crossbeam_channel::{bounded, Sender, Receiver, TrySendError};
use screencapturekit::prelude::*;
use std::thread;

/// A stub representation of a pixel buffer from ScreenCaptureKit
#[derive(Clone)]
pub struct PixelBuffer {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub row_bytes: usize,
}

pub struct CaptureFrame {
    pub buffer: PixelBuffer,
    pub display_id: u32,
    pub scale_factor: f32,
    pub is_dirty: bool,
}

pub struct DisplayManager {}

struct OutputHandler {
    tx: Sender<CaptureFrame>,
    display_id: u32,
    scale_factor: f32,
}

impl SCStreamOutputTrait for OutputHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _type: SCStreamOutputType) {
        if let Some(pixel_buffer) = sample.image_buffer() {
            if let Ok(guard) = pixel_buffer.lock_read_only() {
                let data = guard.as_slice().to_vec();
                let width = guard.width();
                let height = guard.height();
                let row_bytes = guard.bytes_per_row();
                
                let is_dirty = true; // SCFrameStatus::Complete could be checked? Let's assume dirty
                
                let frame = CaptureFrame {
                    buffer: PixelBuffer {
                        data,
                        width,
                        height,
                        row_bytes,
                    },
                    display_id: self.display_id,
                    scale_factor: self.scale_factor,
                    is_dirty,
                };
                
                // Drop frame if channel is full
                let _ = self.tx.try_send(frame);
            }
        }
    }
}

impl DisplayManager {
    pub fn new() -> Self {
        Self {}
    }

    pub fn start_capture(&self, display_id: u32) -> Receiver<CaptureFrame> {
        let (tx, rx) = bounded::<CaptureFrame>(2);
        
        thread::spawn(move || {
            let content = SCShareableContent::get().expect("Failed to get shareable content");
            
            let display = content.displays().into_iter().find(|d| d.display_id() == display_id)
                .or_else(|| content.displays().into_iter().next())
                .expect("No displays found");
            
            let filter = SCContentFilter::create()
                .with_display(&display)
                .build();
                
            let config = SCStreamConfiguration::new()
                .with_width(display.width() as u32)
                .with_height(display.height() as u32);

            let mut stream = SCStream::new(&filter, &config);
            let handler = OutputHandler { tx, display_id, scale_factor: 2.0 };
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
