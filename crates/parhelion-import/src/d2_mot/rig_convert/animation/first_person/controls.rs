//! Carry source rig controls together with source-owned weapon and arm skeletons.
//!
//! Clip slots index these control rows. Replacing only FK bones leaves the
//! carrier's default weapon transforms and control parents in the live rig.
use super::*;
use crate::d2_mot::{payload::Payload, rig_convert::write_array};
use crate::tiger::animation::rig::controls::{FIELDS, check, read, short, validate, word};

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

fn operation(row: &[u8]) -> Vec<u8> {
    let mut signature = row.to_vec();
    for at in [4, 10, 24, 30, 44] {
        signature[at..at + 2].fill(0);
    }
    signature
}

fn arm_count(rig: &Value) -> Result<usize> {
    let skeletons = rig["skeletons"].as_array().context("rig skeletons")?;
    let components = rig["components"].as_array().context("rig components")?;
    // The component owner establishes entity membership. Older calibration exports
    // do not repeat that entity on each decoded skeleton row.
    let mut roles = Vec::with_capacity(skeletons.len());
    for skeleton in skeletons {
        let owners = components
            .iter()
            .filter(|component| component["owner"] == skeleton["owner"])
            .collect::<Vec<_>>();
        ensure!(owners.len() == 1, "rig skeleton ownership is ambiguous");
        roles.push((
            owners[0]["entity"] != rig["runtime_entity"],
            skeleton["bones"].as_array().context("rig bones")?.len(),
        ));
    }
    let count = |first_person| -> Result<usize> {
        let rows = roles
            .iter()
            .filter(|(role, _)| *role == first_person)
            .collect::<Vec<_>>();
        ensure!(rows.len() == 1, "rig skeleton is missing or ambiguous");
        Ok(rows[0].1)
    };
    count(true)?
        .checked_sub(count(false)?)
        .context("shared arm prefix")
}

/// A shared clip bank can retain an older weapon size. In that case the controller's
/// explicit bone lookup establishes the arm slots, while native operations establish the
/// format of the private weapon controls. Group identities and shared arm poses stay exact.
fn structural_shape(
    rows: &[Vec<Vec<u8>>],
    native: &[Vec<Vec<u8>>],
    arms: usize,
) -> Result<clips::slots::Shape> {
    check(rows)?;
    check(native)?;
    ensure!(
        arms <= rows[4].len()
            && arms <= native[4].len()
            && rows[0]
                .iter()
                .map(|r| word(r, 0))
                .eq(native[0].iter().map(|r| word(r, 0))),
        "private control group identities differ"
    );
    let operations = native[1]
        .iter()
        .map(|r| operation(r))
        .collect::<BTreeSet<_>>();
    let mut map = Vec::new();
    let mut next = 0u16;
    for (index, row) in rows[1].iter().enumerate() {
        let bone = usize::from(short(row, 44));
        let keep = if bone < arms {
            let slot = short(&native[4][bone], 0);
            if slot == u16::MAX {
                false
            } else {
                ensure!(
                    slot == next
                        && native[1][usize::from(slot)] == *row
                        && native[2][usize::from(slot)] == rows[2][index],
                    "shared arm control or default pose differs"
                );
                true
            }
        } else {
            true
        };
        if keep {
            ensure!(
                operations.contains(&operation(row)),
                "private control operation lacks native evidence"
            );
        }
        map.push(keep.then_some(next));
        next += u16::from(keep);
    }
    let shape = clips::slots::Shape {
        native_slots: next,
        map,
    };
    let lowered = lower(rows, &shape)?;
    ensure!(
        lowered[0]
            .iter()
            .zip(&native[0])
            .take(rows[0].len().saturating_sub(1))
            .all(|(a, b)| a == b),
        "shared control groups differ"
    );
    Ok(shape)
}

fn paired_shape(
    rows: &[Vec<Vec<u8>>],
    native: &[Vec<Vec<u8>>],
    arms: usize,
    first_person: bool,
    slots: &clips::slots::Table,
) -> Result<clips::slots::Shape> {
    let identity = clips::slots::Shape {
        native_slots: u16::try_from(rows[1].len())?,
        map: (0..u16::try_from(rows[1].len())?).map(Some).collect(),
    };
    let candidate = if first_person {
        slots.shapes().get(&u16::try_from(rows[4].len())?)
    } else {
        Some(&identity)
    };
    if let Some(shape) = candidate
        && lower(rows, shape)? == native
    {
        return Ok(shape.clone());
    }
    let shape = structural_shape(rows, native, arms)?;
    if first_person {
        let prefix = rows[1]
            .iter()
            .take_while(|row| usize::from(short(row, 44)) < arms)
            .count();
        ensure!(
            prefix > 0
                && slots
                    .shapes()
                    .values()
                    .any(|known| known.map.get(..prefix) == shape.map.get(..prefix)),
            "controller arm slots lack paired clip evidence"
        );
    }
    Ok(shape)
}

