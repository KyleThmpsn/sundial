//! What an imported modern glaive needs to play as one in Shadowkeep, which has no glaive.
//!
//! Every modern glaive, exotics included, plays the same first-person set (entity `80C3497C`,
//! bank `80C44E4D`), so one native base serves them all: [`BASE`], whose first-person rig the
//! shipped calibration (`glaive/rig_calibration.json`) pairs the glaive rig with. An import of a
//! glaive uses it without being asked for a donor. The import keeps that base's runtime and
//! gains the parts The Enigma was given by hand:
//!
//! - Its first-person edits: hip fire, holding pose, melee swings, sprint and ready
//!   (`animation`).
//! - Its melee audio (`audio`): swing, hit and surface sounds from its own melee controller,
//!   played through a contact profile whose key the Adaptive Glaive's melee program names as
//!   the glaive's melee attack.
//! - A private projectile assembled from the source bolt's converted controllers, programs
//!   and supported particle events. The graph receipt records native compatibility choices.
//! - The modern glaive shield's first-person particle systems, converted into the model graph.
//!   The shield is shared glaive guard content (entity `80CEE9AC`, sequence `80CED6E3`, control
//!   `shield`), not part of any one glaive, so every glaive converts the same three systems.
//! - A copy of the Armor of the Colossus bubble geometry whose streams are all zero, so the
//!   overshield the shield borrows for its health draws nothing.
//! - A private variant of the base's intrinsic, Adaptive Glaive. Its Glaive Frame attaches the
//!   overshield while aiming, with the HUD status Glaive Shield, and two private copies of
//!   Riskrunner's Arc Conductor glow on the weapon that play the shield systems. Their effects
//!   end when aiming ends. Its Glaive Melee Reach changes the class melee.
//!
//! The variant is the Enigma's (`glaive/adaptive_glaive.json`).
//! Its programs name stock assets and converted nodes by symbol, and every byte patch carries the
//! stock bytes it expects, so the build refuses packages that differ. Two things vary with the
//! import: the stock perks the template names by row resolve here by hash, and the melee action
//! names the model's own contact profile key in place of the Enigma's.
mod animation;
mod audio;

