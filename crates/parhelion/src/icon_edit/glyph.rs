//! Imported perk glyphs keep the transparent inset used by stock 96-pixel artwork.
use image::RgbaImage;

pub(crate) fn fit(source: &RgbaImage) -> RgbaImage {
    const EDGE: u32 = 96;
    const INSET: u32 = 4;
    let source = crate::image_import::fit(source, EDGE, EDGE);
    let reaches_border = source.enumerate_pixels().any(|(x, y, pixel)| {
        pixel[3] >= 16 && (x < INSET || y < INSET || x >= EDGE - INSET || y >= EDGE - INSET)
    });
    // Do not shrink already padded artwork or enlarge deliberately small glyphs.
    if !reaches_border {
        return source;
    }
    let inset = crate::image_import::resize(&source, EDGE - 2 * INSET, EDGE - 2 * INSET);
    let mut canvas = RgbaImage::new(EDGE, EDGE);
    image::imageops::replace(&mut canvas, &inset, i64::from(INSET), i64::from(INSET));
    canvas
}
