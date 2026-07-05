// src-tauri/src/snapshot.rs

use image::ColorType;
use std::path::PathBuf;

/// Swaps BGRA channels to RGBA.
pub fn swap_bgra_to_rgba(buffer: &[u8]) -> Vec<u8> {
    let mut rgba_data = buffer.to_vec();
    for pixel in rgba_data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    rgba_data
}

/// Encodes an RGBA pixel buffer to a temporary PNG file.
pub fn save_frame_as_png(
    rgba_data: &[u8],
    width: usize,
    height: usize,
    frame_id: u64,
) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/contextura-frame-{frame_id}.png"));
    let latest_path = PathBuf::from("/tmp/contextura-frame-latest.png");

    image::save_buffer(
        &path,
        rgba_data,
        u32::try_from(width)?,
        u32::try_from(height)?,
        ColorType::Rgba8,
    )?;
    // Keep the latest captured frame on disk for manual inspection/debugging.
    let _ = image::save_buffer(
        &latest_path,
        rgba_data,
        u32::try_from(width)?,
        u32::try_from(height)?,
        ColorType::Rgba8,
    );

    Ok(path)
}

/// Deletes stale temporary frame files from /tmp.
pub fn cleanup_stale_temp_frames() {
    let Ok(entries) = std::fs::read_dir("/tmp") else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with("contextura-frame-")
            && std::path::Path::new(file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_bgra_to_rgba() {
        let bgra = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let rgba = swap_bgra_to_rgba(&bgra);
        // Swaps indices:
        // First pixel: 10 (B) and 30 (R) should swap -> [30, 20, 10, 40]
        // Second pixel: 50 (B) and 70 (R) should swap -> [70, 60, 50, 80]
        assert_eq!(rgba, vec![30, 20, 10, 40, 70, 60, 50, 80]);
    }
}
