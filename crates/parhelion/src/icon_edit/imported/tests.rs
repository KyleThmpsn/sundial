use super::*;
use crate::{WeaponIconEdit, WeaponRecipe};

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn png_import_fits_without_cropping_and_preserves_transparency() {
    let source = RgbaImage::from_pixel(40, 20, image::Rgba([240, 120, 60, 255]));
    let imported = ImportedIcon::from_bytes(&png(&source)).unwrap();
    let pixels = imported.fit_to(96, 96);
    assert_eq!(pixels.get_pixel(48, 0).0, [0; 4]);
    assert_eq!(pixels.get_pixel(48, 23).0, [0; 4]);
    assert_eq!(pixels.get_pixel(0, 24).0, [240, 120, 60, 255]);
    assert_eq!(pixels.get_pixel(95, 71).0, [240, 120, 60, 255]);
    assert_eq!(pixels.get_pixel(48, 72).0, [0; 4]);
    assert_eq!(imported.fit_to(54, 54).dimensions(), (54, 54));
}

#[test]
fn transparent_rgb_does_not_bleed_into_resized_artwork() {
    let source = RgbaImage::from_fn(192, 192, |x, _| {
        if x < 96 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 255, 0, 0])
        }
    });
    let imported = ImportedIcon::from_bytes(&png(&source)).unwrap();
    for pixel in imported.0.rgba.pixels() {
        assert_eq!(pixel[1], 0);
        assert_eq!(pixel[2], 0);
        if pixel[3] > 0 {
            assert_eq!(pixel[0], 255);
        }
    }
}

#[test]
fn jpeg_import_is_supported() {
    let source = image::RgbImage::from_pixel(32, 32, image::Rgb([200, 40, 80]));
    let mut output = Cursor::new(Vec::new());
    source.write_to(&mut output, ImageFormat::Jpeg).unwrap();
    let imported = ImportedIcon::from_bytes(output.get_ref()).unwrap();
    assert_eq!(imported.0.rgba.dimensions(), (96, 96));
    assert!(imported.0.rgba.pixels().all(|pixel| pixel[3] == 255));
}

#[test]
fn invalid_and_oversized_sources_are_rejected() {
    assert!(ImportedIcon::from_bytes(b"not an image").is_err());
    assert!(ImportedIcon::from_bytes(b"GIF89a").is_err());
    assert!(ImportedIcon::from_bytes(&vec![0; MAX_FILE_BYTES + 1]).is_err());
    let wide = RgbaImage::new(MAX_SOURCE_EDGE + 1, 1);
    assert!(ImportedIcon::from_bytes(&png(&wide)).is_err());
    assert!(ImportedIcon::from_path(Path::new("nonexistent-icon-test.png")).is_err());
}

#[test]
fn embedded_icons_round_trip_in_recipe_without_the_source_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("personal-icon.png");
    RgbaImage::from_pixel(96, 96, image::Rgba([120, 60, 30, 128]))
        .save(&path)
        .unwrap();
    let imported = ImportedIcon::from_path(&path).unwrap();
    directory.close().unwrap();
    let mut recipe = WeaponRecipe::new_weapon("parhelion.import-test").unwrap();
    recipe.overrides.icon_edit.imported_image = Some(imported);
    let encoded = recipe.to_json_pretty().unwrap();
    assert!(encoded.contains("png_base64"));
    assert!(!encoded.contains("personal-icon.png"));
    let decoded = WeaponRecipe::from_json_str(&encoded).unwrap();
    assert_eq!(decoded, recipe);
    assert_eq!(
        decoded.to_spec().unwrap().overrides.icon_edit,
        recipe.overrides.icon_edit
    );
    assert!(!decoded.overrides.icon_edit.is_identity());
    let mut pixels = vec![0; 96 * 96 * 4];
    decoded
        .overrides
        .icon_edit
        .apply_to_rgba8_sized(&mut pixels, 96, 96)
        .unwrap();
    assert!(
        pixels
            .chunks_exact(4)
            .all(|pixel| pixel == [120, 60, 30, 128])
    );
}

#[test]
fn embedded_icons_reject_corrupt_or_noncanonical_data() {
    let malformed = r#"{"png_base64":"not base64!"}"#;
    assert!(serde_json::from_str::<ImportedIcon>(malformed).is_err());
    for image in [RgbaImage::new(1, 1), RgbaImage::new(97, 96)] {
        let encoded = serde_json::to_string(&EmbeddedImage {
            png_base64: STANDARD.encode(png(&image)),
        })
        .unwrap();
        assert!(serde_json::from_str::<ImportedIcon>(&encoded).is_err());
    }
    let oversized = serde_json::to_string(&EmbeddedImage {
        png_base64: "A".repeat(MAX_EMBEDDED_BYTES * 2),
    })
    .unwrap();
    assert!(serde_json::from_str::<ImportedIcon>(&oversized).is_err());
}

#[test]
fn imports_are_transformed_and_reset_color_preserves_source() {
    let source = RgbaImage::from_fn(96, 96, |x, _| {
        if x < 48 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 128])
        }
    });
    let mut edit = WeaponIconEdit {
        imported_image: Some(ImportedIcon::from_bytes(&png(&source)).unwrap()),
        flip_horizontal: true,
        invert: true,
        opacity_percent: 50,
        ..Default::default()
    };
    let mut pixels = vec![7; 96 * 96 * 4];
    edit.apply_to_rgba8_sized(&mut pixels, 96, 96).unwrap();
    assert_eq!(&pixels[..4], &[255, 255, 0, 64]);
    assert_eq!(&pixels[95 * 4..96 * 4], &[0, 255, 255, 128]);
    edit.reset_color();
    assert!(edit.imported_image.is_some());
    assert!(edit.flip_horizontal);
    assert!(edit.color_is_default());
    edit.imported_image = None;
    assert!(!edit.is_identity());
    edit.flip_horizontal = false;
    assert!(edit.is_identity());
}
