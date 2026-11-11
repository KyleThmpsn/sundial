//! The flight of a modeled projectile whose source attaches no contrail, in its carrier's own flow.
//!
//! Without a contrail there is no bolt to take the place of the carrier's flow, which otherwise
//! keeps the native grenade's presentation. The impact grenade's flow `80F2E50A` starts glow
//! particles and a flight light, range-gated flight sounds, and an impact flash, lens flare and
//! decal. Its controls are rebuilt here with one held event per converted source flight system at
//! the projectile origin. Each is bound to the flow's flight marker, moved to that origin, and a
//! world-space system also to `world_space`. The flow's flight light event, adopted and held,
//! names the private light of the first source light. Every other native event is then
//! unreachable, as on a bolted carrier. When nothing converts the flow is left out, so only the
//! model draws the flight. The source's impact effects play through the contact host either way.
use super::*;
use crate::tiger::sequence;
use std::collections::BTreeSet;

/// FNV-1 of `world_space`.
const WORLD_SPACE: u32 = 0x9199_59c0;
const PARTICLES: u32 = 0x8080_6cc6;
const LIGHT: u32 = 0x8080_6f49;

/// The flow's flight light: of the light events its root starts unconditionally for a fixed
/// time, the one that lasts longest, as the flight beam is the longest such particle event.
fn flight_light(p: &Payload) -> Result<Option<usize>> {
    let definition = p.pointer(24)?;
    let controls = p.array(definition + 0x158, 24, Some(0x8080_93e6))?;
    let entry = p.u32(definition + 0x2d0)?;
    if entry & 0xffff != 0 {
        return Ok(None);
    }
    let mut found = Vec::new();
    for row in p.array(definition + 0x168, 24, Some(0x8080_93e6))? {
        let event = p.pointer(row + 16)?;
        if p.u32(event - 4)? != LIGHT {
            continue;
        }
        let duration = p.f32(event + 0x24)?;
        if !duration.is_finite() || duration <= 0. || duration != p.f32(event + 0x28)? {
            continue;
        }
        if unconditional(p, &controls, entry, event)? {
            found.push((duration, event));
        }
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok(match found[..] {
        [(_, event)] => Some(event),
        [(first, event), (second, _), ..] if first > second => Some(event),
        _ => None,
    })
}

pub(super) fn host(
    (cache, pages, native): (&Cache, &mut Pages, &mut Reader),
    scene: &Scene,
    assembly: &mut super::super::assembly::Assembly,
    mut graph: Graph,
    undrawn: Vec<Value>,
) -> Result<Effects> {
    let Some(target) = target(native, &assembly.owners)? else {
        let mut pending = flight::pending(cache, scene);
        pending.extend(undrawn);
        return Ok(Effects {
            graph,
            patches: Vec::new(),
            report: json!({"trails":[],"presentation":"carrier","pending":pending,
                "reason":"The carrier's flow has no flight beam event to host the source flight, so its native flight effects stay"}),
            owner: None,
            allocations: BTreeMap::new(),
        });
    };
    let native_owner = format!("{:08X}", target.owner);
    let lit = flight_light(&assembly.owners[&target.owner])?;
    let (systems, converted, mut pending, light) = flight::modeled(
        cache,
        pages,
        native,
        &mut graph,
        scene,
        ([0.; 3], lit.is_some()),
    )?;
    pending.extend(undrawn);
    if systems.is_empty() && light.is_none() {
        super::super::effect_only::omit(native, assembly, &BTreeSet::from([target.owner]))?;
        return Ok(Effects {
            graph,
            patches: Vec::new(),
            report: json!({"trails":[],"presentation":"model","omitted_flow":native_owner,
                "pending":pending,
                "reason":"No source flight element converts, so the carrier's native flight effects are left out and only the model draws the flight"}),
            owner: None,
            allocations: BTreeMap::new(),
        });
    }
    let allocation_tag = assembly.owners[&target.owner].u32(0x44)?;
    ensure!(
        assembly
            .owners
            .values()
            .filter(|owner| owner.u32(0x44).ok() == Some(allocation_tag))
            .count()
            == 1,
        "The carrier flight flow shares its allocation with another component"
    );
    let mut allocation = (*native.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
    place_marker(&mut assembly.owners, target.name, [0.; 3], [0., 0., 0., 1.])?;
    let p = assembly
        .owners
        .get_mut(&target.owner)
        .context("Carrier flight flow")?;
    let definition = p.pointer(24)?;
    let locators = p.array(definition + 0x1c0, 16, Some(0x8080_8615))?;
    let names = p.array(definition + 0x1d0, 4, Some(0x8080_8614))?;
    ensure!(
        locators.len() == names.len(),
        "Host locator names differ from its locators"
    );
    let marker = u8::try_from(target.locator)?;
    let world = locators
        .iter()
        .zip(&names)
        .position(|(&row, &named)| {
            p.u8(row).ok() == Some(1) && p.u32(named).ok() == Some(WORLD_SPACE)
        })
        .map(u8::try_from)
        .transpose()?;
    let stock = p.clone();
    let name = |index: usize| crate::tiger::audio::bank::hash(&format!("reach/modeled/{index}"));
    // Flag 8 holds an event until its owner retires, as authored trail events do.
    let held = sequence::Timing {
        duration: [1.; 2],
        flags: 8,
        ..sequence::Timing::default()
    };
    let mut children = (0..systems.len())
        .map(|index| sequence::Node {
            name: name(index),
            timing: held,
            kind: sequence::Kind::Event(PARTICLES),
        })
        .collect::<Vec<_>>();
    if light.is_some() {
        children.push(sequence::Node {
            name: name(systems.len()),
            timing: held,
            kind: sequence::Kind::Event(LIGHT),
        });
    }
    let tree = sequence::Node {
        name: crate::tiger::audio::bank::hash("reach/modeled/root"),
        timing: sequence::Timing::default(),
        kind: sequence::Kind::Parallel(children),
    };
    let written = sequence::write(p, &mut allocation, &tree, std::slice::from_ref(&stock))?;
    let mut declared = Vec::new();
    for (index, flight) in systems.iter().enumerate() {
        let bound = if flight.world {
            vec![
                marker,
                world.context(
                    "The carrier flow has no world_space locator for a world-space system",
                )?,
            ]
        } else {
            vec![marker]
        };
        for (offset, symbol) in sequence::particle::write_bound(
            p,
            written.events[&name(index)],
            &[(flight.symbol.clone(), bound)],
        )? {
            declared.push(json!({"offset":offset,"symbol":symbol}));
        }
    }
    if let (Some(symbol), Some(event)) = (&light, lit) {
        // The stock flight light keeps its body under the rebuilt controls, held for the flight,
        // and names its light record at `+0x40`.
        sequence::adopt(p, written.events[&name(systems.len())], event)?;
        let flags = p.u32(event + 0x18)? | 8;
        put(&mut p.0, event + 0x18, &flags.to_le_bytes())?;
        put(&mut p.0, event + 0x40, &u32::MAX.to_le_bytes())?;
        declared.push(json!({"offset":event + 0x40,"symbol":symbol}));
    }
    // The rebuilt controls and events are runtime records past the stock schema, which the
    // client's instance copy leaves out without a definition view after them.
    let end = p.0.len();
    crate::tiger::instance::seal_span(p, &mut declared, end)?;
    let patches = declared
        .iter()
        .map(|patch| {
            Ok((
                target.owner,
                usize::try_from(patch["offset"].as_u64().context("Flight patch offset")?)?,
                patch["symbol"]
                    .as_str()
                    .context("Flight patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Effects {
        graph,
        patches,
        owner: Some(target.owner),
        allocations: BTreeMap::from([(allocation_tag, allocation)]),
        report: json!({"trails":[],"presentation":"source","native_owner":native_owner,
            "native_marker":format!("{:08X}",target.name),"systems":systems.len(),
            "light":light.is_some(),"flight_particles":converted,"pending":pending,
            "gameplay_verified":false,
            "limits":["The carrier flow's native glow, light, flight sounds and impact presentation are left out. Converted systems start with the projectile and last until it retires, bound to its marker at the projectile origin. Source event delays, loops and durations are not carried. A converted light has the native light's brightness in the source light's hue, and a cheap light's radius."]}),
    })
}
