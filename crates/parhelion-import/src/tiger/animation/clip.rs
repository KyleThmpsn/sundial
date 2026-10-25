//! Preserve native clip timing and events while appending explicit local channels.
use crate::{
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Result, ensure};
mod envelope;
mod local;
pub mod lowering;
mod translation;
pub use envelope::grow as grow_static_envelope;
pub use translation::translate;

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
    ensure!(base >= 4, "Native clip codec lacks its class header");
    let codec = p.u16(base)?;
    ensure!(
        matches!(
            (codec, p.u32(base - 4)?),
            (0, 0x80808F73) | (1, 0x80808F72) | (2, 0x80808F71) | (3, 0x80808F6F)
        ),
        "Selected native clip needs an unsupported codec translation"
    );
    ensure!(
        codec == 1 || p.u32(base + 0x10)? as usize == frames,
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
    } else if codec == 1 {
        read_curve(p, base, frames, &mut channels)?;
    } else {
        read_packed(p, base, codec, frames, &mut channels)?;
    }
    Ok(channels)
}

fn read_curve(p: &Payload, base: usize, frames: usize, channels: &mut [Channel; 3]) -> Result<()> {
    use sundial::package_authoring::animation_curve::{Channel as Kind, Decoder};
    let counts = channels.each_ref().map(|channel| channel.slots.len());
    let decoder = Decoder::read(&p.0, base, counts, frames).map_err(anyhow::Error::msg)?;
    for ((channel, kind), width) in channels
        .iter_mut()
        .zip([Kind::Scale, Kind::Rotation, Kind::Position])
        .zip([1, 4, 3])
    {
        for track in 0..channel.slots.len() {
            for point in decoder.samples(kind, track).map_err(anyhow::Error::msg)? {
                channel.values.extend_from_slice(&point[..width]);
            }
        }
    }
    Ok(())
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
    let size_field = match pointer {
        0x10 => 0x128,
        0x18 => 0x12C,
        _ => anyhow::bail!("Unsupported clip codec pointer"),
    };
    // Channel maps belong to the clip, outside the codec's serialized span.
    for (kind, channel) in channels.iter().enumerate() {
        let map = channel
            .slots
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        append(out, maps + kind * 16, 0x8080000A, &map, 2)?;
    }
    if channels.iter().all(|channel| channel.slots.is_empty()) {
        put(out, pointer, &[0; 8])?;
        put(out, size_field, &[0; 4])?;
        return Ok(());
    }
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
    let size = (out.len() - base + 11) & !7;
    out.resize(base - 4 + size, 0);
    put(out, size_field, &u32::try_from(size)?.to_le_bytes())?;
    Ok(())
}

/// A dynamic track whose every component stays within this of its first frame is stored once,
/// in the static span. That is about 0.001 degrees of rotation or 0.01 mm of position.
const STILL: f32 = 1e-5;

