//! Constant control translation without resampling native curves or retiming events.
use super::*;
use anyhow::Context;

fn curve(
    p: &Payload,
    out: &mut [u8],
    base: usize,
    track: usize,
    frames: usize,
    delta: [f32; 3],
) -> Result<()> {
    use sundial::package_authoring::animation_curve::{Channel, Decoder};
    let counts = [
        p.u16(base + 2)? as usize,
        p.u16(base + 4)? as usize,
        p.u16(base + 6)? as usize,
    ];
    // Validate the selected track's keys, time coverage and tangents before editing it.
    Decoder::read(&p.0, base, counts, frames)
        .and_then(|decoder| decoder.samples(Channel::Position, track))
        .map_err(anyhow::Error::msg)?;
    let ranges = p.array(base + 0x50, 4, Some(0x8080000F))?;
    let maps = p.array(base + 0x70, 2, Some(0x80800006))?;
    ensure!(
        ranges.len() == counts[0] + counts[2] && maps.len() == counts.iter().sum::<usize>() + 1,
        "Native curve interval or track counts differ"
    );
    let range = p.f32(ranges[counts[0] + track])?;
    ensure!(
        range.is_finite() && range > 0.,
        "Native translation curve has no finite range"
    );
    let map = p.u16(maps[counts[0] + counts[1] + track])? as i16;
    ensure!(map != 0, "Native curve track has no sample offset");
    let rows = p.array(
        base + if map < 0 { 0x10 } else { 0x20 },
        2,
        Some(0x80800006),
    )?;
    let (start, count) = if map < 0 {
        ((-i32::from(map) - 1) as usize, frames)
    } else {
        let at = map as usize - 1;
        let segments = p.u16(
            *rows
                .get(at + 2)
                .context("Curve header is outside its table")?,
        )? as usize;
        ensure!(
            segments > 0 && segments < frames,
            "Native curve segment count differs"
        );
        (at + 3, segments + 1)
    };
    let shifts = delta.map(|v| (v / range * 32767.).round() as i32);
    for frame in 0..count {
        for (axis, shift) in shifts.iter().enumerate() {
            let at = *rows
                .get(start + frame * 3 + axis)
                .context("Curve translation exceeds samples")?;
            let value = i32::from(p.u16(at)? as i16) + shift;
            ensure!(
                (-32767..=32767).contains(&value),
                "Held offset exceeds native curve translation range"
            );
            put(out, at, &(value as i16).to_le_bytes())?;
        }
    }
    Ok(())
}

/// Adjust one control in an absolute clip. The caller establishes its playback mode.
pub fn translate(
    original: &Payload,
    slot: usize,
    default: [f32; 3],
    delta: [f32; 3],
) -> Result<Payload> {
    let frames = usize::from(original.u16(0x13C)?);
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        original.u64(0)? == original.0.len() as u64
            && original.u64(0x108)? == 0
            && (1..=3600).contains(&frames)
            && slot < slots
            && slots <= 256
            && delta.iter().all(|v| v.is_finite() && v.abs() <= 0.2),
        "Held offset or native clip dimensions differ"
    );
    let mut out = original.0.clone();
    let mut found = false;
    for (pointer, maps, count) in [(0x10, 0xA8, 1), (0x18, 0xD8, frames)] {
        let positions = original.array(maps + 32, 2, Some(0x8080000A))?;
        let tracks = positions
            .iter()
            .enumerate()
            .filter_map(|(i, &at)| (original.u16(at).ok() == Some(slot as u16)).then_some(i))
            .collect::<Vec<_>>();
        ensure!(
            tracks.len() <= 1 && !(found && !tracks.is_empty()),
            "Native control repeats a translation channel"
        );
        let Some(&track) = tracks.first() else {
            continue;
        };
        found = true;
        let base = original.pointer(pointer)?;
        ensure!(base >= 4, "Native clip codec lacks its class header");
        ensure!(
            original.u16(base + 6)? as usize == positions.len(),
            "Native position map differs from codec"
        );
        match original.u16(base)? {
            1 => curve(original, &mut out, base, track, count, delta)?,
            2 => {
                ensure!(
                    original.u32(base - 4)? == 0x80808F71
                        && original.u32(base + 0x10)? as usize == count,
                    "Native interval codec differs"
                );
                let bias = original.array(base + 0x38, 4, Some(0x8080000F))?;
                let first = original.u16(base + 2)? as usize
                    + original.u16(base + 4)? as usize * 4
                    + track * 3;
                for (axis, shift) in delta.iter().enumerate() {
                    let at = *bias
                        .get(first + axis)
                        .context("Native interval bias is missing")?;
                    let value = original.f32(at)? + shift;
                    ensure!(value.is_finite(), "Nonfinite native translation");
                    put(&mut out, at, &value.to_le_bytes())?;
                }
            }
            _ => {
                let mut channels = read(original, pointer, maps, slots, count)?;
                for value in channels[2].values[track * count * 3..(track + 1) * count * 3]
                    .chunks_exact_mut(3)
                {
                    for (v, shift) in value.iter_mut().zip(delta) {
                        *v += shift;
                    }
                }
                write(&mut out, pointer, maps, count, &channels)?;
            }
        }
    }
    if !found {
        let mut fixed = read(original, 0x10, 0xA8, slots, 1)?;
        fixed[2].slots.push(slot as u16);
        fixed[2]
            .values
            .extend(default.into_iter().zip(delta).map(|(v, d)| v + d));
        write(&mut out, 0x10, 0xA8, 1, &fixed)?;
    }
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok(Payload(out))
}
