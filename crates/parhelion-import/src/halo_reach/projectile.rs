//! Source projectile presentations and private native carriers with explicit behavior limits.
mod airstrike;
mod assembly;
mod contact;
mod detonation;
pub(super) mod effect;
mod effect_only;
pub(super) mod motion;
mod program;
mod remote;
pub(super) mod replication;
mod response;
mod shard;
mod tracking;

/// The guidance receipt of a carrier that steers by the source's sampled angular rate.
pub(super) const GUIDED: &str =
    "Source sampled angular rate and commanded-distance ramp on native acquisition";
use super::*;
use crate::presentation::{Graph, put};
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

/// The root weapon's source gameplay, as its behavior node stores it.
fn gameplay(behavior: &Value) -> &Value {
    let root = behavior["root"]["datum"].as_u64().unwrap_or(u64::MAX);
    &behavior["nodes"][format!("{root:08X}")]["data"]["object"]["gameplay"]
}

/// The projectile the source barrel `index` fires.
fn barrel_projectile(behavior: &Value, index: usize) -> Option<u64> {
    gameplay(behavior)["barrels"].get(index)?["projectile"]["datum"].as_u64()
}

/// A charged pair's projectile the weapon fires and its other barrel's, when the two barrels fire
/// two different projectiles the root names.
fn charged_projectiles(behavior: &Value, ids: &BTreeSet<u64>) -> Option<(u64, u64)> {
    let pair = super::firing::charged_pair(gameplay(behavior))?;
    let fired = barrel_projectile(behavior, pair.fired())?;
    let other = barrel_projectile(behavior, pair.other())?;
    (fired != other && ids.contains(&fired) && ids.contains(&other)).then_some((fired, other))
}

/// The projectiles the weapon itself fires. A charged pair's weapon fires its tap barrel's
/// projectile, and its charged barrel's takes that place only through the charged release perk.
/// A Spew-Charge pair's weapon fires its charged barrel's projectile.
fn primary_ids(behavior: &Value) -> Result<BTreeSet<u64>> {
    let root = &behavior["root"]["datum"];
    let ids = behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["owner"] == *root && e["role"] == "projectile")
        .map(|e| e["tag"]["datum"].as_u64().context("Projectile identity"))
        .collect::<Result<BTreeSet<_>>>()?;
    Ok(match charged_projectiles(behavior, &ids) {
        Some((fired, _)) => BTreeSet::from([fired]),
        None => ids,
    })
}

fn primary<'a>(behavior: &Value, projectiles: &'a Value) -> Result<&'a Value> {
    let ids = primary_ids(behavior)?;
    ensure!(
        ids.len() == 1,
        "Native projectile translation requires one primary source projectile"
    );
    let id = *ids.first().unwrap();
    projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|p| p["tag"]["datum"] == id)
        .context("Primary projectile is absent from its source receipt")
}

/// Automatic selection follows the source projectile relation and supported presentation shape.
/// Explicit carriers require a successful translation. Unsupported automatic shapes keep their
/// source receipt without claiming that a native projectile was imported.
pub(super) fn select(
    entry: &mut ImportRequest,
    behavior: &Value,
    projectiles: &Value,
) -> Result<Value> {
    if let Some(mut carrier) = entry.projectile_entity {
        let source = primary(behavior, projectiles)?;
        // A saved graph's carrier can be one an earlier automatic selection chose for another
        // shape of the source, such as the ordinary grenade before a release detonation was read,
        // or the effect-only carrier before a charging trigger fired its charged barrel's modeled
        // round. The current automatic choice replaces it. Other carriers were chosen explicitly.
        let mut replaced = None;
        let current = automatic_carrier(behavior, source)?;
        if AUTOMATIC_CARRIERS.contains(&carrier) && carrier != current {
            replaced = Some(carrier);
            carrier = current;
            entry.projectile_entity = Some(carrier);
        }
        return Ok(json!({"status":"selected","selection":"explicit",
            "source":source["tag"],"native_carrier":carrier,"replaced_carrier":replaced,
            "presentation":if source["has_model"] == true { "source_model" } else { "source_effects" }}));
    }
    let ids = primary_ids(behavior)?;
    if ids.is_empty() {
        return Ok(
            json!({"status":"not_applicable","reason":"The source declares no primary projectile."}),
        );
    }
    let unsupported = |reason: &str| json!({"status":"source_only","reason":reason});
    if entry.runtime_entity.is_some() || entry.source.group != "weap" {
        return Ok(unsupported(
            "Vehicle weapons require their own controller translation.",
        ));
    }
    if ids.len() != 1 {
        return Ok(unsupported(
            "Multiple primary projectiles require a firing controller translation.",
        ));
    }
    let source = projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|source| {
            source["tag"]["datum"]
                .as_u64()
                .is_some_and(|id| ids.contains(&id))
        })
        .context("Primary projectile is absent from its source receipt")?;
    let overrides = &entry.recipe["overrides"];
    if !overrides["projectile"].is_null()
        || (!overrides["fired_graph"].is_null()
            && overrides["fired_graph"] != json!({"imported":"reach-projectile-entity"}))
    {
        return Ok(unsupported(
            "The recipe explicitly selects another firing graph or projectile edit.",
        ));
    }
    let modeled = source["has_model"] == true;
    let shards = shard::Shards::read(&source["behavior"])?.is_some();
    // A guided source without a render model steers on the effect-only carrier.
    if !modeled && shards {
        return Ok(unsupported(
            "An effect-only projectile with attached shards needs a modeled carrier's assembly.",
        ));
    }
    let carrier = automatic_carrier(behavior, source)?;
    entry.projectile_entity = Some(carrier);
    Ok(json!({"status":"selected","selection":"automatic",
        "source":source["tag"],"native_carrier":carrier,
        "presentation":if modeled { "source_model" } else { "source_effects" }}))
}

