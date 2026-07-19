use crate::ocr::{OcrResult, Rect};
use crate::ocr_backend::RawOcrObservation;

const MIN_CONFIDENCE: f32 = 0.3;
const FURIGANA_HEIGHT_RATIO: f32 = 0.4;
const FURIGANA_HORIZONTAL_OVERLAP: f32 = 0.70;
const DUPLICATE_IOU_THRESHOLD: f32 = 0.8;
const DUPLICATE_CONTAINMENT_THRESHOLD: f32 = 0.9;

pub struct OcrPostProcessor {
    furigana_suppression: bool,
}

impl OcrPostProcessor {
    pub fn new(furigana_suppression: bool) -> Self {
        Self {
            furigana_suppression,
        }
    }

    pub fn process(
        &self,
        observations: Vec<RawOcrObservation>,
        screen_width: f32,
        screen_height: f32,
        scale_factor: f32,
    ) -> Vec<OcrResult> {
        let mut results: Vec<OcrResult> = observations
            .into_iter()
            .map(|r| {
                let is_vertical = r.text_angle.abs() > std::f32::consts::PI / 4.0;
                OcrResult {
                    text: r.text,
                    confidence: r.confidence,
                    bounding_box: r.bounding_box,
                    text_angle: r.text_angle,
                    is_vertical,
                    is_furigana: false,
                }
            })
            .collect();

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

        if self.furigana_suppression {
            let mut to_mark_furigana = Vec::new();

            for (i, candidate) in results.iter().enumerate() {
                for (j, parent) in results.iter().enumerate() {
                    if i == j {
                        continue;
                    }

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
                if res.is_furigana {
                    return false;
                }
                let counts = crate::script::count_script_chars(&res.text);
                let is_single_kanji_only =
                    counts.kanji == 1 && counts.hiragana == 0 && counts.katakana == 0;
                let conf_floor = if is_single_kanji_only {
                    0.75
                } else {
                    MIN_CONFIDENCE
                };

                res.confidence >= conf_floor && Self::is_japanese(&res.text)
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

    fn is_japanese(text: &str) -> bool {
        if Self::is_likely_misread_dash(text) {
            return false;
        }
        matches!(
            crate::script::classify_script(text),
            crate::script::ScriptVerdict::Accept
        )
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