/// Codec 2 (`80808F71`), as stock clips store it: 16-bit samples ordered by control, frame and
/// component at `+0x18`, then one float range and one float bias per component at `+0x28` and
/// `+0x38`. A sample decodes as `value / 65535 * range + bias`.
fn write_quantized(
    out: &mut Vec<u8>,
    pointer: usize,
    maps: usize,
    frames: usize,
    channels: &[Channel; 3],
) -> Result<()> {
    if channels.iter().all(|channel| channel.slots.is_empty()) {
        return write(out, pointer, maps, frames, channels);
    }
    ensure!(
        pointer == 0x18,
        "Quantized samples belong to the animated span"
    );
    for (kind, channel) in channels.iter().enumerate() {
        let map = channel
            .slots
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>();
        append(out, maps + kind * 16, 0x8080000A, &map, 2)?;
    }
    let base = (out.len() + 11) & !7;
    out.resize(base - 4, 0);
    out.extend(0x80808F71u32.to_le_bytes());
    out.resize(base + 0x48, 0);
    put(out, pointer, &(base as i64 - pointer as i64).to_le_bytes())?;
    put(out, base, &2u16.to_le_bytes())?;
    put(out, base + 0x10, &u32::try_from(frames)?.to_le_bytes())?;
    let (mut samples, mut ranges, mut biases) = (Vec::new(), Vec::new(), Vec::new());
    for (kind, channel) in channels.iter().enumerate() {
        put(
            out,
            base + 2 + kind * 2,
            &u16::try_from(channel.slots.len())?.to_le_bytes(),
        )?;
        let width = [1, 4, 3][kind];
        ensure!(
            channel.values.len() == channel.slots.len() * frames * width,
            "Animated sample count differs from its controls"
        );
        for track in channel.values.chunks_exact(frames * width) {
            ensure!(track.iter().all(|v| v.is_finite()), "Nonfinite clip sample");
            let mut limits = Vec::with_capacity(width);
            for axis in 0..width {
                let values = track.iter().skip(axis).step_by(width);
                let low = values.clone().fold(f32::INFINITY, |a, &b| a.min(b));
                let high = values.fold(f32::NEG_INFINITY, |a, &b| a.max(b));
                let range = high - low;
                ranges.extend(range.to_le_bytes());
                biases.extend(low.to_le_bytes());
                limits.push((range, low));
            }
            for frame in track.chunks_exact(width) {
                for (&value, &(range, low)) in frame.iter().zip(&limits) {
                    let sample = if range > 0. {
                        ((value - low) / range * 65535.).round().clamp(0., 65535.) as u16
                    } else {
                        0
                    };
                    samples.extend(sample.to_le_bytes());
                }
            }
        }
    }
    append(out, base + 0x18, 0x8080000A, &samples, 2)?;
    append(out, base + 0x28, 0x8080000F, &ranges, 4)?;
    append(out, base + 0x38, 0x8080000F, &biases, 4)?;
    let size = (out.len() - base + 11) & !7;
    out.resize(base - 4 + size, 0);
    put(out, 0x12C, &u32::try_from(size)?.to_le_bytes())?;
    Ok(())
}

/// Order a channel's controls by slot, as every stock map is ordered.
fn sort_channel(channel: &mut Channel, width: usize) {
    let samples = if channel.slots.is_empty() {
        0
    } else {
        channel.values.len() / channel.slots.len()
    };
    let mut tracks = channel
        .slots
        .iter()
        .copied()
        .zip(
            channel
                .values
                .chunks_exact(samples.max(width))
                .map(<[f32]>::to_vec),
        )
        .collect::<Vec<_>>();
    tracks.sort_by_key(|(slot, _)| *slot);
    channel.slots = tracks.iter().map(|(slot, _)| *slot).collect();
    channel.values = tracks.into_iter().flat_map(|(_, values)| values).collect();
}

/// Write both pose spans the way stock clips store them: every dynamic track that never moves
/// joins the static span as one float sample, and the moving tracks are quantized.
fn write_spans(
    out: &mut Vec<u8>,
    mut fixed: [Channel; 3],
    dynamic: [Channel; 3],
    frames: usize,
) -> Result<()> {
    let mut moving: [Channel; 3] = Default::default();
    for (kind, channel) in dynamic.into_iter().enumerate() {
        let width = [1, 4, 3][kind];
        ensure!(
            channel.values.len() == channel.slots.len() * frames * width,
            "Animated sample count differs from its controls"
        );
        for (&slot, track) in channel
            .slots
            .iter()
            .zip(channel.values.chunks_exact(frames * width))
        {
            let first = &track[..width];
            let still = track
                .chunks_exact(width)
                .all(|frame| frame.iter().zip(first).all(|(a, b)| (a - b).abs() <= STILL));
            let target = if still {
                &mut fixed[kind]
            } else {
                &mut moving[kind]
            };
            target.slots.push(slot);
            target
                .values
                .extend_from_slice(if still { first } else { track });
        }
    }
    for (kind, (fixed, moving)) in fixed.iter_mut().zip(&mut moving).enumerate() {
        let width = [1, 4, 3][kind];
        sort_channel(fixed, width);
        sort_channel(moving, width);
        ensure!(
            !fixed.slots.iter().any(|slot| moving.slots.contains(slot))
                && fixed.slots.windows(2).all(|pair| pair[0] < pair[1]),
            "A control repeats within a channel's static and animated maps"
        );
    }
    write(out, 0x10, 0xA8, 1, &fixed)?;
    write_quantized(out, 0x18, 0xD8, frames, &moving)
}

