//! Selected cooked source mass, centre and inertia in one native physical body.
mod inertia;
mod selection;
use super::motion::number;
use crate::{
    halo_reach::{Scene, rig},
    presentation::put,
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};
pub(super) use selection::{forms, physical};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) struct Body {
    pub mass: f32,
    pub center: [f32; 3],
    pub inertia: [[f64; 3]; 3],
    pub report: Value,
}

#[derive(Clone)]
pub(super) struct Properties {
    pub mass: f32,
    pub center: [f32; 3],
    pub inertia: [[f64; 3]; 3],
}

impl Properties {
    /// Each convex child carries a share of its source body's mass and tensor.
    /// All children retain that body's centre, so native addition preserves it.
    pub(super) fn share(&self, count: usize) -> Result<Self> {
        ensure!(count > 0, "Source physical body has no convex children");
        Ok(Self {
            mass: self.mass / count as f32,
            center: self.center,
            inertia: self.inertia.map(|row| row.map(|v| v / count as f64)),
        })
    }

    pub(super) fn rotate(&mut self, frame: &[f32; 16]) {
        self.center = rig::point(frame, self.center, false);
        self.inertia = inertia::rotate(&self.inertia, frame);
    }
}

pub(super) fn properties(scene: &Scene, index: usize) -> Result<Properties> {
    let source = &scene.object.gameplay["rigid_body"];
    let row = source["bodies"]
        .get(index)
        .context("Source physical body")?;
    let mass = number(row, "mass")?;
    ensure!(
        mass > 0. && number(row, "mass_override")? == 0.,
        "Unsupported source physical mass override"
    );
    let node = usize::try_from(row["node"].as_u64().context("Source physics node")?)?;
    let name = source["nodes"]
        .get(node)
        .and_then(Value::as_str)
        .context("Source physics node name")?;
    let model = &scene.models[0].model;
    let matches = model
        .bones
        .iter()
        .enumerate()
        .filter(|(_, bone)| bone.name == name)
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Source physics node is absent or ambiguous in the render rig"
    );
    let worlds = rig::world(&model.bones)?;
    let world = &worlds[matches[0].0];
    Ok(Properties {
        mass,
        center: rig::point(
            world,
            vector(&row["center_source"])?.map(|v| v * scene.metres()),
            true,
        ),
        inertia: inertia::rotate(
            &inertia::read(&row["inertia_source"], scene.metres())?,
            world,
        ),
    })
}

