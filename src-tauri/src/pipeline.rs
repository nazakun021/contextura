// src-tauri/src/pipeline.rs

use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use crate::capture::CaptureFrame;
use crate::ipc::{TranslationBox, TranslationPayload};
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::snapshot::save_frame_as_png;
use crate::styling::StylingEngine;

pub struct ProcessResult {
    pub payload: Option<TranslationPayload>,
    pub clear_context: bool,
    pub manual_reset: bool,
}

pub struct PipelineProcessor {
    pub(crate) motion_detector: MotionDetector,
    pub(crate) debounce: DebounceStateMachine,
    pub(crate) ocr_engine: Arc<crate::ocr::OcrEngine>,
    pub(crate) client: Arc<AsyncMutex<crate::translation::TranslationClient>>,
}

impl PipelineProcessor {
    pub fn new(
        pixel_diff_threshold: u8,
        edge_inset_percent: u32,
        debounce_ms: u64,
        motion_threshold: f32,
        ocr_engine: Arc<crate::ocr::OcrEngine>,
        client: Arc<AsyncMutex<crate::translation::TranslationClient>>,
    ) -> Self {
        Self {
            motion_detector: MotionDetector::new(pixel_diff_threshold, edge_inset_percent),
            debounce: DebounceStateMachine::new(debounce_ms, motion_threshold),
            ocr_engine,
            client,
        }
    }

    pub fn update_settings(
        &mut self,
        debounce_ms: u64,
        motion_threshold: f32,
        pixel_diff_threshold: u8,
        edge_inset_percent: u32,
    ) {
        self.debounce = DebounceStateMachine::new(debounce_ms, motion_threshold);
        self.motion_detector = MotionDetector::new(pixel_diff_threshold, edge_inset_percent);
    }

    pub fn process_motion(&mut self, frame: &CaptureFrame, is_forced: bool) -> DebounceEvent {
        if is_forced {
            return DebounceEvent::Triggered;
        }

        let rgba_data = &frame.buffer.data;
        let thumbnail =
            self.motion_detector
                .downsample(rgba_data, frame.buffer.width, frame.buffer.height);
        let motion_ratio = self.motion_detector.process_thumbnail(&thumbnail);
        self.debounce.update(motion_ratio)
    }

