//! Source hand constraints with equal native bindings and a neutral native secondary weight.
use super::*;

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
        !a.is_empty() && a.len() == b.len(),
        "Constraint binding count differs"
    );
    for (&s, &n) in a.iter().zip(&b) {
        ensure!(
            source.bytes::<64>(s)? == template.bytes::<64>(n)?,
            "Source hand binding differs from its native action"
        );
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
    ensure!(
        counts.iter().all(|&n| n == a.len())
            && matches!(source.u16(transform)?, 2 | 3)
            && source.u32(transform + 0x10)? == u32::from(source.u16(0x128)?),
        "Constraint transforms do not cover their binding rows"
    );
    let report = codec(source, native, 0x40, 0x48, counts)?;
    put(native, 0x130, &source.u32(0x120)?.to_le_bytes());
    put(native, 0x138, &source.u32(0x124)?.to_le_bytes());

    // The added native channel is admitted only with a captured constant-unit witness.
    // Its own frame count follows the source clip even when the paired action is longer.
    ensure!(
        template.u64(0x40)? != 0,
        "Native action has no secondary constraint witness"
    );
    let w = template.pointer(0x40)?;
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