use crate::d2_mot::{
    ornaments,
    particles::native as particles,
    payload::Payload,
    reader::{Reader, write_json},
    rig_convert::animation::equipment,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use tiger_pkg::TagHash;

/// The modern glaive guard's stat, which Shadowkeep shows as Guard Endurance.
pub(crate) const SHIELD_DURATION: u32 = 0x6DCE_F0BA;
/// The shield control's first-person systems: an energy layer, the core energy and the dome.
const SHIELD_SYSTEMS: [u32; 3] = [0x80CE_D6C4, 0x80CE_D6CE, 0x80CE_D6D4];
/// The stock particle system whose package metadata the converted assets borrow.
const PARTICLE_TEMPLATE: u32 = 0x80EF_30DB;
/// Entity variables the converted programs may read, fixed at neutral values. Their uses only
/// scale size or intensity. The shield systems read none of them.
const NEUTRAL_INPUTS: [(u32, f32); 3] =
    [(0xFF04_497C, 0.0), (0x6942_0EF1, 1.0), (0x9F03_B41A, 1.0)];
/// Armor of the Colossus's bubble geometry, drawn by the overshield's gear model.
const BUBBLE_GEOMETRY: u32 = 0x81A6_99D9;
const MESH_CLASS: u32 = 0x8080_7378;
/// Vertex stream fields of the geometry's mesh, then its index stream.
const VERTEX_STREAMS: [usize; 4] = [0, 4, 8, 12];
const INDEX_STREAM: usize = 16;
/// The plug category of weapon frames.
const INTRINSIC_CATEGORY: u64 = 0x67FB_A961;
const FINISHED_PERKS_SLOT: usize = 71;
const FINISHED_PERK_ROW_CLASS: u32 = 0x8080_5C9D;
const KIT: &str = include_str!("glaive/adaptive_glaive.json");
/// The native weapon a glaive is built on: Buzzard, the sidearm whose first-person clips the
/// calibration pairs with source clips.
pub(crate) const BASE: u32 = 0x3005_A7F1;
/// The glaive rig against the base's: source and native clip and event pairs, with the rig
/// export they were taken from.
const CALIBRATION: &str = include_str!("glaive/rig_calibration.json");
/// Magazine, the same stat in both versions.
const MAGAZINE: u32 = 0xE6BE_4C5A;
/// The rounds the base's runtime loads, from 4 at investment 0 to 14 at 100, read from its stat
/// translator.
const BASE_MAGAZINE: [[i32; 2]; 2] = [[0, 4], [100, 14]];
const STAT_TABLE_SLOT: usize = 95;
const STAT_ROW_CLASS: u32 = 0x8080_7D09;
/// The runtime value modern particle programs read as the weapon's damage type.
const DAMAGE_TYPE_INPUT: u32 = 0x49FC_E899;
/// The bolt sequence's event rows, its particle event kind and that event's system rows.
const SEQUENCE_EVENT_ROW: u32 = 0x8080_91F1;
/// A modern animation clip.
const MODERN_CLIP: u32 = 0x8080_8BE0;
const PARTICLE_EVENT: u32 = 4;
const PARTICLE_EVENT_ROW: u32 = 0x8080_67BB;

/// The weapon component's resource class, its barrel field and the barrel's class.
const WEAPON_COMPONENT: &str = "80CEA147";
const BARREL_FIELD: usize = 0x1270;
const BARREL_CLASS: u32 = 0x8080_2A9B;
/// Where a barrel names the projectile it fires. Every glaive's names the same one at both.
const BARREL_PROJECTILES: [usize; 2] = [0xC88, 0xCA0];
const ENTITY_CLASS: u32 = 0x8080_9AD8;

/// The projectile entity the source's barrel fires, or `None` when its runtime has no single
/// weapon component naming one projectile at both barrel places.
fn source_projectile(sr: &mut Reader, rig: &Value) -> Result<Option<u32>> {
    let weapons = rig["components"]
        .as_array()
        .context("source components")?
        .iter()
        .filter(|row| row["class"] == WEAPON_COMPONENT && row["entity"] == rig["runtime_entity"])
        .map(|row| {
            u32::from_str_radix(row["owner"].as_str().context("component owner")?, 16)
                .map_err(Into::into)
        })
        .collect::<Result<BTreeSet<u32>>>()?;
    let [weapon] = weapons.into_iter().collect::<Vec<_>>()[..] else {
        return Ok(None);
    };
    let owner = sr.tag(weapon, None)?;
    let barrel = owner.pointer(24)? + BARREL_FIELD;
    if owner.u32(barrel + 4)? != BARREL_CLASS {
        return Ok(None);
    }
    let named = BARREL_PROJECTILES
        .iter()
        .map(|field| sr.ref64(&owner, barrel + field))
        .collect::<Result<BTreeSet<_>>>()?;
    let [projectile] = named.into_iter().collect::<Vec<_>>()[..] else {
        return Ok(None);
    };
    Ok((sr.reference(projectile).ok() == Some(ENTITY_CLASS)).then_some(projectile))
}

/// The bolt sequence's branch for the source's damage type and the value its particle programs
/// read for it: 1 Solar (`thermal`), 2 Arc, 3 Void, 5 Stasis and 6 Strand, as the branches are
/// named and gated, and 0 for Kinetic, which has no branch and is inferred. It follows the
/// source's own element, so Stasis and Strand look their own even though the weapon keeps its
/// base's damage type for them.
fn element(source: &Value) -> Option<(Option<&'static str>, f32)> {
    Some(match super::gameplay::exported_element(source)? {
        "kinetic" => (None, 0.0),
        "solar" => (Some("thermal"), 1.0),
        "arc" => (Some("arc"), 2.0),
        "void" => (Some("void"), 3.0),
        "stasis" => (Some("stasis"), 5.0),
        "strand" => (Some("strand"), 6.0),
        _ => return None,
    })
}

/// FNV-1 of `text`, the hash sequence controls are named with.
fn fnv1(text: &str) -> u32 {
    text.bytes().fold(0x811C_9DC5u32, |hash, byte| {
        hash.wrapping_mul(16_777_619) ^ u32::from(byte)
    })
}

/// Follow the barrel projectile and sequence ancestry without weapon or bolt hash gates.
fn bolt_systems(sr: &mut Reader, projectile: u32, branch: Option<&str>) -> Result<Vec<u32>> {
    use crate::d2_mot::entity::{links::Graph, sequence::Sequence};
    let entity = sr.tag(projectile, Some(ENTITY_CLASS))?;
    let graph = Graph::read(&entity, true)?;
    let branches = ["thermal", "arc", "void", "stasis", "strand"].map(fnv1);
    let selected = branch.map(fnv1);
    let mut systems = BTreeSet::new();
    for component in graph.components {
        let p = sr.tag(component, None)?;
        if p.u32(p.pointer(16)? + 4)? != 0x80808179 {
            continue;
        }
        let sequence = Sequence::read(&p)?;
        for row in p.array(p.pointer(24)? + 0x1D8, 24, Some(SEQUENCE_EVENT_ROW))? {
            let event = p.pointer(row + 16)?;
            if p.u32(event + 0x1C)? != PARTICLE_EVENT {
                continue;
            }
            let mut parent = Some(usize::try_from(p.i16(event + 6)?)?);
            let mut keep = true;
            while let Some(index) = parent {
                let control = sequence.controls.get(index).context("bolt event parent")?;
                if branches.contains(&control.name) && Some(control.name) != selected {
                    keep = false;
                }
                parent = control.parent;
            }
            if keep {
                for system in p.array(event + 0x28, 24, Some(PARTICLE_EVENT_ROW))? {
                    systems.insert(p.u32(system + 16)?);
                }
            }
        }
    }
    ensure!(
        !systems.is_empty(),
        "source projectile has no supported particle events"
    );
    Ok(systems.into_iter().collect())
}

/// Whether the source's item type is Glaive.
pub(crate) fn is_glaive(source: &Value) -> bool {
    source["item_type"] == "Glaive"
}

/// Whether the source export at `source` is a glaive.
pub(crate) fn exported_glaive(source: &Path) -> Result<bool> {
    let gameplay: Value = serde_json::from_slice(&fs::read(source.join("gameplay.json"))?)?;
    Ok(is_glaive(&gameplay))
}

/// The calibration a glaive's own first-person rig needs on [`BASE`].
pub(crate) fn calibration() -> Result<Value> {
    Ok(serde_json::from_str(CALIBRATION)?)
}

pub(crate) struct Inputs<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    /// The import's prepared folder, holding the render inputs particle conversion reads.
    pub prepared: &'a Path,
    /// The model graph folder the recipe will name.
    pub graph: &'a Path,
    /// A fresh folder for the kit's reads.
    pub work: &'a Path,
    pub source_rig: &'a Value,
    pub native_rig: &'a Value,
}

