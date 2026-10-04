//! Native particle textures: resident headers holding the source mip chain.
use super::{Context, Node};
use anyhow::{Context as _, Result, bail, ensure};

/// Bytes per block and block edge for the BC and plain formats particle materials use.
fn layout(format: u32) -> Result<(usize, usize)> {
    Ok(match format {
        28 | 29 | 35 => (4, 1),
        71 | 72 | 80 | 81 => (8, 4),
        74 | 75 | 77 | 78 | 83 | 84 | 95 | 96 | 98 | 99 => (16, 4),
        _ => bail!("uninspected particle texture format {format}"),
    })
}

pub(super) fn convert(c: &mut Context, source: u32) -> Result<String> {
    let symbol = format!("particle-texture-{source:08X}");
    if c.nodes.contains(&symbol) {
        return Ok(symbol);
    }
    let entry = c
        .source
        .manager
        .get_entry(tiger_pkg::TagHash(source))
        .with_context(|| format!("missing particle texture {source:08X}"))?;
    ensure!(
        entry.file_type == 32,
        "particle texture {source:08X} is not a texture header"
    );
    let header = c.source.tag(source, None)?;
    let (width, height, depth, layers) = (
        header.u16(34)?,
        header.u16(36)?,
        header.u16(38)?,
        header.u16(40)?,
    );
    let (format, mips) = (header.u32(4)?, header.u8(45)?);
    ensure!(
        depth == 1 && layers == 1,
        "particle texture {source:08X} is not a single 2D surface"
    );
    ensure!(
        width > 0 && height > 0 && width <= 16384 && height <= 16384 && (1..=15).contains(&mips),
        "invalid particle texture dimensions"
    );
    let (block, tile) = layout(format)?;
    let mut data = Vec::new();
    let large = header.u32(60)?;
    if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
        data.extend_from_slice(&c.source.tag(large, None)?.0);
    }
    let buffer = c.source.reference(source)?;
    data.extend_from_slice(&c.source.tag(buffer, None)?.0);
    let expected = (0..mips)
        .map(|m| {
            (usize::from(width) >> m).max(1).div_ceil(tile)
                * (usize::from(height) >> m).max(1).div_ceil(tile)
                * block
        })
        .sum::<usize>();
    ensure!(
        data.len() == expected && data.len() == header.u32(0)? as usize,
        "particle texture {source:08X} mip chain is incomplete"
    );
    let mut native = c.templates.texture.header.clone();
    native[0..4].copy_from_slice(&u32::try_from(data.len())?.to_le_bytes());
    native[4..8].copy_from_slice(&format.to_le_bytes());
    native[14..16].copy_from_slice(&width.to_le_bytes());
    native[16..18].copy_from_slice(&height.to_le_bytes());
    native[18..20].copy_from_slice(&1u16.to_le_bytes());
    native[20..22].copy_from_slice(&1u16.to_le_bytes());
    native[22] = header.u8(44)?;
    native[23] = mips;
    native[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
    crate::d2_mot::texture::resident(&mut native, data.len())?;
    let data_symbol = format!("{symbol}-data");
    c.nodes.add(Node {
        symbol: symbol.clone(),
        template: c.templates.texture.header_tag,
        payload: native,
        reference: Some(data_symbol.clone()),
        patches: Vec::new(),
    })?;
    c.nodes.add(Node {
        symbol: data_symbol,
        template: c.templates.texture.data_tag,
        payload: data,
        reference: Some(symbol.clone()),
        patches: Vec::new(),
    })?;
    Ok(symbol)
}
