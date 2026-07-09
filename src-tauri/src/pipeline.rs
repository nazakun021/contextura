// src-tauri/src/pipeline.rs

use rayon::prelude::*;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex as AsyncMutex;

use crate::capture::CaptureFrame;
use crate::ipc::{TranslationBox, TranslationPayload};
use crate::motion::{DebounceEvent, DebounceStateMachine, MotionDetector};
use crate::styling::StylingEngine;

pub struct PipelineProcessor {
    pub(crate) motion_detector: MotionDetector,
    pub(crate) debounce: DebounceStateMachine,
    pub(crate) ocr_engine: Arc<crate::ocr::OcrEngine>,
    pub(crate) client: Arc<AsyncMutex<crate::translation::TranslationClient>>,
    pub last_processed_hash: Option<u64>,
    pub last_payload: Option<TranslationPayload>,
    pub frame_id: u64,
    pub was_scrolling: bool,
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
            last_processed_hash: None,
            last_payload: None,
            frame_id: 0,
            was_scrolling: false,
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
    pub async fn handle_frame<R: tauri::Runtime>(
        &mut self,
        app_handle: &tauri::AppHandle<R>,
        frame: &CaptureFrame,
        is_forced: bool,
        invalidation_rx: &crossbeam_channel::Receiver<crate::context::InvalidationReason>,
        pipeline_tx: &crossbeam_channel::Sender<crate::scheduler::PipelineCommand>,
    ) -> Option<TranslationPayload> {
        let cache_dir = app_handle
            .path()
            .app_cache_dir()
            .expect("Failed to get cache dir");

        let mut clear_context = false;
        let mut manual_reset = false;

        // 1. Drain context invalidations
        while let Ok(reason) = invalidation_rx.try_recv() {
            log::info!("[Context] Invalidation: {reason:?}");
            match reason {
                crate::context::InvalidationReason::AppSwitch { from, to } => {
                    log::info!("[Context] App switch: {from} -> {to} — clearing memory");
                    self.client.lock().await.clear_memory();
                    clear_context = true;
                }
                crate::context::InvalidationReason::ManualReset => {
                    self.client.lock().await.clear_memory();
                    manual_reset = true;
                }
            }
        }

        if clear_context {
            let _ = app_handle.emit("translation-clear", ());
            self.last_payload = None;
            self.last_processed_hash = None;
        }

        if manual_reset {
            crate::scheduler::emit_runtime_notice(
                app_handle,
                "Context Cleared",
                "Translation memory was cleared.",
                "New translations will start without prior context.",
                "info",
                2500,
            );
            self.last_payload = None;
            self.last_processed_hash = None;
        }

        // 2. Compute motion duplicate hash
        let rgba_data = &frame.buffer.data;
        let thumbnail =
            self.motion_detector
                .downsample(rgba_data, frame.buffer.width, frame.buffer.height);
        let frame_hash = crate::motion::compute_thumbnail_hash(&thumbnail);
        if !is_forced
            && self.last_processed_hash == Some(frame_hash)
            && let Some(ref payload) = self.last_payload
        {
            log::debug!("[Pipeline] Frame identical to last processed, reusing cached payload");
            let _ = app_handle.emit("translation-update", payload);
            return Some(payload.clone());
        }

        // 3. Pre-flight health check
        if self.client.lock().await.quick_health_check().await.is_err() {
            log::warn!(
                "[Translation] Pre-flight health check failed — skipping frame and requesting runtime reload"
            );
            let _ = pipeline_tx.try_send(crate::scheduler::PipelineCommand::ReloadRuntime {
                reason: "health check failed before batch".to_string(),
            });
            return None;
        }

        // 4. Run OCR
        let current_frame_id = self.frame_id;
        self.frame_id += 1;

        #[allow(clippy::cast_possible_truncation)]
        let ocr_results = self.ocr_engine.recognize(
            rgba_data,
            frame.buffer.width as u32,
            frame.buffer.height as u32,
            frame.scale_factor,
            &cache_dir,
            current_frame_id,
        );

        let ocr_results = match ocr_results {
            Ok(results) => results,
            Err(error) => {
                log::error!("[OCR] Recognition failed: {error}");
                return None;
            }
        };

        if ocr_results.is_empty() {
            log::debug!("[OCR] No CJK text found in frame {current_frame_id}");
            return None;
        }

        // 5. Run concurrent styling and translation
        let styled_boxes = match self
            .process_concurrent_translation_and_styling(
                &ocr_results,
                rgba_data,
                frame.buffer.width,
                frame.buffer.height,
                frame.scale_factor,
                current_frame_id,
            )
            .await
        {
            Ok(boxes) => boxes,
            Err(error) => {
                log::error!("[Pipeline] Concurrent styling and translation failed: {error}");
                return None;
            }
        };

        let payload = TranslationPayload {
            boxes: styled_boxes,
            scale_factor: frame.scale_factor,
            display_id: frame.display_id,
            frame_id: current_frame_id,
        };

        if let Err(error) = app_handle.emit("translation-update", &payload) {
            log::error!("[IPC] Failed to emit translation-update: {error}");
        }

        self.last_payload = Some(payload.clone());
        self.last_processed_hash = Some(frame_hash);
        Some(payload)
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
            .map(|(index, ((ocr, translation), (bg, fg_color)))| {
                let outcome = crate::guardrails::validate_translation(&ocr.text, translation);
                TranslationBox {
                    id: format!("{frame_id}-{index}"),
                    translated: outcome.cleaned_text,
                    original: ocr.text.clone(),
                    x: ocr.bounding_box.x,
                    y: ocr.bounding_box.y,
                    width: ocr.bounding_box.width,
                    height: ocr.bounding_box.height,
                    is_vertical: ocr.is_vertical,
                    bg_color: bg.to_css_color(),
                    fg_color: fg_color.to_string(),
                    confidence: ocr.confidence,
                    is_degraded: !outcome.accepted,
                }
            })
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

        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let mut processor = processor;
        let result = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.is_some());
        let payload = result.unwrap();
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

        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let mut processor = processor;
        let result = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.is_some());
        let payload = result.unwrap();

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

        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let mut processor = processor;
        let result = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(result.is_some());
        let payload = result.unwrap();

        // Assert only the 3 Japanese lines are present, no English bullet points!
        assert_eq!(payload.boxes.len(), 3);
        assert_eq!(
            payload.boxes[0].original,
            "すべての人間は、生まれながらにして自由であり、かつ、尊厳と"
        );
        assert_eq!(
            payload.boxes[1].original,
            "権利とについて平等である。人間は、理性と良心とを授けられて"
        );
        assert_eq!(
            payload.boxes[2].original,
            "おり、互いに同胞の精神をもって行動しなければならない。"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pipeline_processor_deduplication() {
        use crate::capture::PixelBuffer;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temp_dir = std::env::temp_dir().join("pipeline_dedup_test");
        let _ = std::fs::create_dir_all(&temp_dir);

        let mock_vision_path = temp_dir.join("mock-vision");
        let script_content = "#!/bin/sh\necho '[{\"text\": \"日本語のは\", \"confidence\": 0.99, \"x\": 0.1, \"y\": 0.1, \"width\": 0.5, \"height\": 0.5, \"text_angle\": 0.0}]'\n";
        std::fs::write(&mock_vision_path, script_content).unwrap();
        let _ = std::process::Command::new("chmod")
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
            // Only 1 completion request is accepted! Any extra request will fail the test
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\n  \"choices\": [\n    {\n      \"message\": {\n        \"content\": \"1: test\"\n      }\n    }\n  ]\n}";
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });

        let ocr = Arc::new(OcrEngine::new(false, mock_vision_path));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, port)));
        client.lock().await.set_strategy("qwen");

        // Set mock translation text boxes to match completion numbering
        let mut processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);

        let frame = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![0u8; 10 * 10 * 4],
                width: 10,
                height: 10,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (_invalidation_tx, invalidation_rx) = crossbeam_channel::bounded(2);
        let (pipeline_tx, _pipeline_rx) = crossbeam_channel::bounded(2);

        // First call - should perform OCR and translate
        let res1 = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;
        assert!(res1.is_some());

        // Second call with same frame - should return cached payload without calling LLM
        let res2 = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;
        assert!(res2.is_some());
        assert_eq!(
            res1.unwrap().boxes[0].translated,
            res2.unwrap().boxes[0].translated
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_pipeline_processor_app_invalidation() {
        use crate::capture::PixelBuffer;

        let ocr = Arc::new(OcrEngine::new(false, PathBuf::from("mock-vision")));
        let client = Arc::new(AsyncMutex::new(TranslationClient::new(6, 1234)));
        let mut processor = PipelineProcessor::new(10, 0, 50, 0.05, ocr, client);

        processor.last_processed_hash = Some(12345);
        processor.last_payload = Some(TranslationPayload {
            boxes: vec![],
            scale_factor: 1.0,
            display_id: 0,
            frame_id: 1,
        });

        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (invalidation_tx, invalidation_rx) = crossbeam_channel::bounded(2);
        let (pipeline_tx, _pipeline_rx) = crossbeam_channel::bounded(2);

        invalidation_tx
            .send(crate::context::InvalidationReason::ManualReset)
            .unwrap();

        let frame = CaptureFrame {
            buffer: PixelBuffer {
                data: vec![0u8; 10 * 10 * 4],
                width: 10,
                height: 10,
            },
            display_id: 0,
            scale_factor: 1.0,
        };

        // This call will process invalidation events and clear the memory & caches
        let _ = processor
            .handle_frame(app_handle, &frame, false, &invalidation_rx, &pipeline_tx)
            .await;

        assert!(processor.last_processed_hash.is_none());
        assert!(processor.last_payload.is_none());
    }

    #[test]
    fn test_translation_box_degraded_mapping() {
        let ocr_result = crate::ocr::OcrResult {
            text: "閉じる".to_string(),
            bounding_box: crate::ocr::Rect::new(0.0, 0.0, 10.0, 10.0),
            confidence: 0.9,
            is_vertical: false,
            text_angle: 0.0,
            is_furigana: false,
        };

        // Valid translation
        let outcome_valid = crate::guardrails::validate_translation(&ocr_result.text, "Close");
        let box_valid = TranslationBox {
            id: "box-1".to_string(),
            translated: outcome_valid.cleaned_text,
            original: ocr_result.text.clone(),
            x: ocr_result.bounding_box.x,
            y: ocr_result.bounding_box.y,
            width: ocr_result.bounding_box.width,
            height: ocr_result.bounding_box.height,
            is_vertical: ocr_result.is_vertical,
            bg_color: "#000000".to_string(),
            fg_color: "#ffffff".to_string(),
            confidence: ocr_result.confidence,
            is_degraded: !outcome_valid.accepted,
        };
        assert!(!box_valid.is_degraded);

        // Invalid translation (residual CJK)
        let outcome_invalid =
            crate::guardrails::validate_translation(&ocr_result.text, "Close (閉じる)");
        let box_invalid = TranslationBox {
            id: "box-2".to_string(),
            translated: outcome_invalid.cleaned_text,
            original: ocr_result.text.clone(),
            x: ocr_result.bounding_box.x,
            y: ocr_result.bounding_box.y,
            width: ocr_result.bounding_box.width,
            height: ocr_result.bounding_box.height,
            is_vertical: ocr_result.is_vertical,
            bg_color: "#000000".to_string(),
            fg_color: "#ffffff".to_string(),
            confidence: ocr_result.confidence,
            is_degraded: !outcome_invalid.accepted,
        };
        assert!(box_invalid.is_degraded);
    }
}
