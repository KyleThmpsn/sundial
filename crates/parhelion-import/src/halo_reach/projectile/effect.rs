//! Attached source contrails on private projectile flight effects.
mod host;
use crate::{
    halo_reach::{Scene, cache::Cache, effect::trail, resource::Pages},
    presentation::{Graph, append, put},
    tiger::{markers, payload::Payload, reader::Reader},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
            // Follow the stored parent chain. Conditional branches, named
            // triggers and delayed starts are different effect contracts.
            let mut parent = p.u32(event + 0x14)?;
            let mut reachable = false;
            for _ in 0..controls.len() {
                if parent & 0xffff != 0 {
                    break;
                }
                let ordinal = (parent >> 16) as usize;
                let at =
                    p.pointer(*controls.get(ordinal).context("Projectile effect parent")? + 16)?;
                if !matches!(p.u32(at - 4)?, 0x808093d7 | 0x808093d9)
                    || p.0[at + 0x18..at + 0x30].iter().any(|&v| v != 0)
                    || p.0[at + 0x48..at + 0x58].iter().any(|&v| v != 0)
                {
                    break;
                }
                if parent == entry {
                    reachable = true;
                    break;
                }
                parent = p.u32(at + 0x14)?;
            }
            if !reachable {
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

pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    behavior: &Value,
    assembly: &mut super::assembly::Assembly,
    root: &std::path::Path,
) -> Result<Effects> {
    let mut profiles = Vec::new();
    let mut attachments = Vec::new();
    let mut origin = None;
    for attachment in &scene.object.attachments {
        if attachment["tag"]["group"] != "effe" {
            continue;
        }
        let effect: crate::halo_reach::cache::Tag =
            serde_json::from_value(attachment["tag"].clone())?;
        for event in cache.block(effect.address()? + 0x2c, 0x40)? {
            for part in cache.block(event + 0x1c, 0x64)? {
                let Some(tag) = cache.reference(part + 0x14, None)? else {
                    continue;
                };
                if tag.group != "cntl" {
                    continue;
                }
                let (position, frame) = source_frame(cache, &effect, part, attachment, scene)?;
                ensure!(
                    origin.is_none_or(|old| old == position),
                    "Projectile contrails require separate origins"
                );
                origin = Some(position);
                profiles.extend(trail::read(cache, &tag)?);
                attachments.push(json!({"attachment":attachment,"frame":frame}));
            }
        }
    }
    std::fs::create_dir_all(root)?;
    let mut graph = Graph {
        root: root.into(),
        nodes: Vec::new(),
    };
    if profiles.is_empty() {
        return Ok(Effects {
            graph,
            patches: Vec::new(),
            report: json!({"trails":[]}),
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
    let mut marker_owners = Vec::new();
    for (&owner, p) in owners.iter() {
        let d = p.pointer(24)?;
        if p.u32(d - 4)? != 0x80808507 {
            continue;
        }
        let matching = markers::read_native(p)?
            .into_iter()
            .filter(|m| m.name == target.name)
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
    let origin = origin.context("Projectile trail origin is absent")?;
    marker.position = [origin[0], origin[1], origin[2], 1.];
    // The straight beam approximates the projectile's historical path. Its
    // direction stays opposite flight while its origin follows the source marker.
    marker.orientation = [0., 0., 1., 0.];
    let (payload, _) = markers::rewrite(&owners[&marker_owner], &[marker])?;
    owners.insert(marker_owner, payload);

    let speed = behavior["initial_velocity"]
        .as_f64()
        .context("Projectile trail reference speed")? as f32
        * scene.metres();
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
    let mut systems = Vec::new();
    let mut reports = Vec::new();
    for (index, profile) in profiles.iter().enumerate() {
        let (symbol, report) = trail::emit(
            cache,
            pages,
            native,
            &mut graph,
            profile,
            0x80f2f261,
            &format!("projectile-trail-{index}"),
            scene.metres(),
            speed,
        )?;
        systems.push(symbol);
        reports.push(report);
    }
    let mut resources = crate::tiger::sequence::particle::write(
        p,
        target.event,
        &systems,
        u8::try_from(target.locator)?,
    )?
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
        report: json!({"trails":reports,"source_attachments":attachments,
        "native_owner":format!("{:08X}",target.owner),"native_event":target.event,
        "native_marker":format!("{:08X}",target.name),"authored_host":authored,"origin_metres":origin,"reference_speed_metres_per_second":speed,
        "limits":["The autonomous flight branch contains only source attached contrails and lasts until the projectile retires. Other impact graphs remain native. Source attachment scale functions and projectile speed changes require separate runtime translation."]}),
    })
}
