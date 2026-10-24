//! Source hand constraints with witnessed bone shapes and named targets.
use super::*;
mod secondary;

pub(super) fn convert(
    source: &Payload,
    native: &mut Vec<u8>,
    template: &Payload,
    bones: &[u16],
) -> Result<Vec<Value>> {
    let at = relocate(source, native, 0x30, 0x30)?;
    ensure!(
        source.u32(at - 4)? == 0x808008F4 && template.u64(0x30)? != 0,
        "Source constraint binding class differs"
    );
    let counterpart = template.pointer(0x30)?;
    ensure!(
        template.u32(counterpart - 4)? == 0x80808F59,
        "Native constraint binding class differs"
    );
    let a = source.array(at, 64, Some(0x8080081F))?;
    let b = template.array(counterpart, 64, Some(0x80808F5C))?;
    ensure!(
        !a.is_empty() && !b.is_empty(),
        "Constraint binding witness is empty"
    );
    let identical = a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(&s, &n)| source.0[s..s + 64] == template.0[n..n + 64]);
    for &s in &a {
        if !identical {
            // Materialization maps the three bone indices separately from the target
            // mode/name. Mode 3 resolves that name independently through the target table.
            // Admit only bone/rest shapes and complete target tuples already witnessed
            // in this native action, preserving source row order and target identity.
            ensure!(
                source.u16(s + 16)? == u16::MAX
                    && source.u16(s + 18)? == 3
                    && b.iter()
                        .any(|&n| source.0[s..s + 20] == template.0[n..n + 20]
                            && source.0[s + 24..s + 64] == template.0[n + 24..n + 64])
                    && b.iter()
                        .any(|&n| source.0[s + 16..s + 24] == template.0[n + 16..n + 24]),
                "Source constraint bone shape or named target has no native witness"
            );
        }
        for offset in [0, 2, 16] {
            let bone = source.u16(s + offset)?;
            ensure!(
                bone == u16::MAX || bones.get(usize::from(bone)) == Some(&bone),
                "Constraint binding addresses a remapped bone"
            );
        }
    }
    move_array(source, native, at, at + SHIFT, (0x8080081F, 0x80808F5C))?;
    put(native, at + SHIFT - 4, &0x80808F59u32.to_le_bytes());
    ensure!(
        source.u64(0x38)? != 0 && source.u64(0x40)? != 0,
        "Incomplete source constraint channels"
    );
    let weight = source.pointer(0x38)?;
    let transform = source.pointer(0x40)?;
    ensure!(
        source.u16(weight + 2)? as usize == a.len()
            && source.u32(0x120)? as usize
                == transform
                    .checked_sub(weight)
                    .context("Constraint stream ordering")?
            && transform
                .checked_add(source.u32(0x124)? as usize)
                .is_some_and(|end| end <= source.0.len()),
        "Source constraint sizes or weights differ"
    );
    let counts = [
        source.u16(transform + 2)? as usize,
        source.u16(transform + 4)? as usize,
        source.u16(transform + 6)? as usize,
    ];
    let encoding = source.u16(transform)?;
    let frames = usize::from(source.u16(0x128)?);
    ensure!(
        counts.iter().all(|&n| n == a.len())
            && matches!(encoding, 1..=3)
            && (encoding == 1 || source.u32(transform + 0x10)? as usize == frames),
        "Constraint transforms do not cover their binding rows"
    );
    let report = codec(source, native, 0x40, 0x48, counts)?;
    if encoding == 1 {
        use sundial::package_authoring::animation_curve::{Channel, Decoder};
        let decoder =
            Decoder::read(native, transform + SHIFT, counts, frames).map_err(anyhow::Error::msg)?;
        for (kind, count) in [Channel::Scale, Channel::Rotation, Channel::Position]
            .into_iter()
            .zip(counts)
        {
            for track in 0..count {
                decoder.samples(kind, track).map_err(anyhow::Error::msg)?;
            }
        }
    }
    put(native, 0x130, &source.u32(0x120)?.to_le_bytes());
    put(native, 0x138, &source.u32(0x124)?.to_le_bytes());

    if !identical {
        // Shadowkeep's constraint sampler explicitly supplies one when +0x40 is null.
        // D1 has no secondary stream, so no donor track count or timing is borrowed.
        put(native, 0x40, &0u64.to_le_bytes());
        put(native, 0x134, &0u32.to_le_bytes());
        return Ok(vec![
            report,
            json!({"field":0x30,"bindings":a.len(),
            "binding_rows":"source_rows_with_witnessed_bone_shapes_and_named_targets",
            "secondary_weight":"native_absent_stream_unit_default",
            "source_weight_and_transform_samples_preserved":true}),
        ]);
    }

    // The added native channel is admitted only with a captured constant-unit witness.
    // Its own frame count follows the source clip even when the paired action is longer.
    ensure!(
        template.u64(0x40)? != 0,
        "Native action has no secondary constraint witness"
    );
    let w = template.pointer(0x40)?;
    if template.u32(w - 4)? == 0x80808F7E && template.u16(w)? == 1 {
        secondary::curve(template, native, w, a.len(), frames)?;
        return Ok(vec![
            report,
            json!({"field":0x30,"bindings":a.len(),"binding_rows":"byte_identical_to_native_action",
            "secondary_weight":"constant_unit_curve","source_weight_and_transform_samples_preserved":true}),
        ]);
    }
    ensure!(
        template.u32(w - 4)? == 0x80808F7C
            && template.u16(w)? == 2
            && template.u16(w + 2)? as usize == a.len(),
        "Native secondary constraint layout differs"
    );
    let selectors = template.array(w + 0x10, 2, Some(0x80808F82))?;
    let values = template.array(w + 0x20, 2, Some(0x8080000A))?;
    let ranges = template.array(w + 0x30, 4, Some(0x8080000F))?;
    let biases = template.array(w + 0x40, 4, Some(0x8080000F))?;
    ensure!(
        selectors.len() == a.len()
            && values.is_empty()
            && ranges.len() == a.len()
            && biases.len() == a.len()
            && selectors
                .iter()
                .enumerate()
                .all(|(i, &p)| template.u16(p).ok() == Some(0x8000 | i as u16))
            && ranges.iter().all(|&p| template.f32(p).ok() == Some(0.))
            && biases.iter().all(|&p| template.f32(p).ok() == Some(1.)),
        "Native secondary constraint weight is not neutral"
    );
    let size = template.u32(0x134)? as usize;
    let end = (w - 4)
        .checked_add(size)
        .context("Secondary constraint extent")?;
    for field in [0x10, 0x30, 0x40] {
        let data = template.array_range(w + field, if field == 0x10 { 2 } else { 4 }, None)?;
        ensure!(
            data.start >= w && data.end <= end,
            "Secondary constraint array escapes codec block"
        );
    }
    let start = (native.len() + 11) & !7;
    native.resize(start - 4, 0);
    native.extend_from_slice(
        template
            .0
            .get(w - 4..end)
            .context("Secondary constraint codec bytes")?,
    );
    put(native, 0x40, &(start as i64 - 0x40).to_le_bytes());
    put(
        native,
        start + 12,
        &u32::from(source.u16(0x128)?).to_le_bytes(),
    );
    put(native, 0x134, &u32::try_from(size)?.to_le_bytes());
    Ok(vec![
        report,
        json!({"field":0x30,"bindings":a.len(),"binding_rows":"byte_identical_to_native_action",
        "secondary_weight":"constant_unit","source_weight_and_transform_samples_preserved":true}),
    ])
}
