//! Attached source contrails on private projectile flight effects.
pub(in crate::halo_reach) mod bolt;
mod flight;
mod host;
pub(in crate::halo_reach) mod light;
mod modeled;
mod volume;
use crate::{
    halo_reach::{
        Scene,
        cache::{Cache, Tag},
        effect::{particle, source, trail},
        resource::Pages,
    },
    presentation::{Graph, append, put},
    tiger::{markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) struct Effects {
    pub graph: Graph,
    pub patches: Vec<(u32, usize, String)>,
    pub report: Value,
    pub owner: Option<u32>,
    pub allocations: BTreeMap<u32, Payload>,
}

struct Target {
    owner: u32,
    event: usize,
    locator: usize,
    name: u32,
    duration: f32,
}

fn source_frame(
    cache: &Cache,
    effect: &crate::halo_reach::cache::Tag,
    part: usize,
    attachment: &Value,
    scene: &Scene,
) -> Result<([f32; 3], Value)> {
    ensure!(
        cache.i16(part + 6)? == -1 && cache.floats::<2>(part + 0x54)? == [0.; 2],
        "Projectile contrail needs a secondary location or relative rotation"
    );
    let location = cache.i16(part + 4)?;
    let locations = cache.block(effect.address()? + 0x20, 8)?;
    let name = if location == -1 {
        String::new()
    } else {
        let row = *locations
            .get(usize::try_from(location)?)
            .context("Projectile contrail location")?;
        cache.string_id(cache.u32(row)?)?
    };
    let name = if matches!(name.as_str(), "" | "replace") {
        attachment["marker"]
            .as_str()
            .context("Projectile attachment marker")?
    } else {
        name.as_str()
    };
    let (position, orientation) = if matches!(name, "" | "root") {
        ([0.; 3], [0., 0., 0., 1.])
    } else {
        let model = &scene
            .models
            .first()
            .context("Projectile source model")?
            .model;
        crate::halo_reach::native::seat::marker(model, name)?
    };
    let offset = cache.floats::<3>(part + 0x48)?.map(|v| v * scene.metres());
    ensure!(
        offset.iter().all(|v| v.is_finite()),
        "Nonfinite projectile trail offset"
    );
    let origin = crate::halo_reach::rig::point(
        &crate::halo_reach::rig::transform(position, orientation),
        offset,
        true,
    );
    Ok((
        origin,
        json!({"marker":name,"position":origin,"orientation":orientation}),
    ))
}

/// Whether flow event `event` starts with the root `entry` through plain serial and parallel
/// controls alone, following its stored parent chain. Conditional branches, named triggers and
/// delayed starts are different effect contracts.
fn unconditional(p: &Payload, controls: &[usize], entry: u32, event: usize) -> Result<bool> {
    let mut parent = p.u32(event + 0x14)?;
    for _ in 0..controls.len() {
        if parent & 0xffff != 0 {
            return Ok(false);
        }
        let ordinal = (parent >> 16) as usize;
        let at = p.pointer(*controls.get(ordinal).context("Projectile effect parent")? + 16)?;
        if !matches!(p.u32(at - 4)?, 0x808093d7 | 0x808093d9)
            || p.0[at + 0x18..at + 0x30].iter().any(|&v| v != 0)
            || p.0[at + 0x48..at + 0x58].iter().any(|&v| v != 0)
        {
            return Ok(false);
        }
        if parent == entry {
            return Ok(true);
        }
        parent = p.u32(at + 0x14)?;
    }
    Ok(false)
}

fn target(native: &mut Reader, owners: &BTreeMap<u32, Payload>) -> Result<Option<Target>> {
    let mut candidates = Vec::new();
    for (&owner, p) in owners {
        let definition = p.pointer(24)?;
        if p.u32(definition - 4)? != 0x808084e9 {
            continue;
        }
        let controls = p.array(definition + 0x158, 24, Some(0x808093e6))?;
        let locators = p.array(definition + 0x1c0, 16, Some(0x80808615))?;
        let entry = p.u32(definition + 0x2d0)?;
        if entry & 0xffff != 0 {
            continue;
        }
        for row in p.array(definition + 0x168, 24, Some(0x808093e6))? {
            let event = p.pointer(row + 16)?;
            if p.u32(event - 4)? != 0x80806cc6 {
                continue;
            }
            let systems = p.array(event + 0x40, 24, Some(0x80806cc8))?;
            let [system] = systems.as_slice() else {
                continue;
            };
            let system_tag = p.u32(system + 16)?;
            if system_tag == u32::MAX {
                continue;
            }
            let system_data = native.tag(system_tag, Some(0x80806e28))?;
            if native
                .tag(system_data.u32(0)?, Some(0x80806e2c))?
                .u8(0x14b)?
                != 5
            {
                continue;
            }
            let duration = p.f32(event + 0x24)?;
            if !duration.is_finite() || duration <= 0. || duration != p.f32(event + 0x28)? {
                continue;
            }
            if !unconditional(p, &controls, entry, event)? {
                continue;
            }
            let marker_locators = p
                .array(*system, 1, Some(0x80800009))?
                .into_iter()
                .map(|at| Ok(usize::from(p.u8(at)?)))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter_map(|index| locators.get(index).copied().map(|at| (index, at)))
                .filter(|&(_, at)| p.u8(at).ok() == Some(2) && p.u8(at + 2).ok() == Some(1))
                .collect::<Vec<_>>();
            let [(locator, at)] = marker_locators.as_slice() else {
                continue;
            };
            candidates.push(Target {
                owner,
                event,
                locator: *locator,
                name: p.u32(at + 8)?,
                duration,
            });
        }
    }
    candidates.sort_by(|a, b| b.duration.total_cmp(&a.duration));
    if candidates.is_empty() {
        return Ok(None);
    }
    ensure!(
        candidates.len() == 1 || candidates[0].duration > candidates[1].duration,
        "Projectile flight beam is ambiguous"
    );
    Ok(Some(candidates.remove(0)))
}

/// The attached contrail profiles a modeled carrier's flight host can draw, their attachments and
/// their one origin, and each contrail part or profile it cannot draw, with its reasons. A
/// contrail that does not translate, or one at another origin than the first, stays pending, so
/// it never fails the import.
struct Contrails {
    profiles: Vec<trail::Profile>,
    attachments: Vec<Value>,
    origin: Option<[f32; 3]>,
    pending: Vec<Value>,
}

fn contrails(cache: &Cache, scene: &Scene) -> Contrails {
    let mut result = Contrails {
        profiles: Vec::new(),
        attachments: Vec::new(),
        origin: None,
        pending: Vec::new(),
    };
    for (index, attachment) in scene.object.attachments.iter().enumerate() {
        if attachment["tag"]["group"] != "effe" {
            continue;
        }
        let read = (|| -> Result<()> {
            let effect: Tag = serde_json::from_value(attachment["tag"].clone())?;
            for (event_index, event) in cache
                .block(effect.address()? + 0x2c, 0x40)?
                .into_iter()
                .enumerate()
            {
                for (part_index, part) in cache.block(event + 0x1c, 0x64)?.into_iter().enumerate() {
                    let Some(tag) = cache.reference(part + 0x14, None)? else {
                        continue;
                    };
                    if tag.group != "cntl" {
                        continue;
                    }
                    let source = json!({"attachment":index,"effect":effect,"event":event_index,
                        "part":part_index,"tag":tag});
                    let (position, frame) =
                        match source_frame(cache, &effect, part, attachment, scene) {
                            Ok(found) => found,
                            Err(error) => {
                                result.pending.push(
                                    json!({"source":source,"reasons":[format!("{error:#}")]}),
                                );
                                continue;
                            }
                        };
                    if result.origin.is_some_and(|old| old != position) {
                        result.pending.push(json!({"source":source,
                            "reasons":["The flight host has one marker, at the first contrail's origin"]}));
                        continue;
                    }
                    let mut drawn = false;
                    for (name, profile) in trail::each(cache, &tag)? {
                        match profile {
                            Ok(profile) => {
                                result.profiles.push(profile);
                                drawn = true;
                            }
                            Err(error) => result.pending.push(json!({"source":source,
                                "profile":name,"reasons":[format!("{error:#}")]})),
                        }
                    }
                    if drawn {
                        result.origin = Some(position);
                        result
                            .attachments
                            .push(json!({"attachment":attachment,"frame":frame}));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = read {
            result.pending.push(
                json!({"source":{"attachment":index,"tag":attachment["tag"]},
                "reasons":[format!("The attached effect's contrails do not read: {error:#}")]}),
            );
        }
    }
    result
}

/// Moves the one object-space marker named `name` to `origin`, facing `orientation`.
fn place_marker(
    owners: &mut BTreeMap<u32, Payload>,
    name: u32,
    origin: [f32; 3],
    orientation: [f32; 4],
) -> Result<()> {
    let mut marker_owners = Vec::new();
    for (&owner, p) in owners.iter() {
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x80808507 {
            continue;
        }
        let matching = markers::read_native(p)?
            .into_iter()
            .filter(|m| m.name == name)
            .collect::<Vec<_>>();
        for marker in matching {
            ensure!(
                marker.binding == [0, 0],
                "Projectile trail marker is not in object space"
            );
            marker_owners.push((owner, marker));
        }
    }
    ensure!(
        marker_owners.len() == 1,
        "Projectile trail attachment is absent or ambiguous"
    );
    let (marker_owner, mut marker) = marker_owners[0];
    marker.position = [origin[0], origin[1], origin[2], 1.];
    marker.orientation = orientation;
    let (payload, _) = markers::rewrite(&owners[&marker_owner], &[marker])?;
    owners.insert(marker_owner, payload);
    Ok(())
}

/// The Fallen bolt hosting an additive source contrail in the carrier's own flight flow, with
/// the flight particle systems at the projectile origin beside it and every other flight element
/// pending.
struct Bolted {
    owner: u32,
    patches: Vec<(u32, usize, String)>,
    report: Value,
    allocation: (u32, Payload),
    pending: Vec<Value>,
}

fn bolted(
    (cache, pages, native): (&Cache, &mut Pages, &mut Reader),
    scene: &Scene,
    graph: &mut Graph,
    owners: &mut BTreeMap<u32, Payload>,
    profile: &trail::Profile,
    origin: Option<[f32; 3]>,
) -> Result<Bolted> {
    let target =
        target(native, owners)?.context("The Fallen bolt needs the carrier's own flight effect")?;
    let origin = origin.context("Projectile contrail origin is absent")?;
    place_marker(owners, target.name, origin, [0., 0., 0., 1.])?;
    let (flight, flight_particles, pending, _) =
        flight::modeled(cache, pages, native, graph, scene, (origin, false))?;
    let host = owners
        .get_mut(&target.owner)
        .context("Projectile flight effect owner")?;
    let bolt = bolt::host(
        cache,
        pages,
        native,
        graph,
        profile,
        "projectile-trail-0",
        host,
        (target.name, &flight),
    )?;
    Ok(Bolted {
        owner: target.owner,
        patches: bolt
            .patches
            .into_iter()
            .map(|(offset, symbol)| (target.owner, offset, symbol))
            .collect(),
        report: json!({"trails":[{"source":profile,"bolt":bolt.report}],
            "flight_particles":flight_particles,
            "native_owner":format!("{:08X}",target.owner),"origin_metres":origin,
            "limits":["The Fallen bolt's flight effects stand for the source contrail. Profiles other than the first additive one are not drawn.",
                "Converted flight particle systems start with the projectile and last until it retires, bound to its marker. Source event delays, loops and durations are not carried."]}),
        allocation: bolt.allocation,
        pending,
    })
}

pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    behavior: &Value,
    assembly: &mut super::assembly::Assembly,
    root: &std::path::Path,
) -> Result<Effects> {
    // A carrier without a render owner draws the source's whole flight in its own flow.
    if !assembly
        .owners
        .values()
        .any(|p| super::assembly::Assembly::class(p).ok() == Some(0x8080_72bd))
    {
        return flight::build(cache, pages, native, scene, behavior, assembly, root);
    }
    let Contrails {
        profiles,
        attachments,
        origin,
        pending: undrawn,
    } = contrails(cache, scene);
    let mut pending = flight::pending(cache, scene);
    pending.extend(undrawn.iter().cloned());
    std::fs::create_dir_all(root)?;
    let mut graph = Graph::new(root.into(), Vec::new());
    if profiles.is_empty() {
        return modeled::host((cache, pages, native), scene, assembly, graph, undrawn);
    }
    // An additive contrail draws with the Fallen bolt's flight effects in the carrier's own flow.
    // A bolt that does not build leaves the carrier as it was, and every profile draws as a
    // billboard contrail instead.
    let mut unbolted = None;
    if let Some(profile) = profiles.iter().find(|profile| profile.additive()) {
        let owners = assembly.owners.clone();
        let nodes = graph.nodes.len();
        match bolted(
            (cache, pages, native),
            scene,
            &mut graph,
            &mut assembly.owners,
            profile,
            origin,
        ) {
            Ok(Bolted {
                owner,
                patches,
                mut report,
                allocation,
                mut pending,
            }) => {
                pending.extend(undrawn);
                let others = profiles.iter().filter(|p| !p.additive()).count();
                report["source_attachments"] = json!(attachments);
                report["omitted_profiles"] = json!(others);
                report["pending"] = json!(pending);
                return Ok(Effects {
                    graph,
                    patches,
                    report,
                    owner: Some(owner),
                    allocations: BTreeMap::from([allocation]),
                });
            }
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => {
                assembly.owners = owners;
                graph.nodes.truncate(nodes);
                unbolted = Some(format!("{error:#}"));
            }
        }
    }
    let speed = behavior["initial_velocity"]
        .as_f64()
        .context("Projectile trail reference speed")? as f32
        * scene.metres();
    // Each profile that does not emit takes back its nodes and stays pending.
    let mut trails = Vec::new();
    let mut reports = Vec::new();
    for (index, profile) in profiles.iter().enumerate() {
        let start = graph.nodes.len();
        match trail::emit(
            cache,
            pages,
            native,
            &mut graph,
            profile,
            &format!("projectile-trail-{index}"),
            scene.metres(),
            speed,
        ) {
            Ok((symbol, report)) => {
                trails.push(symbol);
                reports.push(report);
            }
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => {
                graph.nodes.truncate(start);
                pending.push(json!({"source":profile,"reasons":[format!("{error:#}")]}));
            }
        }
    }
    if trails.is_empty() {
        return Ok(Effects {
            graph,
            patches: Vec::new(),
            report: json!({"trails":[],"pending":pending,"unbolted":unbolted}),
            owner: None,
            allocations: BTreeMap::new(),
        });
    }
    let owners = &mut assembly.owners;
    let mut allocations = BTreeMap::new();
    let existing = target(native, owners)?;
    let authored = existing.is_none();
    let target = if let Some(target) = existing {
        target
    } else {
        let (target, tag, allocation) = host::prepare(native, owners)?;
        allocations.insert(tag, allocation);
        target
    };
    let origin = origin.context("Projectile trail origin is absent")?;
    // The straight beam approximates the projectile's historical path. Its
    // direction stays opposite flight while its origin follows the source marker.
    place_marker(owners, target.name, origin, [0., 0., 1., 0.])?;

    let p = owners.get_mut(&target.owner).unwrap();
    let definition = p.pointer(24)?;
    let instance = p.pointer(16)?;
    let controls = p.array(definition + 0x158, 24, Some(0x808093e6))?;
    let entry = p.u32(definition + 0x2d0)?;
    let root = p.pointer(
        *controls
            .get((entry >> 16) as usize)
            .context("Projectile flight root")?
            + 16,
    )?;
    ensure!(
        entry & 0xffff == 0 && matches!(p.u32(root - 4)?, 0x808093d7 | 0x808093d9),
        "Projectile flight root is not an unconditional control"
    );
    let events = p.array(definition + 0x168, 24, Some(0x808093e6))?;
    let ordinal = events
        .iter()
        .position(|row| p.pointer(row + 16).ok() == Some(target.event))
        .context("Projectile trail event ordinal")?;
    let child = (u32::try_from(ordinal)? << 16) | 1;
    append(&mut p.0, root + 0x38, 0x808093fb, &child.to_le_bytes(), 4)?;
    put(&mut p.0, target.event + 0x14, &entry.to_le_bytes())?;
    let flags = p.u32(target.event + 0x18)? | 8;
    put(&mut p.0, target.event + 0x18, &flags.to_le_bytes())?;
    // The flow's controller inputs (definition `+0x188`, `80809789`) and graph connections
    // (instance `+0xD0`, `80809788`) stay because retained nodes still read them. Each connection
    // names this owner's instance as its source and an input names the channel. Validate the
    // pairs so a node cannot evaluate an unresolved source and fault the client when firing.
    let inputs = p.array(definition + 0x188, 40, Some(0x80809789))?;
    let connections = p.array(instance + 0xd0, 96, Some(0x80809788))?;
    ensure!(
        inputs.len() == connections.len(),
        "Projectile flow inputs and connections differ in count"
    );
    for (input, connection) in inputs.iter().zip(&connections) {
        ensure!(
            p.u32(*input)? == target.owner
                && p.u32(input + 4)? == 0x80809788
                && usize::try_from(p.u64(input + 8)?)? == *connection
                && p.u32(*connection)? == target.owner
                && p.u32(connection + 4)? == 0x80809789
                && usize::try_from(p.u64(connection + 8)?)? == *input
                && p.pointer(connection + 0x10)? == instance,
            "Projectile flow input is not paired with a connection from its own instance"
        );
    }
    let locator = u8::try_from(target.locator)?;
    let systems = trails
        .into_iter()
        .map(|symbol| (symbol, vec![locator]))
        .collect::<Vec<_>>();
    let mut resources = crate::tiger::sequence::particle::write_bound(p, target.event, &systems)?
        .into_iter()
        .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
        .collect::<Vec<_>>();
    if authored {
        let end = p.0.len();
        crate::tiger::instance::seal_span(p, &mut resources, end)?;
    }
    let patches = resources
        .into_iter()
        .map(|patch| {
            Ok((
                target.owner,
                usize::try_from(patch["offset"].as_u64().context("Trail patch offset")?)?,
                patch["symbol"]
                    .as_str()
                    .context("Trail patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    if authored {
        host::attach(native, &mut assembly.entity, owners)?;
    }
    Ok(Effects {
        graph,
        patches,
        owner: (!authored).then_some(target.owner),
        allocations,
        report: json!({"trails":reports,"source_attachments":attachments,"pending":pending,
        "unbolted":unbolted,
        "native_owner":format!("{:08X}",target.owner),"native_event":target.event,
        "native_marker":format!("{:08X}",target.name),"authored_host":authored,"origin_metres":origin,"reference_speed_metres_per_second":speed,
        "limits":["The autonomous flight branch contains only source attached contrails and lasts until the projectile retires. Other impact graphs remain native. Source attachment scale functions and projectile speed changes require separate runtime translation."]}),
    })
}

/// The private impact table converted from the source projectile's material effects, by graph
/// symbol.
pub(super) struct Impact {
    pub table: Option<String>,
    pub report: Value,
}

/// The one reference of `role` from the source projectile, if any.
fn projectile_edge<'a>(
    behavior: &'a Value,
    projectile: &Value,
    role: &str,
) -> Result<Option<&'a Value>> {
    let found = behavior["edges"]
        .as_array()
        .context("Projectile effect edges")?
        .iter()
        .filter(|edge| edge["owner"] == projectile["tag"]["datum"] && edge["role"] == role)
        .collect::<Vec<_>>();
    ensure!(
        found.len() <= 1,
        "Source projectile has several {role} references"
    );
    Ok(found.first().copied())
}

/// The source projectile's response to an ordinary contact with its default material: that of the
/// last unconditional response row (`proj +0x320`) for global material 0, the `i16` at row `+0x4`.
/// An unconditional row has no object-type flags (`+0x6`), chance 1 (`+0x8`) and zero angle and
/// velocity bounds (`+0xC..0x1C`). The response is the `i16` at `+0x1C`, 0 for Impact (Detonate).
pub(super) fn contact_response(projectile: &Value) -> Result<Option<i16>> {
    let mut response = None;
    for row in projectile["behavior"]["responses"]
        .as_array()
        .context("Source material responses")?
    {
        let raw = hex::decode(
            row["raw"]
                .as_str()
                .context("Source material response bytes")?,
        )?;
        ensure!(raw.len() == 0x34, "Source material response size differs");
        if i16::from_le_bytes([raw[4], raw[5]]) == 0
            && raw[6..8] == [0; 2]
            && f32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]) == 1.
            && raw[0xc..0x1c].iter().all(|&byte| byte == 0)
        {
            response = Some(i16::from_le_bytes([raw[0x1c], raw[0x1d]]));
        }
    }
    Ok(response)
}

/// The contacts a second the source weapon can make with `projectile`: the highest rate of fire
/// of a barrel firing it (`+0x104`), times that barrel's projectiles per shot (`+0x6A`) and the
/// projectile's conical spread count, yaw times pitch for each row at `proj +0x344`. A barrel
/// fires at the larger of its rounds a second (`+0x04`), and a latch trigger (behavior 1) firing
/// it as its primary barrel waits out the barrel's fire recovery (`+0x28`) between shots, as the
/// cadence translation reads them. A barrel fired only once charged, such as the Plasma Pistol's
/// charged barrel, fires at most once per its trigger's charging time. With no barrel naming the
/// projectile every barrel counts, and a source without barrels has no rate.
fn contact_rate(behavior: &Value, projectile: &Value) -> Result<Option<f32>> {
    let gameplay = gameplay(behavior)?;
    let mut rate = None::<f32>;
    for (index, barrel) in firing(gameplay, projectile) {
        let mut rounds = barrel["rounds_per_second"]
            .as_array()
            .context("Source barrel rate of fire")?
            .iter()
            .map(|value| value.as_f64().context("Source barrel rate of fire"))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(0f64, f64::max);
        let recovery = barrel["fire_recovery_seconds"].as_f64().unwrap_or(0.);
        let latched = gameplay["triggers"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|trigger| trigger["behavior"] == 1 && trigger["primary_barrel"] == index);
        if latched && recovery > 0. {
            rounds = rounds.min(1. / recovery);
        }
        if let Some(wait) = crate::halo_reach::firing::charge_wait(gameplay, index) {
            rounds = rounds.min(1. / f64::from(wait));
        }
        let shots = barrel["projectiles_per_shot"].as_i64().unwrap_or(1).max(1);
        rate = Some(rate.unwrap_or(0.).max((rounds * shots as f64) as f32));
    }
    Ok(rate
        .filter(|rate| rate.is_finite())
        .map(|rate| rate * spread(projectile) as f32))
}

/// The source weapon's gameplay in the behavior export.
fn gameplay(behavior: &Value) -> Result<&Value> {
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    Ok(&behavior["nodes"][format!("{root:08X}")]["data"]["object"]["gameplay"])
}

/// The barrels that fire `projectile`, by index, or every barrel when none names it.
fn firing<'a>(gameplay: &'a Value, projectile: &Value) -> Vec<(usize, &'a Value)> {
    let barrels = gameplay["barrels"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let named = barrels
        .iter()
        .any(|barrel| barrel["projectile"]["datum"] == projectile["tag"]["datum"]);
    barrels
        .iter()
        .enumerate()
        .filter(|(_, barrel)| !named || barrel["projectile"]["datum"] == projectile["tag"]["datum"])
        .collect()
}

/// The projectile's conical spread count, yaw times pitch for each row at `proj +0x344`.
fn spread(projectile: &Value) -> i64 {
    projectile["behavior"]["conical_spread"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            row["yaw_count"].as_i64().unwrap_or(1).max(1)
                * row["pitch_count"].as_i64().unwrap_or(1).max(1)
        })
        .sum::<i64>()
        .max(1)
}

/// System slots one weapon's shard finishes may hold. The contact host's share is 48, and three
/// captured exhaustions left between 86 and 159 slots beside stock world and event content, so
/// the finishes take 32 and both shares stay within the smallest margin.
const FINISH_SLOTS: f32 = 32.;

/// The material effects response type a contact answered with `response` plays. Types follow the
/// projectile responses from 15: Impact (Detonate) 15, Fizzle 16, Overpenetrate 17, Attach 18,
/// Bounce 19, Bounce (Dud) 20 and Fizzle (Ricochet) 21. That order is inferred from the inspected
/// MCC U13 weapon projectiles, whose material effects hold the types their responses use.
const FIRST_RESPONSE_TYPE: i16 = 15;

/// Where a projectile conversion's source recordings are, and the name of its graph section. The
/// name keeps the section's cue banks apart from another section's in the item's linked audio
/// namespace.
pub(super) struct Recordings<'a> {
    pub folder: &'a Path,
    pub section: &'a str,
}

/// The behavior node of source sound `tag`, or why no recording of it was exported.
fn recorded<'a>(
    behavior: &'a Value,
    tag: &Value,
    recordings: &Recordings<'_>,
) -> std::result::Result<&'a Value, &'static str> {
    let node = tag["datum"]
        .as_u64()
        .map(move |datum| &behavior["nodes"][format!("{datum:08X}")])
        .filter(|node| node["tag"]["group"] == "snd!");
    match node {
        Some(node) if recordings.folder.is_dir() => Ok(node),
        Some(_) => {
            Err("Source audio is not configured for this import, so no recording was exported")
        }
        None => {
            Err("The sound is outside the behavior closure, so its recordings were not exported")
        }
    }
}

/// Why a source's ground detonation and detonation sound stay with the native table.
const NO_CONTACT_DETONATION: &str = "The source does not detonate on contact with its default material, and only contacts play the impact table";

/// Why a source contact presentation that needs the private table has no route.
const UNWRITTEN: &str = "No private impact table was written";

/// The source projectile's detonation presentation (`proj +0x1A8`, `+0x1B8`, `+0x1C8`, `+0x218`)
/// in a receipt, with the ground detonation effect and the detonation sound a contact plays when
/// the source detonates on contact.
struct Detonation<'a> {
    ground: Option<Tag>,
    sound: Option<&'a Value>,
    report: Value,
}

fn detonation<'a>(
    behavior: &'a Value,
    projectile: &Value,
    recordings: &Recordings<'_>,
) -> Result<Detonation<'a>> {
    let contact = contact_response(projectile)? == Some(0);
    let mut result = Detonation {
        ground: None,
        sound: None,
        report: json!({"contact_detonation":contact}),
    };
    for (role, key) in [
        ("detonation_started", "started"),
        ("airborne_detonation", "airborne"),
        ("ground_detonation", "ground"),
        ("detonation_sound", "sound"),
    ] {
        let Some(edge) = projectile_edge(behavior, projectile, role)? else {
            continue;
        };
        let (status, reason) = match role {
            "ground_detonation" if contact && edge["tag"]["group"] == "effe" => {
                result.ground = Some(serde_json::from_value(edge["tag"].clone())?);
                ("pending", "The contact host has not been built")
            }
            "ground_detonation" => ("native", NO_CONTACT_DETONATION),
            "airborne_detonation" => (
                "native",
                "A detonation away from a contact runs through the movement's detonation graph at definition +0x180, whose presentation stays native",
            ),
            "detonation_started" => (
                "omitted",
                "No carrier slot plays an effect when the source detonation timer starts",
            ),
            _ if !contact => ("native", NO_CONTACT_DETONATION),
            _ => match recorded(behavior, &edge["tag"], recordings) {
                Ok(node) => {
                    result.sound = Some(node);
                    ("pending", "The contact host has not been built")
                }
                Err(reason) => ("omitted", reason),
            },
        };
        result.report[key] = json!({"status":status,"source":edge["tag"],"reason":reason});
    }
    Ok(result)
}

