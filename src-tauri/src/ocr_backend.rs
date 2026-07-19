use anyhow::Context;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const OCR_HELPER_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct RawOcrObservation {
    pub text: String,
    pub confidence: f32,
    pub bounding_box: crate::ocr::Rect,
    pub text_angle: f32,
}

pub struct VisionHelperBackend {
    vision_helper_path: PathBuf,
}

impl VisionHelperBackend {
    pub fn new(vision_helper_path: PathBuf) -> Self {
        Self { vision_helper_path }
    }

    pub fn recognize(
        &self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        _cache_dir: &Path,
        _frame_id: u64,
    ) -> anyhow::Result<Vec<RawOcrObservation>> {
        let png_bytes =
            crate::snapshot::encode_frame_as_png(rgba_data, width as usize, height as usize)?;

        let child = Command::new(&self.vision_helper_path)
            .arg("--stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to launch vision-helper at {}",
                    self.vision_helper_path.display()
                )
            })?;

        let mut child = child;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&png_bytes)
                .with_context(|| "Failed to send PNG payload to vision-helper stdin")?;
        } else {
            anyhow::bail!("vision-helper stdin was not available");
        }

        let child_pid = child.id();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        let output = match rx.recv_timeout(OCR_HELPER_TIMEOUT) {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => anyhow::bail!("vision-helper I/O error: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = Command::new("kill")
                    .args(["-KILL", &child_pid.to_string()])
                    .status();
                anyhow::bail!(
                    "vision-helper timed out after {}s while processing stdin image",
                    OCR_HELPER_TIMEOUT.as_secs()
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("vision-helper worker disconnected before producing output")
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "vision-helper failed with status {}: {}",
                output.status,
                stderr.trim()
            );
        }

        let raw: Vec<crate::ocr::VisionHelperResult> = serde_json::from_slice(&output.stdout)
            .with_context(|| "vision-helper returned invalid JSON".to_string())?;

        Ok(raw
            .into_iter()
            .filter_map(|r| {
                let text = crate::ocr::OcrEngine::sanitize_text(&r.text);
                if text.is_empty() {
                    return None;
                }
                Some(RawOcrObservation {
                    text,
                    confidence: r.confidence,
                    bounding_box: crate::ocr::Rect::new(r.x, r.y, r.width, r.height),
                    text_angle: r.text_angle,
                })
            })
            .collect())
    }
}
