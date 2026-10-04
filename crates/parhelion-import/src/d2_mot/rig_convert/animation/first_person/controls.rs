//! Carry source rig controls together with source-owned weapon and arm skeletons.
//!
//! Clip slots index these control rows. Replacing only FK bones leaves the
//! carrier's default weapon transforms and control parents in the live rig.
use super::*;
use crate::d2_mot::{payload::Payload, rig_convert::write_array};

const FIELDS: [(usize, usize, usize, u32, u32); 7] = [
    (0xA8, 0x90, 8, 0x80808C5F, 0x8080907F),
    (0xB8, 0xA0, 52, 0x80808C56, 0x80809076),
    (0xC8, 0xB0, 32, 0x80809F4F, 0x80809F75),
    (0xD8, 0xC0, 2, 0x80800006, 0x80800006),
    (0xF0, 0xD8, 2, 0x80800006, 0x80800006),
    (0x100, 0xE8, 2, 0x80800006, 0x80800006),
    (0x128, 0x110, 8, 0x80808BDA, 0x80808FFE),
];

fn read(p: &Payload, modern: bool) -> Result<Vec<Vec<Vec<u8>>>> {
    let resource = p.pointer(24)?;
    ensure!(
        p.u64(0)? == p.0.len() as u64
            && resource >= 4
            && p.u32(resource - 4)? == if modern { 0x80808B5F } else { 0x80808F8F },
        "rig control resource layout differs"
    );
    FIELDS
        .iter()
        .map(|&(s, n, stride, sc, nc)| {
            p.array(
                resource + if modern { s } else { n },
                stride,
                Some(if modern { sc } else { nc }),
            )?
            .into_iter()
            .map(|at| Ok(p.0[at..at + stride].to_vec()))
            .collect()
        })
        .collect()
}

