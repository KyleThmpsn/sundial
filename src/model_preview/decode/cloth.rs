//! Select one native cloth render group and retain its actual display-buffer mapping.
use super::*;
use crate::model_preview::cloth::{Display, Pending};

pub(super) fn select(
    manager: &PackageManager,
    component: Option<&[u8]>,
    bytes: &[u8],
    mut parts: Vec<StagedPart>,
    simulate: bool,
    model: &mut Model,
) -> Result<(Vec<StagedPart>, Option<Pending>), String> {
    let Some(component) = component else {
        return Ok((parts, None));
    };
    let data = pointer(component, 0x18)?;
    if data < 4 || u32_at(component, data - 4)? != 0x8080_7286 {
        return Ok((parts, None));
    }
    let tag = u32_at(component, data + 0x358)?;
    if matches!(tag, 0 | u32::MAX) {
        model
            .notices
            .push("Cloth has no solver definition. Showing stored geometry.".into());
        return Ok((parts, None));
    }
    let definition = checked(manager, tag, 0x8080_727A)?;
    if simulate {
        match playback(manager, &definition, bytes) {
            Ok((selected, pending)) => {
                parts.retain(|p| selected.contains(&p.2));
                return Ok((parts, Some(pending)));
            }
            Err(error) => model
                .notices
                .push(format!("Cloth: {error}. Showing stored geometry.")),
        }
    }
    // Role 1 is the skinned state, whose display vertices come from the model.
    let group = u32_at(&definition, 0x1C)? as usize;
    if group >= 4 || u64_at(&definition, 0x90 + group * 0x180)? != 0 {
        return Err("The stored cloth state requires simulation output".into());
    }
    let selected = group_parts(&definition, bytes, group)?;
    parts.retain(|p| selected.contains(&p.2));
    Ok((parts, None))
}

fn playback(
    manager: &PackageManager,
    definition: &[u8],
    model: &[u8],
) -> Result<(BTreeSet<usize>, Pending), String> {
    let states = [0, 1, 2, 3].map(|role| u32_at(definition, 8 + role * 12).map(|v| v as usize));
    let [steady, skinned, first, second] = states;
    let states = [steady?, skinned?, first?, second?];
    let group = u32_at(definition, 16)? as usize;
    let selected = group_parts(definition, model, group)?;
    let (count, rows) = array(definition, 0x90 + group * 0x180, 0x8080_7280, 12, 64)?;
    let mut display = Vec::with_capacity(count);
    for index in 0..count {
        let at = rows + index * 12;
        let destination = u32_at(definition, at)? as usize;
        if !destination.is_multiple_of(48) {
            return Err("The cloth display offset is not a complete vertex".into());
        }
        display.push(Display {
            first: destination / 48,
            buffer: u32_at(definition, at + 4)? as usize,
            count: u32_at(definition, at + 8)? as usize,
        });
    }
    let solver = manager
        .read_tag(u32_at(definition, 0x690)?)
        .map_err(|e| format!("Cloth solver could not be read: {e}"))?;
    let pending = Pending::read(&solver, states, display)?;
    Ok((selected, pending))
}

fn group_parts(definition: &[u8], model: &[u8], group: usize) -> Result<BTreeSet<usize>, String> {
    if group >= 4 {
        return Err("Invalid cloth render group".into());
    }
    let (meshes, mesh) = array(model, 0x10, 0x8080_7378, 136, 1024)?;
    if meshes != 1 {
        return Err("The cloth render groups require one model mesh".into());
    }
    let (parts, rows) = array(model, mesh + 0x18, 0x8080_737E, 32, 65536)?;
    let mut selected = BTreeSet::new();
    for stage in 0..23 {
        let start = u16_at(model, mesh + 0x28 + stage * 2)? as usize;
        let end = u16_at(model, mesh + 0x2A + stage * 2)? as usize;
        if start > end || end > parts {
            return Err("The cloth render stage exceeds its model".into());
        }
        let at = 0xA0 + group * 0x180 + stage * 16;
        let (count, indices) = array(definition, at, 0x8080_0007, 4, 65536)?;
        for index in 0..count {
            let local = u32_at(definition, indices + index * 4)? as usize;
            if local >= end - start {
                return Err("The cloth render group exceeds its stage".into());
            }
            selected.insert(rows + (start + local) * 32);
        }
    }
    Ok(selected)
}
