//! Preserve native clip timing and events while appending explicit local channels.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Result, ensure};

#[derive(Default)]
struct Channel {
    slots: Vec<u16>,
    values: Vec<f32>,
}

fn read(
    p: &Payload,
    pointer: usize,
    maps: usize,
    slots: usize,
    frames: usize,
) -> Result<[Channel; 3]> {
    let mut channels: [Channel; 3] = Default::default();
    for (kind, channel) in channels.iter_mut().enumerate() {
        channel.slots = p
            .array(maps + kind * 16, 2, Some(0x8080000A))?
            .into_iter()
            .map(|at| p.u16(at))
            .collect::<Result<_>>()?;
        ensure!(
            channel.slots.iter().all(|&s| usize::from(s) < slots),
            "Clip control slot exceeds its rig"
        );
    }
    if p.u64(pointer)? == 0 {
        ensure!(
            channels.iter().all(|c| c.slots.is_empty()),
            "Clip has maps without a codec"
        );
        return Ok(channels);
    }
    let base = p.pointer(pointer)?;
    let codec = p.u16(base)?;
    ensure!(
        matches!(
            (codec, p.u32(base - 4)?),
            (0, 0x80808F73) | (2, 0x80808F71) | (3, 0x80808F6F)
        ),
        "Selected native clip needs an unsupported codec translation"
    );
    ensure!(
        p.u32(base + 0x10)? as usize == frames,
        "Native clip frame counts disagree"
    );
    for (kind, channel) in channels.iter_mut().enumerate() {
        ensure!(
            p.u16(base + 2 + kind * 2)? as usize == channel.slots.len(),
            "Native codec and map counts disagree"
        );
    }
    if codec == 0 {
        read_raw(p, base, frames, &mut channels)?;
    } else {
        read_packed(p, base, codec, frames, &mut channels)?;
    }
    Ok(channels)
}

fn read_raw(p: &Payload, base: usize, frames: usize, channels: &mut [Channel; 3]) -> Result<()> {
    for (kind, channel) in channels.iter_mut().enumerate() {
        let rows = p.array(
            base + 0x18 + kind * 16,
            if kind == 0 { 4 } else { 16 },
            Some([0x8080000F, 0x80800096, 0x80800091][kind]),
        )?;
        ensure!(
            rows.len() == channel.slots.len() * frames,
            "Native float sample count differs"
        );
        for row in rows {
            for axis in 0..[1, 4, 3][kind] {
                channel.values.push(p.f32(row + axis * 4)?);
            }
        }
    }
    Ok(())
}

fn read_packed(
    p: &Payload,
    base: usize,
    codec: u16,
    frames: usize,
    channels: &mut [Channel; 3],
) -> Result<()> {
    let packed = p.array(
        base + if codec == 2 { 0x18 } else { 0x38 },
        2,
        Some(0x8080000A),
    )?;
    let ranges = if codec == 2 {
        p.array(base + 0x28, 4, Some(0x8080000F))?
    } else {
        Vec::new()
    };
    let biases = if codec == 2 {
        p.array(base + 0x38, 4, Some(0x8080000F))?
    } else {
        Vec::new()
    };
    let components: usize = channels
        .iter()
        .zip([1, 4, 3])
        .map(|(c, width)| c.slots.len() * width)
        .sum();
    ensure!(
        packed.len() == components * frames,
        "Native packed sample count differs"
    );
    ensure!(
        codec != 2 || (ranges.len() == components && biases.len() == components),
        "Native interval count differs"
    );
    let mut sample = 0;
    let mut component = 0;
    for (kind, channel) in channels.iter_mut().enumerate() {
        let width = [1, 4, 3][kind];
        for _ in &channel.slots {
            let mut limits = Vec::new();
            for axis in 0..width {
                limits.push(if codec == 2 {
                    (
                        p.f32(ranges[component + axis])?,
                        p.f32(biases[component + axis])?,
                    )
                } else {
                    match kind {
                        0 => (p.f32(base + 0x14)?, p.f32(base + 0x18)?),
                        1 => (2., -1.),
                        _ => (
                            p.f32(base + 0x1C + axis * 4)?,
                            p.f32(base + 0x28 + axis * 4)?,
                        ),
                    }
                });
            }
            for _ in 0..frames {
                for &(range, bias) in &limits {
                    let value = f32::from(p.u16(packed[sample])?) / 65535. * range + bias;
                    sample += 1;
                    channel.values.push(value);
                }
            }
            component += width;
        }
    }
    Ok(())
}

fn write(
    out: &mut Vec<u8>,
    pointer: usize,
    maps: usize,
    frames: usize,
    channels: &[Channel; 3],
) -> Result<()> {
    let base = (out.len() + 11) & !7;
    out.resize(base - 4, 0);
    out.extend(0x80808F73u32.to_le_bytes());
    out.resize(base + 0x48, 0);
    put(out, pointer, &(base as i64 - pointer as i64).to_le_bytes())?;
    put(out, base + 0x10, &(frames as u32).to_le_bytes())?;
    for (kind, channel) in channels.iter().enumerate() {
        put(
            out,
            base + 2 + kind * 2,
            &u16::try_from(channel.slots.len())?.to_le_bytes(),
        )?;
        let map = channel
            .slots
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        append(out, maps + kind * 16, 0x8080000A, &map, 2)?;
        let width = [1, 4, 3][kind];
        let mut data = Vec::new();
        for value in channel.values.chunks_exact(width) {
            ensure!(value.iter().all(|v| v.is_finite()), "Nonfinite clip sample");
            data.extend(value.iter().flat_map(|v| v.to_le_bytes()));
            if kind == 2 {
                data.extend(0f32.to_le_bytes());
            }
        }
        append(
            out,
            base + 0x18 + kind * 16,
            [0x8080000F, 0x80800096, 0x80800091][kind],
            &data,
            if kind == 0 { 4 } else { 16 },
        )?;
    }
    Ok(())
}

