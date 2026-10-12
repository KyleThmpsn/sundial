//! Source wheel contacts with authored support distances and suspension response.
use super::super::seat;
use crate::{
    halo_reach::Scene,
    presentation::{append, put},
    tiger::{allocation::Row, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) struct Contacts {
    pub payload: Payload,
    pub allocation: Payload,
    pub allocation_template: u32,
    /// Definition offsets of marker consumers that take the body's bone provider.
    pub markers: Vec<usize>,
    /// Numeric definition offsets paired with private maneuver channel indices.
    pub inputs: Vec<(usize, usize)>,
    pub report: Value,
}

pub(super) fn pair(
    p: &mut Payload,
    owner: u32,
    i: usize,
    d: usize,
    classes: (u32, u32),
    parent: usize,
) -> Result<()> {
    for (at, class, target) in [(i, classes.1, d), (d, classes.0, i)] {
        put(&mut p.0, at, &owner.to_le_bytes())?;
        put(&mut p.0, at + 4, &class.to_le_bytes())?;
        put(&mut p.0, at + 8, &(target as u64).to_le_bytes())?;
    }
    put(
        &mut p.0,
        i + 16,
        &(parent as i64 - (i + 16) as i64).to_le_bytes(),
    )
}

fn number(value: &Value, name: &str) -> Result<f32> {
    let number = value[name]
        .as_f64()
        .with_context(|| format!("Source wheel lacks {name}"))? as f32;
    ensure!(
        number.is_finite() && number >= 0.,
        "Invalid source wheel {name}"
    );
    Ok(number)
}

fn wheel_bottom(model: &crate::halo_reach::model::Model, wheel: &Value) -> Result<f32> {
    let marker = seat::selected_marker(
        model,
        wheel["marker"].as_str().context("Source wheel marker")?,
    )?;
    // Physics damage-region names need not match render-model regions. A
    // region-scoped contact marker directly identifies the selected tire mesh.
    let region = if let Some(index) = marker.region {
        model.regions.get(index)
    } else {
        model
            .regions
            .iter()
            .find(|r| wheel["region"].as_str() == Some(r.name.as_str()))
    }
    .context("Source wheel render region is absent")?;
    let selected = model
        .selected
        .get(&region.name)
        .context("Source wheel selection is absent")?;
    let permutation = region
        .permutations
        .iter()
        .find(|p| &p.name == selected)
        .context("Source wheel permutation is absent")?;
    let mut bottom = f32::INFINITY;
    for primitive in model
        .primitives
        .iter()
        .filter(|p| permutation.sections.contains(&p.section))
    {
        for &index in &primitive.indices {
            let position = primitive
                .vertices
                .get(index as usize)
                .context("Source wheel vertex index")?
                .position;
            ensure!(
                position.iter().all(|v| v.is_finite()),
                "Nonfinite source wheel geometry"
            );
            bottom = bottom.min(position[2]);
        }
    }
    ensure!(
        bottom.is_finite(),
        "Selected source wheel has no drawn geometry"
    );
    Ok(bottom)
}

pub(super) fn build(
    native: &mut Reader,
    original: &Payload,
    scene: &Scene,
    bones: &Value,
) -> Result<Contacts> {
    let controls = &scene.object.gameplay["contacts"];
    let wheels = controls["wheels"]
        .as_array()
        .context("Source vehicle wheel contacts")?;
    ensure!(
        (2..=16).contains(&wheels.len()) && controls["anti_gravity_count"] == 0,
        "Wheeled translation requires a bounded friction-only source contact set"
    );
    let i = original.pointer(16)?;
    let d = original.pointer(24)?;
    let owner = original.u32(i)?;
    ensure!(
        original.u32(i - 4)? == 0x80803d4c && original.u32(d - 4)? == 0x80803d65,
        "Native vehicle contact implementation differs"
    );
    ensure!(
        original.u64(i + 0x50)? == 0 && original.u64(i + 0x60)? == 0,
        "Additional native contact families need explicit translation"
    );
    let template = native.tag(0x80bfc32a, Some(0x80809c36))?;
    let td = template.pointer(24)?;
    ensure!(
        template.u32(td - 4)? == 0x80803d65,
        "Native wheel template differs"
    );
    let rows = template.array(td + 0x130, 0x88, Some(0x80803d4f))?;
    let wd = *rows
        .first()
        .context("Native wheel template has no contacts")?;
    let wi = usize::try_from(template.u64(wd + 8)?)?;
    let settings = template.pointer(wd + 0x78)?;
    ensure!(
        template.u64(wd + 0x80)? == 0 && template.u64(wi + 0xe8)? == 0,
        "Native wheel has an unsupported optional response"
    );
    let marker_rows = template.array(wd + 0x60, 64, Some(0x80809c00))?;
    ensure!(marker_rows.len() == 1, "Native wheel marker shape differs");
    let mut output = original.clone();
    put(&mut output.0, i + 0x30, &[0; 16])?;
    put(&mut output.0, d + 0x120, &[0; 16])?;
    // The native tire consumer uses +1C * +20 for lateral static friction.
    // Calibrate that coefficient to the source ground friction while retaining
    // the native directional ratio and moving-friction transition shape.
    let source_grip = number(controls, "ground_friction")?;
    let template_grip = template.f32(td + 0xa8)?
        * template.f32(settings + 0x1c)?
        * template.f32(settings + 0x20)?;
    ensure!(
        source_grip > 0. && template_grip.is_finite() && template_grip > 0.,
        "Invalid vehicle tire friction calibration"
    );
    let grip_scale = source_grip / template_grip;
    ensure!(
        grip_scale.is_finite() && grip_scale > 0. && grip_scale <= 20.,
        "Source tire friction exceeds the supported native calibration"
    );
    for field in [0xa8, 0xac] {
        put(
            &mut output.0,
            d + field,
            &(template.f32(td + field)? * grip_scale).to_le_bytes(),
        )?;
    }
    append(
        &mut output.0,
        i + 0x40,
        0x80803d4e,
        &vec![0; wheels.len() * 0x140],
        0x140,
    )?;
    let instances = output.array(i + 0x40, 0x140, Some(0x80803d4e))?;
    append(
        &mut output.0,
        d + 0x130,
        0x80803d4f,
        &vec![0; wheels.len() * 0x88],
        0x88,
    )?;
    let definitions = output.array(d + 0x130, 0x88, Some(0x80803d4f))?;
    let mut report = Vec::new();
    let mut markers = Vec::new();
    let travel = number(controls, "ground_depth_source")? * super::METRES;
    let damping_ratio = number(controls, "ground_damping")?;
    let gravity = number(controls, "gravity_scale")?;
    ensure!(
        travel > 0. && gravity > 0. && damping_ratio > 0.,
        "Source wheel suspension values are invalid"
    );
    let stiffness = 1. / travel;
    let damping = 2. * damping_ratio * (stiffness / (12.713999 * gravity)).sqrt();
    put(&mut output.0, d + 0xd4, &gravity.to_le_bytes())?;
    for ((&instance, &definition), wheel) in instances.iter().zip(&definitions).zip(wheels) {
        output.0[instance..instance + 0x140].copy_from_slice(&template.0[wi..wi + 0x140]);
        output.0[definition..definition + 0x88].copy_from_slice(&template.0[wd..wd + 0x88]);
        pair(
            &mut output,
            owner,
            instance,
            definition,
            (0x80803d4e, 0x80803d4f),
            i,
        )?;
        pair(
            &mut output,
            owner,
            instance + 0x30,
            definition + 0x10,
            (0x80809663, 0x80809664),
            instance,
        )?;
        pair(
            &mut output,
            owner,
            instance + 0x50,
            definition + 0x20,
            (0x80809bd6, 0x80809bd7),
            instance + 0x30,
        )?;
        put(
            &mut output.0,
            instance + 0xa0,
            &(i as i64 - (instance + 0xa0) as i64).to_le_bytes(),
        )?;
        let flags = u32::try_from(wheel["flags"].as_u64().context("Source wheel flags")?)?;
        ensure!(flags & !0x7f == 0, "Unsupported source wheel flags");
        let native_flags = (flags >> 1) & 0x1f;
        put(
            &mut output.0,
            definition + 0x70,
            &native_flags.to_le_bytes(),
        )?;
        let settings_at = (output.0.len() + 11) & !7;
        output.0.resize(settings_at + 0x58, 0);
        put(&mut output.0, settings_at - 4, &0x80803d52u32.to_le_bytes())?;
        output.0[settings_at..settings_at + 0x58]
            .copy_from_slice(&template.0[settings..settings + 0x58]);
        put(
            &mut output.0,
            definition + 0x78,
            &(settings_at as i64 - (definition + 0x78) as i64).to_le_bytes(),
        )?;
        let radius = number(wheel, "radius_source")? * super::METRES;
        let name = wheel["marker"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Source wheel marker")?;
        let source = seat::marker(&scene.models[0].model, name)?;
        let bottom = wheel_bottom(&scene.models[0].model, wheel)?;
        let rest = source.0[2] - bottom;
        let reach = rest + travel;
        ensure!(
            radius > 0. && rest > 0. && reach.is_finite(),
            "Invalid wheel support distance"
        );
        // The native spring supports its share of weight at +C. Fit that
        // distance to the selected tire, then add declared source travel.
        // Its two damping fields are coefficients per unit supported weight.
        for (field, value) in [
            (8, reach),
            (12, rest),
            (16, stiffness),
            (20, damping),
            (24, damping),
        ] {
            put(&mut output.0, settings_at + field, &value.to_le_bytes())?;
        }
        let (position, rotation) = seat::frame(bones, 0, source)?;
        let hash = super::super::name_hash(name);
        let mut marker = template.0[marker_rows[0]..marker_rows[0] + 64].to_vec();
        put(&mut marker, 0, &[0; 8])?;
        for (index, value) in rotation.into_iter().chain(position).chain([1.]).enumerate() {
            put(&mut marker, 16 + index * 4, &value.to_le_bytes())?;
        }
        put(&mut marker, 0x30, &hash.to_le_bytes())?;
        put(&mut output.0, definition + 0x54, &hash.to_le_bytes())?;
        append(&mut output.0, definition + 0x60, 0x80809c00, &marker, 64)?;
        markers.push(definition + 0x20);
        report.push(json!({"source":name,"flags":native_flags,"radius_metres":radius,
            "rest_distance_metres":rest,"visible_bottom_metres":bottom,"travel_metres":travel,
            "stiffness_per_metre":stiffness,"damping_ratio":damping_ratio,"damping_seconds_per_metre":damping,
            "suspension_reach_metres":reach,"source_mass_not_translated":number(wheel,"mass_source")?,
            "source_position":source.0,"source_orientation":source.1,"position":position,"orientation":rotation,
            "instance":instance,"definition":definition,"settings":settings_at}));
    }
    finish(
        native,
        original,
        output,
        vec![Row::named(0xdab835fb, 0x80803d4e, wheels.len(), Vec::new())],
        markers,
        json!({
            "wheels":report,"anti_gravity_points":0,"gameplay_verified":false,
            "friction":{"source_ground_friction":source_grip,"native_template_lateral_static":template_grip,"multiplier":grip_scale},
            "limits":["Support distance follows the selected tire geometry. Source ground depth and damping set authored travel and damping. Lateral static friction follows the source ground coefficient while native directional and moving-friction ratios remain. This is not an exact reproduction of the source solver. Source contact damage and damaged-wheel behavior are not translated."]
        }),
    )
}

pub(super) fn finish(
    native: &mut Reader,
    original: &Payload,
    mut output: Payload,
    allocations: Vec<Row>,
    markers: Vec<usize>,
    mut report: Value,
) -> Result<Contacts> {
    let i = output.pointer(16)?;
    // These child callbacks initialize retained scalar notification values,
    // not antigravity simulation. Preserve them and the root's release method.
    let mut roots = 0;
    for init in output.array(48, 16, Some(0x808091a4))? {
        let target = output.pointer(init)?;
        let class = output.u32(init + 8)?;
        if target == i && class == 0x80803d4c {
            ensure!(
                output.u16(init + 12)? == 2 && output.u16(init + 14)? == 3,
                "Native physical lifecycle differs"
            );
            roots += 1;
        } else {
            ensure!(
                class == 0x80803d69
                    && output.u16(init + 12)? == 0
                    && output.u16(init + 14)? == u16::MAX
                    && target + 0x40 <= original.pointer(24)?
                    && output.u32(target + 4)? == 0x80803d6a,
                "Native contact initializer needs explicit translation"
            );
        }
    }
    ensure!(
        roots == 1,
        "Native physical lifecycle is absent or ambiguous"
    );
    let allocation_template = original.u32(0x44)?;
    let allocation = native.tag(allocation_template, Some(0x80809bbb))?;
    let mut allocation = Payload(
        allocation
            .0
            .get(..48)
            .context("Native contact allocation header")?
            .to_vec(),
    );
    Row::write(&allocations, &mut allocation, 32)?;
    let end = output.0.len();
    let span = crate::tiger::instance::seal_span(&mut output, &mut Vec::new(), end)?;
    report["instance_layout"] = json!(span);
    Ok(Contacts {
        payload: output,
        allocation,
        allocation_template,
        markers,
        inputs: Vec::new(),
        report,
    })
}
