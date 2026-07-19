// src-tauri/src/ocr.rs

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionHelperResult {
    pub text: String,
    pub confidence: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text_angle: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub text: String,
    pub confidence: f32,
    pub bounding_box: Rect,
    pub text_angle: f32,
    pub is_vertical: bool,
    pub is_furigana: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Check if another rect overlaps this one horizontally
    pub fn overlaps_horizontally(&self, other: &Rect, threshold_percent: f32) -> bool {
        let max_x = self.x.max(other.x);
        let min_right = (self.x + self.width).min(other.x + other.width);

        if min_right <= max_x {
            return false;
        }

        let overlap_width = min_right - max_x;
        let self_width = self.width;

        (overlap_width / self_width) >= threshold_percent
    }

    pub(crate) fn area(&self) -> f32 {
        self.width * self.height
    }

    pub(crate) fn right(&self) -> f32 {
        self.x + self.width
    }

    pub(crate) fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub(crate) fn intersection_area(&self, other: &Rect) -> f32 {
        let x_overlap = 0.0f32.max(self.right().min(other.right()) - self.x.max(other.x));
        let y_overlap = 0.0f32.max(self.bottom().min(other.bottom()) - self.y.max(other.y));
        x_overlap * y_overlap
    }

    pub(crate) fn intersection_ratio(&self, other: &Rect) -> f32 {
        let intersection = self.intersection_area(other);
        let smaller_area = self.area().min(other.area());
        if smaller_area <= 0.0 {
            0.0
        } else {
            intersection / smaller_area
        }
    }
}

pub struct OcrEngine {
    post_processor: crate::ocr_post_processor::OcrPostProcessor,
    backend: crate::ocr_backend::VisionHelperBackend,
}

impl OcrEngine {
    pub fn new(furigana_suppression: bool, vision_helper_path: PathBuf) -> Self {
        Self {
            post_processor: crate::ocr_post_processor::OcrPostProcessor::new(furigana_suppression),
            backend: crate::ocr_backend::VisionHelperBackend::new(vision_helper_path),
        }
    }

    pub fn recognize(
        &self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        scale_factor: f32,
        cache_dir: &Path,
        frame_id: u64,
    ) -> anyhow::Result<Vec<OcrResult>> {
        let results = self
            .backend
            .recognize(rgba_data, width, height, cache_dir, frame_id)?;

        #[allow(clippy::cast_precision_loss)]
        Ok(self
            .post_processor
            .process(results, width as f32, height as f32, scale_factor))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn process_vision_results(
        &self,
        results: Vec<OcrResult>,
        screen_width: f32,
        screen_height: f32,
        scale_factor: f32,
    ) -> Vec<OcrResult> {
        let observations = results
            .into_iter()
            .map(|result| crate::ocr_backend::RawOcrObservation {
                text: result.text,
                confidence: result.confidence,
                bounding_box: result.bounding_box,
                text_angle: result.text_angle,
            })
            .collect();
        self.post_processor
            .process(observations, screen_width, screen_height, scale_factor)
    }

    pub(crate) fn sanitize_text(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::{OcrEngine, OcrResult, Rect};
    use std::path::PathBuf;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn engine(furigana_suppression: bool) -> OcrEngine {
        OcrEngine::new(furigana_suppression, PathBuf::from("vision-helper"))
    }

    fn result(text: &str, confidence: f32, x: f32, y: f32, width: f32, height: f32) -> OcrResult {
        OcrResult {
            text: text.to_string(),
            confidence,
            bounding_box: Rect::new(x, y, width, height),
            text_angle: 0.0,
            is_vertical: false,
            is_furigana: false,
        }
    }

    #[test]
    fn process_vision_results_should_convert_coordinates_to_logical_space() {
        let results = vec![result("日本語のは", 0.9, 0.25, 0.10, 0.50, 0.20)];

        let processed = engine(false).process_vision_results(results, 200.0, 100.0, 2.0);

        assert_close(processed[0].bounding_box.x, 25.0);
        assert_close(processed[0].bounding_box.y, 35.0);
        assert_close(processed[0].bounding_box.width, 50.0);
        assert_close(processed[0].bounding_box.height, 10.0);
    }

    #[test]
    fn process_vision_results_should_keep_mixed_language_text_when_it_contains_cjk() {
        let results = vec![result("生成AI（ChatGPT）とは", 0.9, 0.10, 0.10, 0.30, 0.10)];

        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);

        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn process_vision_results_should_filter_non_cjk_text() {
        let results = vec![result("ChatGPT", 0.9, 0.10, 0.10, 0.30, 0.10)];

        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);

        assert!(processed.is_empty());
    }

    #[test]
    fn process_vision_results_should_filter_likely_misread_dash() {
        let results = vec![
            result("0:00/0:17ー", 0.9, 0.10, 0.10, 0.30, 0.10),
            result("▶ 0:00/0:17ー", 0.9, 0.10, 0.10, 0.30, 0.10),
        ];

        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);

        assert!(processed.is_empty());
    }

