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
    /// Ground sensors added for the source suspension overlays.
    pub suspension: Vec<Suspension>,
    pub report: Value,
}

/// A ground sensor added for one source suspension overlay: the definition offset of the numeric
/// provider reporting its phase, and the source overlay that phase plays.
pub(super) struct Suspension {
    pub provider: usize,
    pub label: String,
    pub clip: String,
}

/// Allocation row naming a contact owner's ground sensor instances, as in the stock tank's.
const SENSOR_ROW: u32 = 0x96262713;

/// Native ground sensors for the source suspension overlays (Reach `jmad +0x110`), one per row,
/// copied from the stock tank's 48 tread sensors. A sensor casts down from its marker. Its update
/// `00EAE7D0` stores the sampled distance at local value `+0x30` and the reported fraction at
/// `+0x34`, `1 - clamp((distance - near) / (far - near), 0, 1)`, with near and far at `+4` and `+8` of
/// the settings record the definition names at `+0x80`, and zero past the far distance. The tank's
/// record is a name with 0.4 and 0.8 metres, and its sensors sit 0.6 metres above the hull origin.
///
/// A Reach overlay runs from full extension at its first frame to full compression at its last,
/// by the ground depth below the row's marker between its two depths in source units. The
/// Warthog's rear overlay lowers its wheel 0.416 below the bind pose at the first frame, and the
/// bind height above ground, 0.199 source units, falls at phase 0.62 of the 0.116 to 0.335 range,
/// between the overlay's third and fourth frames, where the bind pose lies. So each sensor's range
/// runs from the compression depth to the extension depth and its fraction is the overlay phase.
#[expect(
    clippy::too_many_arguments,
    reason = "Ground sensors join the source rows, the template family and the rebuilt owner"
)]
fn ground_sensors(
    template: &Payload,
    output: &mut Payload,
    owner: u32,
    i: usize,
    d: usize,
    scene: &Scene,
    bones: &Value,
    rows: &[crate::halo_reach::animation::native::Suspension],
) -> Result<SuspensionRows> {
    if rows.is_empty() {
        return Ok(Default::default());
    }
    let ti = template.pointer(16)?;
    let td = template.pointer(24)?;
    let first = *template
        .array(td + 0x140, 0xa8, Some(0x80803d68))?
        .first()
        .context("Native ground sensor template is absent")?;
    let first_instance = usize::try_from(template.u64(first + 8)?)?;
    ensure!(
        template.u32(first + 4)? == 0x80803d67
            && template.u32(first_instance + 4)? == 0x80803d68
            && template.pointer(first_instance + 16)? == ti
            && template.u32(first + 0x74)? == 0x80803d69
            && template.u64(first + 0x78)? == (first_instance + 0xa0) as u64
            && template.pointer(first + 0x88)? == first + 0x70,
        "Native ground sensor template differs"
    );
    let markers = template.array(first + 0x60, 64, Some(0x80809c00))?;
    ensure!(
        markers.len() == 1,
        "Native ground sensor marker shape differs"
    );
    let marker_template = template.0[markers[0]..markers[0] + 64].to_vec();
    let initializer = template
        .array(48, 16, Some(0x808091a4))?
        .into_iter()
        .find(|&row| template.pointer(row).ok() == Some(first_instance + 0xa0))
        .context("Native ground sensor value initializer")?;
    let initializer = template.bytes::<16>(initializer)?;
    let count = rows.len();
    append(
        &mut output.0,
        i + 0x50,
        0x80803d67,
        &template.0[first_instance..first_instance + 0xe0].repeat(count),
        0xe0,
    )?;
    let instances = output.array(i + 0x50, 0xe0, Some(0x80803d67))?;
    append(
        &mut output.0,
        d + 0x140,
        0x80803d68,
        &template.0[first..first + 0xa8].repeat(count),
        0xa8,
    )?;
    let definitions = output.array(d + 0x140, 0xa8, Some(0x80803d68))?;
    let model = &scene.models[0].model;
    let mut markers = Vec::new();
    let mut values = Vec::new();
    let mut added = Vec::new();
    let mut report = Vec::new();
    let mut settings = Vec::new();
    for ((&instance, &definition), row) in instances.iter().zip(&definitions).zip(rows) {
        pair(
            output,
            owner,
            instance,
            definition,
            (0x80803d67, 0x80803d68),
            i,
        )?;
        pair(
            output,
            owner,
            instance + 0x20,
            definition + 0x10,
            (0x80809663, 0x80809664),
            instance,
        )?;
        pair(
            output,
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
        pair(
            output,
            owner,
            instance + 0xa0,
            definition + 0x70,
            (0x80803d69, 0x80803d6a),
            instance,
        )?;
        let source = seat::marker(model, &row.marker)?;
        let (mut position, _) = seat::frame(bones, 0, source)?;
        position[2] += row.mass_point_offset * scene.metres();
        let near = row.compression * scene.metres();
        let far = row.extension * scene.metres();
        let hash = super::super::name_hash(&row.marker);
        let mut marker = marker_template.clone();
        put(&mut marker, 0, &[0; 8])?;
        for (index, value) in position.into_iter().enumerate() {
            put(&mut marker, 32 + index * 4, &value.to_le_bytes())?;
        }
        put(&mut marker, 0x30, &hash.to_le_bytes())?;
        put(&mut output.0, definition + 0x54, &hash.to_le_bytes())?;
        append(&mut output.0, definition + 0x60, 0x80809c00, &marker, 64)?;
        let range = super::super::name_hash(&format!("suspension-range:{}", row.label));
        for word in [range, near.to_bits(), far.to_bits()] {
            settings.extend(word.to_le_bytes());
        }
        // The numeric provider's property name, which the bank input it feeds records.
        put(
            &mut output.0,
            definition + 0xa0,
            &super::super::name_hash(&format!("suspension:{}", row.label)).to_le_bytes(),
        )?;
        markers.push(definition + 0x20);
        values.push(instance + 0xa0);
        added.push(Suspension {
            provider: definition + 0x88,
            label: row.label.clone(),
            clip: row.clip.clone(),
        });
        report.push(
            json!({"label":row.label,"clip":row.clip,"marker":row.marker,
            "position":position,"compressed_distance_metres":near,"extended_distance_metres":far,
            "instance":instance,"definition":definition,"provider":definition + 0x88}),
        );
    }
    // The settings records are elements of the definition root's `80803D6C` array at `+0x110`, as
    // the tank's one record is. The loader's physical stream walker `00A40343` reads every word
    // between objects as a class, so a bare record there faulted at sign-in (Dawn 1.7.2,
    // 2026-11-06). Each sensor names its own element.
    ensure!(
        output.u64(d + 0x110)? == 0,
        "Native contact settings records need explicit translation"
    );
    append(&mut output.0, d + 0x110, 0x80803d6c, &settings, 12)?;
    let records = output.array(d + 0x110, 12, Some(0x80803d6c))?;
    for (&definition, &record) in definitions.iter().zip(&records) {
        put(
            &mut output.0,
            definition + 0x80,
            &(record as i64 - (definition + 0x80) as i64).to_le_bytes(),
        )?;
    }
    // Each sensor's local value starts from the template's initializer. Existing rows move with
    // the grown array, so their relative targets are written again.
    let old = output.array(48, 16, Some(0x808091a4))?;
    let mut targets = old
        .iter()
        .map(|&row| Ok((output.pointer(row)?, output.bytes::<8>(row + 8)?)))
        .collect::<Result<Vec<_>>>()?;
    targets.extend(values.iter().map(|&value| {
        let mut tail = [0; 8];
        tail.copy_from_slice(&initializer[8..]);
        (value, tail)
    }));
    let mut data = vec![0; targets.len() * 16];
    for (index, (_, tail)) in targets.iter().enumerate() {
        data[index * 16 + 8..index * 16 + 16].copy_from_slice(tail);
    }
    append(&mut output.0, 48, 0x808091a4, &data, 16)?;
    for (&row, &(target, _)) in output.array(48, 16, Some(0x808091a4))?.iter().zip(&targets) {
        put(
            &mut output.0,
            row,
            &(target as i64 - row as i64).to_le_bytes(),
        )?;
    }
    Ok((markers, values, added, report))
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

/// Source 234DEA multiplies slip by `0.033 * sqrt(0.9 * friction) * wheel mass` and the
/// contact normal force. That product is a friction coefficient per source unit of slip
/// speed. Above the slip threshold, 234D79 uses the same product with the moving friction
/// and the unit slip direction, a coefficient in the native moving form.
fn source_coefficient(friction: f32, wheel_mass: f32) -> f32 {
    0.033 * (0.9 * friction).sqrt() * wheel_mass
}

/// Native static and moving lateral coefficients for one source friction pair, with the
/// slip threshold in source units. Native static friction is flat from 0.1 m/s of slip, so
/// it takes the source force's mean over the static range. A zero threshold leaves only the
/// moving branch. Both are held below `bound`, the flat-ground tipping coefficient with a reserve.
/// In game, translated bodies whose sliding coefficient exceeded it rolled over where the source
/// stays on its wheels, while a body below it did not.
fn lateral(friction: f32, moving: f32, threshold: f32, wheel_mass: f32, bound: f32) -> (f32, f32) {
    let moving = source_coefficient(moving, wheel_mass);
    let mean = if threshold > 0. {
        source_coefficient(friction, wheel_mass) * threshold * 0.5
    } else {
        moving
    };
    (mean.min(bound), moving.min(bound))
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

/// One native template contact a source wheel takes: its instance and definition rows, settings
/// block, marker row and native flags.
struct Template {
    instance: usize,
    definition: usize,
    settings: usize,
    marker: usize,
    flags: Option<u32>,
}

fn template_row(template: &Payload, definition: usize, flags: Option<u32>) -> Result<Template> {
    let instance = usize::try_from(template.u64(definition + 8)?)?;
    ensure!(
        template.u64(definition + 0x80)? == 0 && template.u64(instance + 0xe8)? == 0,
        "Native wheel has an unsupported optional response"
    );
    let markers = template.array(definition + 0x60, 64, Some(0x80809c00))?;
    ensure!(markers.len() == 1, "Native wheel marker shape differs");
    Ok(Template {
        instance,
        definition,
        settings: template.pointer(definition + 0x78)?,
        marker: markers[0],
        flags,
    })
}

/// A tracked carrier's tread drive reads each contact's native flags, which mark the leading
/// contact of each track apart from the rest. Each source tread point takes the template contact
/// on its own side at the same rank from the front, with that contact's flags. Source points on the
/// centre line belong to neither track and are reported.
fn tracks(
    template: &Payload,
    rows: &[usize],
    model: &crate::halo_reach::model::Model,
    wheels: &[Value],
    center: [f32; 3],
) -> Result<(Vec<Value>, Vec<Template>, Vec<String>)> {
    let mut sources = Vec::new();
    for wheel in wheels {
        let name = wheel["marker"].as_str().context("Source wheel marker")?;
        sources.push((seat::marker(model, name)?.0, wheel));
    }
    let widest = sources
        .iter()
        .map(|(position, _)| (position[1] - center[1]).abs())
        .fold(0f32, f32::max);
    ensure!(
        widest > 0.,
        "Source tracked contacts have no lateral spread"
    );
    let mut centre_line = Vec::new();
    let mut sides: [Vec<([f32; 3], &Value)>; 2] = [Vec::new(), Vec::new()];
    for (position, wheel) in sources {
        let lateral = position[1] - center[1];
        if lateral.abs() < widest * 0.1 {
            centre_line.push(wheel["marker"].as_str().unwrap_or_default().to_owned());
        } else {
            sides[usize::from(lateral < 0.)].push((position, wheel));
        }
    }
    let mut native: [Vec<([f32; 3], usize)>; 2] = [Vec::new(), Vec::new()];
    for &row in rows {
        let markers = template.array(row + 0x60, 64, Some(0x80809c00))?;
        ensure!(markers.len() == 1, "Native track marker shape differs");
        let position: [f32; 3] = std::array::from_fn(|axis| {
            template
                .f32(markers[0] + 0x20 + axis * 4)
                .unwrap_or(f32::NAN)
        });
        ensure!(
            position.iter().all(|v| v.is_finite()) && position[1] != 0.,
            "Native track contact lies on the centre line"
        );
        native[usize::from(position[1] < 0.)].push((position, row));
    }
    let mut selected = Vec::new();
    let mut templates = Vec::new();
    for (source, native) in sides.iter_mut().zip(native.iter_mut()) {
        ensure!(
            source.len() == native.len(),
            "Source track has {} contacts where the native track has {}",
            source.len(),
            native.len()
        );
        source.sort_by(|a, b| b.0[0].total_cmp(&a.0[0]));
        native.sort_by(|a, b| b.0[0].total_cmp(&a.0[0]));
        for ((_, wheel), &(_, row)) in source.iter().zip(native.iter()) {
            selected.push((*wheel).clone());
            templates.push(template_row(
                template,
                row,
                Some(template.u32(row + 0x70)?),
            )?);
        }
    }
    Ok((selected, templates, centre_line))
}

/// The tank carrier's second contact family holds its tread ground sensors (definition rows
/// `80803D68`, `0xA8` bytes), each a marker and a local value its presentation bank reads to animate
/// the tread wheels. They carry no support, so a tracked or stationary source on that carrier keeps
/// them as they are, with their marker inputs at row `+0x20` kept connected. Returns their rows, or
/// none when the carrier has no such family. Any other family needs its own translation.
fn tread_sensors(original: &Payload) -> Result<Vec<usize>> {
    let i = original.pointer(16)?;
    let d = original.pointer(24)?;
    ensure!(
        original.u64(i + 0x60)? == 0 && original.u64(d + 0x150)? == 0,
        "Additional native contact families need explicit translation"
    );
    if original.u64(i + 0x50)? == 0 {
        return Ok(Vec::new());
    }
    let sensors = original.array(d + 0x140, 0xa8, Some(0x80803d68))?;
    ensure!(
        !sensors.is_empty() && original.u64(i + 0x50)? == sensors.len() as u64,
        "Additional native contact families need explicit translation"
    );
    for &sensor in &sensors {
        ensure!(
            original.u32(sensor + 0x24)? == 0x80809bd6,
            "Native tread sensor marker input differs"
        );
    }
    Ok(sensors)
}

/// The share of a body's static flat-ground tipping coefficient its lateral grip may reach.
const TIPPING_RESERVE: f32 = 0.7;

/// Native tire row flags are the source friction point flags shifted down one bit: powered at
/// bit 0, front turning at bit 1 and rear turning at bit 2.
const FRONT_TURNING: u32 = 1 << 1;
const REAR_TURNING: u32 = 1 << 2;

/// For a source that steers only its rear wheels, whether each wheel sits ahead of the wheel
/// centre and so steers natively. No stock carrier sets the native rear-turning bit, and a source
/// carrying it on its rear rows steered the wrong way (Forklift, Dawn 1.7.2, 2026-11-06). Such a
/// source steers its front axle instead, so it turns the way its driver steers, about its rear
/// axle rather than its front. `None` for every wheel keeps the source steering as it is.
fn steered_axle(scene: &Scene, wheels: &[Value]) -> Result<Vec<Option<bool>>> {
    let flags = wheels
        .iter()
        .map(|wheel| {
            let flags = wheel["flags"].as_u64().context("Source wheel flags")?;
            Ok((flags >> 1) as u32)
        })
        .collect::<Result<Vec<_>>>()?;
    let rear_only =
        flags.iter().all(|f| f & FRONT_TURNING == 0) && flags.iter().any(|f| f & REAR_TURNING != 0);
    if !rear_only {
        return Ok(vec![None; wheels.len()]);
    }
    let ahead = wheels
        .iter()
        .map(|wheel| {
            let name = wheel["marker"]
                .as_str()
                .filter(|s| !s.is_empty())
                .context("Source wheel marker")?;
            Ok(seat::marker(&scene.models[0].model, name)?.0[0])
        })
        .collect::<Result<Vec<f32>>>()?;
    let centre = ahead.iter().sum::<f32>() / ahead.len() as f32;
    Ok(ahead.into_iter().map(|x| Some(x > centre)).collect())
}

pub(super) fn build(
    native: &mut Reader,
    original: &Payload,
    scene: &Scene,
    bones: &Value,
    body: &super::body::Body,
    tracked: bool,
    suspension: &[crate::halo_reach::animation::native::Suspension],
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
    let ContactTemplates {
        sensors,
        template,
        definition: td,
        rows,
    } = contact_templates(native, original, i, tracked)?;
    let model = &scene.models[0].model;
    let (wheels, templates, centre_line) =
        source_contacts(&template, &rows, model, wheels, body.center, tracked)?;
    let wheels = &wheels;
    let mut output = original.clone();
    put(&mut output.0, i + 0x30, &[0; 16])?;
    put(&mut output.0, d + 0x120, &[0; 16])?;
    let source_grip = number(controls, "ground_friction")?;
    let source_moving_grip = number(controls, "ground_moving_friction")?;
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
    // Native static friction reaches full strength at 0.1 m/s of slip. Bound
    // that plateau below the selected body's flat-ground tipping threshold so
    // its tires can yield before lateral load transfer lifts the inner wheels.
    // The threshold is static. Suspension roll, bumps and turn-in overshoot tip a body
    // sooner, and a Mongoose held at 0.95 of it still rolled easily in game (2026-11-06).
    let static_bound = tipping_grip * TIPPING_RESERVE;
    let multipliers = (template.f32(td + 0xa8)?, template.f32(td + 0xac)?);
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
    let steering = steered_axle(scene, wheels)?;
    for (((((&instance, &definition), wheel), &load), row), steered) in instances
        .iter()
        .zip(&definitions)
        .zip(wheels)
        .zip(&loads)
        .zip(&templates)
        .zip(&steering)
    {
        let (wi, wd, settings) = (row.instance, row.definition, row.settings);
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
        let native_flags = row.flags.unwrap_or(match steered {
            Some(true) => ((flags >> 1) & 0x1f & !REAR_TURNING) | FRONT_TURNING,
            Some(false) => (flags >> 1) & 0x1f & !REAR_TURNING,
            None => (flags >> 1) & 0x1f,
        });
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
        // Only lateral ratios change. The owner multipliers and rolling
        // coefficients also govern traction and braking.
        let wheel_mass = number(wheel, "mass_source")?;
        ensure!(wheel_mass > 0., "Source wheel mass must be positive");
        let normal = lateral(
            source_grip,
            source_moving_grip,
            number(wheel, "friction_velocity_source")?,
            wheel_mass,
            static_bound,
        );
        let braking = [
            "brake_friction",
            "brake_moving_friction",
            "brake_friction_velocity_source",
        ]
        .iter()
        .map(|key| number(wheel, key))
        .collect::<Result<Vec<_>>>()?;
        // A wheel without source handbrake friction keeps its normal response.
        let mut handbrake = if braking.iter().all(|&value| value == 0.) {
            normal
        } else {
            lateral(braking[0], braking[1], braking[2], wheel_mass, static_bound)
        };
        let mut normal = normal;
        // A tracked carrier keeps its native tread drive and steering, which skid the hull round
        // against the template row's own lateral friction: on the tank, 1.2 static and 0.5 moving
        // in front, 2.4 and 0.268 behind. Source grip held the Scorpion's treads at 1.25 while
        // sliding, and it would not turn (Dawn 1.7.2, 2026-11-06). Its rows keep the template
        // row's lateral set and slip thresholds, so the native drive steers as it does the tank.
        if tracked {
            let native =
                |field: usize, rolling: usize, multiplier: f32| -> Result<f32> {
                    Ok(template.f32(settings + field)?
                        * template.f32(settings + rolling)?
                        * multiplier)
                };
            normal = (
                native(0x20, 0x1c, multipliers.0)?,
                native(0x28, 0x24, multipliers.1)?,
            );
            handbrake = (
                native(0x34, 0x30, multipliers.0)?,
                native(0x3c, 0x38, multipliers.1)?,
            );
        } else {
            for (field, rolling, multiplier, value) in [
                (0x20, 0x1c, multipliers.0, normal.0),
                (0x28, 0x24, multipliers.1, normal.1),
                (0x34, 0x30, multipliers.0, handbrake.0),
                (0x3c, 0x38, multipliers.1, handbrake.1),
            ] {
                let base = multiplier * template.f32(settings + rolling)?;
                let ratio = value / base;
                ensure!(
                    base.is_finite()
                        && base > 0.
                        && ratio.is_finite()
                        && (0.0..=20.).contains(&ratio),
                    "Source tire friction exceeds the supported native calibration"
                );
                put(&mut output.0, settings_at + field, &ratio.to_le_bytes())?;
            }
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
        if !tracked {
            for (field, value) in [(0x2c, slip_speed), (0x40, brake_slip_speed)] {
                put(&mut output.0, settings_at + field, &value.to_le_bytes())?;
            }
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
        let (position, _) = seat::frame(bones, 0, source)?;
        // A native contact rolls along its row frame's x axis and casts down its z axis, and the
        // stock tank's rows all keep the vehicle frame. A Reach wheel marker points along its axle,
        // yawed 90 degrees, and rows carrying that frame drove sideways (Forklift, Dawn 1.7.2,
        // 2026-11-06: motion 85 to 90 degrees off its heading). Every row keeps the vehicle frame.
        let rotation = [0f32, 0., 0., 1.];
        let hash = super::super::name_hash(name);
        let mut marker = template.0[row.marker..row.marker + 64].to_vec();
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
            "suspension_reach_metres":reach,"source_mass":wheel_mass,
            "lateral_static":normal.0,"lateral_moving":normal.1,
            "handbrake_lateral_static":handbrake.0,"handbrake_lateral_moving":handbrake.1,
            "slip_threshold_metres_per_second":slip_speed,"brake_slip_threshold_metres_per_second":brake_slip_speed,
            "source_position":source.0,"source_orientation":source.1,"position":position,"orientation":rotation,
            "instance":instance,"definition":definition,"settings":settings_at}));
    }
    markers.extend(sensors.iter().map(|sensor| sensor + 0x20));
    // A tracked carrier keeps its own tread sensors, so only a wheeled one takes ground sensors
    // for the source suspension overlays.
    let (sensor_markers, values, added, ground_report) = if tracked {
        Default::default()
    } else {
        ground_sensors(
            &template,
            &mut output,
            owner,
            i,
            d,
            scene,
            bones,
            suspension,
        )?
    };
    markers.extend(sensor_markers);
    let mut allocations = vec![Row::named(0xdab835fb, 0x80803d4e, wheels.len(), Vec::new())];
    allocations.extend(
        (!added.is_empty()).then(|| Row::named(SENSOR_ROW, 0x80803d67, added.len(), Vec::new())),
    );
    let untranslated_suspension = usize::from(tracked) * suspension.len();
    let mut contacts = finish(
        native,
        original,
        output,
        allocations,
        markers,
        &values,
        json!({
            "wheels":report,"anti_gravity_points":0,"gameplay_verified":false,
            "suspension_sensors":ground_report,
            "untranslated_suspension":untranslated_suspension,
            "retained_tread_sensors":sensors.len(),
            "tracked":tracked,"untranslated_centre_line_points":centre_line,
            "friction":{"source_ground_friction":source_grip,"source_ground_moving_friction":source_moving_grip,
                "static_bound":static_bound,"static_tipping_coefficient":tipping_grip,"support_half_width_metres":half_track,"center_height_metres":height,
                "slope_attenuation_cosines":slopes,"grip_policy":"source_moving_and_static_mean_bounded_by_tipping","tipping_reserve":TIPPING_RESERVE,"rolling_policy":"native_template_unchanged","source_force_curve_translated":"moving_branch"},
            "source_ground_damping":source_damping,"maximum_static_normal_weight_multiple":maximum_normal,
            "limits":["Support loads balance the selected center of mass at the visible tire height. Travel follows source geometry. Wheel slip thresholds and slope attenuation follow source controls. Lateral moving coefficients equal the source moving force per unit normal force. Native static friction is flat above 0.1 m/s of slip while the source force grows with slip, so each static coefficient is the source mean over its static slip range. Both are bounded by 70 percent of the flat-ground tipping coefficient, so tires slide before the body rolls. Where the bound applies, cornering grip is lower than the source force. Handbrake wheels use their source brake friction the same way. Rolling friction retains the native template. The step between static and moving friction does not reproduce the source's continuous growth, so this does not establish handling parity. Source damping is adapted to a native corner damping ratio capped at 0.85, using the equilibrium branch slope and supported corner mass. Whole-body coupling is not a set of independent corners. The tipping estimate does not bound dynamic terrain or suspension impulses. The normal-force bound applies to the static spring curve, not dynamic damping forces.",
                "Each source suspension row takes a ground sensor copied from the stock tank's tread sensors at its marker, reporting one at the compression depth and zero at the extension depth. Wheel contact with the ground needs a gameplay check. Destroyed-region suspension depths are not translated."]
        }),
    )?;
    contacts.suspension = added;
    Ok(contacts)
}

/// A source with turret physics only and no friction or antigravity points rests on its
/// collision. Its carrier keeps no tire rows, so its native drive has nothing to push with. The
/// native tire update `00EA17D0` skips its loop and its traction count when no row is enabled.
pub(super) fn stationary(
    native: &mut Reader,
    original: &Payload,
    scene: &Scene,
) -> Result<Contacts> {
    let controls = &scene.object.gameplay["contacts"];
    ensure!(
        controls["wheels"].as_array().is_none_or(Vec::is_empty)
            && controls["anti_gravity_count"] == 0,
        "A stationary source has no contact points"
    );
    let i = original.pointer(16)?;
    let d = original.pointer(24)?;
    ensure!(
        original.u32(i - 4)? == 0x80803d4c && original.u32(d - 4)? == 0x80803d65,
        "Native vehicle contact implementation differs"
    );
    let sensors = tread_sensors(original)?;
    let mut output = original.clone();
    put(&mut output.0, i + 0x30, &[0; 16])?;
    put(&mut output.0, d + 0x120, &[0; 16])?;
    append(&mut output.0, i + 0x40, 0x80803d4e, &[], 0x140)?;
    append(&mut output.0, d + 0x130, 0x80803d4f, &[], 0x88)?;
    finish(
        native,
        original,
        output,
        vec![Row::named(0xdab835fb, 0x80803d4e, 0, Vec::new())],
        sensors.iter().map(|sensor| sensor + 0x20).collect(),
        &[],
        json!({"wheels":[],"anti_gravity_points":0,"stationary":true,"gameplay_verified":false,
            "retained_tread_sensors":sensors.len(),
            "limits":["The source has turret physics and no contact points, so the body rests on its source collision. The carrier's drive has no tire rows. Native behavior with no rows comes from the tire update's guarded loop, and the carrier's tread drive with no rows still needs a gameplay check."]}),
    )
}

pub(super) fn finish(
    native: &mut Reader,
    original: &Payload,
    mut output: Payload,
    allocations: Vec<Row>,
    markers: Vec<usize>,
    values: &[usize],
    mut report: Value,
) -> Result<Contacts> {
    let i = output.pointer(16)?;
    // These child callbacks initialize retained scalar notification values, and the local values
    // of added ground sensors, not antigravity simulation. Preserve them and the root's release
    // method. Retained values lie in the original instance region, added ones in `values`.
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
                    && (target + 0x40 <= original.pointer(24)? || values.contains(&target))
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
    // Retained families, such as a tracked carrier's tread sensors, keep their allocation rows.
    // Only the rebuilt tire row is replaced, so the carrier's own tire row must be the one named.
    let retained = report["retained_tread_sensors"]
        .as_u64()
        .is_some_and(|count| count > 0);
    let mut allocation = if retained {
        let full = Payload(allocation.0.clone());
        let tires = full
            .array(32, 40, Some(0x80808852))?
            .into_iter()
            .filter(|&row| full.u32(row + 16).is_ok_and(|class| class == 0x80803d4e))
            .map(|row| full.u32(row))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            tires == [0xdab835fb],
            "Native tracked contact allocation names its tire rows differently"
        );
        full
    } else {
        Payload(
            allocation
                .0
                .get(..48)
                .context("Native contact allocation header")?
                .to_vec(),
        )
    };
    if retained {
        Row::replace(&allocations, &mut allocation, 32, &[0xdab835fb])?;
    } else {
        Row::write(&allocations, &mut allocation, 32)?;
    }
    let end = output.0.len();
    let span = crate::tiger::instance::seal_span(&mut output, &mut Vec::new(), end)?;
    report["instance_layout"] = json!(span);
    Ok(Contacts {
        payload: output,
        allocation,
        allocation_template,
        markers,
        inputs: Vec::new(),
        suspension: Vec::new(),
        report,
    })
}

type SuspensionRows = (Vec<usize>, Vec<usize>, Vec<Suspension>, Vec<Value>);

struct ContactTemplates {
    sensors: Vec<usize>,
    template: std::sync::Arc<Payload>,
    definition: usize,
    rows: Vec<usize>,
}

fn contact_templates(
    native: &mut Reader,
    original: &Payload,
    i: usize,
    tracked: bool,
) -> Result<ContactTemplates> {
    let sensors = if tracked {
        tread_sensors(original)?
    } else {
        ensure!(
            original.u64(i + 0x50)? == 0 && original.u64(i + 0x60)? == 0,
            "Additional native contact families need explicit translation"
        );
        Vec::new()
    };
    let template = native.tag(0x80bfc32a, Some(0x80809c36))?;
    let td = template.pointer(24)?;
    ensure!(
        template.u32(td - 4)? == 0x80803d65,
        "Native wheel template differs"
    );
    let rows = template.array(td + 0x130, 0x88, Some(0x80803d4f))?;

    Ok(ContactTemplates {
        sensors,
        template,
        definition: td,
        rows,
    })
}

type WheelContacts = (Vec<Value>, Vec<Template>, Vec<String>);

fn source_contacts(
    template: &Payload,
    rows: &[usize],
    model: &crate::halo_reach::model::Model,
    wheels: &[Value],
    center: [f32; 3],
    tracked: bool,
) -> Result<WheelContacts> {
    if tracked {
        return tracks(template, rows, model, wheels, center);
    }
    let first = *rows
        .first()
        .context("Native wheel template has no contacts")?;
    let templates = (0..wheels.len())
        .map(|_| template_row(template, first, None))
        .collect::<Result<Vec<_>>>()?;
    Ok((wheels.to_vec(), templates, Vec::new()))
}
