//! Source projectile presentations and private native carriers with explicit behavior limits.
mod assembly;
mod effect;
pub(super) mod motion;
mod program;
mod shard;
mod tracking;
use super::*;
use crate::presentation::put;
use crate::tiger::{
    entity::compiled,
    payload::Payload,
    projectile::Assets,
    reader::{Reader, write_json},
};

pub(super) fn export(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    behavior: &Value,
    root: &Path,
) -> Result<Value> {
    let mut report = Vec::new();
    for node in behavior["nodes"]
        .as_object()
        .context("Behavior nodes")?
        .values()
    {
        if node["tag"]["group"] != "proj" {
            continue;
        }
        let tag = cache.find(
            "proj",
            node["tag"]["path"].as_str().context("Projectile path")?,
        )?;
        let object = Object::read(cache, tag)?;
        let mut row = json!({"tag":tag,"behavior":node["data"],"guidance":tracking::policy(&node["data"] )?,"has_model":false});
        if object
            .world
            .as_ref()
            .and_then(|b| b.model.as_ref())
            .is_some()
        {
            let request = ExportRequest {
                cache: cache
                    .path
                    .file_name()
                    .context("Cache filename")?
                    .to_string_lossy()
                    .into_owned(),
                group: "proj".into(),
                path: tag.path.clone(),
                view: View::World,
                variant: None,
                permutations: BTreeMap::new(),
                audio: None,
            };
            let scene = scene(cache, pages, &request)?;
            let directory = format!("projectiles/{:08X}", tag.datum);
            let destination = root.join(&directory);
            fs::create_dir_all(&destination)?;
            gltf::write(cache, pages, &scene, &destination)?;
            write_json(
                &destination.join("source.json"),
                &serde_json::to_value(&scene)?,
            )?;
            row["has_model"] = json!(true);
            row["directory"] = json!(directory);
            row["vertices"] = json!(
                scene
                    .models
                    .iter()
                    .flat_map(|m| &m.model.primitives)
                    .map(|p| p.vertices.len())
                    .sum::<usize>()
            );
            row["triangles"] = json!(
                scene
                    .models
                    .iter()
                    .flat_map(|m| &m.model.primitives)
                    .map(|p| p.indices.len() / 3)
                    .sum::<usize>()
            );
        }
        report.push(row);
    }
    let report = json!(report);
    write_json(&root.join("projectiles.json"), &report)?;
    Ok(report)
}

fn primary<'a>(behavior: &'a Value, projectiles: &'a Value) -> Result<&'a Value> {
    let root = &behavior["root"]["datum"];
    let ids = behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["owner"] == *root && e["role"] == "projectile")
        .map(|e| e["tag"]["datum"].as_u64().context("Projectile identity"))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        ids.len() == 1,
        "Native projectile translation requires one primary source projectile"
    );
    let id = *ids.first().unwrap();
    projectiles.as_array().context("Projectile presentations")?.iter()
        .find(|p| p["tag"]["datum"] == id && p["has_model"] == true)
        .context("Primary source projectile has no render model. Its effects require their own translator")
}

fn number(value: &Value) -> Result<u32> {
    Ok(u32::try_from(
        value.as_u64().context("Native graph identity")?,
    )?)
}

fn resolved(
    node: &Value,
    root: &Path,
    symbols: &BTreeMap<String, u32>,
) -> Result<(Payload, Option<u32>)> {
    let mut payload = Payload(fs::read(
        root.join(node["file"].as_str().context("Presentation file")?),
    )?);
    for patch in node["patches"].as_array().into_iter().flatten() {
        let at = usize::try_from(patch["offset"].as_u64().context("Presentation patch")?)?;
        ensure!(
            payload.u32(at)? == u32::MAX,
            "Presentation patch has no placeholder"
        );
        let target = *symbols
            .get(patch["symbol"].as_str().context("Patch symbol")?)
            .context("Missing presentation dependency")?;
        put(&mut payload.0, at, &target.to_le_bytes())?;
    }
    let reference = node["reference"]
        .as_str()
        .map(|s| symbols.get(s).copied().context("Missing package reference"))
        .transpose()?;
    Ok((payload, reference))
}

