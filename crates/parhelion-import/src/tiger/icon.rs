//! Inventory icon layers as the client's icon containers hold them in every build, and their
//! decoding to RGBA8 for browser thumbnails and recipe artwork.
use anyhow::{Result, ensure};

/// One undecoded texture: `format` is the DXGI format number from the header.
pub struct Layer {
    /// Container slot offset the layer came from.
    pub slot: usize,
    pub texture: u32,
    pub format: u32,
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

/// Decode the same bounded texture formats for browser thumbnails and recipe artwork.
pub fn decode(layer: &Layer) -> Result<Vec<u8>> {
    let (width, height) = (usize::from(layer.width), usize::from(layer.height));
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096,
        "Unsupported image dimensions"
    );
    if matches!(layer.format, 28 | 29) {
        ensure!(
            layer.data.len() == width * height * 4,
            "RGBA image size differs"
        );
        return Ok(layer.data.clone());
    }
    let block_size = match layer.format {
        71 | 72 => 8,
        74 | 75 | 77 | 78 | 98 | 99 => 16,
        format => anyhow::bail!("Unsupported image texture format {format}"),
    };
    let columns = width.div_ceil(4);
    ensure!(
        layer.data.len() == columns * height.div_ceil(4) * block_size,
        "Compressed image size differs"
    );
    let mut rgba = vec![0; width * height * 4];
    for (index, block) in layer.data.chunks_exact(block_size).enumerate() {
        let mut pixels = [0; 64];
        match layer.format {
            71 | 72 => bcdec_rs::bc1(block, &mut pixels, 16),
            74 | 75 => bcdec_rs::bc2(block, &mut pixels, 16),
            77 | 78 => bcdec_rs::bc3(block, &mut pixels, 16),
            _ => bcdec_rs::bc7(block, &mut pixels, 16),
        }
        for y in 0..4 {
            for x in 0..4 {
                let (px, py) = ((index % columns) * 4 + x, (index / columns) * 4 + y);
                if px < width && py < height {
                    let to = (py * width + px) * 4;
                    let from = (y * 4 + x) * 4;
                    rgba[to..to + 4].copy_from_slice(&pixels[from..from + 4]);
                }
            }
        }
    }
    Ok(rgba)
}
