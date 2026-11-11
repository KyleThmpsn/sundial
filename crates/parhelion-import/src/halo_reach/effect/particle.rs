//! Source billboard emitters, with source simulation and material programs.
pub(in crate::halo_reach) mod cheap;
mod material;
pub(in crate::halo_reach) mod model;
mod simulation;
use super::property::{Expr, Property};
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        material::Material,
        native,
        resource::Pages,
    },
    presentation::{Graph, put},
    tiger::{payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(in crate::halo_reach) struct Emitter {
    pub source: Tag,
    pub name: String,
    pub life: f32,
    pub definition: Payload,
    material: Material,
    appearance: u32,
    animation: u32,
    /// The source billboard orientation: 0 and 1 face the screen, 2 lies along the velocity.
    orientation: i16,
    center: [f32; 2],
    scroll: [f32; 2],
    frames: Vec<[f32; 4]>,
    near_fade: [f32; 2],
    /// The distance in meters over which the particle fades in front of the scene, zero without
    /// a depth fade.
    depth_fade: f32,
    /// The source coordinate system: 0 world space, 1 local space and 2 the parent's space.
    coordinates: i64,
    /// Whether the definition stores its particles in the frame of a second locator binding.
    framed: bool,
    /// The source system's environment, disposition and camera mode.
    conditions: Value,
    limits: Vec<String>,
}

impl Emitter {
    /// Gives a world-space source system's particles the frame of a second locator binding and
    /// returns whether it did. The caller then binds the system's event to the emitter's locator
    /// and a `world_space` locator, in that order, as stock trails bind them.
    ///
    /// Native system update reads definition `+0x40` as the binding row of the emitter's frame
    /// and `+0x41` as the row of the frame its particles are stored in. Spawn places each particle
    /// by the emitter's frame relative to the stored frame, and the top byte of `+0x44` counts the
    /// rows. The template keeps both rows on the emitter, so its particles follow the emitter,
    /// which is source local space.
    pub(in crate::halo_reach) fn bind_world(&mut self) -> Result<bool> {
        if self.coordinates != 0 {
            return Ok(false);
        }
        let bytes = &mut self.definition.0;
        ensure!(
            bytes[0x40] == 0 && bytes[0x41] == 0 && bytes[0x47] == 1,
            "Native billboard definition bindings differ"
        );
        bytes[0x41] = 1;
        bytes[0x47] = 2;
        self.framed = true;
        Ok(true)
    }
}

fn values<const N: usize>(value: &Value, field: &str) -> Result<[f32; N]> {
    let rows = value[field]
        .as_array()
        .filter(|rows| rows.len() == N)
        .with_context(|| format!("Source emitter {field}"))?;
    let mut result = [0.; N];
    for (out, row) in result.iter_mut().zip(rows) {
        *out = row.as_f64().context("Source emitter number")? as f32;
        ensure!(out.is_finite(), "Nonfinite source emitter value");
    }
    Ok(result)
}

/// What a source system's creation and view conditions mean on a converted host: why it stays
/// pending, empty when it converts, and what the converted system does differently.
pub(in crate::halo_reach) struct Conditions {
    pub reasons: Vec<String>,
    pub limits: Vec<String>,
    /// The source environment, disposition and camera mode.
    pub report: Value,
}

/// Translates the environment `+0x1A`, disposition `+0x1C` and camera mode `+0x1E` of `system`
/// for a host whose effect has no biped parent: a projectile in flight, a contact, a detonation,
/// a vehicle's damage effect or a weapon's firing effect. Only the firing effect has a
/// first-person parent, and the firing flow places camera modes through its view groups and
/// passes camera mode 0.
///
/// Reach's `event_create_particle_systems` creates a system only when
/// `effect_allowed_by_environment` admits its environment against the effect's state word `+4`.
/// Bit 23 records that the effect's first location lay in water when it started, from
/// `scenario_position_underwater`. Only an effect whose definition sets flag bit 14 runs that
/// test, and the caches set it exactly on effects with an air-only system. Bit 24 records an
/// impact on a wet surface. Bit 4 marks an effect parented to a biped that died more than a
/// second earlier. With it Reach leaves out disposition 1, violent mode only, and without it
/// disposition 2, nonviolent mode only. Camera mode 1 is created only for an effect with a parent
/// object, and `c_particle_system::calculate_pass` draws it only in the first-person view of the
/// player whose unit or held weapon parents the effect. Mode 2 is left out of that view alone.
///
/// Native effect hosts have no submersion or wetness state. An air-only or dry-only system
/// therefore converts with a limit, and a water-only or wet-only system stays pending. Other
/// environment, disposition or camera values are invalid data and fail the import.
pub(in crate::halo_reach) fn conditions(system: &Value) -> Result<Conditions> {
    let field = |name: &str| {
        system[name]
            .as_i64()
            .context("Source particle system condition")
    };
    let (environment, disposition, camera) = (
        field("environment")?,
        field("disposition")?,
        field("camera")?,
    );
    let mut result = Conditions {
        reasons: Vec::new(),
        limits: Vec::new(),
        report: json!({"environment":environment,"disposition":disposition,"camera":camera}),
    };
    match environment {
        0 => {}
        1 => result.limits.push("Reach creates this air-only system only when the effect's first location is out of water as it starts. Native effect hosts have no submersion state, so it also draws where water would have left it out.".into()),
        2 => result.reasons.push("Water-only particle system. Reach creates it only when the effect's first location is in water as it starts, and native effect hosts have no submersion state".into()),
        3 => result.reasons.push("Space-only particle system. Reach's environment test admits it in no effect state".into()),
        4 => result.reasons.push("Wet-only particle system. Reach creates it only for an impact on a wet surface, and native contact hosts have no wetness state".into()),
        5 => result.limits.push("Reach leaves this dry-only system out of an impact on a wet surface. Native contact hosts have no wetness state, so it also draws there.".into()),
        other => anyhow::bail!("Unknown source particle environment {other}"),
    }
    match disposition {
        0 | 1 => {}
        2 => result.reasons.push("Nonviolent-only particle system. Reach creates it only for an effect parented to a biped that died more than a second earlier, and this effect has no biped parent".into()),
        other => anyhow::bail!("Unknown source particle disposition {other}"),
    }
    match camera {
        0 | 2 | 3 => {}
        1 => result.reasons.push("First-person-only particle system. Reach draws it only in the first-person view of the player whose unit or held weapon parents the effect, and this effect has neither".into()),
        other => anyhow::bail!("Unknown source particle camera mode {other}"),
    }
    Ok(result)
}

/// Recognized source contracts that still require another native adapter.
/// Invalid data continues to fail the import instead of becoming an exclusion.
pub(in crate::halo_reach) fn pending(
    cache: &Cache,
    system: &Value,
    emitter: &Value,
) -> Result<Vec<String>> {
    let source: Tag = serde_json::from_value(system["tag"].clone())?;
    ensure!(source.group == "prt3", "Source emitter has no particle");
    let at = source.address()?;
    let emitter_at = usize::try_from(
        emitter["offset"]
            .as_u64()
            .context("Source emitter address")?,
    )?;
    let mut result = Vec::new();
    if !cache.block(at + 4, 0x14)?.is_empty() {
        result.push("Particle attachment events".into());
    }
    if cache.reference(at + 0x1bc, None)?.is_some() || !matches!(cache.i16(at + 0x14)?, 0..=2) {
        result.push("Model particles or another billboard orientation".into());
    }
    if cache.u8(emitter_at + 4)? > 5
        || cache.reference(emitter_at + 0xc, None)?.is_some()
        || cache.reference(emitter_at + 0x1c, None)?.is_some()
    {
        result.push("Custom emitter geometry".into());
    }
    if cache.floats::<6>(emitter_at + 0xa0)? != [0.; 6] {
        result.push("Emitter direction interpolation".into());
    }
    // Coordinate system 0 is world space, 1 local space and 2 the parent's space.
    ensure!(
        matches!(system["coordinates"].as_i64(), Some(0..=2)),
        "Unknown source particle coordinate system"
    );
    result.extend(conditions(system)?.reasons);
    if cache.u32(at + 0x10)? & 0x28000 != 0 {
        result.push("Distortion or model rendering".into());
    }
    let material = crate::halo_reach::material::embedded(cache, source, at + 0x40)?;
    if let Some(reason) = material::pending(&material) {
        result.push(reason.into());
    }
    let kinds = cache
        .block(emitter_at + 0x1ac, 0x18)?
        .into_iter()
        .map(|row| cache.i16(row))
        .collect::<Result<Vec<_>>>()?;
    // Controllers other than physics are left out with a limit when the emitter converts.
    ensure!(
        kinds.iter().all(|kind| (0..=5).contains(kind)),
        "Unknown source particle movement controller"
    );
    Ok(result)
}

/// The curves of the emitter at `emitter_at` and its particle at `at`, by the names the native
/// program reads.
fn properties(
    cache: &Cache,
    at: usize,
    emitter_at: usize,
) -> Result<BTreeMap<&'static str, Property>> {
    let mut properties = BTreeMap::new();
    for (name, offset) in [
        ("offset", 0x48),
        ("radius", 0xb8),
        ("angle", 0xd8),
        ("axis_angle", 0xf8),
        ("starting_count", 0x118),
        ("maximum_count", 0x138),
        ("rate", 0x158),
        ("life", 0x178),
        ("acceleration", 0x1c8),
        ("velocity", 0x200),
        ("rotation", 0x220),
        ("rotation_rate", 0x240),
        ("size", 0x260),
        ("scale", 0x280),
        ("tint", 0x2a0),
        ("alpha", 0x2c0),
        ("black_point", 0x2e0),
    ] {
        properties.insert(name, Property::read(cache, emitter_at + offset)?);
    }
    for (name, offset) in [
        ("aspect", 0xd8),
        ("color", 0xf8),
        ("intensity", 0x118),
        ("particle_alpha", 0x138),
        ("frame", 0x15c),
        ("animation_rate", 0x17c),
        ("palette", 0x19c),
    ] {
        properties.insert(name, Property::read(cache, at + offset)?);
    }
    Ok(properties)
}