/// Every carrier [`automatic_carrier`] chooses.
const AUTOMATIC_CARRIERS: [u32; 4] = [
    effect_only::CARRIER,
    0x8152_82e1,
    remote::CARRIER,
    remote::ORDINARY,
];

/// The carrier automatic selection gives a source. Both modeled Shadowkeep templates have checked
/// object-type-18 render carriers. Shards and guided projectiles replace their mechanics through
/// Assembly. Ordinary projectiles retain the impact-grenade controller, with supported source
/// motion fields applied during conversion, and one that detonates on its trigger's release takes
/// the remote grenade's. A projectile without a render model flies on a carrier that draws none
/// either.
fn automatic_carrier(behavior: &Value, source: &Value) -> Result<u32> {
    let modeled = source["has_model"] == true;
    let assembled = shard::Shards::read(&source["behavior"])?.is_some()
        || tracking::enabled(&source["behavior"])?;
    Ok(if !modeled {
        effect_only::CARRIER
    } else if assembled {
        0x8152_82e1
    } else if remote::tethered(behavior, source)?.is_some() {
        remote::CARRIER
    } else {
        remote::ORDINARY
    })
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
pub(super) fn relocate(
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

/// Emits effect graph node `node` as its reserved asset. An audio node keeps its bank or media
/// storage, and a bank or cue the fields the item's linked audio namespace binds.
fn push_effect(
    assets: &mut Assets,
    node: &Value,
    root: &Path,
    ids: &BTreeMap<String, u32>,
) -> Result<()> {
    let symbol = node["symbol"]
        .as_str()
        .context("Projectile effect symbol")?;
    let id = ids[symbol];
    let template = number(&node["template"])?;
    let (payload, reference) = resolved(node, root, ids)?;
    match node["storage"].as_str() {
        None => assets.push(id, template, payload, reference)?,
        Some(storage @ ("audio_bank" | "audio_media")) => {
            ensure!(
                reference.is_none() && node["patches"].as_array().is_none_or(Vec::is_empty),
                "Projectile audio storage names another node"
            );
            assets.audio(id, template, payload.0, storage == "audio_bank", Vec::new())?;
        }
        Some(storage) => anyhow::bail!("Unsupported projectile effect storage {storage}"),
    }
    if !node["audio_bank"].is_null() {
        let media = node["audio_media"]
            .as_array()
            .context("Projectile audio media")?
            .iter()
            .map(|row| {
                let medium = row["symbol"]
                    .as_str()
                    .and_then(|symbol| ids.get(symbol))
                    .context("Projectile audio medium")?;
                let sample = row["id"].as_u64().context("Projectile audio sample")?;
                Ok((u32::try_from(sample)?, *medium))
            })
            .collect::<Result<Vec<_>>>()?;
        assets.bank(id, node["audio_bank"].clone(), media)?;
    }
    if let Some(events) = node["audio_events"].as_array() {
        let events = events
            .iter()
            .map(|event| {
                let offset = event["offset"].as_u64().context("Projectile cue event")?;
                let event = event["id"].as_u64().context("Projectile cue event")?;
                Ok((usize::try_from(offset)?, u32::try_from(event)?))
            })
            .collect::<Result<Vec<_>>>()?;
        assets.events(id, events)?;
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
/// Where one converted projectile's work and assets go in the graph, and how a source the
/// carrier cannot steer is treated.
struct Layout<'a> {
    /// Work folders for the source model's presentation and the converted effects.
    presentation: &'a str,
    effects: &'a str,
    /// The graph section folder its nodes are written to.
    folder: &'a str,
    /// The symbol prefix its nodes take in place of `reach-projectile-`, when it is not the
    /// weapon's fired projectile.
    prefix: Option<&'a str>,
    /// A guided source without a render model flies unguided on the effect-only carrier when its
    /// guidance does not graft there, rather than refusing the conversion.
    unguided: bool,
    /// Whether the source speed curve may set the launch. A projectile fired from the same
    /// Barrel as one that keeps the Barrel's launch keeps it too.
    curve: bool,
    /// A once-sound flow the projectile hosts, so a sound plays as it spawns.
    launch: Option<&'a Launch>,
    /// Trajectories one shot puts in flight, the pellets of the Barrel that fires it. The
    /// movement's pool holds a row for each.
    trajectories: usize,
}

const FIRED: Layout<'static> = Layout {
    presentation: "projectile-presentation",
    effects: "projectile-effects",
    folder: "projectile",
    prefix: None,
    unguided: true,
    curve: true,
    launch: None,
    trajectories: 1,
};

/// A stock once-sound flow and its host entity, added to a vehicle's translated projectile so the
/// source firing sound plays as the vehicle's barrel spawns it. The flow's cue at `field` names the
/// private cue `cue`, which the vehicle's own graph defines.
pub(super) struct Launch {
    pub host: u32,
    pub flow: u32,
    pub field: usize,
    pub cue: String,
}

/// The `vehicle_weapon` edge of the weapon a vehicle's driver fires: the vehicle's own first
/// weapon, or else the first weapon of a child object without seats, a turret the driver aims, as
/// the Revenant's plasma turret and the Wraith's mortar are. A seated child's weapon belongs to
/// its gunner.
pub(super) fn driver_weapon(behavior: &Value) -> Result<Option<&Value>> {
    let vehicle = &behavior["root"]["datum"];
    let edges = behavior["edges"].as_array().context("Behavior edges")?;
    let weapons = edges
        .iter()
        .filter(|e| e["role"] == "vehicle_weapon")
        .collect::<Vec<_>>();
    let own = weapons
        .iter()
        .copied()
        .filter(|e| e["owner"] == *vehicle)
        .min_by_key(|e| e["offset"].as_u64());
    if own.is_some() {
        return Ok(own);
    }
    let seatless = |e: &&Value| {
        e["owner"].as_u64().is_some_and(|owner| {
            behavior["nodes"][format!("{owner:08X}")]["data"]["object"]["gameplay"]["seats"]
                .as_array()
                .is_some_and(Vec::is_empty)
        })
    };
    let child = weapons
        .iter()
        .copied()
        .filter(seatless)
        .min_by_key(|e| e["offset"].as_u64());
    if child.is_some() {
        return Ok(child);
    }
    // A stationary gun emplacement, with turret physics only, has no driver of its own. Its one
    // seated gun, such as the Shade's, is the gun its native driver fires.
    let node = |datum: &Value| &behavior["nodes"][format!("{:08X}", datum.as_u64().unwrap_or(0))];
    let stationary = node(vehicle)["data"]["object"]["gameplay"]["physics"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty() && rows.iter().all(|row| row["kind"] == "turret"));
    if !stationary {
        return Ok(None);
    }
    let seated = weapons
        .iter()
        .copied()
        .filter(|e| !seatless(e))
        .collect::<Vec<_>>();
    let owners = seated
        .iter()
        .map(|e| e["owner"].as_u64())
        .collect::<std::collections::BTreeSet<_>>();
    Ok(if owners.len() == 1 {
        seated.into_iter().min_by_key(|e| e["offset"].as_u64())
    } else {
        None
    })
}

/// A vehicle's first source weapon's projectile as a private native carrier in the graph's
/// `projectile` section, fired by the vehicle's native barrel in place of the carrier's. The
/// vehicle's behavior is rooted at that weapon, so the conversion reads the weapon's barrels and
/// projectile as a held weapon's would. A weapon without one primary projectile has none.
pub(super) fn vehicle(
    (cache, pages): (&cache::Cache, &mut resource::Pages),
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
    launch: Option<&Launch>,
) -> Result<Option<Value>> {
    let Some(weapon) = driver_weapon(behavior)? else {
        return Ok(None);
    };
    let mut rooted = behavior.clone();
    rooted["root"] = weapon["tag"].clone();
    // The weapon's first primary trigger fires from its primary barrel, as the vehicle's firing
    // sound does. Another trigger's barrel, such as the Banshee cannon's bomb, is not this route.
    let Some(barrel) = gameplay(&rooted)["triggers"]
        .as_array()
        .and_then(|triggers| triggers.iter().find(|t| t["input"] == 0))
        .and_then(|trigger| trigger["primary_barrel"].as_u64())
    else {
        return Ok(None);
    };
    let Some(id) = barrel_projectile(&rooted, usize::try_from(barrel)?) else {
        return Ok(None);
    };
    let source = projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|p| p["tag"]["datum"] == id)
        .context("Vehicle weapon projectile is absent from its source receipt")?;
    let mut entry = entry.clone();
    entry.runtime_entity = None;
    entry.projectile_entity = Some(automatic_carrier(&rooted, source)?);
    let mut section = convert(
        cache,
        pages,
        reader,
        &rooted,
        source,
        &entry,
        root,
        &Layout { launch, ..FIRED },
    )?;
    section["vehicle_weapon"] = weapon["tag"].clone();
    Ok(Some(section))
}

/// A seated child's gun, the weapon of the source object `owner` a gunner fires, as a private
/// native carrier. Its first primary trigger's primary barrel names the projectile, as for the
/// driver's weapon. With `barrel`, the section takes that barrel folder and symbol prefix, so it
/// sits beside the vehicle's fired projectile. Without it, it is the graph's fired projectile.
#[allow(clippy::too_many_arguments)]
pub(super) fn mounted(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
    owner: u64,
    barrel: Option<u8>,
    launch: Option<&Launch>,
) -> Result<Option<Value>> {
    let Some(weapon) = mounted_weapon(behavior, owner)? else {
        return Ok(None);
    };
    let mut rooted = behavior.clone();
    rooted["root"] = weapon["tag"].clone();
    let Some(index) = gameplay(&rooted)["triggers"]
        .as_array()
        .and_then(|triggers| triggers.iter().find(|t| t["input"] == 0))
        .and_then(|trigger| trigger["primary_barrel"].as_u64())
    else {
        return Ok(None);
    };
    let Some(id) = barrel_projectile(&rooted, usize::try_from(index)?) else {
        return Ok(None);
    };
    let source = projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|p| p["tag"]["datum"] == id)
        .context("Mounted weapon projectile is absent from its source receipt")?;
    let mut entry = entry.clone();
    entry.runtime_entity = None;
    entry.projectile_entity = Some(automatic_carrier(&rooted, source)?);
    let folder = barrel.map(|barrel| format!("projectile-barrel-{barrel}"));
    let presentation = folder
        .as_ref()
        .map(|folder| format!("{folder}-presentation"));
    let effects = folder.as_ref().map(|folder| format!("{folder}-effects"));
    let prefix = folder.as_ref().map(|folder| format!("reach-{folder}-"));
    let layout = match (&folder, &presentation, &effects, &prefix) {
        (Some(folder), Some(presentation), Some(effects), Some(prefix)) => Layout {
            presentation,
            effects,
            folder,
            prefix: Some(prefix),
            unguided: true,
            curve: true,
            launch: None,
            trajectories: 1,
        },
        _ => Layout { launch, ..FIRED },
    };
    let mut section = convert(cache, pages, reader, &rooted, source, &entry, root, &layout)?;
    section["vehicle_weapon"] = weapon["tag"].clone();
    section["mounted_owner"] = json!(owner);
    Ok(Some(section))
}

