//! Grow an eventless static envelope together with its retained auxiliary tracks.
use super::*;
use anyhow::Context;

struct Array {
    offset: usize,
    stride: usize,
    class: u32,
    data: Vec<u8>,
}

fn arrays(p: &Payload, base: usize, fields: &[(usize, usize, u32)]) -> Result<Vec<Array>> {
    fields
        .iter()
        .map(|&(offset, stride, class)| {
            let range = p.array_range(base + offset, stride, Some(class))?;
            Ok(Array {
                offset,
                stride,
                class,
                data: p.0[range].to_vec(),
            })
        })
        .collect()
}

fn repeat(data: &[u8], widths: &[usize], old: usize, frames: usize) -> Result<Vec<u8>> {
    ensure!(
        data.len() == widths.iter().sum::<usize>() * old * 2,
        "Static auxiliary sample count differs"
    );
    let mut result = Vec::new();
    let mut at = 0;
    for &width in widths {
        let bytes = width * 2;
        let track = &data[at..at + old * bytes];
        ensure!(
            track
                .chunks_exact(bytes)
                .all(|sample| sample == &track[..bytes]),
            "Cannot extend a varying auxiliary track as a static pose"
        );
        for _ in 0..frames {
            result.extend_from_slice(&track[..bytes]);
        }
        at += old * bytes;
    }
    Ok(result)
}

fn dense(arrays: &mut [Array], widths: &[usize], old: usize, frames: usize) -> Result<()> {
    ensure!(
        arrays[1..4].iter().all(|array| array.data.is_empty()),
        "Static auxiliary envelope has sparse curves"
    );
    let map = arrays.last().context("Auxiliary curve mappings")?;
    ensure!(
        map.data.len() == (widths.len() + 1) * 2 && map.data[widths.len() * 2..] == [0, 0],
        "Auxiliary curve mapping count or sentinel differs"
    );
    let mut packed = Vec::new();
    let mut mappings = Vec::new();
    for (track, &width) in widths.iter().enumerate() {
        let index = i16::from_le_bytes(map.data[track * 2..track * 2 + 2].try_into()?);
        ensure!(index < 0, "Static auxiliary track is not dense");
        let start = (usize::from(index.unsigned_abs()) - 1) * 2;
        let samples = arrays[0]
            .data
            .get(start..start + old * width * 2)
            .context("Static auxiliary track exceeds its samples")?;
        let mapping = i16::try_from(-(i32::try_from(packed.len() / 2)? + 1))?;
        mappings.extend(mapping.to_le_bytes());
        packed.extend(repeat(samples, &[width], old, frames)?);
    }
    mappings.extend(0i16.to_le_bytes());
    arrays[0].data = packed;
    arrays.last_mut().context("Auxiliary curve mappings")?.data = mappings;
    Ok(())
}