// Relocate only typed component references and the separately supplied component table.
fn relocate(
    payload: &mut Payload,
    owners: &BTreeMap<u32, Payload>,
    ids: &BTreeMap<u32, u32>,
    rows: &[usize],
) -> Result<()> {
    for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
        let old = payload.u32(at)?;
        let Some(owner) = owners.get(&old) else {
            continue;
        };
        if !rows.contains(&at) {
            let class = payload.u32(at + 4)?;
            let target = usize::try_from(payload.u64(at + 8)?)?;
            ensure!(
                target
                    .checked_add(16)
                    .is_some_and(|end| end <= owner.0.len())
                    && (class & 0xffff0000 == 0x80800000
                        || (target >= 4 && owner.u32(target - 4)? == class)),
                "Untyped native projectile owner occurrence at {at:X}"
            );
        }
        put(&mut payload.0, at, &ids[&old].to_le_bytes())?;
    }
    Ok(())
}

fn write_allocations(
    reader: &mut Reader,
    assets: &mut Assets,
    ids: &BTreeMap<u32, u32>,
    authored: &BTreeMap<u32, Payload>,
) -> Result<()> {
    for (&tag, &id) in ids {
        let payload = match authored.get(&tag) {
            Some(payload) => payload.clone(),
            None => (*reader.tag(tag, Some(0x80809bbb))?).clone(),
        };
        assets.push(id, tag, payload, None)?;
    }
    Ok(())
}