/// The longest life the emitter's particles can take, in seconds.
fn life(properties: &BTreeMap<&'static str, Property>) -> Result<f32> {
    let life = properties["life"]
        .bounds()?
        .into_iter()
        .fold(0f32, f32::max);
    ensure!(
        life.is_finite() && life > 0. && life <= 120.,
        "Source particle lifetime exceeds the supported finite envelope"
    );
    Ok(life)
}

pub(in crate::halo_reach) fn read(
    cache: &Cache,
    native: &mut Reader,
    system: &Value,
    emitter: &Value,
    metres: f32,
    frame: &[f32; 16],
) -> Result<Emitter> {
    let source: Tag = serde_json::from_value(system["tag"].clone())?;
    ensure!(source.group == "prt3", "Source emitter has no particle");
    let at = source.address()?;
    let emitter_at = usize::try_from(
        emitter["offset"]
            .as_u64()
            .context("Source emitter address")?,
    )?;
    ensure!(
        cache.block(at + 4, 0x14)?.is_empty(),
        "Source particle attachments need event translation"
    );
    ensure!(
        cache.reference(at + 0x1bc, None)?.is_none(),
        "Source model particles need model geometry"
    );
    let orientation = cache.i16(at + 0x14)?;
    ensure!(
        (0..=2).contains(&orientation),
        "Source particle orientation needs another billboard basis"
    );
    ensure!(
        cache.u8(emitter_at + 4)? <= 5
            && cache.reference(emitter_at + 0xc, None)?.is_none()
            && cache.reference(emitter_at + 0x1c, None)?.is_none(),
        "Source emitter shape needs a geometry adapter"
    );
    ensure!(
        cache.floats::<6>(emitter_at + 0xa0)? == [0.; 6],
        "Source emitter direction interpolation needs a local frame adapter"
    );
    let coordinates = system["coordinates"]
        .as_i64()
        .filter(|value| (0..=2).contains(value))
        .context("Unknown source particle coordinate system")?;
    let conditions = self::conditions(system)?;
    ensure!(
        conditions.reasons.is_empty(),
        "Source particle system condition: {}",
        conditions.reasons.join(", ")
    );
    let appearance = cache.u32(at + 0x10)?;
    ensure!(
        appearance & 0x28000 == 0,
        "Source distortion or model particle material needs another draw path"
    );
    let material = crate::halo_reach::material::embedded(cache, source.clone(), at + 0x40)?;
    material::validate(&material)?;
    let frame_rows = cache.block(at + 0x1e4, 16)?;
    ensure!(
        (2..=257).contains(&frame_rows.len()),
        "Source particle frame table is absent or too large"
    );
    let header = cache.floats::<4>(frame_rows[0])?;
    ensure!(
        header == [(frame_rows.len() - 1) as f32, 0., 0., 0.],
        "Source particle frame sequence differs"
    );
    let frames = frame_rows[1..]
        .iter()
        .map(|&row| cache.floats::<4>(row))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        frames.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite source particle atlas"
    );
    let properties = properties(cache, at, emitter_at)?;
    let life = life(&properties)?;
    let mut limits = vec![
        "Native random streams differ from Reach. Source LOD is evaluated at full detail.".into(),
        "Source environment lighting and motion blur require separate scene inputs.".into(),
    ];
    limits.extend(conditions.limits);
    let program = simulation::build(cache, emitter_at, &properties, metres, frame, &mut limits)?;
    let definition = definition(native, &program, life)?;
    let near = values::<2>(system, "near_fade_source")?;
    let depth_fade = material::depth_fade(&material)? * metres;
    if depth_fade > 0. {
        limits.push(
            "Depth fade is skipped where the bound scene depth's size differs from the view's."
                .into(),
        );
    }
    if coordinates == 2 {
        limits.push("Source parent space is drawn in the emitter locator's frame, so particles follow the emitter rather than its parent object.".into());
    }
    Ok(Emitter {
        source,
        name: emitter["name"]
            .as_str()
            .context("Source emitter name")?
            .into(),
        life,
        definition,
        material,
        appearance,
        animation: cache.u32(at + 0x158)?,
        orientation,
        center: cache.floats::<2>(at + 0x20)?,
        scroll: cache.floats::<2>(emitter_at + 0x40)?,
        frames,
        near_fade: [near[0] / metres, near[1] * metres],
        depth_fade,
        coordinates,
        framed: false,
        conditions: conditions.report,
        limits,
    })
}

