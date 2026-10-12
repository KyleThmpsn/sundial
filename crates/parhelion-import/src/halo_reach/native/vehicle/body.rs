//! Selected cooked source mass, centre and inertia in one native physical body.
mod inertia;
use super::{METRES, motion::number};
use crate::{
    halo_reach::{Scene, rig},
    presentation::put,
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) struct Body {
    pub mass: f32,
    pub center: [f32; 3],
    pub inertia: [[f64; 3]; 3],
    pub report: Value,
}

fn vector(value: &Value) -> Result<[f32; 3]> {
    let values = value.as_array().context("Source body vector")?;
    ensure!(values.len() >= 3, "Source body vector is short");
    let mut out = [0.; 3];
    for (out, value) in out.iter_mut().zip(values) {
        *out = value.as_f64().context("Source body vector number")? as f32;
        ensure!(out.is_finite(), "Nonfinite source body vector");
    }
    Ok(out)
}

pub(super) fn selection(
    model: &crate::halo_reach::model::Model,
    source: &Value,
) -> Result<(BTreeSet<usize>, Vec<Value>)> {
    let rows = source["bodies"].as_array().context("Source rigid bodies")?;
    let regions = source["regions"]
        .as_array()
        .context("Source physics regions")?;
    let mut selected = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        if row["region"] == -1 && row["permutation"] == -1 {
            selected.insert(index);
        }
    }
    let mut selection = Vec::new();
    for region in regions {
        let name = region["name"]
            .as_str()
            .context("Source physics region name")?;
        let chosen = model
            .selected
            .get(name)
            .with_context(|| format!("Source physics region {name} has no model selection"))?;
        if chosen.is_empty() {
            continue;
        }
        let permutations = region["permutations"]
            .as_array()
            .context("Source physics permutations")?;
        let matches = permutations
            .iter()
            .filter(|p| p["name"] == *chosen)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Source physics permutation {name}/{chosen} is absent or ambiguous"
        );
        for index in matches[0]["bodies"]
            .as_array()
            .context("Source permutation bodies")?
        {
            let index = usize::try_from(index.as_u64().context("Source rigid body index")?)?;
            ensure!(
                index < rows.len(),
                "Source rigid body index exceeds its table"
            );
            selected.insert(index);
        }
        selection.push(json!({"region":name,"permutation":chosen}));
    }
    ensure!(
        !selected.is_empty(),
        "Source vehicle has no selected rigid bodies"
    );
    Ok((selected, selection))
}

pub(super) fn build(payload: &mut Payload, scene: &Scene, bones: &Value) -> Result<Body> {
    let model = &scene.models[0].model;
    let source = &scene.object.gameplay["rigid_body"];
    let rows = source["bodies"].as_array().context("Source rigid bodies")?;
    let nodes = source["nodes"].as_array().context("Source physics nodes")?;
    let (selected, selection) = selection(model, source)?;
    let world = rig::world(&model.bones)?;
    let mut mass = 0.;
    let mut center = [0.; 3];
    let mut body_rows = Vec::new();
    let mut tensors = Vec::new();
    for &index in &selected {
        let row = &rows[index];
        let weight = number(row, "mass")?;
        ensure!(
            weight > 0. && number(row, "mass_override")? == 0.,
            "Source rigid body mass requires an unsupported override"
        );
        let node = usize::try_from(row["node"].as_u64().context("Source physics node")?)?;
        let name = nodes
            .get(node)
            .and_then(Value::as_str)
            .context("Source physics node name")?;
        let matches = model
            .bones
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name == name)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Source physics node {name} is absent or ambiguous in the model"
        );
        let position = rig::point(
            &world[matches[0].0],
            vector(&row["center_source"])?.map(|v| v * METRES),
            true,
        );
        let tensor = inertia::rotate(
            &inertia::read(&row["inertia_source"], METRES)?,
            &world[matches[0].0],
        );
        for axis in 0..3 {
            center[axis] += position[axis] * weight;
        }
        mass += weight;
        tensors.push((weight, position, tensor));
        body_rows.push(json!({"index":index,"node":name,"mass":weight,"center_metres":position,"inertia_kg_metres_squared":tensor}));
    }
    ensure!(
        mass.is_finite() && mass > 0.,
        "Invalid selected vehicle mass"
    );
    center = center.map(|v| v / mass);
    let mut tensor = [[0.; 3]; 3];
    for (weight, position, inertia) in tensors {
        inertia::add(&mut tensor, &inertia, weight, position, center);
    }
    let (_, orientation) = super::super::seat::frame(bones, 0, ([0.; 3], [0., 0., 0., 1.]))?;
    let native_tensor = inertia::rotate(&tensor, &rig::transform([0.; 3], orientation));
    inertia::validate(&native_tensor)?;
    let i = payload.pointer(16)?;
    let d = payload.pointer(24)?;
    ensure!(
        payload.u32(i - 4)? == 0x808092d8 && payload.u32(d - 4)? == 0x80808a0c,
        "Native physical body implementation differs"
    );
    let rows = payload.array(d + 0x388, 0x90, Some(0x80808c07))?;
    let primary = rows
        .iter()
        .copied()
        .filter(|&r| {
            payload.u32(r + 8).ok() == Some(0)
                && payload
                    .f32(r + 0x60)
                    .is_ok_and(|mass| mass.is_finite() && mass > 0.)
        })
        .collect::<Vec<_>>();
    ensure!(
        primary.len() == 1 && rows.first() == primary.first(),
        "Native primary body selection differs"
    );
    let at = primary[0];
    // The following selector describes retained carrier data. Its value differs
    // across valid primary bodies and does not identify the mass-bearing row.
    let old_mass = payload.f32(at + 0x60)?;
    ensure!(old_mass > 0., "Native primary body has no mass");
    // The cooked source values already include authoring mass and centre
    // adjustments. Convert length squared once and combine the selected bodies.
    for (row, values) in native_tensor.iter().enumerate() {
        for (axis, &value) in values.iter().enumerate() {
            let field = at + 0x30 + row * 16 + axis * 4;
            let value = value as f32;
            ensure!(value.is_finite(), "Native body inertia is not finite");
            put(&mut payload.0, field, &value.to_le_bytes())?;
        }
    }
    let (native_center, _) = super::super::seat::frame(bones, 0, (center, [0., 0., 0., 1.]))?;
    for (axis, value) in native_center.iter().enumerate() {
        put(&mut payload.0, at + 0x20 + axis * 4, &value.to_le_bytes())?;
    }
    put(&mut payload.0, at + 0x60, &mass.to_le_bytes())?;
    Ok(Body {
        mass,
        center,
        inertia: tensor,
        report: json!({
            "source":source["tag"],"selection":selection,"bodies":body_rows,
            "mass":mass,"center_metres":center,"native_center":native_center,"native_row":at,
            "native_inertia_kg_metres_squared":native_tensor,
            "previous_mass":old_mass,"gameplay_verified":false,
            "limits":["Selected cooked source masses and tensors are combined into one body. Source authoring scales are not applied a second time. Source articulated and damaged-body behavior is not translated."]
        }),
    })
}