/// The source flyby sound (`proj +0x240`) and damage response (`proj +0x250`).
fn flyby(behavior: &Value, projectile: &Value) -> Result<Value> {
    let mut report = json!({});
    if let Some(edge) = projectile_edge(behavior, projectile, "flyby_sound")? {
        report["sound"] = json!({"status":"omitted","source":edge["tag"],
            "reason":"No native projectile flyby route is established"});
    }
    if let Some(edge) = projectile_edge(behavior, projectile, "flyby_response")? {
        report["response"] = json!({"status":"omitted","source":edge["tag"],
            "reason":"No native projectile flyby route is established for a source damage response"});
    }
    Ok(report)
}

/// A private copy of the movement's native impact table `donor` whose rows create the source's
/// contact effect and name its contact sound, with the source's detonation and flyby presentation
/// in the receipt.
///
/// The source projectile names a material effects tag for its contacts. The response type of its
/// ordinary default-material response is the contact's, and that type's entry without a material
/// name, or else its default material's, serves every material, for effects and for sounds alike.
/// A source whose ordinary response is Impact (Detonate) also plays its ground detonation effect
/// and detonation sound there. One private host entity plays these effects and that sound together
/// where it is created and retires after their particles.
///
/// A `80808BCD` table has three branch arrays at `+0x80`, `+0x98` and `+0xB0`, of 24-byte groups
/// (`80808BD5`) holding 32-byte rows (`80808BD7`). A group's first word is its kind, and each
/// branch's `+0x10` word masks the kinds its groups hold. A row's first word names a native
/// physical material (`80808BCB` `+0x8`), and its leaf array of 32-bit resources (`80800014`) is at
/// `+0x8`. The first branch's rows name sound cues (`80809802`) and the others name graphs
/// (`80809C0F`). Each row's first graph names the host and its other graphs are cleared. Every cue
/// names the source contact sound when it converts, as the private sword surface table does for
/// melee hits, and otherwise stays.
#[allow(clippy::too_many_arguments)]
pub(super) fn impact(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    graph: &mut Graph,
    behavior: &Value,
    projectile: &Value,
    donor: u32,
    recordings: &Recordings<'_>,
) -> Result<Impact> {
    let mut detonation = detonation(behavior, projectile, recordings)?;
    let Contact {
        mut impact,
        ground: ground_effect,
        sound,
    } = contact(
        cache,
        pages,
        native,
        scene,
        graph,
        (behavior, projectile),
        donor,
        &detonation,
        recordings,
    )?;
    if let Some(source) = &detonation.ground {
        detonation.report["ground"] = match ground_effect {
            Some(effect) if impact.table.is_some() && effect["error"].is_null() => {
                let emitted = effect["emitted_objects"].as_u64().unwrap_or(0)
                    + effect["emitted_particle_systems"].as_u64().unwrap_or(0);
                if emitted > 0 {
                    json!({"status":"translated","source":source,"effect":effect,
                        "limits":["The contact host plays the ground detonation effect beside the contact effect on every native material row. The movement's native detonation graph keeps its own presentation."]})
                } else {
                    json!({"status":"omitted","source":source,"effect":effect,
                        "reason":"The source ground detonation effect has no convertible part"})
                }
            }
            Some(effect) if !effect["error"].is_null() => {
                json!({"status":"omitted","source":source,"effect":effect,
                    "reason":"The source ground detonation effect does not convert"})
            }
            _ => json!({"status":"native","source":source,"reason":UNWRITTEN}),
        };
    }
    if detonation.sound.is_some() {
        let source = detonation.report["sound"]["source"].clone();
        detonation.report["sound"] = match sound {
            Some(cue) if cue["error"].is_null() => {
                json!({"status":"translated","source":source,"cue":cue,
                    "limits":["The contact host plays the detonation sound once on every native material row, since the source detonates on contact. A detonation away from a contact runs through the movement's native detonation graph, which keeps its own sounds."]})
            }
            Some(cue) => json!({"status":"omitted","source":source,"cue":cue,
                "reason":"The source detonation sound does not convert"}),
            None => json!({"status":"native","source":source,"reason":UNWRITTEN}),
        };
    }
    impact.report["detonation"] = detonation.report;
    impact.report["flyby"] = flyby(behavior, projectile)?;
    Ok(impact)
}

