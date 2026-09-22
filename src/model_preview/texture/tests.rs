use super::*;

#[test]
fn bc5_normals_keep_both_linear_channels_and_reject_truncation() {
    let block = [128, 128, 0, 0, 0, 0, 0, 0, 200, 200, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        decode(&block, 83, 3, 2).unwrap(),
        [128, 200, 255, 255].repeat(6)
    );
    assert!(decode(&block[..15], 83, 3, 2).is_err());
    assert_eq!(preview_mip(83, 4096, 1024).unwrap(), (2048, 512, 4_194_304));
}

#[test]
fn material_masks_keep_linear_alpha_during_bilinear_sampling() {
    let texture = Texture {
        tag: 0,
        size: [2, 1],
        rgba: vec![255, 128, 40, 16, 128, 64, 255, 255],
    };
    assert_eq!(texture.sample_rgba([0.25, 0.5]), [255.0, 128.0, 40.0, 16.0]);
    assert_eq!(texture.sample_rgba([0.5, 0.5]), [191.5, 96.0, 147.5, 135.5]);
    assert_eq!(texture.sample_rgba([-0.25, 0.5])[3], 255.0);
}

#[test]
fn grayscale_and_bgra_formats_preserve_channels_and_check_lengths() {
    assert_eq!(
        decode(&[10, 20, 30, 40], 87, 1, 1).unwrap(),
        [30, 20, 10, 40]
    );
    assert_eq!(
        decode(&[10, 20, 30, 0], 88, 1, 1).unwrap(),
        [30, 20, 10, 255]
    );
    assert_eq!(
        decode(&[17, 200], 61, 2, 1).unwrap(),
        [17, 17, 17, 255, 200, 200, 200, 255]
    );
    assert!(decode(&[0; 3], 87, 1, 1).is_err());
    assert!(decode(&[0], 61, 2, 1).is_err());
    let block = [180, 50, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        decode(&block, 80, 3, 2).unwrap(),
        [180, 180, 180, 255].repeat(6)
    );
    assert!(decode(&block[..7], 80, 3, 2).is_err());
}

#[test]
fn mip_selection_accounts_for_compressed_block_sizes_and_non_square_images() {
    assert_eq!(preview_mip(99, 4096, 1024).unwrap(), (2048, 512, 4_194_304));
    assert_eq!(
        preview_mip(80, 8192, 2048).unwrap(),
        (2048, 512, 10_485_760)
    );
    assert_eq!(
        preview_mip(87, 4096, 4096).unwrap(),
        (2048, 2048, 67_108_864)
    );
    assert_eq!(preview_mip(99, 512, 512).unwrap(), (512, 512, 0));
    assert!(preview_mip(999, 4096, 4096).is_err());
}

#[test]
fn color_bindings_take_priority_over_linear_data() {
    assert!(color_rank(99, 1) < color_rank(98, 0));
    assert_eq!(color_rank(99, 0), Some(0));
    assert_eq!(color_rank(83, 0), None); // Two-channel normal/data texture.
    assert_eq!(color_rank(98, 4), None); // Unidentified later linear binding.
}
