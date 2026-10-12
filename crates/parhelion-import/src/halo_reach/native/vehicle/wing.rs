//! Authored physical surfaces for the native flight controller.
mod surface;
use super::{
    body::Body,
    contact::{self, Contacts, pair},
    curve::Program,
    motion::number,
};
use crate::{
    halo_reach::Scene,
    presentation::{append, put},
    tiger::{allocation::Row, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn scalar(
    p: &mut Payload,
    owner: u32,
    i: usize,
    d: usize,
    parent: usize,
    program: Program,
) -> Result<()> {
    pair(p, owner, i, d, (0x808089f5, 0x808089f6), parent)?;
    put(&mut p.0, d + 0x58, &0x811c9dc5u64.to_le_bytes())?;
    program.write(p, d + 16)
}

fn maneuver_inputs(
    p: &mut Payload,
    owner: u32,
    instance: usize,
    definition: usize,
    names: &[u32; 2],
) -> Result<Vec<(usize, usize)>> {
    append(&mut p.0, instance + 0x20, 0x80809788, &[0; 192], 96)?;
    append(&mut p.0, definition + 0x40, 0x80809789, &[0; 80], 40)?;
    let states = p.array(instance + 0x20, 96, Some(0x80809788))?;
    let definitions = p.array(definition + 0x40, 40, Some(0x80809789))?;
    let mut result = Vec::new();
    for (index, (&state, &data)) in states.iter().zip(&definitions).enumerate() {
        pair(p, owner, state, data, (0x80809788, 0x80809789), instance)?;
        for field in [0x20, 0x24, 0x30, 0x38, 0x3c] {
            put(&mut p.0, state + field, &u32::MAX.to_le_bytes())?;
        }
        put(&mut p.0, data + 24, &0x808097c1u64.to_le_bytes())?;
        put(&mut p.0, data + 32, &names[index].to_le_bytes())?;
        result.push((data, index));
    }
    Ok(result)
}

pub(super) fn build(
    native: &mut Reader,
    original: &Payload,
    scene: &Scene,
    bones: &Value,
    controls: &Value,
    body: &Body,
    maneuver_names: Option<[u32; 2]>,
) -> Result<Contacts> {
    let i = original.pointer(16)?;
    let d = original.pointer(24)?;
    let owner = original.u32(i)?;
    ensure!(
        original.u32(i - 4)? == 0x80803d4c && original.u32(d - 4)? == 0x80803d65,
        "Native flight contact implementation differs"
    );
    let contacts = &scene.object.gameplay["contacts"];
    ensure!(
        contacts["anti_gravity_count"] == 0
            && contacts["wheels"].as_array().is_some_and(Vec::is_empty),
        "Authored flight surfaces require a source without ground contact points"
    );
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for p in &scene.models[0].model.primitives {
        for vertex in &p.vertices {
            for axis in 0..3 {
                let value = vertex.position[axis];
                ensure!(value.is_finite(), "Nonfinite source flight bounds");
                low[axis] = low[axis].min(value);
                high[axis] = high[axis].max(value);
            }
        }
    }
    let size: [f32; 3] = std::array::from_fn(|axis| high[axis] - low[axis]);
    ensure!(
        size.iter()
            .all(|v| v.is_finite() && *v > 0.01 && *v < 1000.),
        "Invalid source flight dimensions"
    );
    let gravity = number(contacts, "gravity_scale")?;
    let plan = surface::plan(size, controls, body, gravity, maneuver_names.is_some())?;
    let surfaces = plan.surfaces;
    let count = surfaces.len();
    let template = native.tag(0x80bfc32a, Some(0x80809c36))?;
    let td = template.pointer(24)?;
    let wd = *template
        .array(td + 0x130, 0x88, Some(0x80803d4f))?
        .first()
        .context("Native marker envelope")?;
    let wi = usize::try_from(template.u64(wd + 8)?)?;
    let marker = *template
        .array(wd + 0x60, 64, Some(0x80809c00))?
        .first()
        .context("Native marker row")?;
    let mut output = original.clone();
    for field in [0x30, 0x40, 0x50, 0x60] {
        put(&mut output.0, i + field, &[0; 16])?;
    }
    for field in [0x120, 0x130, 0x140, 0x150] {
        put(&mut output.0, d + field, &[0; 16])?;
    }
    put(&mut output.0, d + 0xd4, &gravity.to_le_bytes())?;
    append(
        &mut output.0,
        i + 0x60,
        0x80803d56,
        &vec![0; count * 0x140],
        0x140,
    )?;
    append(
        &mut output.0,
        d + 0x150,
        0x80803d57,
        &vec![0; count * 0x1a0],
        0x1a0,
    )?;
    let instances = output.array(i + 0x60, 0x140, Some(0x80803d56))?;
    let definitions = output.array(d + 0x150, 0x1a0, Some(0x80803d57))?;
    let mut markers = Vec::new();
    let mut report = Vec::new();
    let mut inputs = Vec::new();
    let mut surface_allocations = Vec::new();
    for ((&instance, &definition), surface) in instances.iter().zip(&definitions).zip(surfaces) {
        pair(
            &mut output,
            owner,
            instance,
            definition,
            (0x80803d56, 0x80803d57),
            i,
        )?;
        output.0[instance + 0x20..instance + 0xa0]
            .copy_from_slice(&template.0[wi + 0x30..wi + 0xb0]);
        output.0[definition + 0x10..definition + 0x70]
            .copy_from_slice(&template.0[wd + 0x10..wd + 0x70]);
        pair(
            &mut output,
            owner,
            instance + 0x20,
            definition + 0x10,
            (0x80809663, 0x80809664),
            instance,
        )?;
        pair(
            &mut output,
            owner,
            instance + 0x40,
            definition + 0x20,
            (0x80809bd6, 0x80809bd7),
            instance + 0x20,
        )?;
        put(
            &mut output.0,
            instance + 0x90,
            &(i as i64 - (instance + 0x90) as i64).to_le_bytes(),
        )?;
        for (si, sd, program) in [
            (0xa0, 0x70, surface.lift.program()?),
            (0xd0, 0xd0, Program::constant(surface.drag)?),
            (0x100, 0x138, surface.response),
        ] {
            scalar(
                &mut output,
                owner,
                instance + si,
                definition + sd,
                instance,
                program,
            )?;
        }
        let control_inputs = if let Some(names) = maneuver_names.filter(|_| surface.axis == 1) {
            inputs.extend(maneuver_inputs(
                &mut output,
                owner,
                instance + 0x100,
                definition + 0x138,
                &names,
            )?);
            2
        } else {
            0
        };
        surface_allocations.push(Row::input(0xecd1c696, control_inputs));
        output.0[definition + 0x130] = surface.axis;
        output.0[definition + 0x131] = 0;
        put(
            &mut output.0,
            definition + 0x198,
            &surface.area.to_le_bytes(),
        )?;
        put(&mut output.0, definition + 0x19c, &1f32.to_le_bytes())?;
        let (position, orientation) =
            super::super::seat::frame(bones, 0, (surface.position, surface.orientation))?;
        let name = super::super::name_hash(surface.name);
        let mut row = template.0[marker..marker + 64].to_vec();
        put(&mut row, 0, &[0; 8])?;
        for (axis, value) in orientation
            .into_iter()
            .chain(position)
            .chain([1.])
            .enumerate()
        {
            put(&mut row, 16 + axis * 4, &value.to_le_bytes())?;
        }
        put(&mut row, 0x30, &name.to_le_bytes())?;
        put(&mut output.0, definition + 0x54, &name.to_le_bytes())?;
        append(&mut output.0, definition + 0x60, 0x80809c00, &row, 64)?;
        markers.push(definition + 0x20);
        report.push(
            json!({"name":surface.name,"instance":instance,"definition":definition,
            "position":position,"orientation":orientation,"source_position_metres":surface.position,
            "source_orientation":surface.orientation,"axis":surface.axis,"area":surface.area,
            "lift_response":surface.lift.report(),"drag_coefficient":surface.drag}),
        );
    }
    let mut contacts = contact::finish(
        native,
        original,
        output,
        vec![Row::named(
            0x784b1b57,
            0x80803d56,
            count,
            Row::elements(surface_allocations),
        )],
        markers,
        json!({"surfaces":report,"anti_gravity_points":0,"wheel_points":0,"source_bounds_metres":[low,high],
            "gravity_scale":gravity,"reference_gravity_metres_per_second_squared":12.713999,
            "stability":plan.report,
            "maneuver_inputs":inputs,"maneuver_channel_names":maneuver_names,
            "gameplay_verified":false,
            "limits":["Authored aerodynamic response using the selected source dimensions and mass. Equilibrium calibration assumes the native base gravity. World gravity modifiers, collision, takeoff and handling need gameplay verification."]}),
    )?;
    contacts.inputs = inputs;
    Ok(contacts)
}
