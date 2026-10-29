//! Source wheel contacts with authored support distances and suspension response.
mod suspension;
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
    body: &super::body::Body,
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
    // Source friction is an input to a progressive force curve, not a native
    // Coulomb coefficient. Preserve it as an upper bound for this adaptation.
    let source_grip = number(controls, "ground_friction")?;
    let maximum_wheel_mass = wheels.iter().try_fold(0f32, |maximum, wheel| {
        Ok::<_, anyhow::Error>(maximum.max(number(wheel, "mass_source")?))
    })?;
    ensure!(maximum_wheel_mass > 0., "Source wheel masses are all zero");
    let model = &scene.models[0].model;
    let mut left = f32::NEG_INFINITY;
    let mut right = f32::INFINITY;
    let mut ground = f32::INFINITY;
    let mut positions = Vec::new();
    for wheel in wheels {
        let name = wheel["marker"].as_str().context("Source wheel marker")?;
        let (position, _) = seat::marker(model, name)?;
        left = left.max(position[1]);
        right = right.min(position[1]);
        ground = ground.min(wheel_bottom(model, wheel)?);
        positions.push(position);
    }
    let loads = suspension::loads(&positions, body.center)?;
    let half_track = (left - body.center[1]).min(body.center[1] - right);
    let height = body.center[2] - ground;
    ensure!(
        half_track > 0. && height > 0. && height.is_finite(),
        "Source wheeled body has no supported lateral balance"
    );
    let tipping_grip = half_track / height;
    // Native static friction reaches full strength at very small slip. Bound
    // that plateau below the selected body's flat-ground tipping threshold so
    // its tires can yield before lateral load transfer lifts the inner wheels.
    // This is a stability adaptation, not the Reach tire-force equation.
    let lateral_grip = source_grip.min(tipping_grip * 0.95);
    let template_grip = template.f32(td + 0xa8)?
        * template.f32(settings + 0x1c)?
        * template.f32(settings + 0x20)?;
    ensure!(
        source_grip > 0. && template_grip.is_finite() && template_grip > 0.,
        "Invalid vehicle tire friction calibration"
    );
    let grip_scale = lateral_grip / template_grip;
    ensure!(
        grip_scale.is_finite() && grip_scale > 0. && grip_scale <= 20.,
        "Source tire friction exceeds the supported native calibration"
    );
    for field in [0xa8, 0xac] {
        put(
            &mut output.0,
            d + field,
            &template.f32(td + field)?.to_le_bytes(),
        )?;
    }
    let slopes = [
        super::motion::number(controls, "maximum_slope_cosine_0")?,
        super::motion::number(controls, "maximum_slope_cosine_1")?,
    ];
    ensure!(
        slopes.iter().all(|v| (-1. ..=1.).contains(v)) && slopes[0] > slopes[1],
        "Source tire slope attenuation requires ordered distinct cosines"
    );
    // Reach 234D2A and native EA208D apply the same clamped cosine ramp.
    for (field, value) in [0xb4, 0xb8].into_iter().zip(slopes) {
        put(&mut output.0, d + field, &value.to_le_bytes())?;
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
    let travel = number(controls, "ground_depth_source")? * scene.metres();
    let source_damping = number(controls, "ground_damping")?;
    let damping_ratio = source_damping.min(0.85);
    let maximum_normal = number(controls, "maximum_normal_force_contribution")?;
    let gravity = number(controls, "gravity_scale")?;
    ensure!(
        travel > 0. && gravity > 0. && damping_ratio > 0. && maximum_normal > 1.,
        "Source wheel suspension values are invalid"
    );
    put(&mut output.0, d + 0xd4, &gravity.to_le_bytes())?;
    for (((&instance, &definition), wheel), &load) in
        instances.iter().zip(&definitions).zip(wheels).zip(&loads)
    {
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
        // Scale only lateral ratios. The common owner multipliers also affect
        // rolling and braking, so scaling those would couple unrelated forces.
        // Reach wheel masses retain their relative lateral weighting.
        let wheel_mass = number(wheel, "mass_source")?;
        let lateral_scale = wheel_mass / maximum_wheel_mass;
        for field in [0x20, 0x28, 0x34, 0x3c] {
            let ratio = template.f32(settings + field)?;
            ensure!(
                ratio.is_finite() && ratio >= 0.,
                "Invalid native lateral tire ratio"
            );
            put(
                &mut output.0,
                settings_at + field,
                &(ratio * grip_scale * lateral_scale).to_le_bytes(),
            )?;
        }
        put(
            &mut output.0,
            definition + 0x78,
            &(settings_at as i64 - (definition + 0x78) as i64).to_le_bytes(),
        )?;
        let radius = number(wheel, "radius_source")? * scene.metres();
        let slip_speed = number(wheel, "friction_velocity_source")? * scene.metres();
        let brake_slip_speed = number(wheel, "brake_friction_velocity_source")? * scene.metres();
        ensure!(
            slip_speed.is_finite() && brake_slip_speed.is_finite(),
            "Source wheel slip threshold exceeds native range"
        );
        // Reach 234D6B and native EA20D9 select the moving branch above this
        // speed. A zero threshold is valid and selects it for nonzero slip.
        for (field, value) in [(0x2c, slip_speed), (0x40, brake_slip_speed)] {
            put(&mut output.0, settings_at + field, &value.to_le_bytes())?;
        }
        let name = wheel["marker"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Source wheel marker")?;
        let source = seat::marker(&scene.models[0].model, name)?;
        let bottom = wheel_bottom(&scene.models[0].model, wheel)?;
        let height = source.0[2] - bottom;
        let reach = height + travel;
        ensure!(
            radius > 0. && height > 0. && reach.is_finite(),
            "Invalid wheel support distance"
        );
        // The native compression curve peaks at 1 + rest * stiffness times
        // supported weight. Honor the source static normal-force bound, and
        // use an authored damping ratio to avoid excessive bump impulses.
        let (rest, stiffness) = suspension::parameters(height, travel, load, maximum_normal)?;
        let (equilibrium_slope, damping) =
            suspension::damping(height, reach, rest, stiffness, load, damping_ratio, gravity)?;
        // Compression stiffness and damping are normalized against base gravity
        // by EB60C0. Extension support instead follows the selected gravity.
        let stiffness = stiffness * gravity;
        // The native spring supports one equal share of weight at +C. Offset
        // that distance so the visible tire height balances the actual body.
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
            "visible_contact_distance_metres":height,"static_weight_multiple":load,
            "stiffness_per_metre":stiffness,"damping_ratio":damping_ratio,"damping_seconds_per_metre":damping,
            "equilibrium_slope_per_metre":equilibrium_slope,"damping_mass_equal_share_multiple":load,
            "suspension_reach_metres":reach,"source_mass":wheel_mass,"lateral_mass_ratio":lateral_scale,
            "slip_threshold_metres_per_second":slip_speed,"brake_slip_threshold_metres_per_second":brake_slip_speed,
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
            "friction":{"source_ground_friction":source_grip,"native_template_lateral_static":template_grip,"multiplier":grip_scale,
                "lateral_static":lateral_grip,"static_tipping_coefficient":tipping_grip,"support_half_width_metres":half_track,"center_height_metres":height,
                "lateral_weighting":"source_wheel_mass_relative_to_maximum","maximum_source_wheel_mass":maximum_wheel_mass,
                "slope_attenuation_cosines":slopes,"grip_policy":"balance_bounded_lateral_only","tipping_reserve":0.95,"rolling_policy":"native_template_unchanged","source_force_curve_translated":false},
            "source_ground_damping":source_damping,"maximum_static_normal_weight_multiple":maximum_normal,
            "limits":["Support loads balance the selected center of mass at the visible tire height. Travel follows source geometry. Wheel slip thresholds and slope attenuation follow source controls. Normal lateral grip is bounded by source ground friction and 95 percent of the flat-ground tipping coefficient, with relative source wheel-mass weighting. Only lateral ratios are scaled. Rolling friction retains the native template. Absolute source force magnitudes, square-root coefficients and the progressive low-speed response are not translated, so this stability adaptation does not establish handling parity. Source damping is adapted to a native corner damping ratio capped at 0.85, using the equilibrium branch slope and supported corner mass. Whole-body coupling is not a set of independent corners. The tipping estimate does not bound dynamic terrain or suspension impulses. The normal-force bound applies to the static spring curve, not dynamic damping forces."]
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
