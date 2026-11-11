//! The source firing effect's beams, drawn on the donor's native trace beam.
//!
//! A Shadowkeep trace rifle draws its beam in its firing presentation, not on its fired entity.
//! Coldheart's selected content row names presentation `80EF2C85`, whose slot-4 entity `80BBC680`
//! holds flow `80EF2C81`, the slot that also plays the firing loop. Five of that flow's particle
//! systems have geometry 6, definition byte `+0x14B`, which no other system the presentation reaches
//! uses, and each holds one particle for 10.05 seconds while the trigger is held. That geometry 6
//! draws the beam between the muzzle and the hit is inferred from those.
//!
//! Reach draws a beam weapon's beam as `beam` parts of its barrel's firing effect. Each geometry-6
//! system the donor's presentation flows start takes a private copy whose pixel program keeps each
//! pixel's luminance in the hue of the source beams' brightest point, as [`beam::brightest`] finds
//! it. That holds whatever the donor program takes its color from, a program immediate, a texture or
//! a vertex color. The donor beam's shape, width, length, timing, texture and brightness stay. Each
//! flow that starts one takes a private copy naming the private systems, and its stock instance
//! allocation, which the copy leaves unchanged, goes with it.
use super::*;
use crate::{
    halo_reach::{effect::beam, native::material::bytecode, projectile::effect::bolt},
    presentation::put,
};

/// The native geometry of a trace beam system.
const TRACE: u8 = 6;
const SYSTEM_CLASS: u32 = 0x8080_6e28;
const DEFINITION_CLASS: u32 = 0x8080_6e2c;
const MATERIAL_CLASS: u32 = 0x8080_71e8;

/// The private flows, allocations, replacement rows and report of a translated trace.
pub(super) struct Trace {
    pub replacements: Vec<Value>,
    pub report: Value,
}

/// The distinct `beam` parts of the source firing effect's events.
fn sources(effect: &Value) -> Result<Vec<Tag>> {
    let mut tags = Vec::<Tag>::new();
    for event in effect["events"].as_array().into_iter().flatten() {
        for part in event["parts"].as_array().into_iter().flatten() {
            if part["tag"]["group"] != "beam" {
                continue;
            }
            let tag: Tag = serde_json::from_value(part["tag"].clone())?;
            if !tags.iter().any(|known| known.path == tag.path) {
                tags.push(tag);
            }
        }
    }
    Ok(tags)
}

/// Each particle event system field of flow `p` that names a trace system, with that system.
fn trace_fields(native: &mut Reader, p: &Payload) -> Result<Vec<(usize, u32)>> {
    let definition = p.pointer(24)?;
    let mut found = Vec::new();
    for row in p.array(definition + 0x168, 24, Some(0x8080_93e6))? {
        let event = p.pointer(row + 16)?;
        if p.u32(event - 4)? != PARTICLE {
            continue;
        }
        for system_row in p.array(event + 0x40, 24, Some(0x8080_6cc8))? {
            let field = system_row + 16;
            let system = p.u32(field)?;
            if system == u32::MAX {
                continue;
            }
            let definition = native.tag(system, Some(SYSTEM_CLASS))?.u32(0)?;
            if native.tag(definition, Some(DEFINITION_CLASS))?.u8(0x14b)? == TRACE {
                found.push((field, system));
            }
        }
    }
    Ok(found)
}

/// A private copy of trace system `system` whose pixel program draws in `hue`.
fn recolor(
    native: &mut Reader,
    graph: &mut Graph,
    system: u32,
    hue: [f32; 3],
) -> Result<(String, Value)> {
    let name = format!("reach-trace-{system:08X}");
    let stock = native.tag(system, Some(SYSTEM_CLASS))?;
    let (definition_tag, material_tag) = (stock.u32(0)?, stock.u32(0x14)?);
    let definition = (*native.tag(definition_tag, Some(DEFINITION_CLASS))?).clone();
    let definition_symbol = format!("{name}-definition");
    graph.add(
        &definition_symbol,
        definition_tag,
        &definition.0,
        None,
        vec![],
    )?;
    let pixel_tag = native.tag(material_tag, Some(MATERIAL_CLASS))?.u32(0x2c8)?;
    let code = native.tag(native.reference(pixel_tag)?, None)?;
    let tinted = crate::tiger::shader::dxbc::tint_output(&code.0, 0, hue)?;
    ensure!(
        tinted != code.0,
        "Native trace pixel program {pixel_tag:08X} declares no color output"
    );
    let pixel = format!("{name}-pixel");
    bytecode(
        native,
        graph,
        &pixel,
        pixel_tag,
        &tinted,
        &format!(
            "// Native program {pixel_tag:08X} with its color output kept at its luminance in the source beam hue."
        ),
    )?;
    let material = bolt::material(native, graph, &name, material_tag, Some(pixel))?;
    let symbol = bolt::system(graph, native, &name, system, definition_symbol, material)?;
    Ok((
        symbol,
        json!({"donor_system":format!("{system:08X}"),"definition":format!("{definition_tag:08X}"),
            "material":format!("{material_tag:08X}"),"pixel_program":format!("{pixel_tag:08X}")}),
    ))
}

