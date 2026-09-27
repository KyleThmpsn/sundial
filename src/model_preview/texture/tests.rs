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
    assert_eq!(texture.sample_ramp(0.0), [255.0, 128.0, 40.0, 16.0]);
    assert_eq!(texture.sample_ramp(1.0), [128.0, 64.0, 255.0, 255.0]);
    let clamp = Sampler {
        u: AddressMode::Clamp,
        v: AddressMode::Clamp,
        border: [0.0; 4],
    };
    assert_eq!(
        texture.sample_with_sampler([0.0, 0.0], &clamp),
        [255.0, 128.0, 40.0, 16.0]
    );
    assert_eq!(
        texture.sample_with_sampler([1.0, 0.0], &clamp),
        [128.0, 64.0, 255.0, 255.0]
    );
}

#[test]
fn native_border_sampler_uses_its_border_color_outside_the_mask() {
    let texture = Texture {
        tag: 0,
        size: [2, 1],
        rgba: vec![255; 8],
    };
    let sampler = Sampler {
        u: AddressMode::Border,
        v: AddressMode::Clamp,
        border: [0.0; 4],
    };
    assert_eq!(
        texture.sample_with_sampler([-0.25, 0.5], &sampler),
        [0.0; 4]
    );
    assert_eq!(
        texture.sample_with_sampler([0.0, 0.5], &sampler),
        [127.5; 4]
    );
    assert_eq!(
        texture.sample_with_sampler([0.25, 0.5], &sampler),
        [255.0; 4]
    );
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
fn packed_float_particle_texture_decodes_native_channels() {
    let red = 15_u32 << 6;
    let green = 14_u32 << 6;
    let pixel = (red | green << 11).to_le_bytes();
    assert_eq!(decode(&pixel, 26, 1, 1).unwrap(), [255, 128, 0, 255]);
    assert!(decode(&pixel[..3], 26, 1, 1).is_err());
    assert_eq!(
        preview_mip(26, 4096, 1024).unwrap(),
        (2048, 512, 16_777_216)
    );
}

#[test]
fn two_channel_unorm_texture_preserves_both_shader_inputs() {
    assert_eq!(
        decode(&[0, 128, 255, 255], 35, 1, 1).unwrap(),
        [128, 255, 0, 255]
    );
    assert!(decode(&[0, 128, 255], 35, 1, 1).is_err());
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