/// Drop the bytes earlier spans and maps left unreachable, then restate each pose and
/// auxiliary span's size from its rebuilt extent.
fn finish(mut out: Vec<u8>) -> Result<Payload> {
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    let compacted = crate::tiger::payload::compact(&Payload(out), 0x80808F49)?;
    let mut payload = compacted.payload;
    for (pointer, size_field) in [
        (0x10, 0x128),
        (0x18, 0x12C),
        (0x38, 0x130),
        (0x40, 0x134),
        (0x48, 0x138),
    ] {
        let size = if payload.u64(pointer)? == 0 {
            0
        } else {
            let base = payload.pointer(pointer)?;
            let end = *compacted
                .extents
                .get(&base)
                .ok_or_else(|| anyhow::anyhow!("Clip span at {pointer:X} was not rebuilt"))?;
            u32::try_from((end - base + 11) & !7)?
        };
        if pointer <= 0x18 || size != 0 {
            put(&mut payload.0, size_field, &size.to_le_bytes())?;
        }
    }
    Ok(payload)
}

/// Re-encode a clip whose animated span stores 32-bit floats, the form earlier converters
/// wrote, as `write_spans` does. Other clips return unchanged.
pub fn compact(original: &Payload) -> Result<Payload> {
    if original.u64(0x18)? == 0 {
        return Ok(original.clone());
    }
    let base = original.pointer(0x18)?;
    if base < 4 || original.u32(base - 4)? != 0x80808F73 {
        return Ok(original.clone());
    }
    let frames = usize::from(original.u16(0x13C)?);
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        original.u64(0)? == original.0.len() as u64 && (1..=3600).contains(&frames),
        "Clip dimensions differ from its envelope"
    );
    let fixed = read(original, 0x10, 0xA8, slots, 1)?;
    let dynamic = read(original, 0x18, 0xD8, slots, frames)?;
    let mut out = original.0.clone();
    write_spans(&mut out, fixed, dynamic, frames)?;
    finish(out)
}

/// Native weighted choices reserve the larger of the two pose codec spans.
pub fn size(payload: &Payload) -> Result<u32> {
    Ok(payload.u32(0x128)?.max(payload.u32(0x12C)?))
}

/// Remap control slots after the caller verifies the controls and their default poses.
/// Dynamic encoded samples stay intact. Native-only absolute slots receive their
/// default poses, additive slots receive neutral deltas, and sparse slots stay absent.
pub fn remap_controls(
    original: &Payload,
    map: &[u16],
    bones: usize,
    controls: &Payload,
    defaults: &[[f32; 8]],
) -> Result<Payload> {
    use std::collections::BTreeSet;
    let old = usize::from(original.u16(0x140)?);
    let total = defaults.len();
    let mode = original.u8(0x181)?;
    ensure!(
        old == map.len()
            && old > 0
            && total <= 256
            && total <= bones
            && map.iter().all(|&s| usize::from(s) < total)
            && map.iter().collect::<BTreeSet<_>>().len() == old
            && mode <= 2,
        "Control remapping requires a supported blend mode and unique complete slot mapping"
    );
    let mut fixed = read(original, 0x10, 0xA8, old, 1)?;
    for channel in &mut fixed {
        for slot in &mut channel.slots {
            *slot = map[usize::from(*slot)];
        }
    }
    for (slot, pose) in defaults.iter().enumerate() {
        if map.contains(&(slot as u16)) || mode == 2 {
            continue;
        }
        let neutral = [0., 0., 0., 1., 0., 0., 0., 1.];
        let pose = if mode == 1 { &neutral } else { pose };
        ensure!(
            pose.iter().all(|v| v.is_finite()),
            "Nonfinite default control pose"
        );
        for (kind, channel) in fixed.iter_mut().enumerate() {
            channel.slots.push(u16::try_from(slot)?);
            channel.values.extend_from_slice(match kind {
                0 => &pose[7..8],
                1 => &pose[..4],
                _ => &pose[4..7],
            });
        }
    }
    let mut out = original.0.clone();
    write(&mut out, 0x10, 0xA8, 1, &fixed)?;
    for field in [0xD8, 0xE8, 0xF8] {
        let rows = original.array(field, 2, Some(0x8080000A))?;
        for row in rows {
            let source = usize::from(original.u16(row)?);
            let target = map
                .get(source)
                .ok_or_else(|| anyhow::anyhow!("Animated slot exceeds its control map"))?;
            put(&mut out, row, &target.to_le_bytes())?;
        }
    }
    put(&mut out, 0x13E, &u16::try_from(bones)?.to_le_bytes())?;
    put(&mut out, 0x140, &u16::try_from(total)?.to_le_bytes())?;
    put(&mut out, 0xA0, &0u32.to_le_bytes())?;
    put(&mut out, 0xA4, &u32::try_from(total - 1)?.to_le_bytes())?;
    groups(&mut out, controls, total)?;
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok(Payload(out))
}