/// Decode one control pose, retaining the rig's defaults for absent channels.
/// The header stores bone and control counts separately on first-person rigs.
pub fn sample(original: &Payload, defaults: &[[f32; 8]], frame: usize) -> Result<Vec<[f32; 8]>> {
    let frames = usize::from(original.u16(0x13C)?);
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        original.u64(0)? == original.0.len() as u64
            && defaults.len() == slots
            && frame < frames
            && (1..=3600).contains(&frames)
            && original.u64(0x108)? == 0,
        "Native clip and control pose dimensions disagree"
    );
    let mut output = defaults.to_vec();
    for (pointer, maps, count, frame) in [(0x10, 0xA8, 1, 0), (0x18, 0xD8, frames, frame)] {
        let channels = read(original, pointer, maps, slots, count)?;
        for (kind, channel) in channels.iter().enumerate() {
            let width = [1, 4, 3][kind];
            let offset = [7, 0, 4][kind];
            ensure!(
                channel.values.len() == channel.slots.len() * count * width,
                "Native control channel sample count differs"
            );
            for (index, &slot) in channel.slots.iter().enumerate() {
                let start = (index * count + frame) * width;
                output[usize::from(slot)][offset..offset + width]
                    .copy_from_slice(&channel.values[start..start + width]);
            }
        }
    }
    ensure!(
        output.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite native control pose"
    );
    Ok(output)
}

/// Replace control motion while retaining the carrier's duration, events and
/// bone count. `poses` is control-major, with one value for every native frame.
pub fn replace(original: &Payload, poses: &[Vec<[f32; 8]>]) -> Result<Payload> {
    let frames = usize::from(original.u16(0x13C)?);
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        original.u64(0)? == original.0.len() as u64
            && (1..=256).contains(&slots)
            && (1..=3600).contains(&frames)
            && poses.len() == slots
            && poses.iter().all(|track| track.len() == frames)
            && original.u64(0x108)? == 0,
        "Replacement motion differs from native control dimensions"
    );
    let mut dynamic: [Channel; 3] = Default::default();
    for (slot, track) in poses.iter().enumerate() {
        for pose in track {
            let norm = pose[..4].iter().map(|v| v * v).sum::<f32>();
            ensure!(
                pose.iter().all(|v| v.is_finite()) && (norm - 1.).abs() < 0.001 && pose[7] > 0.,
                "Invalid replacement control transform"
            );
        }
        for (kind, channel) in dynamic.iter_mut().enumerate() {
            channel.slots.push(u16::try_from(slot)?);
            for pose in track {
                channel.values.extend(match kind {
                    0 => &pose[7..8],
                    1 => &pose[..4],
                    _ => &pose[4..7],
                });
            }
        }
    }
    let mut out = original.0.clone();
    write(&mut out, 0x10, 0xA8, 1, &Default::default())?;
    write(&mut out, 0x18, 0xD8, frames, &dynamic)?;
    put(&mut out, 0xA4, &u32::try_from(slots - 1)?.to_le_bytes())?;
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(Payload(out))
}

/// `poses` is track-major and supplies all eight local transform values. Native
/// frame counts, names, events and bank ordinals are retained by construction.
pub fn extend(
    original: &Payload,
    old: usize,
    total: usize,
    poses: Option<&[Vec<[f32; 8]>]>,
) -> Result<Payload> {
    ensure!(
        original.u64(0)? == original.0.len() as u64
            && original.u16(0x13E)? as usize == old
            && original.u16(0x140)? as usize == old
            && old < total
            && total <= 256,
        "Native clip and extended rig counts disagree"
    );
    let frames = original.u16(0x13C)? as usize;
    ensure!(
        (1..=3600).contains(&frames),
        "Native clip frame budget exceeded"
    );
    let mut out = original.0.clone();
    if let Some(poses) = poses {
        ensure!(
            poses.len() == total - old && poses.iter().all(|p| p.len() == frames),
            "Source poses differ from the retained native frame count"
        );
        let fixed = read(original, 0x10, 0xA8, old, 1)?;
        let mut dynamic = read(original, 0x18, 0xD8, old, frames)?;
        for (index, track) in poses.iter().enumerate() {
            for (kind, channel) in dynamic.iter_mut().enumerate() {
                channel.slots.push(u16::try_from(old + index)?);
                for pose in track {
                    channel.values.extend(match kind {
                        0 => &pose[7..8],
                        1 => &pose[..4],
                        _ => &pose[4..7],
                    });
                }
            }
        }
        write(&mut out, 0x10, 0xA8, 1, &fixed)?;
        write(&mut out, 0x18, 0xD8, frames, &dynamic)?;
        put(&mut out, 0xA4, &((total - 1) as u32).to_le_bytes())?;
    }
    for at in [0x13E, 0x140] {
        put(&mut out, at, &(total as u16).to_le_bytes())?;
    }
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok(Payload(out))
}
