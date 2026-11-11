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
    /// The factor on every source part's mass and tensor, which the collision rows take as well.
    pub mass_scale: f32,
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

    /// The same part at `scale` times its mass. The centre stays, and the tensor scales with the
    /// mass, so the distribution keeps its shape.
    pub(super) fn scaled(&self, scale: f32) -> Self {
        Self {
            mass: self.mass * scale,
            center: self.center,
            inertia: self.inertia.map(|row| row.map(|v| v * f64::from(scale))),
        }
    }

    pub(super) fn rotate(&mut self, frame: &[f32; 16]) {
        self.center = rig::point(frame, self.center, false);
        self.inertia = inertia::rotate(&self.inertia, frame);
    }

    /// The native collector (`004653F0` through `0188A860`) adds each admitted row's tensor
    /// directly, without parallel-axis terms between row centres. Carrying each row's term about
    /// the combined centre in its own tensor makes that plain sum equal the source body's
    /// combined inertia. The row keeps its own centre, so the mass-weighted centre is unchanged.
    pub(super) fn about(&self, center: [f32; 3]) -> Self {
        let mut inertia = [[0.; 3]; 3];
        inertia::add(&mut inertia, &self.inertia, self.mass, self.center, center);
        Self {
            inertia,
            ..self.clone()
        }
    }
}

/// The mass-weighted centre of the initially enabled source bodies, at the object origin's height
/// for a stationary source.
pub(super) fn initial_center(scene: &Scene) -> Result<[f32; 3]> {
    let mut mass = 0.;
    let mut center = [0.; 3];
    for part in physical(scene)?.into_iter().filter(|part| {
        part.condition
            .as_ref()
            .is_none_or(|(_, states)| states.contains(&0))
    }) {
        let properties = properties(scene, part.body)?;
        mass += properties.mass;
        for (center, position) in center.iter_mut().zip(properties.center) {
            *center += position * properties.mass;
        }
    }
    ensure!(
        mass.is_finite() && mass > 0.,
        "Source vehicle has no intact physical mass"
    );
    Ok(anchored(scene, center.map(|v| v / mass)))
}

/// Reach keeps a turret fixed, while a native carrier body is free. A Shade with its mass centre
/// 1.4 m up on its narrow base lay on its side in game (Dawn 1.7.2, 2026-11-06), and with the
/// centre dropped to its origin, which Reach turrets keep at their base, it still lay on its side
/// after a push. A stationary source's centre now mirrors below its origin by its source height,
/// so it lies under the base and gravity turns the body back upright from any tilt short of
/// inverted. Taking the row tensors about that centre also raises the body's resistance to tipping.
/// Only stationary sources, which rest on their collision and never drive, take this centre.
fn anchored(scene: &Scene, center: [f32; 3]) -> [f32; 3] {
    if super::stationary(scene) {
        [center[0], center[1], -center[2].abs()]
    } else {
        center
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
    // The cooked centre already holds the centre of mass offset at `+0x38`, and the cooked tensor
    // the inertia scale at `+0x20`: the cargo truck's hull centroid at z 0.644 less its offset
    // 0.225 is its cooked 0.419, and the Mongoose's cooked tensor is a uniform multiple of its
    // hull's that tracks its 1.5 scale.
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

/// The total mass of the initially enabled source parts.
fn source_mass(scene: &Scene) -> Result<f32> {
    let mut mass = 0.;
    for part in physical(scene)?.into_iter().filter(|part| {
        part.condition
            .as_ref()
            .is_none_or(|(_, states)| states.contains(&0))
    }) {
        mass += properties(scene, part.body)?.mass;
    }
    ensure!(
        mass.is_finite() && mass > 0.,
        "Source vehicle has no intact physical mass"
    );
    Ok(mass)
}

/// The factor that brings the source's intact mass to the carrier's own, for a carrier whose
/// retained drive is tuned to its mass, else 1. The stock tank's tread drive keeps its engine,
/// gears and tread grip, and skids the hull round against the treads' lateral friction, which
/// grows with the load. The Scorpion at its source 21,775 kg, twice the tank's 10,378 kg, on
/// treads 1.2 times as long, drove forward and back but never turned (Dawn 1.7.2, 2026-11-08).
/// Its parts keep their centres and tensor shape at the carrier's mass.
pub(super) fn native_mass_scale(payload: &Payload, scene: &Scene) -> Result<f32> {
    let native = payload.f32(primary(payload)? + 0x60)?;
    ensure!(
        native.is_finite() && native > 0.,
        "Native primary body has no mass"
    );
    Ok(native / source_mass(scene)?)
}

/// `mass_scale` multiplies every selected part's mass and tensor (`native_mass_scale`).
pub(super) fn build(
    payload: &mut Payload,
    scene: &Scene,
    bones: &Value,
    mass_scale: f32,
) -> Result<Body> {
    ensure!(
        mass_scale.is_finite() && mass_scale > 0.,
        "Invalid vehicle mass scale"
    );
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
        } = properties(scene, part.body)?.scaled(mass_scale);
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
    center = anchored(scene, center.map(|v| v / mass));
    // 4653F0 adds the enabled metadata tensors directly. Each collision row carries its
    // parallel-axis term about this centre (`Properties::about`), so that plain sum is the
    // source combined tensor, which flight authority then uses.
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
        mass_scale,
        report: json!({
            "source":source["tag"],"selection":selection,"bodies":body_rows,
            "mass":mass,"mass_scale":mass_scale,"source_mass":mass / mass_scale,"center_metres":center,"native_center":native_center,"native_row":at,
            "native_group":payload.u32(at+12)?,"native_body_name":format!("{:08X}",payload.u32(at)?),"initial_damage":"none",
            "source_combined_inertia_kg_metres_squared":tensor,
            "native_tensor_assembly":"sum_enabled_parts_with_row_parallel_axis",
            "native_inertia_kg_metres_squared":native_tensor,
            "previous_mass":old_mass,"gameplay_verified":false,
            "limits":["Enabled source convex shapes supply their mass, centre and tensor during native body construction. Each row's tensor carries its parallel-axis term about the initial combined centre, so the native plain sum is the source combined tensor. After a damage condition changes the enabled set, the terms still refer to the initial centre. Source authoring scales are not applied twice."]
        }),
    })
}
