//! Xbox texture storage translated without recompressing its authored mip levels.
use super::*;
mod xg;

pub(super) struct Texture {
    pub format: u32,
    pub width: u16,
    pub height: u16,
    pub layers: u16,
    pub mips: u8,
    pub bytes: Vec<u8>,
}

pub(super) fn surface(format: u32, width: usize, height: usize) -> Result<(usize, usize)> {
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384,
        "Unsupported Xbox texture dimensions"
    );
    let (row, rows) = match format {
        28 | 29 => (width * 4, height),
        71 | 72 | 80 | 81 => (width.div_ceil(4) * 8, height.div_ceil(4)),
        77 | 78 | 83 | 84 | 98 | 99 => (width.div_ceil(4) * 16, height.div_ceil(4)),
        _ => anyhow::bail!("Unsupported Xbox texture format {format}"),
    };
    Ok((row, row * rows))
}

fn mip_major(face_major: &[u8], lengths: &[(usize, usize)], layers: usize) -> Result<Vec<u8>> {
    let per_layer = lengths.iter().map(|(_, length)| length).sum::<usize>();
    ensure!(
        face_major.len() == per_layer * layers,
        "Cube map footprint differs from its mip chain"
    );
    let mut result = Vec::with_capacity(face_major.len());
    for (mip, (_, length)) in lengths.iter().enumerate() {
        let start = lengths[..mip]
            .iter()
            .map(|(_, length)| length)
            .sum::<usize>();
        for layer in 0..layers {
            let at = layer * per_layer + start;
            result.extend_from_slice(&face_major[at..at + length]);
        }
    }
    Ok(result)
}

impl Source {
    pub(super) fn linear_texture(&self, tag: u32, library: Option<&Path>) -> Result<Texture> {
        let mut assets = BTreeMap::new();
        let metadata = self.texture(tag, &mut assets)?;
        let header = assets.get(&tag).context("Source texture header")?;
        let mut raw = Vec::new();
        if let Some(high) = metadata["high_mips"].as_u64() {
            raw.extend_from_slice(&assets[&(high as u32)].0);
        }
        let data = metadata["data"]
            .as_u64()
            .context("Source texture storage")? as u32;
        raw.extend_from_slice(&assets[&data].0);
        let format = header.u32(0)?;
        let width = header.u16(0x30)?;
        let height = header.u16(0x32)?;
        let layers = header.u16(0x36)?;
        let mips = header.u8(0x39)?;
        ensure!(
            header.u16(0x34)? == 1 && matches!(layers, 1 | 6),
            "Unsupported Xbox texture topology"
        );
        let maximum = 16 - width.max(height).leading_zeros() as u8;
        ensure!(
            mips > 0 && mips <= maximum,
            "Invalid Xbox texture mip count"
        );
        let mut lengths = Vec::new();
        for mip in 0..mips {
            lengths.push(surface(
                format,
                usize::from((width >> mip).max(1)),
                usize::from((height >> mip).max(1)),
            )?);
        }
        let bytes = if header.u32(4)? == 8 {
            ensure!(
                raw.len()
                    == lengths.iter().map(|(_, length)| length).sum::<usize>()
                        * usize::from(layers),
                "Linear Xbox texture footprint differs"
            );
            raw
        } else {
            xg::linearize(header, &raw, &lengths, library)?
        };
        // The source subresource order is face-major (every mip of face 0, then face 1).
        // Shadowkeep stores a cube map mip-major, all six faces of mip 0 before mip 1, which
        // alkahest's loader reproduces when it fills the D3D11 subresources in package order.
        // A face-major payload reads as five scrambled faces and wrong deep mips in game.
        let bytes = if layers == 6 {
            mip_major(&bytes, &lengths, usize::from(layers))?
        } else {
            bytes
        };
        Ok(Texture {
            format,
            width,
            height,
            layers,
            mips,
            bytes,
        })
    }
}
