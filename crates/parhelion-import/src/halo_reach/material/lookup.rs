//! MCC's DXT5a specular lookup array, decoded without treating its slices as cube faces.
use super::*;

pub(in crate::halo_reach) struct Lookup {
    pub width: usize,
    pub height: usize,
    pub layers: usize,
    pub rgba: Vec<u8>,
}

pub(in crate::halo_reach) fn read(
    c: &Cache,
    pages: &mut Pages,
    tag: &Tag,
    image: usize,
) -> Result<Lookup> {
    let row = *c
        .block(tag.address()? + 124, 56)?
        .get(image)
        .context("Lookup image outside bitmap table")?;
    let width = usize::from(c.u16(row)?);
    let height = usize::from(c.u16(row + 2)?);
    let layers = usize::from(c.u8(row + 4)?);
    ensure!(
        width > 0 && width <= 4096 && height > 0 && height <= 4096 && (1..=32).contains(&layers),
        "Invalid lookup array dimensions"
    );
    ensure!(
        c.u8(row + 5)? & 8 == 0 && c.u8(row + 6)? == 3 && c.i16(row + 8)? == 36,
        "Specular lookup requires a linear-layout MCC DXT5a array"
    );
    ensure!(
        c.u8(row + 16)? == 0 && c.u8(row + 17)? == LINEAR_CURVE,
        "Unsupported lookup mip chain or transfer curve"
    );
    let stride = image_length(width, height, 8, true);
    let length = stride
        .checked_mul(layers)
        .context("Lookup array size overflow")?;
    ensure!(
        length <= 64 * 1024 * 1024 && usize::try_from(c.u32(row + 24)?)? == length,
        "Lookup array payload does not match its slices"
    );
    ensure!(
        height * layers <= 16384 && width * height * layers * 4 <= 64 * 1024 * 1024,
        "Decoded lookup array exceeds the native texture budget"
    );
    let raw = pages.bytes(c, &image_resource(c, tag, row, image)?, 0, length)?;
    let mut rgba = Vec::with_capacity(width * height * layers * 4);
    for slice in raw.chunks_exact(stride) {
        // DXT5a is the unsigned BC4 block. The lookup shader consumes its scalar channel.
        rgba.extend(decode(slice, width, height, 31, false)?);
    }
    Ok(Lookup {
        width,
        height,
        layers,
        rgba,
    })
}
