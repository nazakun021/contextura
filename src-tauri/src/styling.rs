// Background Color Sampling & Contrast Calculation

#[derive(Debug, Clone, Copy)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgba {
    pub fn to_css_color(self) -> String {
        format!("rgba({}, {}, {}, 0.85)", self.r, self.g, self.b)
    }
}

pub struct StylingEngine;

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_arguments,
    clippy::cast_possible_wrap
)]
impl StylingEngine {
    // Relative luminance from WCAG 2.1
    pub fn relative_luminance(r: f32, g: f32, b: f32) -> f32 {
        0.2126 * Self::linearize_channel(r)
            + 0.7152 * Self::linearize_channel(g)
            + 0.0722 * Self::linearize_channel(b)
    }

    pub fn linearize_channel(c: f32) -> f32 {
        if c <= 0.03928 {
            // WCAG formally says 0.04045, but often implemented as 0.03928 depending on standard version
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sample_rect_ring(
        rgba_data: &[u8],
        width: usize,
        height: usize,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
        scale_factor: f32,
    ) -> Rgba {
        // Convert logical rect to pixel coordinates
        let px = (rect_x * scale_factor) as i32;
        let py = (rect_y * scale_factor) as i32;
        let pw = (rect_w * scale_factor) as i32;
        let ph = (rect_h * scale_factor) as i32;

        let mut r_sum = 0u64;
        let mut g_sum = 0u64;
        let mut b_sum = 0u64;
        let mut count = 0u64;

        // Sample 2px ring outside
        let ring_outer = 2;

        for y in (py - ring_outer)..(py + ph + ring_outer) {
            for x in (px - ring_outer)..(px + pw + ring_outer) {
                // Only sample if in the 2px ring
                let is_in_rect = x >= px && x < px + pw && y >= py && y < py + ph;
                if !is_in_rect && x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
                    let idx = (y as usize * width + x as usize) * 4;
                    if idx + 2 < rgba_data.len() {
                        r_sum += u64::from(rgba_data[idx]);
                        g_sum += u64::from(rgba_data[idx + 1]);
                        b_sum += u64::from(rgba_data[idx + 2]);
                        count += 1;
                    }
                }
            }
        }

        if count == 0 {
            return Rgba { r: 0, g: 0, b: 0 };
        }

        Rgba {
            r: (r_sum / count) as u8,
            g: (g_sum / count) as u8,
            b: (b_sum / count) as u8,
        }
    }

    pub fn get_fg_color(r: u8, g: u8, b: u8) -> &'static str {
        let r_f = f32::from(r) / 255.0;
        let g_f = f32::from(g) / 255.0;
        let b_f = f32::from(b) / 255.0;

        let luminance = Self::relative_luminance(r_f, g_f, b_f);
        if luminance > 0.179 {
            "#000000"
        } else {
            "#FFFFFF"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styling_black_bg_should_return_white_text() {
        assert_eq!(StylingEngine::get_fg_color(0, 0, 0), "#FFFFFF");
    }

    #[test]
    fn styling_white_bg_should_return_black_text() {
        assert_eq!(StylingEngine::get_fg_color(255, 255, 255), "#000000");
    }
}