/// What [`contact`] made: the impact table, and the reports of the ground detonation effect and
/// the detonation sound its host plays, when it compiled them.
struct Contact {
    impact: Impact,
    ground: Option<Value>,
    sound: Option<Value>,
}

/// The entry of `field`, `effects` or `sounds`, of material effects type `kind` that serves every
/// material: the one without a material name, or else the default material's.
fn default_entry(kind: &Value, field: &str) -> Option<Value> {
    let entries = kind[field].as_array()?;
    let set = |entry: &&Value| !entry["tag"].is_null();
    entries
        .iter()
        .filter(set)
        .find(|entry| entry["material"] == "")
        .or_else(|| {
            entries
                .iter()
                .filter(set)
                .find(|entry| entry["global_material"] == 0)
        })
        .cloned()
}

/// A contact report that leaves the movement's native impact table in place.
fn kept(report: Value) -> Contact {
    Contact {
        impact: Impact {
            table: None,
            report,
        },
        ground: None,
        sound: None,
    }
}

/// The contact table of [`impact`] and the reports of the ground detonation effect and detonation
/// sound its host plays.
#[allow(clippy::too_many_arguments)]
fn contact(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    graph: &mut Graph,
    (behavior, projectile): (&Value, &Value),
    donor: u32,
    detonation: &Detonation<'_>,
    recordings: &Recordings<'_>,
) -> Result<Contact> {
    let edges = behavior["edges"]
        .as_array()
        .context("Projectile effect edges")?;
    let found = edges
        .iter()
        .filter(|edge| {
            edge["owner"] == projectile["tag"]["datum"]
                && edge["role"] == "material_response"
                && edge["tag"]["group"] == "foot"
        })
        .collect::<Vec<_>>();
    let [edge] = found.as_slice() else {
        return Ok(kept(
            json!({"status":"native","reason":"The source projectile names no single material effects tag"}),
        ));
    };
    let source: Tag = serde_json::from_value(edge["tag"].clone())?;
    let effects = match crate::halo_reach::effect::material::read(cache, source.address()?) {
        Ok(effects) => effects,
        Err(error) => {
            return Ok(kept(json!({"status":"native",
                "reason":format!("The source material effects have another layout: {error:#}")})));
        }
    };
    // The type of the source's ordinary default-material response is the contact's. Its entry
    // without a material name, or else its default material's, stands for every material.
    let Some(response) = contact_response(projectile)? else {
        return Ok(kept(json!({"status":"native",
            "reason":"The source projectile has no unconditional response for its default material"})));
    };
    if !(0..=6).contains(&response) {
        return Ok(kept(json!({"status":"native",
            "reason":format!("The source's default-material response {response} has no known material effects type")})));
    }
    let types = effects["types"].as_array().cloned().unwrap_or_default();
    let response_type = usize::try_from(FIRST_RESPONSE_TYPE + response)?;
    let Some(entry) = types
        .get(response_type)
        .and_then(|kind| default_entry(kind, "effects"))
    else {
        return Ok(kept(
            json!({"status":"native","source":source,"material_effects":effects,
                "contact_response":response,"response_type":response_type,
                "reason":"The source material effects name no default effect for the contact response's type"}),
        ));
    };
    let kind = &types[response_type];
    let tag: Tag = serde_json::from_value(entry["tag"].clone())?;
    let sound_entry = default_entry(kind, "sounds");
    let unwritten = || {
        materials(
            kind,
            &entry,
            sound_entry
                .as_ref()
                .map(|sound| (sound, json!({"status":"omitted","reasons":[UNWRITTEN]}))),
        )
    };
    let mut sources = vec![(&tag, "projectile-impact")];
    if let Some(ground) = &detonation.ground {
        sources.push((ground, "projectile-impact-detonation"));
    }
    let sounds = detonation
        .sound
        .map(|sound| (sound, "detonation"))
        .into_iter()
        .collect::<Vec<_>>();
    let hosted = match crate::halo_reach::native::effect_host(
        cache,
        pages,
        native,
        graph,
        scene,
        "projectile-impact",
        &sources,
        (
            sounds.as_slice(),
            behavior,
            recordings.folder,
            recordings.section,
        ),
        contact_rate(behavior, projectile)?,
    ) {
        Ok(hosted) => hosted,
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(error) => {
            return Ok(kept(
                json!({"status":"native","source":source,"material_effects":effects,
                    "chosen":entry,"materials":unwritten(),
                    "reason":format!("The source contact host does not convert: {error:#}")}),
            ));
        }
    };
    let mut converted = hosted.effects;
    let ground_effect = (converted.len() > 1).then(|| converted.remove(1));
    let converted = converted.remove(0);
    let Some(host) = hosted.entity else {
        let reason = match converted["error"].as_str() {
            Some(error) => format!("The source contact effect does not convert: {error}"),
            None => "The source contact effect has no convertible part".into(),
        };
        return Ok(Contact {
            ground: ground_effect,
            ..kept(
                json!({"status":"native","source":source,"material_effects":effects,
                "chosen":entry,"effect":converted,"materials":unwritten(),"budget":hosted.budget,
                "reason":reason}),
            )
        });
    };
    let converted_sound = sound_entry
        .as_ref()
        .map(|sound| contact_sound(native, graph, behavior, sound, recordings))
        .transpose()?;
    let cue = converted_sound.as_ref().and_then(|(cue, _)| cue.as_deref());
    let (table, patches, mut report) = private_table(native, donor, &host, cue)?;
    let symbol = "projectile-impact-table";
    graph.add(symbol, donor, &table.0, None, patches)?;
    let sound_limit = if cue.is_some() {
        "Every native cue slot of the table names the source contact sound in place of the carrier's per-material cues."
    } else {
        "The table's native per-material sound cues stay."
    };
    let materials = materials(
        kind,
        &entry,
        sound_entry
            .as_ref()
            .zip(converted_sound.map(|(_, record)| record)),
    );
    let fields = json!({"status":"translated","source":source,"material_effects":effects,
        "contact_response":response,"response_type":response_type,"chosen":entry,
        "effect":converted,"materials":materials,"budget":hosted.budget,
        "native_table":format!("{donor:08X}"),"gameplay_verified":false,
        "limits":["Every native material row takes the chosen contact effect and contact sound, since native rows name physical materials by hashed names with no known Reach counterpart.",
            sound_limit,
            "The host keeps the shortest-lived particle systems while the weapon's contact rate times their slot residency stays within the contact share of the client's 512 particle system slots. The others are pending with the system budget reason."]});
    if let (Some(report), Some(fields)) = (report.as_object_mut(), fields.as_object()) {
        report.extend(fields.clone());
    }
    Ok(Contact {
        impact: Impact {
            table: Some(symbol.into()),
            report,
        },
        ground: ground_effect,
        sound: hosted.sounds.into_iter().next(),
    })
}