/// Adds the kit to a prepared glaive: its first-person edits and shield nodes to the model
/// graph and the Adaptive Glaive variant to the recipe. Returns what it added.
pub(crate) fn apply(
    inputs: &Inputs,
    source: &Value,
    recipe: &mut Value,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    ensure!(!inputs.work.exists(), "glaive work folder already exists");
    progress("Fitting the glaive's first-person animation…".into());
    let first_person = animation::apply(&animation::Inputs {
        modern: inputs.modern,
        native: inputs.native,
        graph: inputs.graph,
        work: &inputs.work.join("animation"),
        source_rig: inputs.source_rig,
        native_rig: inputs.native_rig,
        calibration: &calibration()?,
    })?;
    progress("Converting the glaive's melee sounds…".into());
    let melee_audio = audio::apply(&audio::Inputs {
        modern: inputs.modern,
        native: inputs.native,
        graph: inputs.graph,
        work: &inputs.work.join("audio"),
        source_rig: inputs.source_rig,
        namespace: recipe["namespace"].as_str().context("recipe namespace")?,
    })?;
    let mut sr = Reader::discovery(inputs.modern, &inputs.work.join("source"), true)?;
    let projectile = source_projectile(&mut sr, inputs.source_rig)?
        .context("The glaive barrel does not name one supported source projectile")?;
    let kit: Value = serde_json::from_str(KIT)?;
    let graph_path = inputs.graph.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
    ensure!(
        graph.get("particles").is_none(),
        "the glaive model already carries particles"
    );
    progress("Converting the glaive's own runtime animation…".into());
    let runtime_animation = runtime_animation(inputs)?;
    graph["equipment_animation"] = runtime_animation.clone();
    let base = u32::from_str_radix(
        recipe["donor"]["item_hash"]
            .as_str()
            .context("recipe base")?
            .trim_start_matches("0x"),
        16,
    )?;
    let mut native = Reader::discovery(inputs.native, &inputs.work.join("native"), false)?;
    let (lane, plug) = intrinsic(&mut native, base, recipe)?;
    let perks = perk_rows(&mut native, &kit["source_perks"])?;
    let element = element(source);
    let (branch, element_value) = element.context("The glaive damage type is unsupported")?;
    let mut systems = SHIELD_SYSTEMS.to_vec();
    systems.extend(bolt_systems(&mut sr, projectile, branch)?);
    progress("Converting the glaive shield and bolt particles…".into());
    let decompiler = super::native::automatic::decompiler(progress)?;
    let mut inputs_fixed: BTreeMap<u32, f32> = NEUTRAL_INPUTS.into_iter().collect();
    inputs_fixed.insert(DAMAGE_TYPE_INPUT, element.map_or(0.0, |(_, value)| value));
    let mut section = particles::convert(&particles::Request {
        source_packages: inputs.modern,
        native_packages: inputs.native,
        render_inputs: &inputs.prepared.join("render-inputs"),
        decompiler: &decompiler,
        graph: inputs.graph,
        work: &inputs.work.join("particles"),
        systems: &systems,
        inputs: &inputs_fixed,
        template: PARTICLE_TEMPLATE,
    })?;
    let refused = section["refused"]
        .as_object()
        .context("particle refusals")?
        .clone();
    for shield in SHIELD_SYSTEMS {
        ensure!(
            !refused.contains_key(&format!("{shield:08X}")),
            "glaive shield particles did not convert: {}",
            serde_json::to_string(&refused)?
        );
    }
    ensure!(
        recipe["overrides"].get("fired_graph").is_none(),
        "the recipe already fires its own graph"
    );
    ensure!(
        graph.get("projectile").is_none(),
        "the model already carries a projectile"
    );
    progress("Converting the source projectile controllers…".into());
    let projectile_section = super::native::projectile::convert(
        &mut sr,
        &mut native,
        projectile,
        &section,
        inputs.graph,
        &super::native::projectile::Profile {
            namespace: recipe["namespace"].as_str().context("recipe namespace")?,
            element: element_value,
            // The glaive kit uses an instant-fire native carrier. Retain its established
            // launch compensation while translating the source controller graph.
            speed_boost: 120.0,
        },
    )?;
    recipe["overrides"]["fired_graph"] = json!({"imported":projectile_section["root"]});
    let fired = projectile_section["conversion"].clone();
    graph["projectile"] = projectile_section;
    let geometry = hidden_geometry(&mut native, inputs.graph)?;
    section["nodes"]
        .as_array_mut()
        .context("particle nodes")?
        .extend(geometry);
    let variant = variant(&kit, lane, plug, &perks, &graph["audio"]["impact_key"])?;
    let symbols = section["nodes"]
        .as_array()
        .context("particle nodes")?
        .iter()
        .filter_map(|node| node["symbol"].as_str())
        .collect::<Vec<_>>();
    for symbol in named_particles(&variant) {
        ensure!(
            symbols.contains(&symbol.as_str()),
            "the glaive shield names particle {symbol}, which the model lacks"
        );
    }
    let systems = section["systems"].clone();
    graph["particles"] = section;
    write_json(&graph_path, &graph)?;
    let variants = recipe["overrides"]
        .as_object_mut()
        .context("recipe overrides")?
        .entry("socket_plug_variants")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .context("recipe plug variants")?;
    ensure!(
        variants
            .iter()
            .all(|existing| existing["socket_index"] != lane || existing["choice_index"] != 0),
        "the recipe already varies the intrinsic"
    );
    variants.push(variant);
    let magazine = magazine(&mut native, source, recipe)?;
    // Shadowkeep has no glaive page, so a glaive files under Parhelion's own Glaives page for its
    // ammo rather than its base's family. Placement still sends an exotic to the Exotics node.
    let collection = json!({"ammo":recipe["overrides"]["ammo_type"].as_str().context("recipe ammo type")?,
        "family":"glaives"});
    recipe["overrides"]["collection_destination"] = collection.clone();
    // The base's type text would read Sidearm. Shadowkeep has no glaive text in any locale, so
    // the source's English type name serves every locale.
    recipe["type_name"] = source["item_type"].clone();
    let report = json!({"intrinsic_socket":lane,"intrinsic_plug":format!("0x{plug:08X}"),"perks":perks,
        "particle_systems":systems,"bolt":fired,"damage_input":element.map(|(_, value)| value),"impact_key":graph["audio"]["impact_key"],
        "magazine":magazine,"collection":collection,"first_person":first_person,"melee_audio":melee_audio,
        "runtime_animation":{"clips":runtime_animation["clips"],"source_only":runtime_animation["source_only"]},
        "gameplay_verified":false});
    write_json(&inputs.work.join("glaive.json"), &report)?;
    Ok(report)
}