/// The `vehicle_weapon` edge of the weapon a mounted gun object `owner` carries: its first.
pub(super) fn mounted_weapon(behavior: &Value, owner: u64) -> Result<Option<&Value>> {
    Ok(behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["role"] == "vehicle_weapon" && e["owner"].as_u64() == Some(owner))
        .min_by_key(|e| e["offset"].as_u64()))
}

/// The projectile a vehicle's driver fires with the secondary trigger, as a private native carrier
/// in its own barrel section. Reach fires it either from another trigger of the driver weapon on
/// the secondary input (2), such as the Banshee cannon's bomb and the Sabre's missiles, or from
/// the vehicle's next weapon, such as the Seraph's bomb launcher. The section records the weapon,
/// barrel and trigger input it comes from.
#[allow(clippy::too_many_arguments)]
pub(super) fn vehicle_secondary(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
) -> Result<Option<Value>> {
    let Some(primary) = driver_weapon(behavior)? else {
        return Ok(None);
    };
    let rooted = |weapon: &Value| {
        let mut rooted = behavior.clone();
        rooted["root"] = weapon["tag"].clone();
        rooted
    };
    let trigger_barrel = |rooted: &Value, input: u64| {
        gameplay(rooted)["triggers"]
            .as_array()
            .and_then(|triggers| triggers.iter().find(|t| t["input"] == input))
            .and_then(|trigger| trigger["primary_barrel"].as_u64())
    };
    let primary_rooted = rooted(primary);
    let primary_barrel = trigger_barrel(&primary_rooted, 0);
    let primary_projectile = primary_barrel
        .and_then(|barrel| barrel_projectile(&primary_rooted, usize::try_from(barrel).ok()?));
    let mut selected = trigger_barrel(&primary_rooted, 2)
        .filter(|&barrel| Some(barrel) != primary_barrel)
        .map(|barrel| (primary, primary_rooted.clone(), barrel, 2));
    if selected.is_none() {
        let vehicle = &behavior["root"]["datum"];
        let mut others = behavior["edges"]
            .as_array()
            .context("Behavior edges")?
            .iter()
            .filter(|e| {
                e["role"] == "vehicle_weapon"
                    && e["owner"] == *vehicle
                    && e["offset"] != primary["offset"]
            })
            .collect::<Vec<_>>();
        others.sort_by_key(|e| e["offset"].as_u64());
        selected = others.into_iter().find_map(|weapon| {
            let rooted = rooted(weapon);
            let barrel = trigger_barrel(&rooted, 0)?;
            Some((weapon, rooted, barrel, 0))
        });
    }
    let Some((weapon, rooted, barrel, input)) = selected else {
        return Ok(None);
    };
    let Some(id) = barrel_projectile(&rooted, usize::try_from(barrel)?) else {
        return Ok(None);
    };
    if Some(id) == primary_projectile {
        return Ok(None);
    }
    let source = projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|p| p["tag"]["datum"] == id)
        .context("Vehicle secondary projectile is absent from its source receipt")?;
    let mut entry = entry.clone();
    entry.runtime_entity = None;
    entry.projectile_entity = Some(automatic_carrier(&rooted, source)?);
    let folder = format!("projectile-barrel-{barrel}");
    let presentation = format!("{folder}-presentation");
    let effects = format!("{folder}-effects");
    let prefix = format!("reach-{folder}-");
    let mut section = convert(
        cache,
        pages,
        reader,
        &rooted,
        source,
        &entry,
        root,
        &Layout {
            presentation: &presentation,
            effects: &effects,
            folder: &folder,
            prefix: Some(&prefix),
            unguided: true,
            curve: true,
            launch: None,
            trajectories: 1,
        },
    )?;
    section["vehicle_weapon"] = weapon["tag"].clone();
    section["barrel"] = json!(barrel);
    section["trigger_input"] = json!(input);
    Ok(Some(section))
}