    #[allow(clippy::too_many_lines)]
    pub async fn process_frame(
        &self,
        cache_dir: &Path,
        frame: &CaptureFrame,
        frame_id: u64,
        invalidation_rx: &crossbeam_channel::Receiver<crate::context::InvalidationReason>,
        pipeline_tx: &crossbeam_channel::Sender<crate::scheduler::PipelineCommand>,
    ) -> ProcessResult {
        let mut clear_context = false;
        let mut manual_reset = false;

        // 1. Drain context invalidations
        while let Ok(reason) = invalidation_rx.try_recv() {
            log::info!("[Context] Invalidation: {reason:?}");
            match reason {
                crate::context::InvalidationReason::AppSwitch { from, to } => {
                    log::info!("[Context] App switch: {from} -> {to} — clearing memory");
                    self.client.lock().await.memory.clear();
                    clear_context = true;
                }
                crate::context::InvalidationReason::ManualReset => {
                    self.client.lock().await.memory.clear();
                    manual_reset = true;
                }
            }
        }

        // 2. Save frame snapshot
        let rgba_data = &frame.buffer.data;
        let png_path = match save_frame_as_png(
            rgba_data,
            frame.buffer.width,
            frame.buffer.height,
            frame_id,
            cache_dir,
        ) {
            Ok(path) => path,
            Err(error) => {
                log::error!("[OCR] PNG save failed: {error}");
                return ProcessResult {
                    payload: None,
                    clear_context,
                    manual_reset,
                };
            }
        };

        // 3. Pre-flight health check
        if self.client.lock().await.quick_health_check().await.is_err() {
            log::warn!(
                "[Translation] Pre-flight health check failed — skipping frame and requesting runtime reload"
            );
            let _ = pipeline_tx.try_send(crate::scheduler::PipelineCommand::ReloadRuntime {
                reason: "health check failed before batch".to_string(),
            });
            let _ = std::fs::remove_file(&png_path);
            return ProcessResult {
                payload: None,
                clear_context,
                manual_reset,
            };
        }

        // 4. Run OCR
        #[allow(clippy::cast_precision_loss)]
        let ocr_results = self.ocr_engine.recognize(
            &png_path,
            frame.buffer.width as f32,
            frame.buffer.height as f32,
            frame.scale_factor,
        );
        let _ = std::fs::remove_file(&png_path);

        let ocr_results = match ocr_results {
            Ok(results) => results,
            Err(error) => {
                log::error!("[OCR] Recognition failed: {error}");
                return ProcessResult {
                    payload: None,
                    clear_context,
                    manual_reset,
                };
            }
        };

        if ocr_results.is_empty() {
            log::debug!("[OCR] No CJK text found in frame {frame_id}");
            return ProcessResult {
                payload: None,
                clear_context,
                manual_reset,
            };
        }

        // 5. Run concurrent styling and translation
        let styled_boxes = match self
            .process_concurrent_translation_and_styling(
                &ocr_results,
                rgba_data,
                frame.buffer.width,
                frame.buffer.height,
                frame.scale_factor,
                frame_id,
            )
            .await
        {
            Ok(boxes) => boxes,
            Err(error) => {
                log::error!("[Pipeline] Concurrent styling and translation failed: {error}");
                return ProcessResult {
                    payload: None,
                    clear_context,
                    manual_reset,
                };
            }
        };

        let payload = TranslationPayload {
            boxes: styled_boxes,
            scale_factor: frame.scale_factor,
            display_id: frame.display_id,
            frame_id,
        };

        ProcessResult {
            payload: Some(payload),
            clear_context,
            manual_reset,
        }
    }