/// A private copy of impact table `donor` whose rows' first graphs name `host`, whose other graphs
/// are cleared and whose cues name `cue` when there is one, with its patches and its row counts.
///
/// A row whose cues name `cue` also sets its byte at `+0x18`, which keeps the contacted physical
/// material's own sound branch from playing beside the row's cues, so the source sound alone
/// answers the contact.
fn private_table(
    native: &mut Reader,
    donor: u32,
    host: &str,
    cue: Option<&str>,
) -> Result<(Payload, Vec<Value>, Value)> {
    let mut table = (*native.tag(donor, Some(0x8080_8bcd))?).clone();
    let mut patches = Vec::new();
    let (mut hosted, mut cleared, mut retained, mut replaced) = (0, 0, 0, 0);
    let mut suppressed = 0;
    let mut rows = [0; 3];
    for (branch, descriptor) in [0x80, 0x98, 0xb0].into_iter().enumerate() {
        for group in table.array(descriptor, 24, Some(0x8080_8bd5))? {
            for row in table.array(group + 8, 32, Some(0x8080_8bd7))? {
                rows[branch] += 1;
                let mut first = true;
                let named = replaced;
                for slot in table.array(row + 8, 4, Some(0x8080_0014))? {
                    let resource = table.u32(slot)?;
                    if matches!(resource, 0 | u32::MAX) {
                        continue;
                    }
                    let target = if native.tag(resource, Some(0x8080_9c0f)).is_ok() {
                        let target = first.then_some(host);
                        hosted += usize::from(first);
                        cleared += usize::from(!first);
                        first = false;
                        target
                    } else if let Some(cue) =
                        cue.filter(|_| native.tag(resource, Some(0x8080_9802)).is_ok())
                    {
                        replaced += 1;
                        Some(cue)
                    } else {
                        retained += 1;
                        continue;
                    };
                    if let Some(target) = target {
                        patches.push(json!({"offset":slot,"symbol":target}));
                    }
                    put(&mut table.0, slot, &u32::MAX.to_le_bytes())?;
                }
                if replaced > named && table.u8(row + 0x18)? == 0 {
                    table.0[row + 0x18] = 1;
                    suppressed += 1;
                }
            }
        }
    }
    ensure!(hosted > 0, "Native impact table names no entity");
    Ok((
        table,
        patches,
        json!({"native_rows":{"cue_branch":rows[0],"graph_branches":[rows[1],rows[2]]},
            "hosted_rows":hosted,"cleared_entities":cleared,"kept_sounds":retained,
            "replaced_sounds":replaced,"suppressed_material_sounds":suppressed}),
    ))
}

