//! Preserve a witnessed unit secondary constraint over the source clip's complete frame domain.
use super::*;

pub(super) fn curve(
    template: &Payload,
    native: &mut Vec<u8>,
    base: usize,
    tracks: usize,
    frames: usize,
) -> Result<()> {
    let old_frames = usize::from(template.u16(0x13C)?);
    ensure!(
        usize::from(template.u16(base + 2)?) == tracks && (1..=3600).contains(&old_frames),
        "Secondary curve track or frame count differs"
    );
    let size = template.u32(0x134)? as usize;
    let end = (base - 4)
        .checked_add(size)
        .context("Secondary curve extent")?;
    let mut arrays = Vec::new();
    for (field, stride, class) in [
        (0x10, 2, 0x80800006),
        (0x20, 2, 0x80800006),
        (0x30, 1, 0x80800009),
        (0x40, 1, 0x80800009),
        (0x50, 2, 0x80800006),
    ] {
        let range = template.array_range(base + field, stride, Some(class))?;
        ensure!(
            range.is_empty() || (range.start >= base && range.end <= end),
            "Secondary curve array escapes its codec block"
        );
        arrays.push(range);
    }
    let [raw, compressed, times, tangents, mappings] = arrays.as_slice() else {
        unreachable!()
    };
    ensure!(
        mappings.len() == (tracks + 1) * 2
            && template.u16(mappings.end - 2)? as usize == compressed.len() / 2,
        "Secondary curve mapping endpoint differs"
    );
    for row in (mappings.start..mappings.end - 2).step_by(2) {
        let mapping = template.i16(row)?;
        ensure!(mapping != 0, "Secondary curve has no track");
        if mapping < 0 {
            let start = raw.start + (usize::from(mapping.unsigned_abs()) - 1) * 2;
            let stop = start + old_frames * 2;
            ensure!(
                stop <= raw.end
                    && (start..stop)
                        .step_by(2)
                        .all(|at| template.i16(at).ok() == Some(32767)),
                "Secondary dense weight is not constant unit"
            );
        } else {
            let start = compressed.start + (mapping as usize - 1) * 2;
            ensure!(
                start + 6 <= compressed.end,
                "Secondary curve header exceeds its array"
            );
            let time = usize::try_from(template.i16(start)?)?;
            let tangent = usize::try_from(template.i16(start + 2)?)?;
            let segments = usize::try_from(template.i16(start + 4)?)?;
            ensure!(
                segments > 0
                    && segments < old_frames
                    && time + segments <= times.len()
                    && tangent + segments <= tangents.len()
                    && start + 6 + (segments + 1) * 2 <= compressed.end,
                "Secondary curve exceeds its sample arrays"
            );
            let deltas = &template.0[times.start + time..times.start + time + segments];
            ensure!(
                deltas.iter().all(|&v| v > 0)
                    && deltas.iter().map(|&v| usize::from(v)).sum::<usize>() == old_frames - 1
                    && template.0[tangents.start + tangent..tangents.start + tangent + segments]
                        .iter()
                        .all(|&v| v == 0x77)
                    && (0..=segments).all(|i| template.i16(start + 6 + i * 2).ok() == Some(32767)),
                "Secondary curve weight is not constant unit"
            );
        }
    }
    // Dense scalar tracks are sampled with the owning clip's frame indices. Repack each
    // validated constant track to that full domain, retaining the witnessed codec header.
    let start = (native.len() + 11) & !7;
    native.resize(start - 4, 0);
    native.extend(0x80808F7Eu32.to_le_bytes());
    native.extend_from_slice(&template.0[base..base + 16]);
    native.resize(start + 0x60, 0);
    let samples = vec![32767i16; tracks * frames]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut maps = Vec::new();
    for track in 0..tracks {
        maps.extend(i16::try_from(-(i32::try_from(track * frames)? + 1))?.to_le_bytes());
    }
    maps.extend(0i16.to_le_bytes());
    for (field, class, bytes, stride) in [
        (0x10, 0x80800006, samples.as_slice(), 2),
        (0x20, 0x80800006, &[], 2),
        (0x30, 0x80800009, &[], 1),
        (0x40, 0x80800009, &[], 1),
        (0x50, 0x80800006, maps.as_slice(), 2),
    ] {
        crate::presentation::append(native, start + field, class, bytes, stride)?;
    }
    put(native, 0x40, &(start as i64 - 0x40).to_le_bytes());
    let size = (native.len() - start + 11) & !7;
    native.resize(start - 4 + size, 0);
    put(native, 0x134, &u32::try_from(size)?.to_le_bytes());
    Ok(())
}