/// Whether a modern clip animates any bone: a nonempty scale, rotation or position list among
/// its animated tracks at +0xD8.
fn animated(sr: &mut Reader, clip: u32) -> Result<bool> {
    let p = sr.tag(clip, Some(MODERN_CLIP))?;
    Ok([0xD8, 0xE8, 0xF8]
        .into_iter()
        .map(|at| p.u64(at))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(|count| *count > 0))
}

/// The glaive runtime's own clip bank holds rest poses and one clip that moves a part of the
/// weapon, the piece turning on its lower shaft (weapon bone 1). The base's runtime bank plays
/// its own clips by state, made for the base's bones, which the glaive's own skeleton replaces.
/// So every base runtime clip takes the moving clip, and the piece turns in every state.
fn runtime_animation(inputs: &Inputs) -> Result<Value> {
    let mut sr = Reader::discovery(inputs.modern, &inputs.work.join("runtime-source"), true)?;
    let source = equipment::runtime_clips(&mut sr, inputs.source_rig, true)?;
    let mut moving = Vec::new();
    for (tag, _) in &source {
        if animated(&mut sr, *tag)? && !moving.contains(tag) {
            moving.push(*tag);
        }
    }
    sr.finish()?;
    let [moving] = moving[..] else {
        anyhow::bail!(
            "the glaive runtime bank moves {} clips, not one",
            moving.len()
        );
    };
    let mut nr = Reader::discovery(inputs.native, &inputs.work.join("runtime-native"), false)?;
    let native = equipment::runtime_clips(&mut nr, inputs.native_rig, false)?;
    nr.finish()?;
    let pairs = native
        .iter()
        .map(|(tag, _)| *tag)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|tag| (tag, moving))
        .collect::<Vec<_>>();
    equipment::prepare_paired(
        inputs.modern,
        inputs.native,
        inputs.source_rig,
        inputs.native_rig,
        &inputs.work.join("runtime"),
        inputs.graph,
        &pairs,
    )
}