pub(in crate::halo_reach) fn emit(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    emitter: &Emitter,
    prefix: &str,
) -> Result<(String, Value)> {
    let material = material::emit(cache, pages, native, graph, emitter, prefix)?;
    let symbol = system(native, graph, prefix, &emitter.definition, &material)?;
    let mut limits = emitter.limits.clone();
    let space = match (emitter.coordinates, emitter.framed) {
        (1, _) => "local",
        (2, _) => "parent",
        (_, true) => "world",
        (_, false) => {
            limits.push("The source system is world space, but its host binds only the emitter's locator, so its particles follow the emitter.".into());
            "emitter"
        }
    };
    Ok((
        symbol,
        json!({"source":emitter.source,"emitter":emitter.name,"maximum_life":emitter.life,
        "atlas_frames":emitter.frames.len(),"space":space,"conditions":emitter.conditions,
        "limits":limits,"gameplay_verified":false}),
    ))
}

/// The native billboard definition running `program`, whose particles live at most `life`
/// seconds.
fn definition(
    native: &mut Reader,
    program: &crate::tiger::particle::Program,
    life: f32,
) -> Result<Payload> {
    let original = native.tag(0x80c0d9c7, Some(0x80806e2c))?;
    ensure!(
        original.u8(0x14b)? == 0 && original.u8(0x142)? == 8,
        "Native billboard definition envelope differs"
    );
    let mut definition = program.write(&original, 8, 8)?;
    // Positions and velocities are authored by the source program.
    // The native producers supply zero, then section three publishes them.
    definition.0[0x115] = 0;
    definition.0[0x116] = 0;
    definition.0[0x149] = 0;
    definition.0[0x14a] = 1;
    put(&mut definition.0, 0x120, &(life + 0.05).to_le_bytes())?;
    Ok(definition)
}

/// Adds the native billboard system `{prefix}-system` drawing `definition` with `material`.
fn system(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    definition: &Payload,
    material: &str,
) -> Result<String> {
    let name = format!("{prefix}-definition");
    graph.add(&name, 0x80c0d9c7, &definition.0, None, Vec::new())?;
    let original = native.tag(0x80fc7e04, Some(0x80806e28))?;
    ensure!(
        original.u32(0x18)? == 0x80c70b97
            && [4, 8, 16]
                .into_iter()
                .all(|at| original.u32(at).ok() == Some(u32::MAX)),
        "Native billboard system needs another computational stage"
    );
    let mut system = original.0.clone();
    for field in [0, 0x14] {
        put(&mut system, field, &u32::MAX.to_le_bytes())?;
    }
    let symbol = format!("{prefix}-system");
    graph.add(
        &symbol,
        0x80fc7e04,
        &system,
        None,
        vec![
            json!({"offset":0,"symbol":name}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    Ok(symbol)
}