/// The cue made from chosen contact sound `entry` for the impact table's cue slots, if it
/// converts, and its record in the materials receipt.
fn contact_sound(
    native: &mut Reader,
    graph: &mut Graph,
    behavior: &Value,
    entry: &Value,
    recordings: &Recordings<'_>,
) -> Result<(Option<String>, Value)> {
    let node = match recorded(behavior, &entry["tag"], recordings) {
        Ok(node) => node,
        Err(reason) => return Ok((None, json!({"status":"omitted","reasons":[reason]}))),
    };
    match crate::halo_reach::native::contact_cue(
        native,
        graph,
        node,
        "projectile-impact-sound-contact",
        &format!("{}/contact", recordings.section),
        recordings.folder,
    ) {
        Ok((cue, report)) => Ok((
            Some(cue),
            json!({"status":"translated","cue":report,
                "limits":["Every native cue slot plays the sound, for every surface and for each native contact kind the table's cue rows serve."]}),
        )),
        Err(error) if crate::cancellation::is_cancelled(&error) => Err(error),
        Err(error) => Ok((
            None,
            json!({"status":"omitted",
                "reasons":[format!("The source contact sound does not convert: {error:#}")]}),
        )),
    }
}

/// Native impact rows name physical materials by hashed names, and no Reach global material is
/// known to match one.
const MATERIAL_ROUTE: &str = "Native impact rows name physical materials by hashed names with no known Reach counterpart, so every row takes the chosen effect";

