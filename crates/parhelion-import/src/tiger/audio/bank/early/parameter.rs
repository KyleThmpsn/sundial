//! Versioned property fields and packed node flags. Curves retain source control points.
use super::*;

fn property(id: u8) -> Result<u8> {
    Ok(match id {
        0..=3 | 8..=0x17 | 0x1A..=0x2C => id,
        4..=6 => id + 1,
        7 => 0x3A,
        0x18 => 0x19,
        0x19 => 0x3B,
        _ => bail!("early property {id:02X} has no native mapping"),
    })
}

pub(super) fn properties(r: &mut Read<'_>, w: &mut Write, ranged: bool) -> Result<()> {
    let count = usize::from(r.u8()?);
    let ids = r.take(count)?.to_vec();
    let width = if ranged { 8 } else { 4 };
    let values = r.take(count * width)?;
    let mut rows = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| Ok((property(id)?, index)))
        .collect::<Result<Vec<_>>>()?;
    rows.sort_unstable();
    ensure!(
        rows.windows(2).all(|v| v[0].0 != v[1].0),
        "duplicate early property"
    );
    w.u8(count as u8);
    for &(id, _) in &rows {
        w.u8(id);
    }
    for (_, index) in rows {
        w.bytes
            .extend_from_slice(&values[index * width..(index + 1) * width]);
    }
    Ok(())
}

pub(super) fn curve(r: &mut Read<'_>, w: &mut Write, points: usize) -> Result<()> {
    ensure!(points <= 65536, "early curve exceeds limit");
    let mut previous = None;
    for _ in 0..points {
        let row = r.take(12)?;
        let x = f32::from_le_bytes(row[..4].try_into()?);
        let y = f32::from_le_bytes(row[4..8].try_into()?);
        let interpolation = u32::from_le_bytes(row[8..12].try_into()?);
        ensure!(
            x.is_finite() && y.is_finite() && previous.is_none_or(|p| x >= p) && interpolation <= 9,
            "invalid early curve point"
        );
        previous = Some(x);
        w.bytes.extend_from_slice(row);
    }
    Ok(())
}

pub(super) fn rtpcs(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    let count = r.u16()?;
    w.u16(count);
    for _ in 0..count {
        let id = r.u32()?;
        dependencies.params.insert(id);
        w.u32(id);
        let source = r.u32()?;
        // Shared curve identities in original 89/113 banks establish these
        // node parameter numbers and their additive accumulation mode.
        let native = match source {
            0 | 2 | 3 => source as u8,
            5 => 7,
            0x0F..=0x11 => (source + 0x29) as u8,
            0x16 => 0x3C,
            0x17 => 0x3E,
            _ => bail!("early RTPC parameter {source:02X} needs a native accumulation mapping"),
        };
        w.u8(0);
        w.u8(1);
        w.u8(native);
        w.copy(r, 5)?; // Curve identity and scaling.
        let points = r.u16()?;
        w.u16(points);
        curve(r, w, usize::from(points))?;
    }
    Ok(())
}

fn position(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let bits = r.u8()?;
    ensure!(bits <= 1, "early position flags need another mapping");
    if bits == 0 {
        w.u8(0xC0);
        return Ok(());
    }
    let flat = flag(r)?;
    let spatial = flag(r)?;
    let panner = if flat == 1 { flag(r)? } else { 0 };
    let mut native = 0xC1 | (flat << 1) | (panner << 2);
    if spatial == 0 {
        w.u8(native);
        return Ok(());
    }
    let kind = r.u32()?;
    ensure!(
        kind == 1,
        "early automated position requires path translation"
    );
    let attenuation = r.u32()?;
    let spatialized = flag(r)?;
    let dynamic = flag(r)?;
    native |= 8 | (spatialized << 4);
    w.u8(native);
    w.u8(1 | ((1 - dynamic) << 3));
    w.reference(attenuation, Kind::Object);
    Ok(())
}

fn advanced(r: &mut Read<'_>, w: &mut Write) -> Result<()> {
    let queue = r.u8()?;
    ensure!(queue <= 2, "early virtual queue mode differs");
    let kill = flag(r)?;
    let virtualize = flag(r)?;
    let limit = r.u16()?;
    let global = flag(r)?;
    let threshold = r.u8()?;
    ensure!(threshold <= 2, "early virtual threshold differs");
    let override_limit = flag(r)?;
    let override_voice = flag(r)?;
    w.u8(kill | (virtualize << 1) | (global << 2) | (override_limit << 3) | (override_voice << 4));
    w.u8(queue);
    w.u16(limit);
    w.u8(threshold);
    w.u8(flags(r, 4)?);
    Ok(())
}

pub(super) fn node(
    r: &mut Read<'_>,
    w: &mut Write,
    dependencies: &mut settings::Dependencies,
) -> Result<()> {
    w.u8(flag(r)?);
    let fx = r.u8()?;
    ensure!(fx <= 4, "early node has too many effects");
    w.u8(fx);
    if fx != 0 {
        w.copy(r, 1)?;
        let mut slots = BTreeSet::new();
        for _ in 0..fx {
            let slot = r.u8()?;
            ensure!(slot < 4 && slots.insert(slot), "invalid early effect slot");
            w.u8(slot);
            w.object(r)?;
            w.u8(flag(r)?);
            w.u8(flag(r)?);
        }
    }
    w.u8(0); // No attachment override exists in 89.
    w.object(r)?; // The bus is validated against the target Init hierarchy by the caller.
    w.object(r)?;
    w.u8(flags(r, 2)?);
    properties(r, w, false)?;
    properties(r, w, true)?;
    position(r, w)?;
    let aux = flags(r, 4)?;
    w.u8(aux);
    if aux & 8 != 0 {
        for _ in 0..4 {
            w.object(r)?;
        }
    }
    advanced(r, w)?;
    let groups = r.count()?;
    w.u32(groups as u32);
    for _ in 0..groups {
        let group = r.u32()?;
        dependencies.groups.insert(group);
        w.u32(group);
        w.copy(r, 1)?;
        let count = r.u16()?;
        w.u16(count);
        for _ in 0..count {
            w.copy(r, 4)?;
            w.object(r)?;
        }
    }
    rtpcs(r, w, dependencies)
}
