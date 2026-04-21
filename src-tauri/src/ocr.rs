use serde::{Deserialize, Serialize};

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
        Self { x, y, width, height }
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
}

impl OcrEngine {
    pub fn new(furigana_suppression: bool) -> Self {
        Self {
            furigana_suppression,
        }
    }

    pub fn process_vision_results(&self, mut results: Vec<OcrResult>, screen_height: f32, scale_factor: f32) -> Vec<OcrResult> {
        // 1. Coordinate Conversion (Bottom-left origin to Top-left logical)
        for result in &mut results {
            let mut x = result.bounding_box.x;
            let mut y = (1.0 - result.bounding_box.y - result.bounding_box.height) * screen_height;
            let mut width = result.bounding_box.width;
            let mut height = result.bounding_box.height;

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
                    if candidate.bounding_box.height < (parent.bounding_box.height * 0.4) {
                        if candidate.bounding_box.overlaps_horizontally(&parent.bounding_box, 0.70) {
                            to_mark_furigana.push(i);
                            break;
                        }
                    }
                }
            }

            for idx in to_mark_furigana {
                results[idx].is_furigana = true;
            }
        }

        // 3. Filtering
        results.into_iter()
            .filter(|r| r.confidence >= 0.4)
            .filter(|r| !r.is_furigana)
            .collect()
    }
}