    pub async fn process_concurrent_translation_and_styling(
        &self,
        ocr_results: &[crate::ocr::OcrResult],
        rgba_data: &[u8],
        buf_width: usize,
        buf_height: usize,
        scale: f32,
        frame_id: u64,
    ) -> anyhow::Result<Vec<TranslationBox>> {
        if ocr_results.is_empty() {
            return Ok(vec![]);
        }

        let texts = ocr_results
            .iter()
            .map(|result| result.text.clone())
            .collect::<Vec<_>>();

        // styling sampling in Rayon threadpool
        let ocr_results_clone = ocr_results.to_vec();
        let rgba_data_clone = rgba_data.to_vec();
        let styling_task = tokio::task::spawn_blocking(move || {
            ocr_results_clone
                .par_iter()
                .map(|ocr| {
                    let bg = StylingEngine::sample_rect_ring(
                        &rgba_data_clone,
                        buf_width,
                        buf_height,
                        ocr.bounding_box.x,
                        ocr.bounding_box.y,
                        ocr.bounding_box.width,
                        ocr.bounding_box.height,
                        scale,
                    );
                    let fg_color = StylingEngine::get_fg_color(bg.r, bg.g, bg.b);
                    (bg, fg_color)
                })
                .collect::<Vec<_>>()
        });

        // concurrent translation request
        let (translations_res, styling_res) = tokio::join!(
            async {
                let mut guard = self.client.lock().await;
                guard.translate_batch(&texts).await
            },
            styling_task
        );

        let translations = translations_res?;
        let styling_colors =
            styling_res.map_err(|e| anyhow::anyhow!("Styling thread join failed: {e}"))?;

        if translations.len() != ocr_results.len() {
            anyhow::bail!(
                "Translation batch count mismatch: expected {}, got {}",
                ocr_results.len(),
                translations.len()
            );
        }

        let styled_boxes = ocr_results
            .iter()
            .zip(translations.iter())
            .zip(styling_colors.iter())
            .enumerate()
            .map(
                |(index, ((ocr, translation), (bg, fg_color)))| TranslationBox {
                    id: format!("{frame_id}-{index}"),
                    translated: translation.clone(),
                    original: ocr.text.clone(),
                    x: ocr.bounding_box.x,
                    y: ocr.bounding_box.y,
                    width: ocr.bounding_box.width,
                    height: ocr.bounding_box.height,
                    is_vertical: ocr.is_vertical,
                    bg_color: bg.to_css_color(),
                    fg_color: fg_color.to_string(),
                    confidence: ocr.confidence,
                },
            )
            .collect::<Vec<_>>();

        Ok(styled_boxes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureFrame, PixelBuffer};
    use crate::ocr::OcrEngine;
    use crate::translation::TranslationClient;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn test_pipeline_motion_debounce_flow() {
        let ocr = Arc::new(OcrEngine::new(false, PathBuf::from("mock-vision")));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, 8765)));
        let mut processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);

        let frame1 = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![0u8; 100 * 100 * 4],
                width: 100,
                height: 100,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let frame2 = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![255u8; 100 * 100 * 4],
                width: 100,
                height: 100,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let ev1 = processor.process_motion(&frame1, false);
        assert_eq!(ev1, DebounceEvent::None);

        let ev2 = processor.process_motion(&frame2, false);
        assert_eq!(ev2, DebounceEvent::MotionDetected);
    }

    #[tokio::test]
    async fn test_pipeline_async_frame_flow() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("contextura-test-{unique}"));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. Create a mock vision-helper executable script
        let mock_vision_path = temp_dir.join("mock-vision-helper");
        let script_content = "#!/bin/sh\necho '[{\"text\": \"日本語のは\", \"confidence\": 1.0, \"x\": 0.1, \"y\": 0.1, \"width\": 0.5, \"height\": 0.2, \"text_angle\": 0.0}]'\n";
        std::fs::write(&mock_vision_path, script_content).unwrap();

        // chmod +x on Mac
        std::process::Command::new("chmod")
            .args(["+x", mock_vision_path.to_str().unwrap()])
            .status()
            .unwrap();

        // 2. Start a mock server for translation requests
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            // Health check request
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\": \"ok\"}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
            // Chat completion request
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\n  \"choices\": [\n    {\n      \"message\": {\n        \"content\": \"1: English text\"\n      }\n    }\n  ]\n}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let ocr = Arc::new(OcrEngine::new(false, mock_vision_path));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, port)));
        // Set strategy to Qwen so the mock chat completion works with numbered batches
        client.lock().await.set_strategy("qwen");

        let processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);

        let frame = CaptureFrame {
            buffer: PixelBuffer {
                // 10x10 pixels buffer
                data: vec![0u8; 10 * 10 * 4],
                width: 10,
                height: 10,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let (_invalidation_tx, invalidation_rx) = crossbeam_channel::bounded(2);
        let (pipeline_tx, _pipeline_rx) = crossbeam_channel::bounded(2);

        let result = processor
            .process_frame(&temp_dir, &frame, 123, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.payload.is_some());
        let payload = result.payload.unwrap();
        assert_eq!(payload.boxes.len(), 1);
        assert_eq!(payload.boxes[0].original, "日本語のは");
        assert_eq!(payload.boxes[0].translated, "English text");

        // Clean up mock vision script
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pipeline_filters_english_from_cjk_flow() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("contextura-test-filter-{unique}"));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Mock vision helper returns both a Japanese line and a pure English line
        let mock_vision_path = temp_dir.join("mock-vision-helper");
        let script_content = "#!/bin/sh\necho '[{\"text\": \"日本語のは\", \"confidence\": 1.0, \"x\": 0.1, \"y\": 0.1, \"width\": 0.5, \"height\": 0.2, \"text_angle\": 0.0}, {\"text\": \"Hello World\", \"confidence\": 1.0, \"x\": 0.1, \"y\": 0.4, \"width\": 0.5, \"height\": 0.2, \"text_angle\": 0.0}]'\n";
        std::fs::write(&mock_vision_path, script_content).unwrap();

        std::process::Command::new("chmod")
            .args(["+x", mock_vision_path.to_str().unwrap()])
            .status()
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            // Health check
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\": \"ok\"}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
            // Chat completion - expect only 1 text to translate!
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\n  \"choices\": [\n    {\n      \"message\": {\n        \"content\": \"1: English translation\"\n      }\n    }\n  ]\n}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let ocr = Arc::new(OcrEngine::new(false, mock_vision_path));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, port)));
        client.lock().await.set_strategy("qwen");

        let processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);
        let frame = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![0u8; 10 * 10 * 4],
                width: 10,
                height: 10,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let (_invalidation_tx, invalidation_rx) = crossbeam_channel::bounded(2);
        let (pipeline_tx, _pipeline_rx) = crossbeam_channel::bounded(2);

        let result = processor
            .process_frame(&temp_dir, &frame, 124, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.payload.is_some());
        let payload = result.payload.unwrap();
        
        // Assert only the Japanese text got through and was translated
        assert_eq!(payload.boxes.len(), 1);
        assert_eq!(payload.boxes[0].original, "日本語のは");
        assert_eq!(payload.boxes[0].translated, "English translation");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pipeline_filters_omniglot_mixed_page_flow() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("contextura-test-omniglot-{unique}"));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Mock vision helper returns the mixed bullet points and Japanese text block
        let mock_vision_path = temp_dir.join("mock-vision-helper");
        let script_content = r#"#!/bin/sh