#[allow(clippy::cognitive_complexity)]
pub(super) fn native(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
) -> Result<Value> {
    ensure!(
        entry.runtime_entity.is_none(),
        "Vehicle weapon routing needs an explicit vehicle controller translation"
    );
    let source = primary(behavior, projectiles)?;
    let mut request = entry.source.clone();
    request.group = "proj".into();
    request.path = source["tag"]["path"]
        .as_str()
        .context("Projectile path")?
        .into();
    request.view = View::World;
    request.variant = None;
    request.permutations.clear();
    let scene = scene(cache, pages, &request)?;
    let presentation_root = root.join("projectile-presentation");
    let presentation = super::native::build(
        cache,
        pages,
        reader,
        &scene,
        entry,
        &presentation_root,
        true,
        None,
    )?;
    let carrier = number(&presentation["source_entity"])?;
    let model_owner = number(&presentation["source_owner"])?;
    let (mut assembly, shards) = assembly::Assembly::prepare(reader, carrier, &source["behavior"])?;
    let guidance = json!({"source":tracking::policy(&source["behavior"] )?,
        "translation":if tracking::enabled(&source["behavior"])? { "Source sampled angular rate and commanded-distance ramp on native acquisition" }
            else { "Retained native carrier guidance" },"gameplay_verified":false});
    let motion = motion::apply(
        &assembly.entity,
        &mut assembly.owners,
        &source["behavior"],
        rig::METRES_PER_UNIT,
        assembly.source_trajectory,
    )?;
    let effects = effect::build(
        cache,
        pages,
        reader,
        &scene,
        &source["behavior"],
        &mut assembly,
        &root.join("projectile-effects"),
    )?;
    let original = assembly.entity;
    let mut owners = assembly.owners;
    let rows = original.array(16, 12, Some(0x80809c04))?;
    let mut assets = Assets::default();
    let mut symbols = BTreeMap::new();
    let nodes = presentation["nodes"]
        .as_array()
        .context("Projectile presentation nodes")?;
    for node in nodes {
        let symbol = node["symbol"].as_str().context("Presentation symbol")?;
        symbols.insert(
            symbol.to_owned(),
            assets.reserve(reader, format!("reach-projectile-{symbol}"))?,
        );
    }
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for &row in &rows {
        let tag = original.u32(row)?;
        let payload = owners.get(&tag).context("Assembled projectile component")?;
        let id = if tag == model_owner {
            symbols["owner"]
        } else {
            assets.reserve(reader, format!("reach-projectile-component-{tag:08X}"))?
        };
        ensure!(
            ids.insert(tag, id).is_none(),
            "Repeated native projectile component"
        );
        let allocation = payload.u32(0x44)?;
        if let std::collections::btree_map::Entry::Vacant(slot) = allocations.entry(allocation) {
            reader.tag(allocation, Some(0x80809bbb))?;
            slot.insert(assets.reserve(
                reader,
                format!("reach-projectile-allocation-{allocation:08X}"),
            )?);
        }
    }
    if let Some(owner) = effects.owner {
        // Flow nodes still read these controller inputs, so each input needs its entity
        // connection to resolve a source at runtime.
        let flow = &owners[&owner];
        let inputs = flow.array(flow.pointer(24)? + 0x188, 40, Some(0x80809789))?;
        let graph = crate::tiger::entity::links::Graph::read(&original, false)?;
        let connected = graph
            .connections
            .iter()
            .chain(&graph.named_connections)
            .filter_map(|row| row.consumer.object)
            .filter(|object| object.owner == owner && object.class == 0x80809789)
            .count();
        ensure!(
            inputs.is_empty() || connected > 0,
            "Projectile flow inputs have no entity connections"
        );
    }
    let rows = original.array(16, 12, Some(0x80809c04))?;
    let shard_report = shards
        .as_ref()
        .map(|shards| shards.append(reader, &mut assets, &mut owners))
        .transpose()?;
    let mut effect_ids = BTreeMap::new();
    for node in &effects.graph.nodes {
        let symbol = node["symbol"]
            .as_str()
            .context("Projectile effect symbol")?;
        effect_ids.insert(
            symbol.to_owned(),
            assets.reserve(reader, format!("reach-projectile-effect-{symbol}"))?,
        );
    }
    for (owner, offset, symbol) in &effects.patches {
        let target = effect_ids.get(symbol).context("Projectile effect target")?;
        put(
            &mut owners.get_mut(owner).context("Projectile effect owner")?.0,
            *offset,
            &target.to_le_bytes(),
        )?;
    }
    let mut converted = BTreeMap::new();
    for node in nodes {
        let symbol = node["symbol"].as_str().context("Presentation symbol")?;
        if symbol == "entity" {
            continue;
        }
        let (mut payload, reference) = resolved(node, &presentation_root, &symbols)?;
        if symbol == "owner" {
            let allocation = allocations[&owners[&model_owner].u32(0x44)?];
            put(&mut payload.0, 0x44, &allocation.to_le_bytes())?;
            converted.insert(model_owner, payload.clone());
        }
        assets.push(
            symbols[symbol],
            number(&node["template"])?,
            payload,
            reference,
        )?;
    }
    for node in &effects.graph.nodes {
        let symbol = node["symbol"]
            .as_str()
            .context("Projectile effect symbol")?;
        let (payload, reference) = resolved(node, &effects.graph.root, &effect_ids)?;
        assets.push(
            effect_ids[symbol],
            number(&node["template"])?,
            payload,
            reference,
        )?;
    }
    for (&tag, original) in &owners {
        if tag == model_owner {
            continue;
        }
        let mut payload = original.clone();
        relocate(&mut payload, &owners, &ids, &[])?;
        put(
            &mut payload.0,
            0x44,
            &allocations[&original.u32(0x44)?].to_le_bytes(),
        )?;
        assets.push(ids[&tag], tag, payload.clone(), None)?;
        converted.insert(tag, payload);
    }
    let mut authored_allocations = assembly.allocations;
    for (tag, payload) in effects.allocations {
        ensure!(
            authored_allocations.insert(tag, payload).is_none(),
            "Projectile allocation edits overlap"
        );
    }
    write_allocations(reader, &mut assets, &allocations, &authored_allocations)?;
    let mut entity = original.clone();
    relocate(&mut entity, &owners, &ids, &rows)?;
    let root_id = symbols["entity"];
    let replication_id = assets.reserve(reader, "reach-projectile-replication".into())?;
    let replication_template = original.u32(0x88)?;
    reader.tag(replication_template, Some(0x80809bb6))?;
    let components = rows
        .iter()
        .map(|&row| {
            let tag = original.u32(row)?;
            Ok(compiled::Component {
                owner: ids[&tag],
                payload: &converted[&tag],
                template: &owners[&tag],
                entity: &original,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    assets.push(
        replication_id,
        replication_template,
        compiled::replication::emit(root_id, &components)?,
        None,
    )?;
    put(&mut entity.0, 0x88, &replication_id.to_le_bytes())?;
    crate::tiger::entity::links::Graph::read(&entity, false)?;
    assets.push(root_id, carrier, entity, None)?;
    assets.write(root,root_id,json!({"source":source["tag"],"native_carrier":carrier,
        "vertices":presentation["vertices"],"triangles":presentation["triangles"],"source_behavior":source["behavior"],
        "launch_policy":if assembly.source_trajectory { Some("source_player_trajectory") } else { None },
        "native_skinning":presentation["native_skinning"],"motion":motion,"effects":effects.report,"shards":shard_report,"guidance":guidance,"gameplay_verified":false,
        "retained_native_behavior":["Native collision and damage calculation", "Native impact presentation", "Native acquisition geometry where source tracking is enabled"],
        "limits":["Source projectile geometry, supported flight contrails, gravity and range are translated. Guided and attached shard carriers also use source player speed curves. Source target acquisition, contact continuation, live-component relaunch and external modifiers require gameplay calibration."]}))
}