/// The weapon's fired projectile as a private native carrier in the graph's `projectile` section.
pub(super) fn native(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
) -> Result<Value> {
    let source = primary(behavior, projectiles)?;
    let layout = Layout {
        trajectories: pellets(behavior, entry)?,
        ..FIRED
    };
    let mut section = convert(cache, pages, reader, behavior, source, entry, root, &layout)?;
    // An airstrike or EMP source can ride on another barrel's projectile, so these receipts cover
    // every source projectile of the weapon.
    let strike = airstrike::receipt(behavior, projectiles, &section["conversion"])?;
    section["conversion"]["airstrike"] = strike;
    section["conversion"]["emp"] = detonation::emp(behavior, projectiles)?;
    Ok(section)
}

/// Trajectories the weapon's own shot puts in flight: its Barrel's pellets. The recipe gives the
/// Barrel its explicit count, or else the source fired barrel's projectiles per shot when one barrel
/// or a charged pair selects it, so the pool takes the larger.
fn pellets(behavior: &Value, entry: &ImportRequest) -> Result<usize> {
    let gameplay = gameplay(behavior);
    let pair = super::firing::charged_pair(gameplay);
    let source = match gameplay["barrels"].as_array() {
        Some(barrels) if barrels.len() == 1 || pair.is_some() => super::native::projectile_count(
            &barrels[pair.map_or(0, |pair| pair.fired())],
            behavior,
        )?,
        _ => 1,
    };
    let explicit = entry.recipe["overrides"]["barrel"]["pellets"]
        .as_i64()
        .unwrap_or(1);
    Ok(usize::try_from(source.max(explicit).max(1))?)
}

