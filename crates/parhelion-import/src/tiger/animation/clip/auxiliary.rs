//! Constraint streams use binding-row ordinals rather than control-slot maps.
use super::*;
use anyhow::Context;

/// All integer-frame auxiliary transforms, indexed by frame and binding row.
pub fn constraint_transforms(clip: &Payload, rows: usize) -> Result<Vec<Vec<[f32; 8]>>> {
    let frames = usize::from(clip.u16(0x13C)?);
    ensure!(
        (1..=3600).contains(&frames) && rows <= 256,
        "Constraint dimensions differ"
    );
    ensure!(
        clip.u64(0x48)? != 0,
        "Constraint transform stream is absent"
    );
    let mut channels = std::array::from_fn(|_| Channel {
        slots: (0..rows).map(|row| row as u16).collect(),
        values: Vec::new(),
    });
    read_codec(clip, clip.pointer(0x48)?, frames, &mut channels)?;
    let mut result = vec![vec![crate::tiger::animation::pose::Pose::IDENTITY.0; rows]; frames];
    for (kind, channel) in channels.iter().enumerate() {
        let (offset, width) = [(7, 1), (0, 4), (4, 3)][kind];
        ensure!(
            channel.values.len() == rows * frames * width,
            "Constraint transform sample count differs"
        );
        for row in 0..rows {
            for (frame, values) in result.iter_mut().enumerate() {
                let start = (row * frames + frame) * width;
                values[row][offset..offset + width]
                    .copy_from_slice(&channel.values[start..start + width]);
            }
        }
    }
    Ok(result)
}

/// Primary or secondary constraint weights, indexed by binding row and integer frame.
/// A missing weight stream means one, as in the native constraint sampler.
pub fn constraint_weights(clip: &Payload, field: usize, rows: usize) -> Result<Vec<Vec<f32>>> {
    let frames = usize::from(clip.u16(0x13C)?);
    ensure!(
        matches!(field, 0x38 | 0x40) && rows <= 256 && (1..=3600).contains(&frames),
        "Constraint weight dimensions differ"
    );
    if clip.u64(field)? == 0 {
        return Ok(vec![vec![1.; frames]; rows]);
    }
    let base = clip.pointer(field)?;
    let class = clip.u32(base.checked_sub(4).context("Constraint weight header")?)?;
    let mut values = match (class, clip.u16(base)?) {
        (0x80808F7E, 1) => {
            sundial::package_authoring::animation_curve::scalar_samples(&clip.0, base, rows, frames)
                .map_err(anyhow::Error::msg)?
        }
        (0x80808F7C, 2) => intervals(clip, base, rows, frames)?,
        _ => anyhow::bail!("Unsupported constraint weight codec {class:08X}"),
    };
    for value in values.iter_mut().flatten() {
        ensure!(value.is_finite(), "Nonfinite constraint weight");
        *value = value.clamp(0., 1.);
    }
    Ok(values)
}

fn intervals(p: &Payload, base: usize, rows: usize, frames: usize) -> Result<Vec<Vec<f32>>> {
    ensure!(
        usize::from(p.u16(base + 2)?) == rows && p.u32(base + 0xC)? as usize == frames,
        "Constraint weight tracks or frames differ"
    );
    let selectors = p.array(base + 0x10, 2, Some(0x80808F82))?;
    let samples = p.array(base + 0x20, 2, Some(0x8080000A))?;
    let ranges = p.array(base + 0x30, 4, Some(0x8080000F))?;
    let biases = p.array(base + 0x40, 4, Some(0x8080000F))?;
    ensure!(
        [selectors.len(), ranges.len(), biases.len()]
            .into_iter()
            .all(|count| count == rows),
        "Constraint weight interval tables differ"
    );
    (0..rows)
        .map(|row| {
            let selector = p.u16(selectors[row])?;
            let bias = p.f32(biases[row])?;
            match selector >> 14 {
                2 => Ok(vec![bias; frames]),
                1 => {
                    let first = usize::from(selector & 0x3FFF) * frames;
                    let range = p.f32(ranges[row])?;
                    let data = samples
                        .get(first..first + frames)
                        .context("Constraint weight sample range")?;
                    data.iter()
                        .map(|&at| Ok(f32::from(p.u16(at)?) / 65535. * range + bias))
                        .collect()
                }
                _ => anyhow::bail!("Constraint weight selector has no checked output"),
            }
        })
        .collect()
}
