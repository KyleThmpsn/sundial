//! Checked MCC layer-major arrays, preserving each layer's complete compressed mip chain.
use super::*;

pub struct Array {
    pub width: usize,
    pub height: usize,
    pub levels: u8,
    pub format: u32,
    pub curve: u8,
    pub layers: Vec<Vec<u8>>,
}

pub fn read(c: &Cache, pages: &mut Pages, tag: &Tag, image: usize) -> Result<Array> {
    let row = *c
        .block(tag.address()? + 124, 56)?
        .get(image)
        .context("Array image outside bitmap table")?;
    let width = usize::from(c.u16(row)?);
    let height = usize::from(c.u16(row + 2)?);
    let count = usize::from(c.u8(row + 4)?);
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384,
        "Invalid array dimensions"
    );
    ensure!(
        c.u8(row + 5)? & 8 == 0 && c.u8(row + 6)? == 3,
        "Bitmap is not a linear MCC 2D array"
    );
    ensure!((1..=32).contains(&count), "Invalid array layer count");
    let source_format = c.i16(row + 8)?;
    let format = match source_format {
        14 => 71,
        15 => 74,
        16 => 77,
        38 => 84,
        44 => 83,
        _ => anyhow::bail!("Unsupported compressed array format {source_format}"),
    };
    let levels = c
        .u8(row + 16)?
        .checked_add(1)
        .context("Array mip count overflow")?;
    ensure!(
        u32::from(levels) <= width.max(height).ilog2() + 1,
        "Array mip count exceeds dimensions"
    );
    let (unit, compressed) = texel_layout(source_format, tag)?;
    let chain = (0..levels)
        .map(|level| {
            image_length(
                (width >> level).max(1),
                (height >> level).max(1),
                unit,
                compressed,
            )
        })
        .sum::<usize>();
    let length = chain
        .checked_mul(count)
        .context("Array byte count overflow")?;
    ensure!(
        length <= 256 * 1024 * 1024 && usize::try_from(c.u32(row + 24)?)? == length,
        "Array payload does not match its layers and mips"
    );
    let resource = image_resource(c, tag, row, image)?;
    // The verified array layout is contiguous. A streamed array's interleaving is not established
    // by the separate top-mip layout used for ordinary 2D images.
    ensure!(
        !resource.secondary,
        "Streamed MCC arrays require a validated mip layout"
    );
    let raw = pages.bytes(c, &resource, 0, length)?;
    Ok(Array {
        width,
        height,
        levels,
        format,
        curve: c.u8(row + 17)?,
        layers: raw.chunks_exact(chain).map(<[u8]>::to_vec).collect(),
    })
}