/// Materialize implicit absolute channels when compatible control rigs have different defaults.
/// Authored constant channels and encoded dynamic streams retain their original values.
pub fn bind_defaults(original: &Payload, defaults: &[[f32; 8]]) -> Result<Payload> {
    ensure!(
        usize::from(original.u16(0x140)?) == defaults.len(),
        "Default binding requires the complete control pose"
    );
    bind_defaults_prefix(original, defaults)
}

/// Bind only the known leading controls of an absolute clip. Borrowed clips can
/// share the arm groups while retaining an unknown, excluded weapon suffix.
pub fn bind_defaults_prefix(original: &Payload, defaults: &[[f32; 8]]) -> Result<Payload> {
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        original.u8(0x181)? == 0 && !defaults.is_empty() && defaults.len() <= slots,
        "Default binding requires an absolute clip with a known control prefix"
    );
    let mut fixed = read(original, 0x10, 0xA8, slots, 1)?;
    let mut last = u32::try_from(defaults.len() - 1)?;
    for (kind, channel) in fixed.iter_mut().enumerate() {
        let dynamic = original
            .array(0xD8 + kind * 16, 2, Some(0x8080000A))?
            .into_iter()
            .map(|at| original.u16(at))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        ensure!(
            dynamic.iter().all(|&slot| usize::from(slot) < slots),
            "Dynamic control exceeds its clip"
        );
        for &slot in channel.slots.iter().chain(&dynamic) {
            last = last.max(u32::from(slot));
        }
        for (slot, pose) in defaults.iter().enumerate() {
            let slot = u16::try_from(slot)?;
            if channel.slots.contains(&slot) || dynamic.contains(&slot) {
                continue;
            }
            ensure!(
                pose.iter().all(|v| v.is_finite()),
                "Nonfinite source default pose"
            );
            channel.slots.push(slot);
            channel.values.extend_from_slice(match kind {
                0 => &pose[7..8],
                1 => &pose[..4],
                _ => &pose[4..7],
            });
        }
    }
    let mut output = original.0.clone();
    write(&mut output, 0x10, 0xA8, 1, &fixed)?;
    put(&mut output, 0xA0, &0u32.to_le_bytes())?;
    put(&mut output, 0xA4, &last.to_le_bytes())?;
    let size = output.len() as u64;
    put(&mut output, 0, &size.to_le_bytes())?;
    Ok(Payload(output))
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

/// Sample the complete leading control groups shared by a clip and target rig.
/// The native mixer stops at the first different group. Remaining target
/// controls retain their defaults instead of borrowing another weapon's joints.
pub fn sample_common(
    original: &Payload,
    defaults: &[[f32; 8]],
    controls: &Payload,
    frame: usize,
) -> Result<Vec<[f32; 8]>> {
    let definition = controls.pointer(24)?;
    ensure!(
        controls.u32(definition - 4)? == 0x80808f8f && original.u8(0x181)? == 0,
        "Shared control sampling requires absolute native poses"
    );
    let groups = |payload: &Payload, field| -> Result<Vec<(u32, usize)>> {
        payload
            .array(field, 8, Some(0x8080907f))?
            .into_iter()
            .map(|at| Ok((payload.u32(at)?, usize::try_from(payload.u32(at + 4)?)?)))
            .collect()
    };
    let source = groups(original, 0x170)?;
    let target = groups(controls, definition + 0x90)?;
    let slots = usize::from(original.u16(0x140)?);
    ensure!(
        (1..=256).contains(&slots)
            && source.iter().all(|(_, count)| *count > 0)
            && target.iter().all(|(_, count)| *count > 0)
            && source.iter().map(|(_, count)| count).sum::<usize>() == slots
            && target.iter().map(|(_, count)| count).sum::<usize>() == defaults.len(),
        "Native control groups do not cover their poses"
    );
    let common = source
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .map(|((_, count), _)| count)
        .sum::<usize>();
    ensure!(
        common > 0,
        "Native clip and target have no common control group"
    );
    let mut source_defaults = vec![[0., 0., 0., 1., 0., 0., 0., 1.]; slots];
    source_defaults[..common].copy_from_slice(&defaults[..common]);
    let sampled = sample(original, &source_defaults, frame)?;
    let mut output = defaults.to_vec();
    output[..common].copy_from_slice(&sampled[..common]);
    Ok(output)
}