/// The private weapon section can change size. Shared arm controls retain the slots
/// established by the carrier's paired clips. New weapon controls are emitted in their
/// explicit serialized order, with every operation checked against that carrier.
pub(super) fn owned_slots(
    sr: &mut Reader,
    nr: &mut Reader,
    source: &Value,
    native: &Value,
    calibration: &Value,
    slots: &mut clips::slots::Table,
) -> Result<()> {
    let (_, modern) = owner(sr, source, true, true)?;
    let (_, reference) = owner(sr, calibration, true, true)?;
    let (_, original) = owner(nr, native, false, true)?;
    let rows = read(&modern, true)?;
    let known = read(&reference, true)?;
    check(&rows)?;
    check(&known)?;
    let arms = arm_count(calibration)?;
    ensure!(arms == arm_count(native)?, "calibration arm prefix differs");
    let shape = paired_shape(&known, &read(&original, false)?, arms, true, slots)?;
    let pairs = super::super::super::owned::validate(source, calibration)?;
    let weapon = pairs
        .iter()
        .find(|(_, _, fp)| !fp)
        .context("weapon skeleton")?
        .0;
    let weapon_bones = source["skeletons"]
        .as_array()
        .context("skeletons")?
        .iter()
        .find(|s| s["owner"] == format!("{weapon:08X}"))
        .and_then(|s| s["bones"].as_array())
        .context("weapon bones")?
        .len();
    let arms = rows[4]
        .len()
        .checked_sub(weapon_bones)
        .context("shared arm controls")?;
    ensure!(
        rows[4].get(..arms) == known[4].get(..arms),
        "shared arm control bindings differ"
    );
    for (row, mapped) in known[1].iter().zip(&shape.map) {
        ensure!(
            mapped.is_some() || usize::from(short(row, 44)) < arms,
            "calibration omits a weapon control"
        );
    }
    let operations = known[1]
        .iter()
        .map(|row| operation(row))
        .collect::<BTreeSet<_>>();
    let mut map = Vec::with_capacity(rows[1].len());
    let mut next = 0u16;
    for row in &rows[1] {
        ensure!(
            operations.contains(&operation(row)),
            "source rig control operation has no native format evidence"
        );
        let bone = usize::from(short(row, 44));
        let keep = if bone < arms {
            let reference_slot = usize::from(short(&known[4][bone], 0));
            ensure!(
                known[1].get(reference_slot) == Some(row),
                "shared arm control operation differs"
            );
            let mapped = *shape.map.get(reference_slot).context("shared arm slot")?;
            ensure!(
                mapped.is_none_or(|slot| slot == next),
                "shared arm control order differs"
            );
            mapped.is_some()
        } else {
            true
        };
        map.push(keep.then_some(next));
        if keep {
            next = next
                .checked_add(1)
                .context("private control slot overflow")?;
        }
    }
    let shape = clips::slots::Shape {
        native_slots: next,
        map,
    };
    lower(&rows, &shape)?;
    slots.insert_owned_shape(u16::try_from(rows[4].len())?, shape.clone())?;
    slots.owned_groups(
        rows[0]
            .iter()
            .map(|row| (word(row, 0), word(row, 4)))
            .collect(),
        rows[5].iter().map(|row| short(row, 0)).collect(),
        shape,
    );
    Ok(())
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
    let arms = if first_person {
        arm_count(calibration)?
    } else {
        0
    };
    ensure!(
        !first_person || arms == arm_count(native)?,
        "calibration arm prefix differs"
    );
    paired_shape(&known, &native_rows, arms, first_person, slots)?;
    let rows = read(&modern, true)?;
    ensure!(
        rows[4].len() == bones,
        "source rig control shape differs from calibration"
    );
    // Only bone references may change a validated control operation here. New
    // operations need their own evidence instead of borrowing an unrelated row.
    let operations = known[1]
        .iter()
        .map(|row| operation(row))
        .collect::<BTreeSet<_>>();
    for row in &rows[1] {
        ensure!(
            operations.contains(&operation(row)),
            "source rig control operation has no native format evidence"
        );
    }
    let weapon_shape;
    let shape = if first_person {
        slots
            .shapes()
            .get(&u16::try_from(bones)?)
            .context("source rig lacks calibrated control slots")?
    } else {
        let count = u16::try_from(rows[1].len())?;
        weapon_shape = clips::slots::Shape {
            native_slots: count,
            map: (0..count).map(Some).collect(),
        };
        &weapon_shape
    };
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
