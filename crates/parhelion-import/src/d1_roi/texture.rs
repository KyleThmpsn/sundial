//! Xbox texture storage translated without recompressing its authored mip levels.
use super::*;
use serde::Deserialize;
mod amd;
pub use amd::{LICENSE, SOURCE_URL};

/// Storage metadata from the Xbox One source descriptor. Multisampled render targets and
/// depth/stencil planes are not serialized color textures and are not accepted here.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Descriptor {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub layers: u32,
    pub mips: u32,
    pub tile_mode: u32,
}

pub(super) struct Texture {
    pub format: u32,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub layers: u16,
    pub mips: u8,
    pub bytes: Vec<u8>,
}

/// Bytes per stored element and pixel width/height of that element. Signed, floating-point
/// and sRGB variants retain their bytes. Packed video, planar and depth formats are excluded.
fn storage(format: u32) -> Result<(usize, usize)> {
    Ok(match format {
        1..=4 => (16, 1),
        9..=18 => (8, 1),
        23..=39 | 41..=43 | 87..=93 => (4, 1),
        48..=54 | 56..=59 | 85 | 86 | 115 => (2, 1),
        60..=65 => (1, 1),
        70..=72 | 79..=81 => (8, 4),
        73..=78 | 82..=84 | 94..=99 => (16, 4),
        _ => anyhow::bail!("Unsupported Xbox texture format {format}"),
    })
}

pub(super) fn surface(format: u32, width: usize, height: usize) -> Result<(usize, usize)> {
    ensure!(
        (1..=16384).contains(&width) && (1..=16384).contains(&height),
        "Unsupported Xbox texture dimensions"
    );
    let (bytes, block) = storage(format)?;
    let row = width.div_ceil(block) * bytes;
    Ok((row, row * height.div_ceil(block)))
}

struct Level {
    width: u32,
    height: u32,
    depth: u32,
    bytes: usize,
}

impl Descriptor {
    fn levels(&self) -> Result<Vec<Level>> {
        ensure!(
            (1..=16384).contains(&self.width)
                && (1..=16384).contains(&self.height)
                && (1..=2048).contains(&self.depth),
            "Unsupported Xbox texture dimensions"
        );
        ensure!(
            matches!(self.layers, 1 | 6) && (self.depth == 1 || self.layers == 1),
            "Unsupported Xbox texture topology"
        );
        let maximum = 32 - self.width.max(self.height).max(self.depth).leading_zeros();
        ensure!(
            self.mips > 0 && self.mips <= maximum,
            "Invalid Xbox texture mip count"
        );
        ensure!(
            matches!(self.tile_mode, 8 | 13 | 14),
            "Unsupported Xbox tile mode {}",
            self.tile_mode
        );
        let (bytes, block) = storage(self.format)?;
        (0..self.mips)
            .map(|mip| {
                let width = (self.width >> mip).max(1).div_ceil(block as u32);
                let height = (self.height >> mip).max(1).div_ceil(block as u32);
                let depth = (self.depth >> mip).max(1);
                let length = u64::from(width) * u64::from(height) * u64::from(depth) * bytes as u64;
                Ok(Level {
                    width,
                    height,
                    depth,
                    bytes: usize::try_from(length)?,
                })
            })
            .collect()
    }
}

fn allocate(length: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .context("Allocate decoded Xbox texture")?;
    bytes.resize(length, 0);
    Ok(bytes)
}

/// Decode complete source storage into native mip-major order. Each mip contains all cube
/// faces, or every depth slice of a volume. High and resident mip payloads must be joined first.
/// Uses only statically linked AMD code, with no optional DLL or platform-specific fallback.
pub fn decode(descriptor: &Descriptor, source: &[u8]) -> Result<Vec<u8>> {
    let levels = descriptor.levels()?;
    let per_layer = levels
        .iter()
        .try_fold(0usize, |total, level| total.checked_add(level.bytes))
        .context("Xbox mip chain size overflow")?;
    let length = per_layer
        .checked_mul(descriptor.layers as usize)
        .context("Xbox texture size overflow")?;
    ensure!(length <= source.len(), "Xbox texture storage is truncated");
    if descriptor.tile_mode == 8 {
        return linear(descriptor, source, &levels, per_layer, length);
    }
    tiled(descriptor, source, &levels, length)
}