/// The value a display curve shows for `value`, holding its end points beyond them.
fn shown(curve: &[[i32; 2]], value: i32) -> Option<f64> {
    let (first, last) = (curve.first()?, curve.last()?);
    if value <= first[0] {
        return Some(f64::from(first[1]));
    }
    if value >= last[0] {
        return Some(f64::from(last[1]));
    }
    let pair = curve.windows(2).find(|pair| value <= pair[1][0])?;
    let [[x0, y0], [x1, y1]] = [pair[0], pair[1]];
    Some(f64::from(y0) + f64::from(y1 - y0) * f64::from(value - x0) / f64::from(x1 - x0))
}

/// Sets Magazine so the base loads the rounds the source weapon shows, and shows the base's
/// own curve for it. The source curve alone would show the source's rounds while the base
/// loaded a different number from the same value.
fn magazine(native: &mut Reader, source: &Value, recipe: &mut Value) -> Result<Value> {
    let curve = |stat: &Value| -> Option<Vec<[i32; 2]>> {
        stat["display"]
            .as_array()?
            .iter()
            .map(|point| {
                Some([
                    i32::try_from(point[0].as_i64()?).ok()?,
                    i32::try_from(point[1].as_i64()?).ok()?,
                ])
            })
            .collect()
    };
    let Some(source_curve) = source["stat_group"]["stats"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|stat| stat["hash"] == MAGAZINE)
        .and_then(curve)
    else {
        return Ok(json!({"kept":"the source group does not show Magazine"}));
    };
    let Some(value) = source["stats"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|stat| stat["hash"] == MAGAZINE && stat["literal"] == true)
        .and_then(|stat| stat["value"].as_i64())
    else {
        return Ok(json!({"kept":"the source sets no plain Magazine value"}));
    };
    let rounds = shown(&source_curve, i32::try_from(value)?)
        .context("source Magazine curve is empty")?
        .round();
    let [[low_value, low], [high_value, high]] = BASE_MAGAZINE;
    let investment = (f64::from(low_value)
        + (rounds - f64::from(low)) * f64::from(high_value - low_value) / f64::from(high - low))
    .round()
    .clamp(f64::from(low_value), f64::from(high_value)) as i32;
    let index = stat_index(native, MAGAZINE)?;
    let overrides = &mut recipe["overrides"];
    let stats = overrides["investment_stats"]
        .as_array_mut()
        .context("recipe investment stats")?;
    stats.retain(|stat| stat["definition_index"] != index);
    stats.push(json!({"definition_index":index,"value":investment}));
    stats.sort_by_key(|stat| stat["definition_index"].as_u64());
    if let Some(row) = overrides["custom_stat_group"]["stats"]
        .as_array_mut()
        .and_then(|rows| rows.iter_mut().find(|row| row["definition_index"] == index))
    {
        row["display"] = json!(BASE_MAGAZINE);
        row.as_object_mut()
            .context("Magazine row")?
            .remove("is_linear");
    }
    Ok(json!({"source_value":value,"rounds":rounds,"investment":investment}))
}

