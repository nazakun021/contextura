// src-tauri/src/ocr.rs

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MIN_CONFIDENCE: f32 = 0.3;
const FURIGANA_HEIGHT_RATIO: f32 = 0.4;
const FURIGANA_HORIZONTAL_OVERLAP: f32 = 0.70;
const OCR_HELPER_TIMEOUT: Duration = Duration::from_secs(8);
const DUPLICATE_IOU_THRESHOLD: f32 = 0.8;
const DUPLICATE_CONTAINMENT_THRESHOLD: f32 = 0.9;

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

    fn area(&self) -> f32 {
        self.width * self.height
    }

    fn right(&self) -> f32 {
        self.x + self.width
    }

    fn bottom(&self) -> f32 {
        self.y + self.height
    }

    fn intersection_area(&self, other: &Rect) -> f32 {
        let x_overlap = 0.0f32.max(self.right().min(other.right()) - self.x.max(other.x));
        let y_overlap = 0.0f32.max(self.bottom().min(other.bottom()) - self.y.max(other.y));
        x_overlap * y_overlap
    }

    fn intersection_ratio(&self, other: &Rect) -> f32 {
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
    furigana_suppression: bool,
    vision_helper_path: PathBuf,
}

impl OcrEngine {
    pub fn new(furigana_suppression: bool, vision_helper_path: PathBuf) -> Self {
        Self {
            furigana_suppression,
            vision_helper_path,
        }
    }