fn linear(
    descriptor: &Descriptor,
    source: &[u8],
    levels: &[Level],
    per_layer: usize,
    length: usize,
) -> Result<Vec<u8>> {
    ensure!(
        source.len() == length,
        "Linear Xbox texture footprint differs"
    );
    let mut output = allocate(length)?;
    let mut offset = 0;
    let mut destination = 0;
    for level in levels {
        crate::cancellation::check()?;
        for layer in 0..descriptor.layers as usize {
            let start = layer * per_layer + offset;
            output[destination..destination + level.bytes]
                .copy_from_slice(&source[start..start + level.bytes]);
            destination += level.bytes;
        }
        offset += level.bytes;
    }
    Ok(output)
}

fn tiled(
    descriptor: &Descriptor,
    source: &[u8],
    levels: &[Level],
    length: usize,
) -> Result<Vec<u8>> {
    let library = amd::Library::new()?;
    let mut surfaces = Vec::new();
    let mut footprint = 0u64;
    let mut base_pitch = 0;
    for (mip, level) in levels.iter().enumerate() {
        crate::cancellation::check()?;
        let surface = library.surface(&amd::Request {
            width: level.width,
            height: level.height,
            slices: if descriptor.depth > 1 {
                level.depth
            } else {
                descriptor.layers
            },
            bits: storage(descriptor.format)?.0 as u32 * 8,
            mode: descriptor.tile_mode,
            mip: mip as u32,
            mips: descriptor.mips,
            base_pitch,
            volume: u32::from(descriptor.depth > 1),
        })?;
        if mip == 0 {
            base_pitch = surface.pitch;
        }
        let mask = u64::from(surface.alignment) - 1;
        let start = footprint
            .checked_add(mask)
            .context("Xbox mip alignment overflow")?
            & !mask;
        footprint = start
            .checked_add(surface.bytes)
            .context("Xbox tiled footprint overflow")?;
        surfaces.push((
            usize::try_from(start)?,
            usize::try_from(footprint)?,
            surface,
        ));
    }
    ensure!(
        footprint == source.len() as u64,
        "Xbox tiled footprint differs from the complete source payload"
    );
    let mut output = allocate(length)?;
    let mut destination = 0;
    for (level, (start, end, surface)) in levels.iter().zip(surfaces) {
        for layer in 0..descriptor.layers {
            crate::cancellation::check()?;
            library.copy(
                &surface,
                (level.width, level.height),
                (layer, level.depth),
                &source[start..end],
                &mut output[destination..destination + level.bytes],
            )?;
            destination += level.bytes;
        }
    }
    Ok(output)
}

impl Source {
    // The legacy parameter is retained for callers with saved options, but never loads a DLL.
    pub(super) fn linear_texture(&self, tag: u32, _library: Option<&Path>) -> Result<Texture> {
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
        let descriptor = Descriptor {
            format: header.u32(0)?,
            width: u32::from(header.u16(0x30)?),
            height: u32::from(header.u16(0x32)?),
            depth: u32::from(header.u16(0x34)?),
            layers: u32::from(header.u16(0x36)?),
            mips: u32::from(header.u8(0x39)?),
            tile_mode: header.u32(4)?,
        };
        let bytes =
            decode(&descriptor, &raw).with_context(|| format!("Decode Xbox texture {tag:08X}"))?;
        Ok(Texture {
            format: descriptor.format,
            width: descriptor.width as u16,
            height: descriptor.height as u16,
            depth: descriptor.depth as u16,
            layers: descriptor.layers as u16,
            mips: descriptor.mips as u8,
            bytes,
        })
    }
}