/// The row of a stat definition in these packages, found by hash.
fn stat_index(native: &mut Reader, hash: u32) -> Result<u16> {
    let globals = native.tag(
        native
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|tag| tag.name == "investment_globals")
            .context("native globals")?
            .hash
            .0,
        None,
    )?;
    let root = native.tag(globals.u32(16)?, None)?;
    let stats = native.tag(root.u32(8 + STAT_TABLE_SLOT * 16)?, None)?;
    let index = stats
        .array(8, 32, Some(STAT_ROW_CLASS))?
        .into_iter()
        .position(|at| stats.u32(at).ok() == Some(hash))
        .with_context(|| format!("stat {hash:08X} is missing"))?;
    Ok(u16::try_from(index)?)
}

/// The base's frame socket and the plug its recipe column, or else the base, gives it first.
fn intrinsic(native: &mut Reader, base: u32, recipe: &Value) -> Result<(usize, u32)> {
    let sockets = ornaments::native(native, base)?;
    let frames = sockets["sockets"]
        .as_array()
        .context("native sockets")?
        .iter()
        .enumerate()
        .filter(|(_, socket)| {
            socket["choices"].as_array().is_some_and(|choices| {
                choices.iter().any(|choice| {
                    choice["default"] == true && choice["category"] == INTRINSIC_CATEGORY
                })
            })
        })
        .collect::<Vec<_>>();
    let &[(lane, socket)] = frames.as_slice() else {
        anyhow::bail!("the base has {} frame sockets, not one", frames.len());
    };
    let plug = match recipe["overrides"]["socket_columns"][lane]["choices"][0].as_str() {
        Some(hash) => u32::from_str_radix(hash.trim_start_matches("0x"), 16)?,
        None => u32::try_from(
            socket["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|choice| choice["default"] == true)
                .and_then(|choice| choice["hash"].as_u64())
                .context("base frame")?,
        )?,
    };
    ensure!(
        socket["choices"].as_array().is_some_and(|choices| choices
            .iter()
            .any(|choice| choice["hash"] == plug && choice["category"] == INTRINSIC_CATEGORY)),
        "the recipe's first frame choice {plug:08X} is not one of the base's frames"
    );
    Ok((lane, plug))
}

/// Each finished perk row the template names, at its row in these packages, found by hash.
fn perk_rows(native: &mut Reader, wanted: &Value) -> Result<BTreeMap<String, u16>> {
    let globals = native.tag(
        native
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|tag| tag.name == "investment_globals")
            .context("native globals")?
            .hash
            .0,
        None,
    )?;
    let catalog = native.tag(globals.u32(16 + FINISHED_PERKS_SLOT * 16)?, None)?;
    let rows = catalog.array(8, 0x18, Some(FINISHED_PERK_ROW_CLASS))?;
    let mut found = BTreeMap::new();
    for (row, hash) in wanted.as_object().context("kit perks")? {
        let hash = u32::from_str_radix(
            hash.as_str()
                .context("kit perk hash")?
                .trim_start_matches("0x"),
            16,
        )?;
        let index = rows
            .iter()
            .position(|at| catalog.u32(*at).ok() == Some(hash))
            .with_context(|| format!("stock perk {hash:08X} is missing"))?;
        found.insert(row.clone(), u16::try_from(index)?);
    }
    Ok(found)
}