echo '[
  {"text": "• Type of writing system: semanto-phonetic", "confidence": 1.0, "x": 0.1, "y": 0.1, "width": 0.5, "height": 0.05, "text_angle": 0.0},
  {"text": "• Writing direction: right to left in vertical columns...", "confidence": 1.0, "x": 0.1, "y": 0.2, "width": 0.5, "height": 0.05, "text_angle": 0.0},
  {"text": "• Script family: (Chinese) Oracle bone script...", "confidence": 1.0, "x": 0.1, "y": 0.3, "width": 0.5, "height": 0.05, "text_angle": 0.0},
  {"text": "すべての人間は、生まれながらにして自由であり、かつ、尊厳と", "confidence": 1.0, "x": 0.1, "y": 0.4, "width": 0.5, "height": 0.05, "text_angle": 0.0},
  {"text": "権利とについて平等である。人間は、理性と良心とを授けられて", "confidence": 1.0, "x": 0.1, "y": 0.5, "width": 0.5, "height": 0.05, "text_angle": 0.0},
  {"text": "おり、互いに同胞の精神をもって行動しなければならない。", "confidence": 1.0, "x": 0.1, "y": 0.6, "width": 0.5, "height": 0.05, "text_angle": 0.0}
]'
"#;
        std::fs::write(&mock_vision_path, script_content).unwrap();

        std::process::Command::new("chmod")
            .args(["+x", mock_vision_path.to_str().unwrap()])
            .status()
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            // Health check
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\": \"ok\"}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
            // Chat completion - expect only 3 Japanese texts to translate!
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\n  \"choices\": [\n    {\n      \"message\": {\n        \"content\": \"1: Translation One\\n2: Translation Two\\n3: Translation Three\"\n      }\n    }\n  ]\n}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let ocr = Arc::new(OcrEngine::new(false, mock_vision_path));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, port)));
        client.lock().await.set_strategy("qwen");

        let processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);
        let frame = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![0u8; 10 * 10 * 4],
                width: 10,
                height: 10,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let (_invalidation_tx, invalidation_rx) = crossbeam_channel::bounded(2);
        let (pipeline_tx, _pipeline_rx) = crossbeam_channel::bounded(2);

        let result = processor
            .process_frame(&temp_dir, &frame, 125, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.payload.is_some());
        let payload = result.payload.unwrap();

        // Assert only the 3 Japanese lines are present, no English bullet points!
        assert_eq!(payload.boxes.len(), 3);
        assert_eq!(payload.boxes[0].original, "すべての人間は、生まれながらにして自由であり、かつ、尊厳と");
        assert_eq!(payload.boxes[1].original, "権利とについて平等である。人間は、理性と良心とを授けられて");
        assert_eq!(payload.boxes[2].original, "おり、互いに同胞の精神をもって行動しなければならない。");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
