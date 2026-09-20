use super::*;

pub(crate) fn image(color: [u8; 4]) -> crate::icon_edit::ImportedIcon {
    let pixels = image::RgbaImage::from_pixel(96, 96, image::Rgba(color));
    let mut png = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    crate::icon_edit::ImportedIcon::from_bytes(&png.into_inner()).unwrap()
}

#[test]
fn edited_images_get_new_textures_while_identical_saved_copies_share_one() {
    let ctx = egui::Context::default();
    let white = image([255; 4]);
    let first = image_texture(&ctx, &white);
    let restored = serde_json::from_slice(&serde_json::to_vec(&white).unwrap()).unwrap();
    assert_eq!(image_texture(&ctx, &restored).id(), first.id());
    let edited = image([96, 64, 32, 255]);
    let second = image_texture(&ctx, &edited);
    assert_ne!(second.id(), first.id());
    assert_eq!(image_texture(&ctx, &white).id(), first.id());
    assert_eq!(image_texture(&ctx, &edited).id(), second.id());
    assert_eq!(first.size(), [96, 96]);
}