    #[test]
    fn process_vision_results_should_mark_small_overlapping_text_as_furigana() {
        // Original coordinates:
        // parent: x: 0.10, y: 0.10, width: 0.30, height: 0.20
        // candidate: x: 0.10, y: 0.12, width: 0.30, height: 0.05
        // Under bottom-left coordinate conversion:
        // parent y_conv = (1.0 - 0.10 - 0.20) * 100.0 / 1.0 = 70.0
        // parent height_conv = 0.20 * 100.0 / 1.0 = 20.0
        // candidate y_conv = (1.0 - 0.12 - 0.05) * 100.0 / 1.0 = 83.0
        // candidate height_conv = 0.05 * 100.0 / 1.0 = 5.0
        // Let's check candidate height < parent height * 0.40: 5.0 < 20.0 * 0.40 (8.0), which is true.
        // Wait, does candidate overlaps parent horizontally by 70%?
        // Both have x=0.10, width=0.30, so yes, they overlap 100%.
        // Wait, why did it fail? Ah, candidate.bounding_box.height is 5.0, parent is 20.0.
        // Is it because `Self::is_japanese` was rejected?
        // No, both "漢字の" (2 kana: 'の') and "かんじの" (5 kana: 'か','ん','じ','の') are Japanese!
        // Wait, let's check: "かんじの" is 'か' (hiragana), 'ん' (hiragana), 'じ' (hiragana), 'の' (hiragana). That's 4 kana.
        // But what about "漢字の"? It is '漢' (kanji), '字' (kanji), 'の' (hiragana). That is exactly 1 kana!
        // Ah! "漢字の" only has 1 kana, so it is REJECTED by `is_japanese` because `MIN_KANA_COUNT = 2`!
        // That's why it was filtered out! Both parent and candidate must pass `is_japanese` first,
        // or else they don't even reach furigana suppression!
        let results = vec![
            result("漢字のだよ", 0.9, 0.10, 0.10, 0.30, 0.20),
            result("かんじのだよ", 0.9, 0.10, 0.12, 0.30, 0.05),
        ];

        let processed = engine(true).process_vision_results(results, 100.0, 100.0, 1.0);

        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].text, "漢字のだよ");
    }

    #[test]
    fn process_vision_results_should_merge_overlapping_boxes_and_keep_higher_confidence_text() {
        let results = vec![
            result("日本のだ", 0.6, 0.10, 0.10, 0.20, 0.10),
            result("日本のだ", 0.9, 0.10, 0.10, 0.20, 0.10),
        ];

        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);

        assert_eq!(processed.len(), 1);
        assert_eq!(processed[0].text, "日本のだ");
        assert_close(processed[0].confidence, 0.9);
    }

    #[test]
    fn process_vision_results_should_keep_distinct_overlapping_text() {
        // Must use distinct texts that each have at least 2 kana to pass is_japanese.
        // E.g. "日本のだ" (2 kana: 'の', 'だ') and "日本語のだ" (3 kana: 'の', 'だ', 'の' is not there, wait, 'の','だ' are 2 kana).
        // Let's use: "日本の" (2 kana: 'の', 'の' -- wait, 'の' is 1 kana, 'の' is 1 kana, so "日本の" is 1 kana 'の').
        // Ah! "日本の" has only 1 kana ('の')! That's why it was rejected by is_japanese (kana_count = 1)!
        // Let's use "日本のだ" (2 kana: 'の', 'だ') and "日本語のだ" (3 kana: 'の', 'だ', 'の' wait, 'ご'/'の'/'だ').
        let results = vec![
            result("日本のだ", 0.9, 0.10, 0.10, 0.30, 0.12),
            result("日本語のだ", 0.8, 0.12, 0.11, 0.31, 0.12),
        ];

        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);

        assert_eq!(processed.len(), 2);
        assert_eq!(processed[0].text, "日本のだ");
        assert_eq!(processed[1].text, "日本語のだ");
    }

    #[test]
    fn recognize_should_support_stdin_helper_contract() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("contextura-ocr-stdin-{unique}"));
        std::fs::create_dir_all(&temp_dir).expect("temp dir should be created");

        let helper_path = temp_dir.join("mock-vision-helper-stdin.sh");
        let script = r#"#!/bin/sh
if [ "$1" != "--stdin" ]; then
  echo "expected --stdin" >&2
  exit 64
fi
bytes=$(wc -c | tr -d ' ')
if [ "$bytes" -le 0 ]; then
  echo "expected non-empty stdin" >&2
  exit 65