/// The template's variant on the base's frame, its perks at their rows here, and its melee
/// impact action on the model's own key or left out.
fn variant(
    kit: &Value,
    lane: usize,
    plug: u32,
    perks: &BTreeMap<String, u16>,
    impact_key: &Value,
) -> Result<Value> {
    let row = |index: &Value| -> Result<Value> {
        let index = index.as_u64().context("kit perk row")?.to_string();
        Ok(json!(perks.get(&index).with_context(|| format!(
            "kit perk row {index} has no hash"
        ))?))
    };
    let mut variant = kit["variant"].clone();
    for perk in variant["additional_sandbox_perks"]
        .as_array_mut()
        .context("kit perks")?
    {
        *perk = row(perk)?;
    }
    // The template's key is the Enigma's contact profile, which is also its melee attack. Without
    // the model's own the melee would be the class melee.
    let template_key = &kit["impact_key"];
    let impact_key = impact_key
        .as_str()
        .context("the glaive melee needs the model's impact key")?;
    for perk in variant["sandbox_perks"]
        .as_array_mut()
        .context("kit programs")?
    {
        perk["source_perk_index"] = row(&perk["source_perk_index"])?;
        let actions = perk["program"]["actions"]
            .as_array_mut()
            .context("kit actions")?;
        for action in actions.iter_mut() {
            if &action["key"] == template_key {
                action["key"] = json!(format!("0x{impact_key}"));
            }
        }
    }
    variant["socket_index"] = json!(lane);
    variant["choice_index"] = json!(0);
    variant["source_plug_hash"] = json!(format!("0x{plug:08X}"));
    Ok(variant)
}

fn named_particles(variant: &Value) -> Vec<String> {
    let mut symbols = Vec::new();
    for perk in variant["sandbox_perks"].as_array().into_iter().flatten() {
        for asset in perk["program"]["native_asset_patches"]
            .as_array()
            .into_iter()
            .flatten()
        {
            for patch in asset["patches"].as_array().into_iter().flatten() {
                if let Some(symbol) = patch["imported_particle"].as_str() {
                    symbols.push(symbol.to_owned());
                }
            }
        }
    }
    symbols
}

