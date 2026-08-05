// src-tauri/src/bin/gen_icon.rs

use image::{Rgba, RgbaImage, imageops::FilterType};
use std::fs;

const CANVAS: u32 = 1024;

fn is_inside_rounded_rect(x: u32, y: u32, left: u32, top: u32, right: u32, bottom: u32, radius: u32) -> bool {
    if x < left || x >= right || y < top || y >= bottom {
        return false;
    }
    let closest_x = x.clamp(left + radius, right - radius - 1);
    let closest_y = y.clamp(top + radius, bottom - radius - 1);
    let delta_x = i64::from(x) - i64::from(closest_x);
    let delta_y = i64::from(y) - i64::from(closest_y);
    delta_x * delta_x + delta_y * delta_y <= i64::from(radius * radius)
}

fn fill_rounded_rect(image: &mut RgbaImage, left: u32, top: u32, right: u32, bottom: u32, radius: u32, color: Rgba<u8>) {
    for y in top..bottom {
        for x in left..right {
            if is_inside_rounded_rect(x, y, left, top, right, bottom, radius) {
                image.put_pixel(x, y, color);
            }
        }
    }
}

fn fill_triangle(image: &mut RgbaImage, top: (u32, u32), left: (u32, u32), right: (u32, u32), color: Rgba<u8>) {
    let min_x = top.0.min(left.0).min(right.0);
    let max_x = top.0.max(left.0).max(right.0);
    let min_y = top.1.min(left.1).min(right.1);
    let max_y = top.1.max(left.1).max(right.1);
    let area = (i64::from(left.0) - i64::from(top.0)) * (i64::from(right.1) - i64::from(top.1))
        - (i64::from(left.1) - i64::from(top.1)) * (i64::from(right.0) - i64::from(top.0));

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let first = (i64::from(left.0) - i64::from(top.0)) * (i64::from(y) - i64::from(top.1))
                - (i64::from(left.1) - i64::from(top.1)) * (i64::from(x) - i64::from(top.0));
            let second = (i64::from(right.0) - i64::from(left.0)) * (i64::from(y) - i64::from(left.1))
                - (i64::from(right.1) - i64::from(left.1)) * (i64::from(x) - i64::from(left.0));
            let third = (i64::from(top.0) - i64::from(right.0)) * (i64::from(y) - i64::from(right.1))
                - (i64::from(top.1) - i64::from(right.1)) * (i64::from(x) - i64::from(right.0));
            if (first >= 0 && second >= 0 && third >= 0 && area >= 0)
                || (first <= 0 && second <= 0 && third <= 0 && area < 0)
            {
                image.put_pixel(x, y, color);
            }
        }
    }
}

fn main() -> Result<(), image::ImageError> {
    let mut icon = RgbaImage::new(CANVAS, CANVAS);
    for y in 0..CANVAS {
        for x in 0..CANVAS {
            if is_inside_rounded_rect(x, y, 32, 32, 992, 992, 210) {
                let blue = 114 + u8::try_from((x * 24) / CANVAS).unwrap_or_default();
                let green = 119 + u8::try_from((y * 36) / CANVAS).unwrap_or_default();
                icon.put_pixel(x, y, Rgba([14, green, blue, u8::MAX]));
            }
        }
    }

    fill_rounded_rect(&mut icon, 158, 176, 866, 766, 92, Rgba([236, 253, 250, u8::MAX]));
    fill_rounded_rect(&mut icon, 192, 210, 832, 674, 58, Rgba([11, 94, 103, u8::MAX]));
    fill_rounded_rect(&mut icon, 292, 300, 732, 566, 82, Rgba([255, 255, 255, u8::MAX]));
    fill_triangle(&mut icon, (520, 656), (494, 540), (618, 540), Rgba([255, 255, 255, u8::MAX]));
    fill_rounded_rect(&mut icon, 374, 374, 650, 414, 18, Rgba([13, 148, 136, u8::MAX]));
    fill_rounded_rect(&mut icon, 374, 450, 582, 490, 18, Rgba([250, 204, 21, u8::MAX]));
    fill_rounded_rect(&mut icon, 266, 734, 758, 774, 18, Rgba([20, 184, 166, u8::MAX]));

    fs::create_dir_all("icons").expect("icons directory should be creatable");
    let output = image::imageops::resize(&icon, 512, 512, FilterType::Lanczos3);
    output.save("icons/icon.png")?;
    output.save("icons/32x32.png")?;
    output.save("icons/128x128.png")?;
    output.save("icons/128x128@2x.png")?;
    Ok(())
}