fi
echo '[{"text":"日本語のは","confidence":1.0,"x":0.1,"y":0.1,"width":0.5,"height":0.2,"text_angle":0.0}]'
"#;

        std::fs::write(&helper_path, script).expect("mock helper should be written");
        let chmod_status = std::process::Command::new("chmod")
            .args(["+x", helper_path.to_str().expect("utf8 path")])
            .status()
            .expect("chmod should run");
        assert!(chmod_status.success());

        let engine = OcrEngine::new(false, helper_path);
        let rgba = vec![0u8; 16 * 16 * 4];
        let recognized = engine
            .recognize(&rgba, 16, 16, 1.0, &temp_dir, 1)
            .expect("stdin OCR recognition should succeed");

        assert_eq!(recognized.len(), 1);
        assert_eq!(recognized[0].text, "日本語のは");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn process_vision_results_should_keep_vertical_box_geometry() {
        let mut vertical = result("縦書きの", 0.9, 0.20, 0.10, 0.10, 0.30);
        vertical.text_angle = std::f32::consts::PI / 2.0;
        vertical.is_vertical = true;

        let processed = engine(false).process_vision_results(vec![vertical], 100.0, 100.0, 1.0);

        assert_eq!(processed.len(), 1);
        assert_close(processed[0].bounding_box.width, 10.0);
        assert_close(processed[0].bounding_box.height, 30.0);
    }

    #[test]
    fn is_japanese_accepts_pure_katakana() {
        let results = vec![result("コンピューター", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_katakana_with_kanji() {
        let results = vec![result("アニメ化", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_hiragana_with_kanji() {
        let results = vec![result("日本語のテキスト", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_mixed_japanese_english() {
        let results = vec![result("生成AIとは何か", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_kanji_only() {
        let results = vec![result("漢字", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_single_kana_with_kanji() {
        // e.g. 2 kanji + 1 kana = 3 characters total, but only 1 is kana ('の')
        let results = vec![result("日本語の", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_accepts_kanji_only_common_signage() {
        for text in &["出口", "注意", "設定", "保存", "終了"] {
            let results = vec![result(text, 0.9, 0.10, 0.10, 0.30, 0.10)];
            let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
            assert_eq!(processed.len(), 1, "Expected accept for: {text}");
        }
    }

    #[test]
    fn is_japanese_accepts_single_kanji_with_high_confidence() {
        let results = vec![result("駅", 0.85, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_rejects_single_kanji_low_confidence() {
        let results = vec![result("駅", 0.60, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_simplified_chinese_only_chars() {
        for text in &["们", "这", "说"] {
            let results = vec![result(text, 0.9, 0.10, 0.10, 0.30, 0.10)];
            let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
            assert_eq!(processed.len(), 0, "Expected reject for simplified: {text}");
        }
    }

    #[test]
    fn is_japanese_accepts_pure_hiragana_two_chars() {
        let results = vec![result("はい", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 1);
    }

    #[test]
    fn is_japanese_rejects_pure_hiragana_one_char() {
        let results = vec![result("は", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_pure_english() {
        let results = vec![result("ChatGPT", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_timestamp_with_stray_mark() {
        let results = vec![
            result("0:00/0:17ー", 0.9, 0.10, 0.10, 0.30, 0.10),
            result("▶ 0:00/0:17ー", 0.9, 0.10, 0.10, 0.30, 0.10),
        ];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_chinese_kanji_block() {
        let results = vec![result("你好世界", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_omniglot_english_bullet_points() {
        let cases = vec![
            "• Type of writing system: semanto-phonetic",
            "• Writing direction: right to left in vertical columns running from top to bottom, or left to right in hortizontal lines.",
            "• Script family: (Chinese) Oracle bone script, Seal script, Clerical script, Regular script, Kanji, Hiragana, Katakana",
            "• Used to write: Ainu, Amami, Japanese, Kikai, Miyakoan, Okinawan, Okinoerabu, Tarama, Tokunoshima, Yaeyama, Yonaguni, Yoron",
        ];

        for text in cases {
            let results = vec![result(text, 0.9, 0.10, 0.10, 0.30, 0.10)];
            let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
            assert_eq!(processed.len(), 0, "Expected string to be rejected: {text}");
        }
    }

    #[test]
    fn is_japanese_rejects_katakana_punctuation_only() {
        let results = vec![result("・", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_single_katakana_char() {
        let results = vec![result("ア", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_bullet_points_with_english() {
        let cases = vec![
            "・ Type of writing system: semanto-phonetic",
            "・ Used to write: Ainu, Amami, Japanese, Kikai",
            "• Script family： （Chinese） Oracle bone script",
        ];

        for text in cases {
            let results = vec![result(text, 0.9, 0.10, 0.10, 0.30, 0.10)];
            let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
            assert_eq!(processed.len(), 0, "Expected string to be rejected: {text}");
        }
    }

    #[test]
    fn test_ocr_engine_recognize_raw_buffer() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("ocr_recognize_test_{unique}"));
        let _ = std::fs::create_dir_all(&temp_dir);

        let engine = OcrEngine::new(false, PathBuf::from("non-existent-vision-helper"));
        let rgba_data = vec![0; 400]; // 10x10 RGBA image

        let res = engine.recognize(&rgba_data, 10, 10, 1.0, &temp_dir, 9999);

        // It should return an error because the helper binary does not exist
        assert!(res.is_err());

        // But the temporary frame file must have been deleted/cleaned up!
        let expected_temp_file = temp_dir.join("contextura-frame-9999.png");
        assert!(
            !expected_temp_file.exists(),
            "Expected temporary frame to be deleted after recognition attempt"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