/// Gives the assembled movement's trajectory pool a row for each of `count` trajectories a shot
/// puts in flight. The client reads past a pool with fewer rows than the Barrel's pellets, and the
/// build cannot grow an imported pool, so a shotgun's carrier pool of one is grown here while its
/// owner still names itself by its stock tag. New rows copy the carrier's last one.
fn trajectory_pool(assembly: &mut assembly::Assembly, count: usize) -> Result<Value> {
    use sundial::package_authoring::entity::{
        grow_projectile_trajectories, projectile_trajectory_capacity, weapon_component_bindings,
    };
    if count <= 1 {
        return Ok(json!({"pellets":count,"status":"native"}));
    }
    let bindings =
        weapon_component_bindings(&assembly.entity.0, 0x0437_756d).map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("A pellet projectile needs exactly one movement component");
    };
    let owner = assembly
        .owners
        .get_mut(&binding.owner_tag)
        .context("Projectile movement owner is absent")?;
    let instance = usize::try_from(binding.resource_offset)?;
    let carrier = projectile_trajectory_capacity(&owner.0, binding.owner_tag, instance)
        .map_err(anyhow::Error::msg)?;
    grow_projectile_trajectories(
        &mut assembly.entity.0,
        binding.owner_tag,
        &mut owner.0,
        instance,
        count,
    )
    .map_err(anyhow::Error::msg)?;
    let rows = projectile_trajectory_capacity(&owner.0, binding.owner_tag, instance)
        .map_err(anyhow::Error::msg)?;
    ensure!(rows >= count, "The grown trajectory pool is short");
    Ok(
        json!({"pellets":count,"carrier_rows":carrier,"rows":rows,"owner":format!("{:08X}",binding.owner_tag)}),
    )
}

/// A charged pair's charged barrel projectile as a private native carrier of its own, which the
/// charged release perk names in place of a native projectile. The section records the barrel
/// it fires from. It flies on the carrier the fired projectile's rules choose, except that a
/// guided source without a render model flies unguided where its guidance does not graft onto
/// the effect-only carrier. Shards still need a modeled carrier. `curve` lets the source speed
/// curve set the launch, as it does for a fired projectile that flies by its source trajectory.
#[allow(clippy::too_many_arguments)]
pub(super) fn charged(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    projectiles: &Value,
    entry: &ImportRequest,
    root: &Path,
    curve: bool,
) -> Result<Value> {
    let ids = behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["owner"] == behavior["root"]["datum"] && e["role"] == "projectile")
        .map(|e| e["tag"]["datum"].as_u64().context("Projectile identity"))
        .collect::<Result<BTreeSet<_>>>()?;
    let barrel = super::firing::charged_pair(gameplay(behavior))
        .context("The source has no charged pair of barrels")?
        .released()
        .context("A Spew-Charge pair fires its charged barrel as the weapon's own shot")?;
    let (_, id) = charged_projectiles(behavior, &ids)
        .context("The charged barrel fires no projectile of its own")?;
    let source = projectiles
        .as_array()
        .context("Projectile presentations")?
        .iter()
        .find(|p| p["tag"]["datum"] == id)
        .context("Charged projectile is absent from its source receipt")?;
    let modeled = source["has_model"] == true;
    let shards = shard::Shards::read(&source["behavior"])?.is_some();
    ensure!(
        modeled || !shards,
        "An effect-only projectile with attached shards needs a modeled carrier's assembly"
    );
    let mut entry = entry.clone();
    entry.projectile_entity = Some(automatic_carrier(behavior, source)?);
    let folder = format!("projectile-barrel-{barrel}");
    let presentation = format!("{folder}-presentation");
    let effects = format!("{folder}-effects");
    let prefix = format!("reach-{folder}-");
    let mut section = convert(
        cache,
        pages,
        reader,
        behavior,
        source,
        &entry,
        root,
        &Layout {
            presentation: &presentation,
            effects: &effects,
            folder: &folder,
            prefix: Some(&prefix),
            unguided: true,
            curve,
            launch: None,
            trajectories: 1,
        },
    )?;
    section["barrel"] = json!(barrel);
    Ok(section)
}

/// Converts `source` into the section `layout` names. A marker whose contact calls an airstrike
/// carries the strike's bomb in the same section, converted first so the strike host can name it.
#[allow(clippy::too_many_arguments)]
fn convert(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    source: &Value,
    entry: &ImportRequest,
    root: &Path,
    layout: &Layout<'_>,
) -> Result<Value> {
    let mut assets = Assets::default();
    let prepared = airstrike::prepare(
        cache,
        pages,
        reader,
        behavior,
        source,
        entry,
        root,
        layout,
        &mut assets,
    )?;
    let (root_id, report) = assemble(
        cache,
        pages,
        reader,
        behavior,
        source,
        entry,
        root,
        layout,
        &mut assets,
        &prepared,
    )?;
    if let Some(prefix) = layout.prefix {
        assets.rename("reach-projectile-", prefix)?;
    }
    assets.write_in(root, layout.folder, root_id, report)
}