pub(super) fn write(
    payload: &mut Payload,
    at: usize,
    bones: &Value,
    properties: &Properties,
) -> Result<()> {
    let (_, orientation) = super::super::seat::frame(bones, 0, ([0.; 3], [0., 0., 0., 1.]))?;
    let tensor = inertia::rotate(&properties.inertia, &rig::transform([0.; 3], orientation));
    inertia::validate(&tensor)?;
    for (row, values) in tensor.iter().enumerate() {
        for (axis, &value) in values.iter().enumerate() {
            let value = value as f32;
            ensure!(value.is_finite(), "Nonfinite native body inertia");
            put(
                &mut payload.0,
                at + 0x30 + row * 16 + axis * 4,
                &value.to_le_bytes(),
            )?;
        }
        put(&mut payload.0, at + 0x3c + row * 16, &[0; 4])?;
    }
    let (center, _) = super::super::seat::frame(bones, 0, (properties.center, [0., 0., 0., 1.]))?;
    for (axis, value) in center.iter().enumerate() {
        put(&mut payload.0, at + 0x20 + axis * 4, &value.to_le_bytes())?;
    }
    put(&mut payload.0, at + 0x2c, &[0; 4])?;
    put(&mut payload.0, at + 0x60, &properties.mass.to_le_bytes())
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

fn undamaged(payload: &Payload, group: usize) -> Result<bool> {
    ensure!(
        payload.u64(group)? == 0x18,
        "Native body condition policy differs"
    );
    let pairs = payload.array(group + 8, 8, Some(0x80809c31))?;
    if pairs.is_empty() {
        return Ok(true);
    }
    let mut regions = 0;
    let mut damage = Vec::new();
    for pair in pairs {
        match payload.u32(pair)? {
            0x51e7a18d => regions += 1,
            0x6a807a1a => damage.push(payload.u32(pair + 4)?),
            _ => anyhow::bail!("Native body needs another initial condition contract"),
        }
    }
    ensure!(
        regions == 1 && !damage.is_empty(),
        "Native body damage conditions are incomplete"
    );
    Ok(damage.contains(&super::damage::hash("none")))
}

/// A carrier can have separate massive root rows for intact and damaged forms.
/// Select its undamaged condition before preserving the primary body envelope.
pub(super) fn primary(payload: &Payload) -> Result<usize> {
    let definition = payload.pointer(24)?;
    let groups = payload.array(definition + 0x38, 24, Some(0x80809c2c))?;
    let mut selected = Vec::new();
    for row in payload.array(definition + 0x388, 0x90, Some(0x80808c07))? {
        let mass = payload.f32(row + 0x60)?;
        ensure!(mass.is_finite() && mass >= 0., "Invalid native body mass");
        if payload.u32(row + 8)? != 0 || mass == 0. {
            continue;
        }
        let group = *groups
            .get(payload.u32(row + 12)? as usize)
            .context("Native body condition index exceeds its groups")?;
        if undamaged(payload, group)? {
            selected.push(row);
        }
    }
    ensure!(
        selected.len() == 1,
        "Native undamaged primary body is absent or ambiguous"
    );
    Ok(selected[0])
}

pub(super) fn build(payload: &mut Payload, scene: &Scene, bones: &Value) -> Result<Body> {
    let source = &scene.object.gameplay["rigid_body"];
    let selected = physical(scene)?
        .into_iter()
        .filter(|part| {
            part.condition
                .as_ref()
                .is_none_or(|(_, states)| states.contains(&0))
        })
        .collect::<Vec<_>>();
    ensure!(
        !selected.is_empty(),
        "Source vehicle has no intact physical bodies"
    );
    let selection = selected
        .iter()
        .map(|part| json!({"body":part.body,"condition":part.condition}))
        .collect::<Vec<_>>();
    let mut mass = 0.;
    let mut center = [0.; 3];
    let mut body_rows = Vec::new();
    let mut tensors = Vec::new();
    for part in selected {
        let Properties {
            mass: weight,
            center: position,
            inertia: tensor,
        } = properties(scene, part.body)?;
        for axis in 0..3 {
            center[axis] += position[axis] * weight;
        }
        mass += weight;
        tensors.push((weight, position, tensor));
        body_rows.push(json!({"index":part.body,"mass":weight,"center_metres":position,"inertia_kg_metres_squared":tensor}));
    }
    ensure!(
        mass.is_finite() && mass > 0.,
        "Invalid selected vehicle mass"
    );
    center = center.map(|v| v / mass);
    let mut source_tensor = [[0.; 3]; 3];
    let mut tensor = [[0.; 3]; 3];
    for (weight, position, inertia) in tensors {
        inertia::add(&mut source_tensor, &inertia, weight, position, center);
        // 4653F0 adds the enabled metadata tensors directly. Flight authority
        // must use that actual matrix, including when a source body is split
        // into several convex children for the native collector.
        for row in 0..3 {
            for column in 0..3 {
                tensor[row][column] += inertia[row][column];
            }
        }
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
    let at = primary(payload)?;
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
            "native_group":payload.u32(at+12)?,"native_body_name":format!("{:08X}",payload.u32(at)?),"initial_damage":"none",
            "source_combined_inertia_kg_metres_squared":source_tensor,
            "native_tensor_assembly":"sum_enabled_parts",
            "native_inertia_kg_metres_squared":native_tensor,
            "previous_mass":old_mass,"gameplay_verified":false,
            "limits":["Enabled source convex shapes supply their mass, centre and additive tensor during native body construction. Flight authority uses that assembled tensor. The native sum does not add parallel-axis terms between independently conditioned parts. Source authoring scales are not applied twice."]
        }),
    })
}