fn stream(p: &Payload, out: &mut Vec<u8>, field: usize, old: usize, frames: usize) -> Result<()> {
    if p.u64(field)? == 0 {
        return Ok(());
    }
    let base = p.pointer(field)?;
    ensure!(base >= 4, "Auxiliary codec lacks its class header");
    let class = p.u32(base - 4)?;
    let transform = matches!(field, 0x48 | 0x60);
    let counts = if transform {
        [p.u16(base + 2)?, p.u16(base + 4)?, p.u16(base + 6)?]
    } else {
        [p.u16(base + 2)?, 0, 0]
    };
    ensure!(
        counts.iter().all(|&count| count <= 256),
        "Auxiliary track count exceeds native capacity"
    );
    let widths = |rotation, position| {
        [1, rotation, position]
            .into_iter()
            .zip(counts)
            .flat_map(|(width, count)| std::iter::repeat_n(width, usize::from(count)))
            .collect::<Vec<_>>()
    };
    let curve_fields = [
        (0x10, 2, 0x80800006),
        (0x20, 2, 0x80800006),
        (0x30, 1, 0x80800009),
        (0x40, 1, 0x80800009),
        (0x50, 2, 0x80800006),
    ];
    let (header_size, data, frame_field) = match (field, class, p.u16(base)?) {
        (0x28, 0x80808F78, 1) | (0x38 | 0x40, 0x80808F7E, 1) => {
            let mut data = arrays(p, base, &curve_fields)?;
            let widths = vec![if field == 0x28 { 4 } else { 1 }; usize::from(counts[0])];
            dense(&mut data, &widths, old, frames)?;
            (0x60, data, None)
        }
        (0x48, 0x80808F72, 1) => {
            let mut fields = curve_fields[..4].to_vec();
            fields.extend([
                (0x50, 4, 0x8080000F),
                (0x60, 4, 0x8080000F),
                (0x70, 2, 0x80800006),
            ]);
            let mut data = arrays(p, base, &fields)?;
            dense(&mut data, &widths(3, 3), old, frames)?;
            (0x80, data, None)
        }
        (0x20, 0x80808F7C, 2) => {
            ensure!(
                p.u32(base + 0xC)? as usize == old,
                "Auxiliary frame counts disagree"
            );
            let data = arrays(
                p,
                base,
                &[
                    (0x10, 2, 0x80808F82),
                    (0x20, 2, 0x8080000A),
                    (0x30, 4, 0x8080000F),
                    (0x40, 4, 0x8080000F),
                ],
            )?;
            ensure!(
                data[0].data.len() == usize::from(counts[0]) * 2
                    && data[1].data.is_empty()
                    && data[2].data.len() == usize::from(counts[0]) * 4
                    && data[3].data.len() == usize::from(counts[0]) * 4
                    && data[0].data.chunks_exact(2).all(|bytes| {
                        let selector = u16::from_le_bytes([bytes[0], bytes[1]]);
                        selector >> 14 == 2
                    }),
                "Static scalar envelope requires constant selectors"
            );
            (0x50, data, Some(0xC))
        }
        (0x60, 0x80808F71, 2) => {
            ensure!(
                p.u32(base + 0x10)? as usize == old,
                "Auxiliary frame counts disagree"
            );
            let mut data = arrays(
                p,
                base,
                &[
                    (0x18, 2, 0x8080000A),
                    (0x28, 4, 0x8080000F),
                    (0x38, 4, 0x8080000F),
                ],
            )?;
            let widths = widths(4, 3);
            let components = widths.iter().sum::<usize>();
            ensure!(
                data[1].data.len() == components * 4 && data[2].data.len() == components * 4,
                "Static auxiliary intervals differ"
            );
            data[0].data = repeat(&data[0].data, &widths, old, frames)?;
            (0x48, data, Some(0x10))
        }
        _ => anyhow::bail!("Unsupported static auxiliary codec {class:08X} at {field:X}"),
    };
    let header =
        p.0.get(base..base + header_size)
            .context("Auxiliary codec header")?;
    let target = (out.len() + 11) & !7;
    out.resize(target - 4, 0);
    out.extend(class.to_le_bytes());
    out.extend_from_slice(header);
    put(out, field, &(target as i64 - field as i64).to_le_bytes())?;
    if let Some(offset) = frame_field {
        put(out, target + offset, &u32::try_from(frames)?.to_le_bytes())?;
    }
    for array in &data {
        append(
            out,
            target + array.offset,
            u64::from(array.class),
            &array.data,
            array.stride,
        )?;
    }
    let size = (out.len() - target + 11) & !7;
    out.resize(target - 4 + size, 0);
    if let Some(size_field) = match field {
        0x38 => Some(0x130),
        0x40 => Some(0x134),
        0x48 => Some(0x138),
        _ => None,
    } {
        put(out, size_field, &u32::try_from(size)?.to_le_bytes())?;
    }
    Ok(())
}

/// Extend a two-frame idle envelope before replacing its main pose tracks.
/// Every retained timed auxiliary stream must be a supported constant stream.
pub fn grow(original: &Payload, frames: usize) -> Result<Payload> {
    let old = usize::from(original.u16(0x13C)?);
    ensure!(
        old == 2
            && (2..=3600).contains(&frames)
            && original.u64(0)? == original.0.len() as u64
            && original.u64(0x160)? == 0
            && original.u64(0x168)? == 0,
        "Static envelope dimensions or events differ"
    );
    for field in [8, 0x50, 0x58, 0x68] {
        ensure!(
            original.u64(field)? == 0,
            "Static envelope has an unsupported stream"
        );
    }
    let mut out = original.0.clone();
    for field in [0x20, 0x28, 0x38, 0x40, 0x48, 0x60] {
        stream(original, &mut out, field, old, frames)?;
    }
    put(&mut out, 0x13C, &u16::try_from(frames)?.to_le_bytes())?;
    let size = out.len() as u64;
    put(&mut out, 0, &size.to_le_bytes())?;
    Ok(Payload(out))
}
