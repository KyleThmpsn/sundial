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
    let at = w.bytes.len();
    w.u16(count);
    let mut kept = 0u16;
    for _ in 0..count {
        let id = r.u32()?;
        let source = r.u32()?;
        // Shared curve identities in original 89/113 banks establish these
        // node parameter numbers and their additive accumulation mode.
        let mapping = match source {
            0 | 2 | 3 => Some((1, source as u8)),
            5 => Some((1, 7)),
            0x0F..=0x11 => Some((1, (source + 0x29) as u8)),
            0x16 => Some((1, 0x3C)),
            0x17 => Some((1, 0x3E)),
            // Positioning type keeps its number. Native banks drive it exclusively, with the
            // same game parameter and a constant curve between zero and one.
            0x0A => Some((0, 0x0A)),
            _ => None,
        };
        // A parameter without an established native number leaves the bank with its curve, so
        // the object plays at its authored value for that parameter instead of refusing the whole
        // bank. Choosing a number without that evidence could drive the wrong native parameter.
        // The Necrochasm shot's bank `80AA85AB` drives parameter `13` from game parameter
        // `2ACC9909`, which native banks use with a 0 to 500 range for distance filters. Its curve
        // `004A3BA9` occurs in no native bank and is zero at zero, so the shooter hears no
        // difference. The dropped curve's own reading is not established.
        let Some((accumulation, native)) = mapping else {
            r.take(5)?;
            let points = r.u16()?;
            r.take(usize::from(points) * 12)?;
            continue;
        };
        dependencies.params.insert(id);
        w.u32(id);
        kept += 1;
        w.u8(0);
        w.u8(accumulation);
        w.u8(native);
        w.copy(r, 5)?; // Curve identity and scaling.
        let points = r.u16()?;
        w.u16(points);
        let start = w.bytes.len();
        curve(r, w, usize::from(points))?;
        if source == 0x0A {
            // Native positioning type selects a route rather than blending between them.
            let rows = &w.bytes[start..];
            ensure!(
                rows.chunks_exact(12).all(|row| {
                    let y = f32::from_le_bytes(row[4..8].try_into().expect("four bytes"));
                    y == 0.0 || y == 1.0
                }) && rows
                    .chunks_exact(12)
                    .rev()
                    .skip(1)
                    .all(|row| row[8..12] == 9u32.to_le_bytes()),
                "early positioning type curve is not a constant switch"
            );
        }
    }
    w.bytes[at..at + 2].copy_from_slice(&kept.to_le_bytes());
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
    let attenuation = r.u32()?;
    let spatialized = flag(r)?;
    match kind {
        1 => {
            let dynamic = flag(r)?;
            native |= 8 | (spatialized << 4);
            w.u8(native);
            w.u8(1 | ((1 - dynamic) << 3));
            w.reference(attenuation, Kind::Object);
            Ok(())
        }
        0 => {
            native |= 8;
            path(r, w, native, spatialized, attenuation)
        }
        _ => bail!("early 3D position type {kind} needs another mapping"),
    }
}

/// A user-defined 3D position: a playlist of vertex paths that the sound follows relative to the
/// listener. Version 89 stores the path mode as a UInt32 and the looping and follow-orientation
/// choices as bytes after it, and each playlist item's random range as two Float32 values.
/// Version 113 stores the mode as a byte, the vertices and playlist in the same layout, a third
/// range value after each pair, and position type 0 in the 3D byte, as all 1,668 automated objects
/// in Shadowkeep's stock banks do. The third range is zero in each of their twenty most common
/// range forms, and their vertices lie in the source paths' XZ plane. The looping and follow-orientation choices move into the positioning byte,
/// whose bits 4 to 7 only automated stock objects clear selectively. Which of those bits holds
/// which choice is not established, so only a source that sets spatialization, looping and
/// follow-orientation together converts, to the stock form `F9` that 570 automated objects store.
/// The source shot mixer `0C5285A7` of the shared weapon bank `80AA3103` is one.
fn path(
    r: &mut Read<'_>,
    w: &mut Write,
    native: u8,
    spatialized: u8,
    attenuation: u32,
) -> Result<()> {
    let mode = r.u32()?;
    ensure!(mode <= 3, "early path mode {mode} differs");
    let looping = flag(r)?;
    let transition = r.u32()?;
    ensure!(
        i32::try_from(transition).is_ok(),
        "early path transition is negative"
    );
    let follow = flag(r)?;
    ensure!(
        spatialized == 1 && looping == 1 && follow == 1,
        "early automated position flags (spatialized {spatialized}, looping {looping}, follow \
         orientation {follow}) need another mapping"
    );
    w.u8(native | 0xF0);
    w.u8(0);
    w.reference(attenuation, Kind::Object);
    w.u8(mode as u8);
    w.u32(transition);
    let vertices = r.u32()?;
    ensure!(vertices <= 4096, "early path has too many vertices");
    w.u32(vertices);
    for _ in 0..vertices {
        let row = r.take(16)?;
        ensure!(
            row[..12]
                .chunks_exact(4)
                .all(|v| f32::from_le_bytes(v.try_into().expect("four bytes")).is_finite())
                && i32::from_le_bytes(row[12..].try_into()?) >= 0,
            "invalid early path vertex"
        );
        w.bytes.extend_from_slice(row);
    }
    let items = r.u32()?;
    ensure!(
        (1..=1024).contains(&items),
        "early path playlist count differs"
    );
    w.u32(items);
    for _ in 0..items {
        let offset = r.u32()?;
        let count = r.u32()?;
        ensure!(
            count > 0 && offset.checked_add(count).is_some_and(|end| end <= vertices),
            "early path playlist item exceeds its vertices"
        );
        w.u32(offset);
        w.u32(count);
    }
    for _ in 0..items {
        let row = r.take(8)?;
        ensure!(
            row.chunks_exact(4).all(|v| {
                let v = f32::from_le_bytes(v.try_into().expect("four bytes"));
                v.is_finite() && v >= 0.0
            }),
            "invalid early path range"
        );
        w.bytes.extend_from_slice(row);
        // Version 113 adds a third range, zero in every common stock form.
        w.bytes.extend_from_slice(&0f32.to_le_bytes());
    }
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
