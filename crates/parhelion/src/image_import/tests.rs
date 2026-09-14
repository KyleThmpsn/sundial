use super::*;
use crate::{hud_icon::HudImage, icon_edit::ImportedIcon, presentation::Artwork};

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
