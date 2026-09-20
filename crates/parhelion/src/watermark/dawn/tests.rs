use super::*;

#[test]
fn dawn_artwork_does_not_require_the_sunrise_donor_alpha_mask() {
    let donor = vec![0; 45 * 45 * 4];
    assert_eq!(
        authored_texture(crate::branding::Branding::Dawn, 2, 45, 45, &donor).unwrap(),
        render(2).unwrap()
    );
    assert!(authored_texture(crate::branding::Branding::Sunrise, 2, 45, 45, &donor).is_err());
}

#[test]
fn corner_glyphs_follow_stock_optical_anchors() {
    // Forsaken and Curse of Osiris place the 96px glyph around x=15.5.
    // Mirror that anchor for the tooltip corner, with the same optical height.
    // Measure only white ink, excluding the black native corner plate.
    for (index, expected_x) in [(0, 15.5), (1, 38.5)] {
        let native = image::load_from_memory(TEXTURES[index])
            .unwrap()
            .into_rgba8();
        for (image, scale) in [(native, 1.0), (render(index).unwrap(), 4.0)] {
            let (mut mass, mut x_mass, mut y_mass) = (0.0, 0.0, 0.0);
            for (x, y, pixel) in image.enumerate_pixels() {
                let weight = f64::from(pixel[0]) * f64::from(pixel[3]);
                mass += weight;
                x_mass += (f64::from(x) + 0.5) / scale * weight;
                y_mass += (f64::from(y) + 0.5) / scale * weight;
            }
            assert!(mass > 0.0);
            assert!((x_mass / mass - expected_x).abs() < 0.15);
            assert!((y_mass / mass - 15.5).abs() < 0.25);
        }
    }
}

#[test]
fn all_lanes_preserve_native_geometry_and_palette() {
    for (index, (width, height)) in TEXTURE_DIMENSIONS.into_iter().enumerate() {
        let image = render(index).unwrap();
        assert_eq!(image.dimensions(), (width * 4, height * 4));
        assert!(image.pixels().any(|pixel| pixel[3] == 0));
        assert!(image.pixels().any(|pixel| pixel[3] >= 200));
        if matches!(index, 0 | 1 | 4 | 5) {
            let sunrise = render_output_texture(index).unwrap();
            for (before, after) in sunrise.pixels().zip(image.pixels()) {
                if before[3] == 0 {
                    assert!(after[3] <= 2, "Dawn escapes native corner plate {index}");
                }
            }
        }
    }
    let dark = render(2).unwrap();
    let white = render(3).unwrap();
    assert!(dark.pixels().zip(white.pixels()).all(|(a, b)| a[3] == b[3]));
}