/// The source firing effect's beams on the donor's trace in `group`, the presentation the firing
/// effect binds. `rebuilt` lists the flow owners another prepared flow already replaces. `None`
/// when the source effect has no beam.
#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    cache: &Cache,
    native: &mut Reader,
    assets: &mut Graph,
    root: &Path,
    (owners, allocations): (&mut Vec<Value>, &mut Vec<Value>),
    group: &Value,
    effect: &Value,
    rebuilt: &[u32],
) -> Result<Option<Trace>> {
    let tags = sources(effect)?;
    if tags.is_empty() {
        return Ok(None);
    }
    let mut beams = Vec::new();
    let mut read = Vec::new();
    for tag in &tags {
        for (name, result) in beam::each(cache, tag)? {
            match result {
                Ok(beam) => {
                    read.push(json!({"source":tag,"beam":name,"albedo":beam.albedo}));
                    beams.push(beam);
                }
                Err(error) => read.push(json!({"source":tag,"beam":name,
                    "status":"omitted","reason":format!("{error:#}")})),
            }
        }
    }
    let Some((hue, from, percentile)) = beam::brightest(&beams)? else {
        return Ok(Some(Trace {
            replacements: Vec::new(),
            report: json!({"status":"native","beams":read,
                "reason":"No source beam draws its own profile color"}),
        }));
    };
    let mut private = BTreeMap::<u32, (String, Value)>::new();
    let mut replacements = Vec::new();
    let mut flows = Vec::new();
    for slot in group["slots"].as_array().into_iter().flatten() {
        let entity = tag(&slot["entity"], "presentation entity")?;
        for (owner, _) in particle_flows(native, entity)? {
            if rebuilt.contains(&owner) {
                continue;
            }
            let mut payload = (*native.tag(owner, Some(0x8080_9c36))?).clone();
            let fields = trace_fields(native, &payload)?;
            if fields.is_empty() {
                continue;
            }
            let mut patches = Vec::new();
            for &(field, system) in &fields {
                if let std::collections::btree_map::Entry::Vacant(entry) = private.entry(system) {
                    let copy = recolor(native, assets, system, hue)?;
                    entry.insert(copy);
                }
                put(&mut payload.0, field, &u32::MAX.to_le_bytes())?;
                patches.push(json!({"offset":field,"symbol":private[&system].0}));
            }
            let symbol = format!("reach-trace-flow-{owner:08X}");
            let allocation_tag = payload.u32(0x44)?;
            let allocation = native.tag(allocation_tag, Some(0x8080_9bbb))?;
            let allocation_symbol = format!("{symbol}-allocation");
            crate::graph::add(root, owners, &symbol, owner, &payload.0, None, patches)?;
            crate::graph::add(
                root,
                allocations,
                &allocation_symbol,
                allocation_tag,
                &allocation.0,
                None,
                Vec::new(),
            )?;
            replacements.push(json!({"slot":slot["slot"],"entity":format!("{entity:08X}"),
                "component_owner":format!("{owner:08X}"),"owner":symbol,"allocation":allocation_symbol}));
            flows.push(json!({"slot":slot["slot"],"entity":format!("{entity:08X}"),
                "owner":format!("{owner:08X}"),"trace_events":fields.len()}));
        }
    }
    let report = if replacements.is_empty() {
        json!({"status":"native","beams":read,
            "reason":"The donor presentation starts no native trace beam, a geometry 6 particle system"})
    } else {
        json!({"status":"partial","beams":read,"hue":hue,"hue_beam":from,"hue_percentile":percentile,
            "flows":flows,"systems":private.values().map(|(_, report)| report.clone()).collect::<Vec<_>>(),
            "gameplay_verified":false,
            "limits":["Each native trace system keeps its own luminance in the hue of the source beams' brightest point. The source color gradient along the beam, its width, length, textures and pulse are not drawn, and the donor's beam shape and timing stay.",
                "That geometry 6 draws the native trace between the muzzle and the hit is inferred from Coldheart's presentation."]})
    };
    Ok(Some(Trace {
        replacements,
        report,
    }))
}
