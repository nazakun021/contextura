// src-tauri/src/snapshot.rs

use image::ColorType;
use image::ImageEncoder;
use std::path::Path;

/// Swaps BGRA channels to RGBA in-place.
pub fn swap_bgra_to_rgba_inplace(buffer: &mut [u8]) {
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

/// Encodes an RGBA pixel buffer to a temporary PNG file.
pub fn encode_frame_as_png(
    rgba_data: &[u8],
    width: usize,
    height: usize,
) -> anyhow::Result<Vec<u8>> {
    let mut png_bytes = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        encoder.write_image(
            rgba_data,
            u32::try_from(width)?,
            u32::try_from(height)?,
            ColorType::Rgba8,
        )?;
    }
    Ok(png_bytes)
}

/// Deletes stale temporary frame files from the cache directory.
pub fn cleanup_stale_temp_frames(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if (file_name.starts_with("contextura-frame-")
            && std::path::Path::new(file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png")))
            || file_name == "contextura-frame-latest.png"
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
        let mut data = vec![10, 20, 30, 40, 50, 60, 70, 80];
        swap_bgra_to_rgba_inplace(&mut data);
        // Swaps indices:
        // First pixel: 10 (B) and 30 (R) should swap -> [30, 20, 10, 40]
        // Second pixel: 50 (B) and 70 (R) should swap -> [70, 60, 50, 80]
        assert_eq!(data, vec![30, 20, 10, 40, 70, 60, 50, 80]);
    }

    #[test]
    fn test_encode_and_cleanup_frames() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("contextura_test_cache_{unique}"));
        let _ = std::fs::create_dir_all(&temp_dir);

        // Create an unrelated file that should NOT be cleaned up
        let unrelated_path = temp_dir.join("unrelated.txt");
        std::fs::write(&unrelated_path, "important data").unwrap();

        let rgba_data = vec![0; 400]; // 10x10 RGBA image
        let png_bytes = encode_frame_as_png(&rgba_data, 10, 10).unwrap();
        let path = temp_dir.join("contextura-frame-9999.png");
        let latest = temp_dir.join("contextura-frame-latest.png");
        std::fs::write(&path, &png_bytes).unwrap();
        std::fs::write(&latest, &png_bytes).unwrap();

        // Verify that the frame is correctly saved in the specified cache directory
        assert!(path.exists(), "Expected snapshot to exist at {path:?}");
        assert!(latest.exists());

        cleanup_stale_temp_frames(&temp_dir);
        assert!(!path.exists());
        assert!(!latest.exists());

        // Verify that unrelated file was NOT deleted
        assert!(
            unrelated_path.exists(),
            "Expected unrelated.txt to be preserved"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