    pub fn recognize(
        &self,
        png_path: &Path,
        screen_width: f32,
        screen_height: f32,
        scale_factor: f32,
    ) -> anyhow::Result<Vec<OcrResult>> {
        let child = Command::new(&self.vision_helper_path)
            .arg(png_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to launch vision-helper at {}",
                    self.vision_helper_path.display()
                )
            })?;

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
                    "vision-helper timed out after {}s while reading {}",
                    OCR_HELPER_TIMEOUT.as_secs(),
                    png_path.display()
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("vision-helper worker disconnected before producing output");
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

        let raw: Vec<VisionHelperResult> = serde_json::from_slice(&output.stdout)
            .with_context(|| "vision-helper returned invalid JSON".to_string())?;

        let results: Vec<OcrResult> = raw
            .into_iter()
            .filter_map(|r| {
                let text = Self::sanitize_text(&r.text);
                if text.is_empty() {
                    return None;
                }

                let is_vertical = r.text_angle.abs() > std::f32::consts::PI / 4.0;
                Some(OcrResult {
                    text,
                    confidence: r.confidence,
                    bounding_box: Rect::new(r.x, r.y, r.width, r.height),
                    text_angle: r.text_angle,
                    is_vertical,
                    is_furigana: false,
                })
            })
            .collect();

        Ok(self.process_vision_results(results, screen_width, screen_height, scale_factor))
    }

    pub fn process_vision_results(
        &self,
        mut results: Vec<OcrResult>,
        screen_width: f32,
        screen_height: f32,
        scale_factor: f32,
    ) -> Vec<OcrResult> {
        // 1. Coordinate Conversion (Bottom-left origin to Top-left logical)
        for result in &mut results {
            let mut x = result.bounding_box.x * screen_width;
            let mut y = (1.0 - result.bounding_box.y - result.bounding_box.height) * screen_height;
            let mut width = result.bounding_box.width * screen_width;
            let mut height = result.bounding_box.height * screen_height;

            x /= scale_factor;
            y /= scale_factor;
            width /= scale_factor;
            height /= scale_factor;

            result.bounding_box = Rect::new(x, y, width, height);
        }

        // 2. Furigana Suppression
        if self.furigana_suppression {
            let mut to_mark_furigana = Vec::new();

            for (i, candidate) in results.iter().enumerate() {
                for (j, parent) in results.iter().enumerate() {
                    if i == j {
                        continue;
                    }

                    // Box height < 40% of overlapping box height -> furigana
                    if candidate.bounding_box.height
                        < (parent.bounding_box.height * FURIGANA_HEIGHT_RATIO)
                        && candidate.bounding_box.overlaps_horizontally(
                            &parent.bounding_box,
                            FURIGANA_HORIZONTAL_OVERLAP,
                        )
                    {
                        to_mark_furigana.push(i);
                        break;
                    }
                }
            }

            for idx in to_mark_furigana {
                results[idx].is_furigana = true;
            }
        }

        let mut filtered_results = results
            .into_iter()
            .filter(|res| {
                !res.is_furigana && res.confidence >= MIN_CONFIDENCE && Self::is_japanese(&res.text)
            })
            .collect::<Vec<_>>();

        filtered_results.sort_by(Self::reading_order_cmp);

        let mut deduped_results: Vec<OcrResult> = Vec::new();
        for res in filtered_results {
            if let Some(existing) = deduped_results
                .iter_mut()
                .find(|existing| Self::is_duplicate_detection(existing, &res))
            {
                if res.confidence > existing.confidence {
                    *existing = res;
                }
                continue;
            }

            deduped_results.push(res);
        }

        deduped_results
    }

    fn is_likely_misread_dash(text: &str) -> bool {
        if !text.contains('ー') {
            return false;
        }
        let stripped_of_mark: String = text.chars().filter(|&c| c != 'ー').collect();
        let has_digits_or_ascii = stripped_of_mark
            .chars()
            .any(|c| c.is_alphanumeric() && c.is_ascii());
        let has_real_japanese = text
            .chars()
            .any(|c| matches!(c, '\u{3040}'..='\u{309F}' | '\u{4E00}'..='\u{9FFF}'));
        has_digits_or_ascii && !has_real_japanese
    }

    const MIN_KANA_COUNT: usize = 2;

    fn is_japanese(text: &str) -> bool {
        if Self::is_likely_misread_dash(text) {
            return false;
        }

        let mut hiragana_count = 0;
        let mut katakana_count = 0;
        let mut kanji_count = 0;

        for c in text.chars() {
            match c {
                '\u{3040}'..='\u{309F}' => hiragana_count += 1,
                '\u{30A0}'..='\u{30FF}' => {
                    if c != '\u{30FB}' {
                        katakana_count += 1;
                    }
                }
                '\u{4E00}'..='\u{9FFF}' => kanji_count += 1,
                _ => {}
            }
        }

        let kana_count = hiragana_count + katakana_count;

        // Rule 1: Pure katakana exception (kana count matches katakana count, at least MIN_KANA_COUNT, no kanji)
        if kana_count == katakana_count
            && katakana_count >= Self::MIN_KANA_COUNT
            && kanji_count == 0
        {
            return true;
        }

        // Rule 2: Kana-presence gate
        kana_count >= Self::MIN_KANA_COUNT
    }

    fn calculate_iou(a: &Rect, b: &Rect) -> f32 {
        let intersection = a.intersection_area(b);
        let area_a = a.area();
        let area_b = b.area();
        let union = area_a + area_b - intersection;
        if union <= 0.0 {
            0.0
        } else {
            intersection / union
        }
    }

    fn sanitize_text(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn is_duplicate_detection(existing: &OcrResult, candidate: &OcrResult) -> bool {
        existing.text == candidate.text
            && (Self::calculate_iou(&existing.bounding_box, &candidate.bounding_box)
                >= DUPLICATE_IOU_THRESHOLD
                || existing
                    .bounding_box
                    .intersection_ratio(&candidate.bounding_box)
                    >= DUPLICATE_CONTAINMENT_THRESHOLD)
    }

    fn reading_order_cmp(a: &OcrResult, b: &OcrResult) -> std::cmp::Ordering {
        use std::cmp::Ordering;

        const EPSILON: f32 = 4.0;

        if a.is_vertical && b.is_vertical {
            if (a.bounding_box.x - b.bounding_box.x).abs() > EPSILON {
                return b
                    .bounding_box
                    .x
                    .partial_cmp(&a.bounding_box.x)
                    .unwrap_or(Ordering::Equal);
            }

            return a
                .bounding_box
                .y
                .partial_cmp(&b.bounding_box.y)
                .unwrap_or(Ordering::Equal);
        }

        if (a.bounding_box.y - b.bounding_box.y).abs() > EPSILON {
            return a
                .bounding_box
                .y
                .partial_cmp(&b.bounding_box.y)
                .unwrap_or(Ordering::Equal);
        }

        a.bounding_box
            .x
            .partial_cmp(&b.bounding_box.x)
            .unwrap_or(Ordering::Equal)
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
    fn is_japanese_rejects_kanji_only() {
        let results = vec![result("漢字", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
    }

    #[test]
    fn is_japanese_rejects_single_kana_with_kanji() {
        // e.g. 2 kanji + 1 kana = 3 characters total, but only 1 is kana ('の')
        let results = vec![result("日本語の", 0.9, 0.10, 0.10, 0.30, 0.10)];
        let processed = engine(false).process_vision_results(results, 100.0, 100.0, 1.0);
        assert_eq!(processed.len(), 0);
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
}
