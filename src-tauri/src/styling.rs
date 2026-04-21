// Background Color Sampling & Contrast Calculation

#[derive(Debug, Clone, Copy)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub fn to_css_color(&self) -> String {
        format!("rgba({}, {}, {}, 0.85)", self.r, self.g, self.b)
    }
}

pub struct StylingEngine;

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

    pub fn get_fg_color(r: u8, g: u8, b: u8) -> &'static str {
        let r_f = r as f32 / 255.0;
        let g_f = g as f32 / 255.0;
        let b_f = b as f32 / 255.0;

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
