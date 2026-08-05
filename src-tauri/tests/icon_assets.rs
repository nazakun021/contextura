use image::GenericImageView;

#[test]
fn packaged_icon_uses_the_contextura_translation_mark() {
    let icon_path = format!("{}/icons/icon.png", env!("CARGO_MANIFEST_DIR"));
    let icon = image::open(icon_path).expect("packaged app icon should be readable");

    assert_eq!(icon.dimensions(), (512, 512));
    assert_eq!(
        icon.get_pixel(0, 0).0[3],
        0,
        "icon corner should be transparent"
    );
    assert!(
        icon.pixels()
            .any(|(_, _, pixel)| pixel.0 == [250, 204, 21, 255]),
        "icon should contain the translation-mark accent"
    );
}
