//! Source billboard emitters, with source simulation and material programs.
mod material;
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
    center: [f32; 2],
    scroll: [f32; 2],
    frames: Vec<[f32; 4]>,
    near_fade: [f32; 2],
    limits: Vec<String>,
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
    if cache.reference(at + 0x1bc, None)?.is_some() || !matches!(cache.i16(at + 0x14)?, 0 | 1) {
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
    if system["coordinates"] != 0
        || system["environment"] != 0
        || system["disposition"] != 0
        || system["camera"] != 0
    {
        result.push("Local-space or conditional particle activation".into());
    }
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
    ensure!(
        kinds.iter().all(|kind| (0..=4).contains(kind)),
        "Unknown source particle movement controller"
    );
    if kinds.iter().any(|&kind| kind != 0) {
        result.push(format!("Movement controllers {kinds:?} require collision, swarm, wind or turbulence translation"));
    }
    Ok(result)
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
    ensure!(
        matches!(cache.i16(at + 0x14)?, 0 | 1),
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
    ensure!(
        system["coordinates"] == 0
            && system["environment"] == 0
            && system["disposition"] == 0
            && system["camera"] == 0,
        "Source particle system has a conditional or local-space policy"
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
    let life = properties["life"]
        .bounds()?
        .into_iter()
        .fold(0f32, f32::max);
    ensure!(
        life.is_finite() && life > 0. && life <= 120.,
        "Source particle lifetime exceeds the supported finite envelope"
    );
    let mut limits = vec![
        "Native random streams differ from Reach. Source LOD is evaluated at full detail.".into(),
        "Source depth fading, environment lighting and motion blur require separate scene inputs."
            .into(),
    ];
    let program = simulation::build(cache, emitter_at, &properties, metres, frame, &mut limits)?;
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
    let near = values::<2>(system, "near_fade_source")?;
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
        center: cache.floats::<2>(at + 0x20)?,
        scroll: cache.floats::<2>(emitter_at + 0x40)?,
        frames,
        near_fade: [near[0] / metres, near[1] * metres],
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
    let definition = format!("{prefix}-definition");
    graph.add(
        &definition,
        0x80c0d9c7,
        &emitter.definition.0,
        None,
        Vec::new(),
    )?;
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
            json!({"offset":0,"symbol":definition}),
            json!({"offset":0x14,"symbol":material}),
        ],
    )?;
    Ok((
        symbol,
        json!({"source":emitter.source,"emitter":emitter.name,"maximum_life":emitter.life,
        "atlas_frames":emitter.frames.len(),"limits":emitter.limits,"gameplay_verified":false}),
    ))
}
