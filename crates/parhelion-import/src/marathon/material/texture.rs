//! Resident source textures, including both halves of a streamed mip chain.
use crate::{
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};

fn mip_size(width: usize, height: usize, format: u32) -> Result<usize> {
    let bytes = match format {
        28 | 29 => width.checked_mul(height).and_then(|n| n.checked_mul(4)),
        71 | 72 | 80 => width
            .div_ceil(4)
            .checked_mul(height.div_ceil(4))
            .and_then(|n| n.checked_mul(8)),
        98 | 99 => width
            .div_ceil(4)
            .checked_mul(height.div_ceil(4))
            .and_then(|n| n.checked_mul(16)),
        _ => anyhow::bail!("Unsupported Marathon texture format {format}"),
    };
    bytes.context("Marathon texture size overflow")
}

fn storage(source: &mut Reader, tag: u32, header: &Payload) -> Result<Vec<u8>> {
    ensure!(
        header.0.len() == 64
            && header.u16(32)? == 0xcafe
            && header.u16(38)? == 1
            && header.u16(40)? == 1
            && header.u16(42)? > 0,
        "Marathon material requires an ordinary 2D texture"
    );
    let width = usize::from(header.u16(34)?);
    let height = usize::from(header.u16(36)?);
    let mips = header.u8(45)?;
    ensure!(
        (1..=16384).contains(&width)
            && (1..=16384).contains(&height)
            && mips > 0
            && u32::from(mips) <= width.max(height).ilog2() + 1,
        "Marathon texture dimensions or mip count differ"
    );
    let format = header.u32(4)?;
    let mut boundaries = Vec::new();
    let mut total = 0usize;
    for mip in 0..mips {
        total = total
            .checked_add(mip_size(
                (width >> mip).max(1),
                (height >> mip).max(1),
                format,
            )?)
            .context("Marathon mip chain size overflow")?;
        boundaries.push(total);
    }
    ensure!(
        usize::try_from(header.u32(0)?)? == total,
        "Marathon texture byte count does not describe its mip chain"
    );
    let small_tag = source.reference(tag)?;
    let small = source.tag(small_tag, None)?;
    let large_tag = header.u32(60)?;
    let mut bytes = if [0, u32::MAX, 0x811c9dc5].contains(&large_tag) {
        Vec::new()
    } else {
        ensure!(large_tag != small_tag, "Marathon mip buffers alias");
        let large = source.tag(large_tag, None)?;
        ensure!(
            boundaries.contains(&large.0.len()) && large.0.len() < total,
            "Marathon streaming split is not a mip boundary"
        );
        large.0.clone()
    };
    bytes.extend_from_slice(&small.0);
    ensure!(bytes.len() == total, "Truncated Marathon texture mip chain");
    Ok(bytes)
}

pub(super) fn build(
    source: &mut Reader,
    native: &mut Reader,
    graph: &mut Graph,
    tag: u32,
) -> Result<String> {
    let name = format!("texture-{tag:08X}");
    if graph.nodes.iter().any(|node| node["symbol"] == name) {
        return Ok(name);
    }
    let original = source.tag(tag, None)?;
    let bytes = storage(source, tag, &original)
        .with_context(|| format!("Reading Marathon texture {tag:08X}"))?;
    let format = original.u32(4)?;
    // Fixed resources need the native detail-texture package metadata.
    let template = if matches!(format, 29 | 72 | 99) {
        0x80ec31b9
    } else {
        0x80ec31bc
    };
    let mut header = native.tag(template, None)?.0.clone();
    put(&mut header, 0, &u32::try_from(bytes.len())?.to_le_bytes())?;
    put(&mut header, 4, &format.to_le_bytes())?;
    put(&mut header, 14, &original.bytes::<8>(34)?)?;
    header[23] = original.u8(45)?;
    put(&mut header, 36, &u32::MAX.to_le_bytes())?;
    crate::tiger::texture::resident(&mut header, bytes.len())?;
    let data = format!("{name}-data");
    graph.add(
        &data,
        native.reference(template)?,
        &bytes,
        Some(&name),
        vec![],
    )?;
    graph.add(&name, template, &header, Some(&data), vec![])?;
    Ok(name)
}
