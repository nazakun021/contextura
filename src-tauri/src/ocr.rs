use serde::{Deserialize, Serialize};
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        let output = Command::new(&self.vision_helper_path)
            .arg(png_path)
            .output()?;

        let raw: Vec<VisionHelperResult> = serde_json::from_slice(&output.stdout)?;

        let results: Vec<OcrResult> = raw
            .into_iter()
            .map(|r| {
                let is_vertical = r.text_angle.abs() > std::f32::consts::PI / 4.0;
                OcrResult {
                    text: r.text,
                    confidence: r.confidence,
                    bounding_box: Rect::new(r.x, r.y, r.width, r.height),
                    text_angle: r.text_angle,
                    is_vertical,
                    is_furigana: false,
                }
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

            if result.is_vertical {
                std::mem::swap(&mut width, &mut height);
            }

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
                    if candidate.bounding_box.height < (parent.bounding_box.height * 0.4)
                        && candidate
                            .bounding_box
                            .overlaps_horizontally(&parent.bounding_box, 0.70)
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

        // 3. Merging overlapping boxes (IoU > 0.3)
        let mut merged_results: Vec<OcrResult> = Vec::new();
        for res in results {
            if res.is_furigana || res.confidence < 0.4 || !Self::contains_cjk(&res.text) {
                continue;
            }

            let mut merged = false;
            for existing in &mut merged_results {
                if Self::calculate_iou(&res.bounding_box, &existing.bounding_box) > 0.3 {
                    // Simple merge: take the union of boxes and the text with higher confidence
                    let x = res.bounding_box.x.min(existing.bounding_box.x);
                    let y = res.bounding_box.y.min(existing.bounding_box.y);
                    let r = (res.bounding_box.x + res.bounding_box.width)
                        .max(existing.bounding_box.x + existing.bounding_box.width);
                    let b = (res.bounding_box.y + res.bounding_box.height)
                        .max(existing.bounding_box.y + existing.bounding_box.height);

                    existing.bounding_box = Rect::new(x, y, r - x, b - y);
                    if res.confidence > existing.confidence {
                        existing.text.clone_from(&res.text);
                        existing.confidence = res.confidence;
                    }
                    merged = true;
                    break;
                }
            }

            if !merged {
                merged_results.push(res);
            }
        }

        merged_results
    }

    fn contains_cjk(text: &str) -> bool {
        text.chars().any(|c| {
            matches!(c, '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}')
        })
    }

    fn calculate_iou(a: &Rect, b: &Rect) -> f32 {
        let x_overlap = 0.0f32.max(a.x.add(a.width).min(b.x.add(b.width)) - a.x.max(b.x));
        let y_overlap = 0.0f32.max(a.y.add(a.height).min(b.y.add(b.height)) - a.y.max(b.y));
        let intersection = x_overlap * y_overlap;
        let area_a = a.width * a.height;
        let area_b = b.width * b.height;
        let union = area_a + area_b - intersection;
        if union <= 0.0 {
            0.0
        } else {
            intersection / union
        }
    }
}