/// Author complete absolute control poses for the selected rig while retaining
/// the base or replacement action envelope's duration and events.
pub fn replace_for(
    original: &Payload,
    poses: &[Vec<[f32; 8]>],
    bones: usize,
    controls: &Payload,
) -> Result<Payload> {
    ensure!(
        matches!(original.u8(0x181)?, 0 | 2) && (1..=256).contains(&bones),
        "Control motion requires an absolute native base or replacement action"
    );
    let mut envelope = original.clone();
    put(&mut envelope.0, 0x13e, &u16::try_from(bones)?.to_le_bytes())?;
    put(
        &mut envelope.0,
        0x140,
        &u16::try_from(poses.len())?.to_le_bytes(),
    )?;
    let selected = if original.u8(0x181)? == 2 {
        let mut selected = std::collections::BTreeSet::new();
        for field in [0xA8, 0xB8, 0xC8, 0xD8, 0xE8, 0xF8] {
            for row in original.array(field, 2, Some(0x8080000A))? {
                let slot = usize::from(original.u16(row)?);
                ensure!(slot < poses.len(), "Replacement control exceeds its rig");
                selected.insert(slot);
            }
        }
        ensure!(!selected.is_empty(), "Replacement action has no controls");
        Some(selected)
    } else {
        None
    };
    let mut output = replace_selected(&envelope, poses, selected.as_ref())?;
    groups(&mut output.0, controls, poses.len())?;
    let size = output.0.len() as u64;
    put(&mut output.0, 0, &size.to_le_bytes())?;
    finish(output.0)
}

/// Author complete additive control deltas for the selected rig while retaining the action's
/// duration and events. A control without motion carries the identity delta, which the native
/// additive mixer leaves unchanged.
pub fn replace_deltas_for(
    original: &Payload,
    deltas: &[Vec<[f32; 8]>],
    bones: usize,
    controls: &Payload,
) -> Result<Payload> {
    ensure!(
        original.u8(0x181)? == 1 && (1..=256).contains(&bones),
        "Control deltas require an additive native action"
    );
    let mut envelope = original.clone();
    put(&mut envelope.0, 0x13e, &u16::try_from(bones)?.to_le_bytes())?;
    put(
        &mut envelope.0,
        0x140,
        &u16::try_from(deltas.len())?.to_le_bytes(),
    )?;
    let mut output = replace_selected(&envelope, deltas, None)?;
    groups(&mut output.0, controls, deltas.len())?;
    let size = output.0.len() as u64;
    put(&mut output.0, 0, &size.to_le_bytes())?;
    finish(output.0)
}

/// Replace control motion while retaining the carrier's duration, events and
/// bone count. `poses` is control-major, with one value for every native frame.
pub fn replace(original: &Payload, poses: &[Vec<[f32; 8]>]) -> Result<Payload> {
    finish(replace_selected(original, poses, None)?.0)
}

fn replace_selected(
    original: &Payload,
    poses: &[Vec<[f32; 8]>],
    selected: Option<&std::collections::BTreeSet<usize>>,
) -> Result<Payload> {
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
        if selected.is_some_and(|selected| !selected.contains(&slot)) {
            continue;
        }
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
    write_spans(&mut out, Default::default(), dynamic, frames)?;
    put(&mut out, 0xA4, &u32::try_from(slots - 1)?.to_le_bytes())?;
    let length = out.len() as u64;
    put(&mut out, 0, &length.to_le_bytes())?;
    Ok(Payload(out))
}

