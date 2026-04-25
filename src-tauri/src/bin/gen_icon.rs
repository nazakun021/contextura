// src-tauri/src/bin/gen_icon.rs

use image::{Rgba, RgbaImage};
use std::fs;

fn main() {
    let mut img = RgbaImage::new(512, 512);
    for pixel in img.pixels_mut() {
        *pixel = Rgba([255, 0, 0, 255]);
    }
    fs::create_dir_all("icons").unwrap();
    img.save("icons/icon.png").unwrap();
    img.save("icons/32x32.png").unwrap();
    img.save("icons/128x128.png").unwrap();
    img.save("icons/128x128@2x.png").unwrap();
    img.save("icons/icon.icns").unwrap();
    img.save("icons/icon.ico").unwrap();
}
