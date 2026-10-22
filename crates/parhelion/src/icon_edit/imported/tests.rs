use super::*;
use crate::WeaponRecipe;
use crate::image_import::MAX_SOURCE_EDGE;

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png).unwrap();
    output.into_inner()
}

#[test]
fn invalid_and_oversized_sources_are_rejected() {
    assert!(ImportedIcon::from_bytes(b"not an image").is_err());
    assert!(ImportedIcon::from_bytes(b"GIF89a").is_err());
    let wide = RgbaImage::new(MAX_SOURCE_EDGE + 1, 1);
    assert!(ImportedIcon::from_bytes(&png(&wide)).is_err());
    let directory = tempfile::tempdir().unwrap();
    assert!(ImportedIcon::from_path(&directory.path().join("missing.png")).is_err());
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
