use super::*;

fn layer(slot: usize, width: u16, height: u16, data: Vec<u8>) -> icon::Layer {
    icon::Layer {
        slot,
        texture: 1,
        format: 28,
        width,
        height,
        data,
    }
}

#[test]
fn icon_compositing_requires_readable_primary_artwork_and_bounds_its_canvas() {
    assert!(composite(&[layer(0x14, u16::MAX, u16::MAX, vec![])]).is_err());
    assert!(composite(&[layer(0x14, 1, 1, vec![])]).is_err());
    assert!(
        composite(&[
            layer(0x20, 1, 1, vec![255, 0, 0, 255]),
            layer(0x14, 1, 1, vec![]),
        ])
        .is_err()
    );
    let (size, pixels) = composite(&[
        layer(0x20, 1, 1, vec![255, 0, 0, 255]),
        layer(0x14, 1, 1, vec![0, 0, 255, 128]),
        layer(0x24, 1, 1, vec![]),
    ])
    .unwrap();
    assert_eq!(size, [1, 1]);
    assert_eq!(pixels, [127, 0, 128, 255]);
    crate::test_support::artifact(
        "importer-icon-composite.json",
        &serde_json::json!({"size":size,"pixels":pixels,"corrupt_primary_rejected":true,"oversized_canvas_rejected":true}),
    );
}

#[test]
fn cache_reads_reject_oversized_or_corrupt_images_and_accept_valid_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("icon.png");
    image::save_buffer(&path, &[20, 30, 40, 255], 1, 1, image::ColorType::Rgba8).unwrap();
    assert_eq!(cached_image(&path).unwrap().as_raw(), &[20, 30, 40, 255]);
    std::fs::write(&path, b"truncated png").unwrap();
    assert!(cached_image(&path).is_err());
    // The encoded image is tiny but the dimension exceeds the decoded allocation limit.
    image::save_buffer(&path, &vec![0; 4097 * 4], 4097, 1, image::ColorType::Rgba8).unwrap();
    assert!(cached_image(&path).is_err());
}