/// One source projectile's private carrier, reserved and pushed into `assets`, with its root
/// identity and conversion receipt. `prepared` is the airstrike a marker calls.
#[allow(clippy::too_many_arguments)]
fn assemble(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    source: &Value,
    entry: &ImportRequest,
    root: &Path,
    layout: &Layout<'_>,
    assets: &mut Assets,
    prepared: &airstrike::Prepared,
) -> Result<(u32, Value)> {
    ensure!(
        entry.runtime_entity.is_none(),
        "Vehicle weapon routing needs an explicit vehicle controller translation"
    );
    let PreparedCarrier {
        scene,
        presentation,
        presentation_root,
        carrier,
        model_owner,
        modeled,
        guided,
        mut assembly,
        shards,
    } = prepare_carrier((cache, pages), reader, source, entry, root, layout)?;
    if let Some(launch) = layout.launch {
        assembly.host(reader, launch.flow, launch.host)?;
    }
    // Every private carrier flies unreplicated, as the effect-only carrier does.
    let unreplicated = replication::omit(reader, &mut assembly)?;
    let assembled = std::mem::take(&mut assembly.report);
    let steered = guided && (modeled || assembled["steering"]["status"] == "translated");
    let guidance = json!({"source":tracking::policy(&source["behavior"] )?,
        "translation":if steered { GUIDED }
            else if guided { "Unguided. The source steers, and its guidance did not graft onto the effect-only carrier" }
            else if modeled { "Retained native carrier guidance" }
            else { "Unguided, without the carrier's steering" },"gameplay_verified":false});
    let motion = apply_motion(&mut assembly, &source["behavior"], layout.curve)?;
    let trajectories = trajectory_pool(&mut assembly, layout.trajectories)?;
    let PreparedEffects {
        effects,
        explosion,
        impact,
    } = prepare_effects(
        (cache, pages),
        reader,
        &scene,
        (behavior, source),
        &mut assembly,
        (root, layout),
        shards.is_some(),
    )?;
    let original = assembly.entity;
    let mut owners = assembly.owners;
    if let Some(launch) = layout.launch {
        let cue = assets.external(reader, &launch.cue)?;
        put(
            &mut owners
                .get_mut(&launch.flow)
                .context("Hosted launch flow")?
                .0,
            launch.field,
            &cue.to_le_bytes(),
        )?;
    }
    let rows = original.array(16, 12, Some(0x80809c04))?;
    let (nodes, symbols) = reserve_presentation(reader, assets, presentation.as_ref())?;
    let (ids, allocations) = reserve_components(
        reader,
        assets,
        (&original, &rows),
        &owners,
        model_owner.map(|owner| (owner, symbols["owner"])),
    )?;
    validate_flow_inputs(&original, &owners, effects.owner)?;
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
    let explosion_systems = explosion
        .systems
        .iter()
        .map(|(symbol, rows)| {
            Ok((
                effect_ids
                    .get(symbol)
                    .copied()
                    .context("Supercombine particle system")?,
                *rows,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let explosion_light = explosion
        .light
        .as_ref()
        .map(|light| -> Result<shard::Light> {
            Ok(shard::Light {
                record: *effect_ids
                    .get(&light.symbol)
                    .context("Supercombine light")?,
                stock: light.template,
                seconds: light.seconds,
            })
        })
        .transpose()?;
    let impact_table = impact
        .table
        .as_ref()
        .map(|symbol| {
            effect_ids
                .get(symbol)
                .copied()
                .context("Projectile impact table")
        })
        .transpose()?;
    let shard_report = shards
        .as_ref()
        .map(|shards| {
            shards.append(
                reader,
                assets,
                &mut owners,
                (behavior, source),
                (&explosion_systems, explosion.life, explosion_light.as_ref()),
                impact_table,
            )
        })
        .transpose()?;
    // Without shards the carrier keeps its own contact responses, apart from a hosted source bounce.
    let ricochet = match &shard_report {
        Some(report) => report.get("ricochet").cloned(),
        None => Some(response::ricochet(
            reader,
            &mut owners,
            &mut assembly.allocations,
            source,
        )?),
    };
    if shards.is_none()
        && let Some(table) = impact_table
    {
        effect::place_impact(&mut owners, table)?;
    }
    for (owner, offset, symbol) in &effects.patches {
        let target = effect_ids.get(symbol).context("Projectile effect target")?;
        put(
            &mut owners.get_mut(owner).context("Projectile effect owner")?.0,
            *offset,
            &target.to_le_bytes(),
        )?;
    }
    let detonation = detonation::apply(
        reader,
        assets,
        &mut owners,
        behavior,
        source,
        rig::METRES_PER_UNIT,
    )?;
    // Without shards the carrier's own direct hit, finish and rest timer give way to the source's.
    let contact = if shards.is_none() {
        Some(contact::ordinary(
            reader,
            assets,
            &mut owners,
            behavior,
            source,
            &detonation,
        )?)
    } else {
        None
    };
    let remote = remote::receipt(cache, &owners, behavior, source)?;
    let strike = airstrike::host(reader, assets, &mut owners, prepared)?;
    let converted = emit_projectile_components(
        assets,
        (nodes, &presentation_root, &symbols),
        (&effects.graph, &effect_ids),
        (&owners, &ids, &allocations),
        model_owner,
    )?;
    let mut authored_allocations = assembly.allocations;
    for (tag, payload) in effects.allocations {
        ensure!(
            authored_allocations.insert(tag, payload).is_none(),
            "Projectile allocation edits overlap"
        );
    }
    write_allocations(reader, assets, &allocations, &authored_allocations)?;
    let mut entity = original.clone();
    relocate(&mut entity, &owners, &ids, &rows)?;
    let root_id = symbols["entity"];
    let replicated = emit_replication(
        reader,
        assets,
        (&original, &mut entity, &rows),
        (&ids, &owners, &converted),
        root_id,
    )?;
    crate::tiger::entity::links::Graph::read(&entity, false)?;
    assets.push(root_id, carrier, entity, None)?;
    let shown = |field: &str| {
        presentation
            .as_ref()
            .map_or(Value::Null, |presentation| presentation[field].clone())
    };
    let (retained, limits) = if modeled {
        (
            json!([
                "Native collision and direct contact damage",
                "Native impact presentation",
                "Native acquisition geometry where source tracking is enabled"
            ]),
            json!([
                "Source projectile geometry, supported flight contrails, gravity, range and source player speed curves are translated, and a source radial detonation where the detonation report records it. Source target acquisition, contact continuation, live-component relaunch and external modifiers require gameplay calibration."
            ]),
        )
    } else {
        (
            json!([
                "Native collision and the firing weapon's contact damage",
                "Native contact responses apart from a hosted source bounce"
            ]),
            json!([
                "The source projectile has no render model. The carrier draws only its converted attached flight effects, or its own flight effects where the effects report says they stand in. Source gravity, range and speed are translated where the motion report records them. Source impact damage, flyby sounds, light volumes and lens flares are not translated."
            ]),
        )
    };
    Ok((
        root_id,
        json!({"source":source["tag"],"native_carrier":carrier,
        "presentation":if modeled { "source_model" } else { "source_effects" },"replicated":replicated,
        "vertices":shown("vertices"),"triangles":shown("triangles"),"source_behavior":source["behavior"],
        "launch_policy":if assembly.source_trajectory { Some("source_player_trajectory") } else { None },
        "native_skinning":shown("native_skinning"),"replication":unreplicated,"motion":motion,"trajectories":trajectories,"effects":effects.report,"detonation":detonation,"shards":shard_report,"responses":response::summary(source, ricochet.as_ref(), &strike)?,"contact":contact,"remote_detonation":remote,"strike":strike,"explosion":explosion.report,"impact":impact.report,"guidance":guidance,"assembly":assembled,"gameplay_verified":false,
        "retained_native_behavior":retained,"limits":limits}),
    ))
}

fn apply_motion(assembly: &mut assembly::Assembly, behavior: &Value, curve: bool) -> Result<Value> {
    // A carrier flies the source speed curve when the native distance curve can hold it. The
    // build then holds the donor Barrel's launch multipliers at one, so the private movement
    // alone sets the launch speed. Otherwise the carrier keeps the speed it was assembled with.
    let mut curved = assembly.owners.clone();
    let attempt = if curve {
        motion::apply(
            &assembly.entity,
            &mut curved,
            behavior,
            rig::METRES_PER_UNIT,
            true,
        )
    } else {
        Err(anyhow::anyhow!(
            "The weapon's fired projectile keeps the Barrel's launch, which this projectile shares"
        ))
    };
    let motion = match attempt {
        Ok(motion) => {
            assembly.owners = curved;
            assembly.source_trajectory = true;
            motion
        }
        Err(error) => {
            let mut motion = motion::apply(
                &assembly.entity,
                &mut assembly.owners,
                behavior,
                rig::METRES_PER_UNIT,
                assembly.source_trajectory,
            )?;
            motion["source_curve_omitted"] = json!(format!("{error:#}"));
            motion
        }
    };
    Ok(motion)
}

type ComponentIds = (BTreeMap<u32, u32>, BTreeMap<u32, u32>);

fn reserve_components(
    reader: &mut Reader,
    assets: &mut Assets,
    (original, rows): (&Payload, &[usize]),
    owners: &BTreeMap<u32, Payload>,
    model: Option<(u32, u32)>,
) -> Result<ComponentIds> {
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for &row in rows {
        let tag = original.u32(row)?;
        let payload = owners.get(&tag).context("Assembled projectile component")?;
        let id = if let Some((_, symbol)) = model.filter(|(owner, _)| *owner == tag) {
            symbol
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
    Ok((ids, allocations))
}

type ReplicatedComponents<'a> = (
    &'a BTreeMap<u32, u32>,
    &'a BTreeMap<u32, Payload>,
    &'a BTreeMap<u32, Payload>,
);

fn emit_replication(
    reader: &mut Reader,
    assets: &mut Assets,
    (original, entity, rows): (&Payload, &mut Payload, &[usize]),
    (ids, owners, converted): ReplicatedComponents<'_>,
    root_id: u32,
) -> Result<bool> {
    // A carrier without a replication record, as the effect-only carrier is, keeps none.
    let replication_template = original.u32(0x88)?;
    let replicated = replication_template != u32::MAX;
    if replicated {
        let replication_id = assets.reserve(reader, "reach-projectile-replication".into())?;
        reader.tag(replication_template, Some(0x80809bb6))?;
        let components = rows
            .iter()
            .map(|&row| {
                let tag = original.u32(row)?;
                Ok(compiled::Component {
                    owner: ids[&tag],
                    payload: &converted[&tag],
                    template: &owners[&tag],
                    entity: original,
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
    }
    Ok(replicated)
}

struct PreparedCarrier {
    scene: Scene,
    presentation: Option<Value>,
    presentation_root: PathBuf,
    carrier: u32,
    model_owner: Option<u32>,
    modeled: bool,
    guided: bool,
    assembly: assembly::Assembly,
    shards: Option<shard::Shards>,
}

fn prepare_carrier(
    (cache, pages): (&cache::Cache, &mut resource::Pages),
    reader: &mut Reader,
    source: &Value,
    entry: &ImportRequest,
    root: &Path,
    layout: &Layout<'_>,
) -> Result<PreparedCarrier> {
    let mut request = entry.source.clone();
    request.group = "proj".into();
    request.path = source["tag"]["path"]
        .as_str()
        .context("Projectile path")?
        .into();
    request.view = View::World;
    request.variant = None;
    request.permutations.clear();
    let presentation_root = root.join(layout.presentation);
    // A source projectile without a render model flies on a carrier that draws none either and
    // shows only its converted attached effects.
    let modeled = source["has_model"] == true;
    // Without a model a guided source steers on the effect-only carrier, or flies unguided where
    // its guidance does not graft and the layout lets it.
    let guided = tracking::enabled(&source["behavior"])?;
    let (scene, presentation) = if modeled {
        let scene = scene(cache, pages, &request)?;
        let presentation = super::native::build(
            cache,
            pages,
            reader,
            &scene,
            entry,
            &presentation_root,
            true,
            None,
            [None, None],
        )?;
        (scene, Some(presentation))
    } else {
        (effect_only::scene(cache, &request)?, None)
    };
    let carrier = match &presentation {
        Some(presentation) => number(&presentation["source_entity"])?,
        None => entry.projectile_entity.context("Projectile carrier")?,
    };
    let model_owner = presentation
        .as_ref()
        .map(|presentation| number(&presentation["source_owner"]))
        .transpose()?;
    let (assembly, shards) = if modeled {
        assembly::Assembly::prepare(reader, carrier, &source["behavior"])?
    } else {
        (
            effect_only::prepare(reader, carrier, &source["behavior"], layout.unguided)?,
            None,
        )
    };
    Ok(PreparedCarrier {
        scene,
        presentation,
        presentation_root,
        carrier,
        model_owner,
        modeled,
        guided,
        assembly,
        shards,
    })
}

type PresentationAssets<'a> = (&'a [Value], &'a Path, &'a BTreeMap<String, u32>);
type ComponentAssets<'a> = (
    &'a BTreeMap<u32, Payload>,
    &'a BTreeMap<u32, u32>,
    &'a BTreeMap<u32, u32>,
);

fn emit_projectile_components(
    assets: &mut Assets,
    (nodes, presentation_root, symbols): PresentationAssets<'_>,
    (effects, effect_ids): (&Graph, &BTreeMap<String, u32>),
    (owners, ids, allocations): ComponentAssets<'_>,
    model_owner: Option<u32>,
) -> Result<BTreeMap<u32, Payload>> {
    let mut converted = BTreeMap::new();
    for node in nodes {
        let symbol = node["symbol"].as_str().context("Presentation symbol")?;
        if symbol == "entity" {
            continue;
        }
        let (mut payload, reference) = resolved(node, presentation_root, symbols)?;
        if symbol == "owner" {
            let model_owner = model_owner.context("Projectile render owner")?;
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
    for node in &effects.nodes {
        push_effect(assets, node, &effects.root, effect_ids)?;
    }
    for (&tag, original) in owners {
        if Some(tag) == model_owner {
            continue;
        }
        let mut payload = original.clone();
        relocate(&mut payload, owners, ids, &[])?;
        put(
            &mut payload.0,
            0x44,
            &allocations[&original.u32(0x44)?].to_le_bytes(),
        )?;
        assets.push(ids[&tag], tag, payload.clone(), None)?;
        converted.insert(tag, payload);
    }
    Ok(converted)
}

struct PreparedEffects {
    effects: effect::Effects,
    explosion: effect::Explosion,
    impact: effect::Impact,
}

fn validate_flow_inputs(
    entity: &Payload,
    owners: &BTreeMap<u32, Payload>,
    owner: Option<u32>,
) -> Result<()> {
    let Some(owner) = owner else { return Ok(()) };
    // Flow nodes still read these controller inputs, so the entity must provide a source.
    let flow = &owners[&owner];
    let inputs = flow.array(flow.pointer(24)? + 0x188, 40, Some(0x80809789))?;
    let graph = crate::tiger::entity::links::Graph::read(entity, false)?;
    let connected = graph
        .connections
        .iter()
        .chain(&graph.named_connections)
        .filter_map(|row| row.consumer.object)
        .any(|object| object.owner == owner && object.class == 0x80809789);
    ensure!(
        inputs.is_empty() || connected,
        "Projectile flow inputs have no entity connections"
    );
    Ok(())
}

fn prepare_effects(
    (cache, pages): (&cache::Cache, &mut resource::Pages),
    reader: &mut Reader,
    scene: &Scene,
    (behavior, source): (&Value, &Value),
    assembly: &mut assembly::Assembly,
    (root, layout): (&Path, &Layout<'_>),
    has_shards: bool,
) -> Result<PreparedEffects> {
    let mut effects = effect::build(
        cache,
        pages,
        reader,
        scene,
        &source["behavior"],
        assembly,
        &root.join(layout.effects),
    )?;
    // A supercombine shows the source's own detonation in place of the native finish's.
    let explosion = if has_shards {
        effect::explosion(
            cache,
            pages,
            reader,
            scene,
            &mut effects.graph,
            behavior,
            source,
        )?
    } else {
        effect::Explosion::default()
    };
    // The source's contact effects and sounds replace the movement's native impact table. Source
    // recordings sit beside the graph folder, as the weapon's other one-shot cues read them.
    let recordings = root
        .parent()
        .context("Projectile import folder")?
        .join("source/audio");
    let impact = match effect::native_impact(&assembly.owners)? {
        Some(donor) => effect::impact(
            cache,
            pages,
            reader,
            scene,
            &mut effects.graph,
            behavior,
            source,
            donor,
            &effect::Recordings {
                folder: &recordings,
                section: layout.folder,
            },
        )?,
        None => effect::Impact {
            table: None,
            report: json!({"status":"native","reason":"The movement names no impact table"}),
        },
    };
    Ok(PreparedEffects {
        effects,
        explosion,
        impact,
    })
}

type PresentationIds<'a> = (&'a [Value], BTreeMap<String, u32>);

fn reserve_presentation<'a>(
    reader: &mut Reader,
    assets: &mut Assets,
    presentation: Option<&'a Value>,
) -> Result<PresentationIds<'a>> {
    let mut symbols = BTreeMap::new();
    let nodes = presentation
        .map(|presentation| {
            presentation["nodes"]
                .as_array()
                .context("Projectile presentation nodes")
        })
        .transpose()?
        .map(Vec::as_slice)
        .unwrap_or_default();
    for node in nodes {
        let symbol = node["symbol"].as_str().context("Presentation symbol")?;
        symbols.insert(
            symbol.to_owned(),
            assets.reserve(reader, format!("reach-projectile-{symbol}"))?,
        );
    }
    if presentation.is_none() {
        symbols.insert(
            "entity".to_owned(),
            assets.reserve(reader, "reach-projectile-entity".into())?,
        );
    }
    Ok((nodes, symbols))
}