/// Why a material effect entry's secondary reference stays out.
const SECONDARY: &str =
    "The role of a material effect entry's secondary reference is not established";

/// What becomes of the chosen response type `kind`'s other per-material effects and sounds, of
/// the chosen effect `entry`'s secondary effect, and of the chosen contact `sound` entry, whose
/// record follows it.
fn materials(kind: &Value, entry: &Value, sound: Option<(&Value, Value)>) -> Value {
    let effects = kind["effects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| !row["tag"].is_null() && *row != entry)
        .map(|row| {
            json!({"material":row["material"],"global_material":row["global_material"],
                "source":row["tag"],"status":"omitted","reason":MATERIAL_ROUTE})
        })
        .collect::<Vec<_>>();
    let (chosen, record) = sound.unzip();
    let sounds = kind["sounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| !row["tag"].is_null())
        .map(|row| {
            let mut result = json!({"material":row["material"],
                "global_material":row["global_material"],"source":row["tag"]});
            match (chosen, record.as_ref().and_then(Value::as_object)) {
                (Some(chosen), Some(fields)) if chosen == row => {
                    if let Some(result) = result.as_object_mut() {
                        result.extend(fields.clone());
                    }
                    if !row["secondary"].is_null() {
                        result["secondary"] = json!({"source":row["secondary"],
                            "status":"omitted","reason":SECONDARY});
                    }
                }
                _ => {
                    result["status"] = json!("omitted");
                    result["reasons"] = json!([MATERIAL_ROUTE]);
                }
            }
            result
        })
        .collect::<Vec<_>>();
    let secondary = if entry["secondary"].is_null() {
        Value::Null
    } else {
        json!({"source":entry["secondary"],"status":"omitted","reason":SECONDARY})
    };
    json!({"effects":effects,"contact_sounds":sounds,"secondary":secondary})
}

/// Names `table` as the impact table at definition `+0x1E0` of the one movement in `owners`, in
/// both of its definition views.
pub(super) fn place_impact(owners: &mut BTreeMap<u32, Payload>, table: u32) -> Result<()> {
    let mut movements = owners
        .values_mut()
        .filter(|p| {
            p.pointer(24)
                .and_then(|d| p.u32(d - 4))
                .is_ok_and(|class| class == 0x8080_388f)
        })
        .collect::<Vec<_>>();
    let [p] = movements.as_mut_slice() else {
        anyhow::bail!("Projectile movement is absent or ambiguous")
    };
    let definition = p.pointer(24)?;
    let views = [definition, usize::try_from(p.u64(p.pointer(16)? + 8)?)?];
    for view in views {
        put(&mut p.0, view + 0x1e0, &table.to_le_bytes())?;
    }
    Ok(())
}

/// The native impact table the one movement in `owners` names, if any.
pub(super) fn native_impact(owners: &BTreeMap<u32, Payload>) -> Result<Option<u32>> {
    let movements = owners
        .values()
        .filter(|p| {
            p.pointer(24)
                .and_then(|d| p.u32(d - 4))
                .is_ok_and(|class| class == 0x8080_388f)
        })
        .collect::<Vec<_>>();
    let [p] = movements.as_slice() else {
        return Ok(None);
    };
    let table = p.u32(p.pointer(24)? + 0x1e0)?;
    Ok((table != u32::MAX).then_some(table))
}

/// Native billboard systems converted from the source projectile's super detonation effect, each
/// with its definition's transform row count (`+0x47`), and the longest particle life among them.
/// A finish keeps its host alive that long, since these systems end with their host. `light`
/// recolors the finish's native light.
#[derive(Default)]
pub(super) struct Explosion {
    pub systems: Vec<(String, u8)>,
    pub life: f32,
    pub light: Option<Light>,
    pub report: Value,
}

/// A private copy of the native light record `template` in the source detonation light's color,
/// and the seconds the source light lasts, zero for no limit.
pub(in crate::halo_reach) struct Light {
    pub symbol: String,
    pub template: u32,
    pub seconds: f32,
}

/// Converts the supported emitters of the source `super_detonation` effect, as damage effects
/// convert theirs, at the finish's own origin, and gives the finish's native light the effect's
/// first light color. Event delays and the effect's sounds, light intensities, screen effects and
/// contrail emitters are not carried.
///
/// The shard collector finishes each collected projectile through the finish graph, one per native
/// update, and a lone projectile finishes through it when its timer ends. Finishes therefore start
/// at the weapon's contact rate, each with every converted system. They keep the shortest-lived
/// systems while `rate * 2 * life * systems` stays within `FINISH_SLOTS`, the contact host's
/// estimate of live slots with no event timeline, and the rest stay pending with the reason.
pub(super) fn explosion(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    graph: &mut Graph,
    behavior: &Value,
    projectile: &Value,
) -> Result<Explosion> {
    let edges = behavior["edges"]
        .as_array()
        .context("Projectile effect edges")?;
    let tags = edges
        .iter()
        .filter(|edge| {
            edge["owner"] == projectile["tag"]["datum"]
                && edge["role"] == "super_detonation"
                && edge["tag"]["group"] == "effe"
        })
        .collect::<Vec<_>>();
    let [edge] = tags.as_slice() else {
        return Ok(Explosion {
            report: json!({"status":"native","reason":"The source projectile has no single super detonation effect"}),
            ..Explosion::default()
        });
    };
    let tag: Tag = serde_json::from_value(edge["tag"].clone())?;
    let effect = source::read(cache, tag.address()?)?;
    let frame = crate::halo_reach::rig::transform([0.; 3], [0., 0., 0., 1.]);
    let mut result = Explosion::default();
    let mut emitted = Vec::new();
    let mut pending = Vec::new();
    let mut candidates = Vec::new();
    for (event_index, event) in effect["events"]
        .as_array()
        .context("Super detonation events")?
        .iter()
        .enumerate()
    {
        for (system_index, system) in event["particles"]
            .as_array()
            .context("Super detonation particles")?
            .iter()
            .enumerate()
        {
            if system["tag"].is_null() {
                continue;
            }
            for (emitter_index, emitter) in system["emitters"]
                .as_array()
                .context("Super detonation emitters")?
                .iter()
                .enumerate()
            {
                let reasons = particle::pending(cache, system, emitter)?;
                if !reasons.is_empty() {
                    pending.push(json!({"event":event_index,"system":system_index,
                        "emitter":emitter_index,"source":system["tag"],"reasons":reasons}));
                    continue;
                }
                let parsed =
                    particle::read(cache, native, system, emitter, scene.metres(), &frame)?;
                candidates.push(((event_index, system_index, emitter_index), system, parsed));
            }
        }
    }
    let rate = contact_rate(behavior, projectile)?;
    let mut order = (0..candidates.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| candidates[a].2.life.total_cmp(&candidates[b].2.life));
    let mut admitted = std::collections::BTreeSet::new();
    for (count, &index) in order.iter().enumerate() {
        let life = candidates[index].2.life;
        if !rate.is_none_or(|rate| rate * 2. * life * (count + 1) as f32 <= FINISH_SLOTS) {
            break;
        }
        admitted.insert(index);
    }
    for (index, ((event_index, system_index, emitter_index), system, parsed)) in
        candidates.into_iter().enumerate()
    {
        if !admitted.contains(&index) {
            pending.push(json!({"event":event_index,"system":system_index,
                "emitter":emitter_index,"source":system["tag"],"maximum_life":parsed.life,
                "reasons":["Finish system budget. Another system in every finish would keep more of the client's system slots alive at the weapon's contact rate than the finishes' share allows"]}));
            continue;
        }
        result.life = result.life.max(parsed.life);
        let rows = parsed.definition.u8(0x47)?;
        let (symbol, report) = particle::emit(
            cache,
            pages,
            native,
            graph,
            &parsed,
            &format!("supercombine-{event_index}-{system_index}-{emitter_index}"),
        )?;
        result.systems.push((symbol, rows));
        emitted.push(report);
    }
    let lights = effect["events"]
        .as_array()
        .context("Super detonation events")?
        .iter()
        .flat_map(|event| event["parts"].as_array().into_iter().flatten())
        .filter(|part| part["tag"]["group"] == "ligh")
        .collect::<Vec<_>>();
    let light = match lights.first() {
        _ if result.systems.is_empty() => {
            json!({"status":"native","reason":"The native finish keeps its presentation"})
        }
        None => json!({"status":"native","reason":"The source detonation has no light"}),
        Some(part) => {
            let tag: Tag = serde_json::from_value(part["tag"].clone())?;
            let (color, seconds) = crate::halo_reach::level::native::lights::color(cache, &tag)?;
            match light::template(native, color) {
                Ok(template) => {
                    let (placed, report) = template.add(native, graph, seconds)?;
                    result.light = Some(placed);
                    json!({"status":"translated","source":tag,"lights":lights.len(),
                        "linear_color":color,"seconds":seconds,"native":report,
                        "gameplay_verified":false,
                        "limits":["Hue and saturation are the source light's, at the native light's own brightness and range. The source intensity curve is not carried."]})
                }
                Err(error) => json!({"status":"native","source":tag,
                    "reason":format!("The native finish light has another structure: {error:#}")}),
            }
        }
    };
    result.report = json!({"status":if result.systems.is_empty() { "native" } else { "translated" },
        "source":tag,"systems":result.systems.len(),"maximum_particle_life":result.life,
        "budget":{"contacts_per_second":rate,"slots":FINISH_SLOTS,"admitted":admitted.len(),
            "dropped":order.len() - admitted.len()},
        "particles":emitted,"pending":pending,"light":light,"gameplay_verified":false,
        "limits":["Source event delays, sounds, screen effects and contrail emitters are not carried. Every converted emitter starts at the finish origin when the finish runs.",
            "Each collected shard plays the finish, so one source supercombine draws as many overlapping copies as the collection holds."]});
    Ok(result)
}