/// `poses` is track-major and supplies absolute local transforms. Additive
/// carriers receive deltas from the rig's default controls. Native frame counts,
/// names, events and bank ordinals are retained by construction.
pub fn extend(
    original: &Payload,
    old: usize,
    total: usize,
    controls: &Payload,
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
        let local = local::Local::read(original, controls, total)?;
        for (index, track) in poses.iter().enumerate() {
            let track = track
                .iter()
                .map(|&pose| local.encode(old + index, pose))
                .collect::<Result<Vec<_>>>()?;
            for (kind, channel) in dynamic.iter_mut().enumerate() {
                channel.slots.push(u16::try_from(old + index)?);
                for pose in &track {
                    channel.values.extend(match kind {
                        0 => &pose[7..8],
                        1 => &pose[..4],
                        _ => &pose[4..7],
                    });
                }
            }
        }
        write_spans(&mut out, fixed, dynamic, frames)?;
        put(&mut out, 0xA4, &((total - 1) as u32).to_le_bytes())?;
    }
    for at in [0x13E, 0x140] {
        put(&mut out, at, &(total as u16).to_le_bytes())?;
    }
    groups(&mut out, controls, total)?;
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    if poses.is_some() {
        finish(out)
    } else {
        Ok(Payload(out))
    }
}

/// Emit sparse local poses on an extended rig using the carrier's composition
/// operation. Selected controls keep their rest position and scale after mixing.
pub fn rotations(
    original: &Payload,
    total: usize,
    controls: &Payload,
    tracks: &std::collections::BTreeMap<usize, Vec<[f32; 8]>>,
) -> Result<Payload> {
    let frames = usize::from(original.u16(0x13C)?);
    ensure!(
        original.u64(0)? == original.0.len() as u64
            && (1..=256).contains(&total)
            && (2..=3600).contains(&frames)
            && original.u64(0x108)? == 0
            && original.u64(0x160)? == 0
            && !tracks.is_empty(),
        "Rotational overlay template differs"
    );
    let mut fixed: [Channel; 3] = Default::default();
    let mut dynamic: [Channel; 3] = Default::default();
    let local = local::Local::read(original, controls, total)?;
    for (&slot, samples) in tracks {
        ensure!(
            slot < total && samples.len() == frames,
            "Overlay control dimensions differ"
        );
        let first = local.encode(slot, samples[0])?;
        dynamic[1].slots.push(u16::try_from(slot)?);
        fixed[0].slots.push(u16::try_from(slot)?);
        fixed[0].values.push(first[7]);
        fixed[2].slots.push(u16::try_from(slot)?);
        fixed[2].values.extend(&first[4..7]);
        for sample in samples {
            let norm = sample[..4].iter().map(|v| v * v).sum::<f32>();
            ensure!(
                sample.iter().all(|v| v.is_finite())
                    && (norm - 1.).abs() < 0.001
                    && sample[7] > 0.
                    && sample[4..] == samples[0][4..],
                "Rotational pose is invalid or changes its rest translation or scale"
            );
            dynamic[1].values.extend(&local.encode(slot, *sample)?[..4]);
        }
    }
    let mut out = original.0.clone();
    write_spans(&mut out, fixed, dynamic, frames)?;
    put(&mut out, 0xA4, &u32::try_from(total - 1)?.to_le_bytes())?;
    for field in [0x13E, 0x140] {
        put(&mut out, field, &u16::try_from(total)?.to_le_bytes())?;
    }
    // The carrier's clip names do not describe this authored motion. Control
    // groups must still match the target rig or the native mixer masks it out.
    put(&mut out, 0x150, &[0; 16])?;
    groups(&mut out, controls, total)?;
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    finish(out)
}

fn groups(out: &mut Vec<u8>, controls: &Payload, total: usize) -> Result<()> {
    let definition = controls.pointer(24)?;
    ensure!(
        controls.u32(definition - 4)? == 0x80808F8F,
        "Clip groups require native rig controls"
    );
    let rows = controls.array(definition + 0x90, 8, Some(0x8080907F))?;
    let mut data = Vec::with_capacity(rows.len() * 8);
    let mut covered = 0usize;
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        let count = controls.u32(row + 4)? as usize;
        ensure!(
            names.insert(controls.u32(row)?) && (1..=256).contains(&count),
            "Clip control group is empty, repeated or too large"
        );
        covered += count;
        data.extend_from_slice(&controls.0[row..row + 8]);
    }
    ensure!(
        covered == total,
        "Clip groups do not cover the target controls"
    );
    append(out, 0x170, 0x8080907F, &data, 8)?;
    Ok(())
}
