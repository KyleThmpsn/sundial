//! Select complete skinned cloth draws when native cloth physics is unavailable.
use super::*;

fn simulated(model: &Payload, part: usize) -> Result<bool> {
    Ok(model.u8(part + 29)? == 3 && model.u8(part + 30)? == 127 && model.u32(part + 24)? & 8 != 0)
}

fn range(model: &Payload, part: usize) -> Result<(u16, u32, u32)> {
    Ok((
        model.u16(part + 6)?,
        model.u32(part + 8)?,
        model.u32(part + 12)?,
    ))
}

fn fallback(
    model: &mut Payload,
    mesh: usize,
    read: &mut dyn FnMut(u32) -> Result<Payload>,
) -> Result<()> {
    let parts = model.array(mesh + 32, 36, Some(0x80806ECB))?;
    let ranges = (0..25)
        .map(|i| model.u16(mesh + 48 + i * 2).map(usize::from))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        ranges[0] == 0 && ranges[24] == parts.len() && ranges.windows(2).all(|w| w[0] <= w[1]),
        "cloth stage ranges do not cover its draw records"
    );
    let mut records = Vec::new();
    let mut counts = vec![0u16];
    for stage in 0..24 {
        let rows = &parts[ranges[stage]..ranges[stage + 1]];
        for &part in rows {
            if simulated(model, part)? {
                let geometry = range(model, part)?;
                let retained = rows
                    .iter()
                    .copied()
                    .filter(|&other| {
                        model.u8(other + 29).ok() == Some(0)
                            && model.u8(other + 30).ok() == Some(0)
                            && range(model, other).ok() == Some(geometry)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    retained.len() == 1,
                    "cloth simulation draw has no unique skinned fallback"
                );
                // Keep the original surface equations, rather than borrowing another material.
                let a = model.u32(part)?;
                let b = model.u32(retained[0])?;
                if a != u32::MAX && b != u32::MAX {
                    let a = read(a)?;
                    let b = read(b)?;
                    ensure!(
                        a.u32(8)? == b.u32(8)? && a.u32(0x2B0)? == b.u32(0x2B0)?,
                        "cloth fallback changes the pixel program or material stage"
                    );
                } else {
                    ensure!(a == b, "cloth fallback has an unmatched implicit material");
                }
            } else {
                records.push(model.0[part..part + 36].to_vec());
            }
        }
        counts.push(u16::try_from(records.len())?);
    }
    ensure!(!records.is_empty(), "cloth has no skinned draw records");
    let header = model.pointer(mesh + 40)?;
    let count = u64::try_from(records.len())?.to_le_bytes();
    model.0[mesh + 32..mesh + 40].copy_from_slice(&count);
    model.0[header..header + 8].copy_from_slice(&count);
    for (index, record) in records.iter().enumerate() {
        let at = header + 16 + index * 36;
        model.0[at..at + 36].copy_from_slice(record);
    }
    for (stage, count) in counts.into_iter().enumerate() {
        let at = mesh + 48 + stage * 2;
        model.0[at..at + 2].copy_from_slice(&count.to_le_bytes());
    }
    Ok(())
}

pub(super) fn prepare(
    mut model: Payload,
    read: &mut dyn FnMut(u32) -> Result<Payload>,
) -> Result<Payload> {
    for mesh in model.array(16, 128, Some(0x80806EC5))? {
        let positions = read(model.u32(mesh)?)?;
        if (positions.u16(4)?, positions.u16(6)?) != (48, 1) {
            continue;
        }
        let uv = read(model.u32(mesh + 4)?)?;
        let skin = read(model.u32(mesh + 8)?)?;
        ensure!(
            (uv.u16(4)?, uv.u16(6)?, skin.u16(4)?, skin.u16(6)?) == (4, 1, 8, 1),
            "float cloth stream declarations differ"
        );
        // This is the package's three-stream cloth declaration, not an item identity.
        ensure!(
            (0..24).all(|stage| model.u8(mesh + 98 + stage).ok() == Some(13)),
            "float cloth input layout differs"
        );
        fallback(&mut model, mesh, read)?;
    }
    Ok(model)
}