fn short(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn check(rows: &[Vec<Vec<u8>>]) -> Result<()> {
    let controls = rows[1].len();
    let bones = rows[4].len();
    ensure!(
        controls > 0
            && controls < 0x800
            && bones > 0
            && bones < 0x800
            && rows[2].len() == controls
            && rows[5].len() == controls,
        "rig control counts disagree"
    );
    ensure!(
        rows[0].iter().map(|r| u64::from(word(r, 4))).sum::<u64>() == controls as u64,
        "rig control groups do not cover their slots"
    );
    for (slot, row) in rows[1].iter().enumerate() {
        let bone = usize::from(short(row, 44));
        ensure!(
            bone < bones
                && short(&rows[4][bone], 0) as usize == slot
                && short(&rows[5][slot], 0) as usize == bone,
            "rig control bone lookup is not bidirectional"
        );
        for at in [4, 10, 24, 30] {
            let parent = short(row, at);
            ensure!(
                parent == u16::MAX || usize::from(parent) < bones,
                "rig control parent exceeds skeleton"
            );
        }
    }
    for (bone, row) in rows[4].iter().enumerate() {
        let slot = short(row, 0);
        ensure!(
            slot == u16::MAX
                || (usize::from(slot) < controls
                    && short(&rows[5][usize::from(slot)], 0) as usize == bone),
            "rig control forward lookup has an invalid slot"
        );
    }
    for row in &rows[2] {
        let f = |at| f32::from_le_bytes(row[at..at + 4].try_into().unwrap());
        let norm = (0..4).map(|i| f(i * 4).powi(2)).sum::<f32>();
        ensure!(
            (0..8).all(|i| f(i * 4).is_finite()) && (norm - 1.0).abs() < 1e-3 && f(28) == 1.0,
            "rig control default transform is invalid"
        );
    }
    for row in &rows[6] {
        ensure!(
            (word(row, 4) as usize) < bones,
            "named rig control exceeds skeleton"
        );
    }
    Ok(())
}

fn lower(rows: &[Vec<Vec<u8>>], shape: &clips::slots::Shape) -> Result<Vec<Vec<Vec<u8>>>> {
    check(rows)?;
    ensure!(
        rows[1].len() == shape.map.len(),
        "rig control and clip slot counts differ"
    );
    let mut result = rows.to_vec();
    let mut start = 0usize;
    for row in &mut result[0] {
        let end = start
            .checked_add(word(row, 4) as usize)
            .context("rig group extent")?;
        let group = shape
            .map
            .get(start..end)
            .context("rig group exceeds slot map")?;
        let count = u32::try_from(group.iter().filter(|slot| slot.is_some()).count())?;
        row[4..8].copy_from_slice(&count.to_le_bytes());
        start = end;
    }
    for index in [1, 2, 5] {
        result[index] = rows[index]
            .iter()
            .zip(&shape.map)
            .filter(|(_, slot)| slot.is_some())
            .map(|(row, _)| row.clone())
            .collect();
    }
    for row in &mut result[4] {
        let old = short(row, 0);
        let slot = if old == u16::MAX {
            old
        } else {
            shape
                .map
                .get(usize::from(old))
                .context("rig bone slot exceeds clip mapping")?
                .unwrap_or(u16::MAX)
        };
        row.copy_from_slice(&slot.to_le_bytes());
    }
    ensure!(
        result[1].len() == usize::from(shape.native_slots),
        "rig control output count differs"
    );
    check(&result)?;
    Ok(result)
}

/// Validate the native resource and the allocation-local state carried with it.
pub fn validate(bytes: &[u8]) -> Result<()> {
    let p = Payload(bytes.to_vec());
    check(&read(&p, false)?)?;
    let instance = p.pointer(16)?;
    let end = instance
        .checked_add(usize::try_from(p.u64(0x48)?)?)
        .context("rig control instance extent")?;
    ensure!(
        instance >= 4 && p.u32(instance - 4)? == 0x80808F96 && end <= p.pointer(24)?,
        "rig control instance layout differs"
    );
    let state = p.array_range(instance + 0x30, 48, Some(0x80808F92))?;
    ensure!(
        state.is_empty() || (state.start >= instance && state.end <= end),
        "rig control state escapes its instance"
    );
    Ok(())
}

/// Convert a source rig control resource onto a native template whose slots map one to one, as a
/// standalone rig with its own skeleton does. Every array is the source's, and the template keeps
/// the fields no array covers.
pub fn convert_unmapped(source: &[u8], template: &[u8]) -> Result<Payload> {
    let modern = Payload(source.to_vec());
    let original = Payload(template.to_vec());
    validate(&original.0)?;
    let rows = read(&modern, true)?;
    let count = u16::try_from(rows[1].len())?;
    let shape = clips::slots::Shape {
        native_slots: count,
        map: (0..count).map(Some).collect(),
    };
    let converted = lower(&rows, &shape)?;
    let mut result = original.clone();
    let base = result.pointer(24)?;
    for (rows, &(_, field, _, _, class)) in converted.iter().zip(&FIELDS) {
        write_array(
            &mut result.0,
            base + field,
            class,
            rows.len(),
            &rows.concat(),
        )?;
    }
    let size = result.0.len() as u64;
    result.0[..8].copy_from_slice(&size.to_le_bytes());
    validate(&result.0)?;
    ensure!(
        read(&result, false)? == converted,
        "source rig controls changed during serialization"
    );
    Ok(result)
}

fn owner(
    reader: &mut Reader,
    rig: &Value,
    modern: bool,
    first_person: bool,
) -> Result<(u32, Payload)> {
    let class = if modern { "80808B5F" } else { "80808F8F" };
    let rows = rig["components"]
        .as_array()
        .context("rig control components")?
        .iter()
        .filter(|c| (c["entity"] != rig["runtime_entity"]) == first_person && c["class"] == class)
        .collect::<Vec<_>>();
    ensure!(rows.len() == 1, "rig controls missing or ambiguous");
    let tag = hex(&rows[0]["owner"])?;
    Ok((
        tag,
        (*reader.tag(tag, Some(if modern { 0x80809B06 } else { 0x80809C36 }))?).clone(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    sr: &mut Reader,
    nr: &mut Reader,
    source: &Value,
    native: &Value,
    calibration: &Value,
    slots: &clips::slots::Table,
    bones: usize,
    first_person: bool,
    graph: &Path,
) -> Result<Value> {
    let (from, modern) = owner(sr, source, true, first_person)?;
    let (to, original) = owner(nr, native, false, first_person)?;
    let (_, reference) = owner(sr, calibration, true, first_person)?;
    validate(&original.0)?;
    let known = read(&reference, true)?;
    let native_rows = read(&original, false)?;
    let weapon_shape;
    let shape = if first_person {
        slots
            .shapes()
            .get(&u16::try_from(bones)?)
            .context("rig controls lack calibrated clip slots")?
    } else {
        ensure!(
            known[1].len() == native_rows[1].len(),
            "weapon control slot counts differ"
        );
        let count = u16::try_from(known[1].len())?;
        weapon_shape = clips::slots::Shape {
            native_slots: count,
            map: (0..count).map(Some).collect(),
        };
        &weapon_shape
    };
    ensure!(
        lower(&known, shape)? == native_rows,
        "paired rig control format evidence differs"
    );
    let rows = read(&modern, true)?;
    ensure!(
        rows[4].len() == bones && rows[1].len() == known[1].len(),
        "source rig control shape differs from calibration"
    );
    // Only bone references may change a validated control operation here. New
    // operations need their own evidence instead of borrowing an unrelated row.
    for (row, template) in rows[1].iter().zip(&known[1]) {
        let mut signature = row.clone();
        for at in [4, 10, 24, 30, 44] {
            signature[at..at + 2].copy_from_slice(&template[at..at + 2]);
        }
        ensure!(
            signature == *template,
            "source rig control operation has no native format evidence"
        );
    }
    let converted = lower(&rows, shape)?;
    let mut result = original.clone();
    let base = result.pointer(24)?;
    for (rows, &(_, field, _, _, class)) in converted.iter().zip(&FIELDS) {
        write_array(
            &mut result.0,
            base + field,
            class,
            rows.len(),
            &rows.concat(),
        )?;
    }
    let size = result.0.len() as u64;
    result.0[..8].copy_from_slice(&size.to_le_bytes());
    validate(&result.0)?;
    ensure!(
        read(&result, false)? == converted,
        "source rig controls changed during serialization"
    );
    let file = format!("animation/controls-{from:08X}.bin");
    let template_file = format!("animation/controls-{from:08X}-template.bin");
    fs::write(graph.join(&file), &result.0)?;
    fs::write(graph.join(&template_file), &original.0)?;
    Ok(
        json!({"kind":"controls","source":from,"native":to,"first_person":first_person,
        "file":file,"template_file":template_file,"bones":bones,"source_slots":shape.map.len(),
        "native_slots":shape.native_slots,"skipped_source_slots":shape.skipped(),"gameplay_verified":false}),
    )
}
