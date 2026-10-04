use super::*;
use crate::{hud_icon::HudImage, icon_edit::ImportedIcon, presentation::Artwork};

#[test]
fn extreme_aspect_imports_render_the_center_at_canvas_size() {
    let directory = tempfile::tempdir().unwrap();
    for horizontal in [false, true] {
        let (width, height) = if horizontal { (2400, 1) } else { (1, 2400) };
        let source = RgbaImage::from_fn(width, height, |x, y| {
            let position = if horizontal { x } else { y };
            image::Rgba(if (1100..1300).contains(&position) {
                [255, 255, 255, 128]
            } else {
                [255, 0, 0, 0]
            })
        });
        let path = directory.path().join("narrow.png");
        source.save(&path).unwrap();
        let imported = EmbeddedImage::from_path(&path).unwrap();
        let result = cover(imported.pixels(), 512, 256);
        assert_eq!(result.dimensions(), (512, 256));
        assert!(result.pixels().all(|pixel| pixel.0 == [255, 255, 255, 128]));
        if let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            result
                .save(std::path::PathBuf::from(directory).join(format!("cover-{horizontal}.png")))
                .unwrap();
        }
    }
}

#[test]
fn a_detailed_import_can_be_saved_and_reopened_at_its_supported_resolution() {
    let directory = tempfile::tempdir().unwrap();
    let mut random = 0xA193_84F5_u32;
    let source = image::RgbImage::from_fn(MAX_EMBEDDED_EDGE, MAX_EMBEDDED_EDGE, |_, _| {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let bytes = random.to_le_bytes();
        image::Rgb([bytes[0], bytes[1], bytes[2]])
    });
    let input = directory.path().join("detailed.jpg");
    source.save(&input).unwrap();
    let imported = EmbeddedImage::from_path(&input).unwrap();
    let saved = directory.path().join("embedded.json");
    serde_json::to_writer(File::create(&saved).unwrap(), &imported).unwrap();
    assert!(
        std::fs::metadata(&saved).unwrap().len() > 12 * 1024 * 1024,
        "fixture must exceed the old reader limit"
    );
    let reopened: EmbeddedImage =
        serde_json::from_reader(std::io::BufReader::new(File::open(saved).unwrap())).unwrap();
    assert_eq!(
        reopened.pixels().dimensions(),
        (MAX_EMBEDDED_EDGE, MAX_EMBEDDED_EDGE)
    );
    assert_eq!(reopened, imported);
    if let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        cover(reopened.pixels(), 320, 180)
            .save(std::path::PathBuf::from(directory).join("reopened-image.png"))
            .unwrap();
    }
}

#[test]
fn importers_preserve_their_format_and_composition_contracts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.png");
    let source = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 40, 80]));
    let mut jpeg = Cursor::new(Vec::new());
    source.write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
    std::fs::write(&path, jpeg.get_ref()).unwrap();

    // Source imports detect the contents independently of the filename.
    assert!(ImportedIcon::from_bytes(jpeg.get_ref()).is_ok());
    let artwork = Artwork::from_path(&path).unwrap();
    assert_eq!(artwork.pixels().dimensions(), (40, 20));
    assert!(artwork.composition().is_some());
    assert!(HudImage::from_path(&path).is_err());
    assert!(HudImage::from_png(jpeg.get_ref()).is_err());
    assert!(Artwork::from_png(jpeg.get_ref()).is_err());

    let mut png = Cursor::new(Vec::new());
    source.write_to(&mut png, ImageFormat::Png).unwrap();
    std::fs::write(&path, png.get_ref()).unwrap();
    assert!(HudImage::from_path(&path).is_ok());
    let source_artwork = Artwork::from_path(&path).unwrap();
    assert_eq!(source_artwork.pixels().dimensions(), (40, 20));
    assert!(source_artwork.composition().is_some());
    let normalized_artwork = Artwork::from_png(png.get_ref()).unwrap();
    assert_eq!(normalized_artwork.pixels().dimensions(), (512, 512));
    assert!(normalized_artwork.composition().is_none());
}

#[test]
fn all_import_paths_reject_oversized_files_and_dimensions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oversized.png");
    let oversized = vec![0; MAX_FILE_BYTES + 1];
    std::fs::write(&path, &oversized).unwrap();
    assert!(read_path(&path).unwrap_err().contains("16 MiB"));
    assert!(ImportedIcon::from_bytes(&oversized).is_err());
    assert!(HudImage::from_png(&oversized).is_err());
    assert!(Artwork::from_png(&oversized).is_err());
    assert!(HudImage::from_path(&path).is_err());
    assert!(Artwork::from_path(&path).is_err());

    let mut png = Cursor::new(Vec::new());
    RgbaImage::new(1, MAX_SOURCE_EDGE + 1)
        .write_to(&mut png, ImageFormat::Png)
        .unwrap();
    std::fs::write(&path, png.get_ref()).unwrap();
    assert!(ImportedIcon::from_bytes(png.get_ref()).is_err());
    assert!(HudImage::from_png(png.get_ref()).is_err());
    assert!(Artwork::from_png(png.get_ref()).is_err());
    assert!(HudImage::from_path(&path).is_err());
    assert!(Artwork::from_path(&path).is_err());
}