/// A copy of the bubble geometry that draws nothing, as particle graph nodes. The copy keeps
/// every byte of the stock geometry except its stream references, which name private streams
/// with the stock headers and all-zero data. Every index is 0, so every triangle is degenerate.
/// The copy's streams never depend on a stock buffer being resident.
fn hidden_geometry(native: &mut Reader, graph: &Path) -> Result<Vec<Value>> {
    let symbol = format!("overshield-hidden-geometry-{BUBBLE_GEOMETRY:08X}");
    let mut geometry = native.tag(BUBBLE_GEOMETRY, None)?.0.clone();
    let stock = Payload(geometry.clone());
    ensure!(
        usize::try_from(stock.u64(0)?)? == geometry.len(),
        "stock bubble geometry envelope differs"
    );
    let head = stock.pointer(24)?;
    ensure!(
        stock.u64(16)? == 1 && stock.u64(head)? == 1 && stock.u32(head + 8)? == MESH_CLASS,
        "stock bubble geometry holds other than one mesh"
    );
    let mesh = head + 16;
    ensure!(
        stock.u64(mesh + 24)? > 0,
        "stock bubble geometry has no parts"
    );
    let streams = VERTEX_STREAMS
        .into_iter()
        .chain([INDEX_STREAM])
        .filter_map(|at| {
            let tag = stock.u32(mesh + at).ok()?;
            (tag != u32::MAX).then_some((at, tag))
        })
        .collect::<Vec<_>>();
    ensure!(
        streams.len() >= 2 && streams.iter().any(|(at, _)| *at == INDEX_STREAM),
        "stock bubble geometry streams differ"
    );
    let folder = graph.join("particles");
    fs::create_dir_all(&folder)?;
    let mut nodes = Vec::new();
    let mut patches = Vec::new();
    for (at, header_tag) in streams {
        let entry = native
            .manager
            .get_entry(TagHash(header_tag))
            .with_context(|| format!("stream {header_tag:08X}"))?;
        let header = native.tag(header_tag, None)?;
        let size = if at == INDEX_STREAM {
            ensure!(
                entry.file_type == 32 && entry.file_subtype == 6 && header.0.len() == 24,
                "index stream {header_tag:08X} header differs"
            );
            usize::try_from(header.u64(8)?)?
        } else {
            ensure!(
                entry.file_type == 32 && entry.file_subtype == 4 && header.0.len() == 12,
                "vertex stream {header_tag:08X} header differs"
            );
            usize::try_from(header.u32(0)?)?
        };
        let data_tag = entry.reference;
        let data = native
            .manager
            .get_entry(TagHash(data_tag))
            .with_context(|| format!("stream data {data_tag:08X}"))?;
        ensure!(
            data.file_type == 40
                && usize::try_from(data.file_size)? == size
                && data.reference == header_tag,
            "stream {header_tag:08X} data differs"
        );
        let stream = format!("{symbol}-stream-{header_tag:08X}");
        let stream_data = format!("{stream}-data");
        fs::write(folder.join(format!("{stream}.bin")), &header.0)?;
        fs::write(folder.join(format!("{stream_data}.bin")), vec![0; size])?;
        nodes.push(
            json!({"symbol":stream,"file":format!("particles/{stream}.bin"),
            "template":header_tag,"reference":stream_data,"patches":[]}),
        );
        nodes.push(
            json!({"symbol":stream_data,"file":format!("particles/{stream_data}.bin"),
            "template":data_tag,"reference":stream,"patches":[]}),
        );
        geometry[mesh + at..mesh + at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        patches.push(json!({"offset":mesh + at,"symbol":stream}));
    }
    fs::write(folder.join(format!("{symbol}.bin")), &geometry)?;
    nodes.push(
        json!({"symbol":symbol,"file":format!("particles/{symbol}.bin"),
        "template":BUBBLE_GEOMETRY,"reference":null,"patches":patches}),
    );
    Ok(nodes)
}
