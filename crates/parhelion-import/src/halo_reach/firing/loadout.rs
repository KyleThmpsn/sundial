//! The perk columns and damage type a Reach weapon states where the donor would otherwise decide
//! them.
//!
//! The source has no perks, so every weapon drops the donor's trait columns and the intrinsic is
//! its only trait. Its part columns, such as the barrel and magazine, keep the plugs that leave
//! reload alone.
//!
//! The source has no damage types either, so a default is an authored choice, and a recipe that
//! names a damage type keeps it. A weapon whose projectiles collect on their target and detonate
//! together at a threshold, as Reach's needle weapons do, is Void. The source weapons in `KINETIC`
//! are Kinetic. Any other weapon keeps the donor's damage type.
//!
//! A weapon given a default sits in the slot of its damage type when the donor sits in the other
//! one and the recipe names no slot: Energy for Void, Kinetic for Kinetic. A Power donor keeps its
//! slot. A donor that carries its damage type in an elemental socket cannot be made Kinetic with
//! that socket in place, so a Kinetic default removes the socket.
//!
//! A plug changes reload through a Reload Speed investment stat, or through a perk that attaches
//! a Property Modifiers record on the Reload component or the Reload Speed weapon stat with an
//! amount that is not neutral.
//!
//! The donor's perk columns follow the conversion, so a reimport replaces what the recipe held on
//! them and drops their private plugs. The intrinsic and every other socket keep the recipe's.
use super::release::plug_perks;
use crate::tiger::{reader::Reader, shadowkeep};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use sundial::package_authoring::{self as authoring, fnv1, runtime::modifiers};

/// The native item definition and its inventory slot byte: 0 Kinetic, 1 Energy, 2 Power.
const ITEM: u32 = 0x8080_7BEA;
const SLOT: usize = 0xB8;
const KINETIC_SLOT: u8 = 0;
const ENERGY_SLOT: u8 = 1;
/// The socket type whose plug sets a weapon's elemental damage type.
const ELEMENTAL_SOCKET: u16 = 68;
/// Source weapons that are Kinetic unless the recipe names a damage type, by source weapon path.
const KINETIC: &[&str] = &[
    r"objects\weapons\rifle\dmr\dmr",
    r"objects\weapons\rifle\assault_rifle\assault_rifle",
];
/// The Reload Speed stat.
const RELOAD_SPEED: u32 = 0xF9A0_4987;
/// Native plugs a perk socket offers, and the category of a weapon's intrinsic frame.
const PERK_ITEM_KIND: u64 = 15;
const INTRINSIC: u64 = fnv1("intrinsics") as u64;
/// Plug categories that present a part of the weapon. A perk column offering none is a trait
/// column.
const PARTS: [u32; 13] = [
    fnv1("barrels"),
    fnv1("tubes"),
    fnv1("scopes"),
    fnv1("magazines"),
    fnv1("magazines_gl"),
    fnv1("batteries"),
    fnv1("stocks"),
    fnv1("grips"),
    fnv1("bowstrings"),
    fnv1("arrows"),
    fnv1("blades"),
    fnv1("guards"),
    fnv1("hafts"),
];
/// A donor lane the weapon leaves without choices.
const REMOVED: u16 = u16::MAX;
/// The most choices a column offers freely. The rest join the set the socket rolls one plug from,
/// which holds at most the rows one ownership mask covers.
const REUSABLE_CHOICES: usize = 4;
const ROLL_POOL: usize = 64;
/// An entity, its component rows, a component's owner and a Property Modifiers definition.
const ENTITY: u32 = 0x8080_9C0F;
const COMPONENT_ROW: u32 = 0x8080_9C04;
const OWNER: u32 = 0x8080_9C36;
const MODIFIERS: u32 = 0x8080_3AFE;
/// The modifier component interfaces that reach reload: every Reload input, and the Reload Speed
/// input of Weapon Stats.
const RELOAD_COMPONENT: u8 = 3;
const WEAPON_STATS: u8 = 14;
const RELOAD_SPEED_INPUT: u16 = 9;

/// Whether the source projectile `data` collects on its target and detonates at a threshold.
fn collects(data: &Value) -> bool {
    data["super_detonation_count"]
        .as_i64()
        .is_some_and(|count| count > 0)
}

/// A default damage type: its recipe name, the slot it belongs in and the donor slot byte it
/// moves the weapon out of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Damage {
    Void,
    Kinetic,
}

impl Damage {
    const fn name(self) -> &'static str {
        match self {
            Self::Void => "void",
            Self::Kinetic => "kinetic",
        }
    }

    const fn slot(self) -> &'static str {
        match self {
            Self::Void => "energy",
            Self::Kinetic => "kinetic",
        }
    }

    const fn other_slot(self) -> u8 {
        match self {
            Self::Void => KINETIC_SLOT,
            Self::Kinetic => ENERGY_SLOT,
        }
    }
}

/// Writes the perk columns and any default damage type of the source weapon at path `weapon`
/// into `recipe` and returns their receipt. `projectile` is the source projectile the weapon
/// fires, when it fires one barrel's.
pub(in crate::halo_reach) fn apply(
    native: &mut Reader,
    donor: u32,
    weapon: &Value,
    projectile: Option<&Value>,
    recipe: &mut Value,
) -> Result<Value> {
    let default = if projectile.is_some_and(collects) {
        Some((
            Damage::Void,
            "The fired source projectile detonates its collected shards at a threshold",
        ))
    } else if KINETIC.iter().any(|path| weapon.as_str() == Some(*path)) {
        Some((Damage::Kinetic, "The source weapon is listed as Kinetic"))
    } else {
        None
    };
    let report = shadowkeep::sockets(native, donor)?;
    // A Kinetic default can remove the donor's elemental socket, which the columns then keep.
    let damage = match default {
        Some((default, basis)) => {
            let mut receipt = damage(native, donor, default, &report, recipe)?;
            receipt["basis"] = json!(basis);
            receipt
        }
        None => json!({"status":"donor"}),
    };
    let columns = columns(native, &report, recipe)?;
    Ok(
        json!({"damage":damage,"columns":columns,"gameplay_verified":false,
        "limits":"A plug that changes reload another way than a Reload Speed stat or an attached reload modifier is not recognized."}),
    )
}

/// The `default` damage type, unless the recipe names one. The weapon moves to the default's slot
/// when the donor sits in the other one and the recipe names no slot.
fn damage(
    native: &mut Reader,
    donor: u32,
    default: Damage,
    report: &Value,
    recipe: &mut Value,
) -> Result<Value> {
    let overrides = &mut recipe["overrides"];
    if !overrides["modern_damage_type"].is_null() {
        return Ok(json!({"status":"retained","damage_type":overrides["modern_damage_type"]}));
    }
    overrides["modern_damage_type"] = json!(default.name());
    let tag = shadowkeep::item_tag(native, donor)?;
    let slot = native.tag(tag, Some(ITEM))?.u8(SLOT)?;
    if slot == default.other_slot() && overrides["inventory_slot"].is_null() {
        overrides["inventory_slot"] = json!(default.slot());
    }
    let inventory_slot = overrides["inventory_slot"].clone();
    let removed = if default == Damage::Kinetic {
        remove_elemental_socket(report, recipe)?
    } else {
        Vec::new()
    };
    Ok(
        json!({"status":"default","damage_type":default.name(),"donor_slot":slot,
        "inventory_slot":inventory_slot,"removed_elemental_sockets":removed}),
    )
}

/// Removes the donor's elemental damage socket in the recipe's socket columns, on each such lane
/// the recipe leaves to the donor, and returns the lanes. The build then carries the damage type
/// on the weapon itself, and Kinetic needs no carrier.
fn remove_elemental_socket(report: &Value, recipe: &mut Value) -> Result<Vec<usize>> {
    let sockets = report["sockets"].as_array().context("Donor sockets")?;
    let mut lanes = recipe["overrides"]["socket_columns"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if lanes.len() < sockets.len() {
        lanes.resize(sockets.len(), Value::Null);
    }
    let mut removed = Vec::new();
    for (lane, socket) in sockets.iter().enumerate() {
        if socket["socket_type"] == ELEMENTAL_SOCKET && lanes[lane].is_null() {
            lanes[lane] = json!({"choices":[],"socket_type":REMOVED});
            removed.push(lane);
        }
    }
    if !removed.is_empty() {
        let mut columns = json!(lanes);
        shadowkeep::fill_native_defaults(report, &mut columns, &mut Vec::new())?;
        recipe["overrides"]["socket_columns"] = columns;
    }
    Ok(removed)
}

/// Removes the donor's trait lanes and filters its part lanes in the recipe's socket columns.
/// `report` is the donor's sockets.
fn columns(native: &mut Reader, report: &Value, recipe: &mut Value) -> Result<Value> {
    let sockets = report["sockets"].as_array().context("Donor sockets")?;
    let stat = *shadowkeep::stat_definitions(native)?
        .get(&RELOAD_SPEED)
        .context("Reload Speed stat definition")?;
    let manager = authoring::PackageManager::new(
        &native.manager.package_dir,
        native.manager.version,
        Some(native.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let globals = manager
        .read_tag(
            authoring::resolve_live_named_tag(&manager, "investment_globals", None)
                .map_err(anyhow::Error::msg)?,
        )
        .map_err(anyhow::Error::msg)?;
    let mut lanes = recipe["overrides"]["socket_columns"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if lanes.len() < sockets.len() {
        lanes.resize(sockets.len(), Value::Null);
    }
    let mut owned = BTreeSet::new();
    let mut removed = Vec::new();
    let mut filtered = Vec::new();
    let mut unread = Vec::new();
    for (lane, socket) in sockets.iter().enumerate() {
        let choices = socket["choices"].as_array().context("Donor socket plugs")?;
        let perks = choices.iter().all(|choice| {
            choice["item_kind"].as_u64() == Some(PERK_ITEM_KIND)
                && choice["art_indices"].as_array().is_some_and(Vec::is_empty)
        });
        // The elemental damage socket keeps the plug that sets the damage type.
        if socket["socket_type"] == REMOVED
            || socket["socket_type"] == ELEMENTAL_SOCKET
            || choices.is_empty()
            || !perks
        {
            continue;
        }
        let default = choices
            .iter()
            .find(|choice| choice["default"] == true)
            .or_else(|| {
                choices
                    .iter()
                    .find(|choice| choice["hash"] == socket["curated_fallback"])
            })
            .unwrap_or(&choices[0]);
        if default["category"].as_u64() == Some(INTRINSIC) {
            continue;
        }
        owned.insert(lane as u64);
        let part = choices.iter().any(|choice| {
            choice["category"]
                .as_u64()
                .is_some_and(|category| PARTS.iter().any(|part| u64::from(*part) == category))
        });
        if !part {
            lanes[lane] = json!({"choices":[],"socket_type":REMOVED});
            removed.push(json!({"lane":lane,"reason":"trait column"}));
            continue;
        }
        let mut kept = Vec::<String>::new();
        let mut dropped = Vec::<Value>::new();
        for choice in std::iter::once(default).chain(choices) {
            let hash = u32::try_from(choice["hash"].as_u64().context("Donor plug identity")?)?;
            let plug = format!("0x{hash:08X}");
            if kept.contains(&plug) || dropped.iter().any(|row| row["plug"] == plug.as_str()) {
                continue;
            }
            let change =
                reload_change(native, &manager, &globals, choice, hash, stat, &mut unread)?;
            match change {
                Some(change) => dropped.push(json!({"plug":plug,"change":change})),
                None => kept.push(plug),
            }
        }
        if kept.is_empty() {
            lanes[lane] = json!({"choices":[],"socket_type":REMOVED});
            removed.push(json!({"lane":lane,"reason":"every plug changes reload",
                "dropped":dropped}));
        } else if dropped.is_empty() {
            // The donor's own column stands.
            lanes[lane] = Value::Null;
        } else {
            kept.truncate(ROLL_POOL);
            let count = kept.len();
            let mut column = json!({"choices":kept,"socket_type":socket["socket_type"]});
            if count > REUSABLE_CHOICES {
                column["reusable_choices"] = json!(REUSABLE_CHOICES);
                column["randomized_selection_program"] = json!([{"opcode":11,"operand":1}]);
            }
            lanes[lane] = column;
            filtered.push(json!({"lane":lane,"kept":count,"dropped":dropped}));
        }
    }
    // Read through `get_mut`, since indexing a recipe without private plugs would add a null.
    if let Some(variants) = recipe["overrides"]
        .get_mut("socket_plug_variants")
        .and_then(Value::as_array_mut)
    {
        variants.retain(|variant| {
            variant["socket_index"]
                .as_u64()
                .is_none_or(|lane| !owned.contains(&lane))
        });
    }
    let mut native_defaults = Vec::new();
    let mut columns = json!(lanes);
    if columns
        .as_array()
        .is_some_and(|lanes| lanes.iter().all(Value::is_null))
    {
        columns = json!([]);
    } else {
        shadowkeep::fill_native_defaults(report, &mut columns, &mut native_defaults)?;
    }
    recipe["overrides"]["socket_columns"] = columns;
    Ok(
        json!({"owned_lanes":owned,"removed":removed,"filtered":filtered,
        "native_defaults":native_defaults,"unread":unread}),
    )
}

/// How the native plug `choice` changes reload, when it does. A perk graph that does not read is
/// recorded in `unread` and counts as no change.
fn reload_change(
    native: &mut Reader,
    manager: &authoring::PackageManager,
    globals: &[u8],
    choice: &Value,
    hash: u32,
    stat: u16,
    unread: &mut Vec<Value>,
) -> Result<Option<Value>> {
    if let Some(value) = shadowkeep::investment_stats(native, hash)?
        .get(&stat)
        .filter(|value| **value != 0)
    {
        return Ok(Some(json!({"reload_speed_stat":value})));
    }
    let tag = u32::from_str_radix(choice["tag"].as_str().context("Donor plug tag")?, 16)?;
    let definition = native.tag(tag, None)?;
    // A plug whose perk rows do not read grants no perk this can follow.
    let Ok(perks) = plug_perks(&definition) else {
        return Ok(None);
    };
    for perk in perks {
        // A perk without a runtime action attaches nothing.
        let Ok(runtime) = authoring::sandbox_perk::load_sandbox_perk_runtime_action(
            manager,
            globals,
            usize::from(perk),
        ) else {
            continue;
        };
        for graph in &runtime.graphs {
            let graph = graph.tag.0;
            match reload_modifier(native, graph) {
                Ok(Some(record)) => {
                    return Ok(Some(json!({"perk":perk,"graph":format!("{graph:08X}"),
                        "modifier":record})));
                }
                Ok(None) => {}
                Err(error) => unread.push(json!({"plug":format!("0x{hash:08X}"),"perk":perk,
                    "graph":format!("{graph:08X}"),"reason":format!("{error:#}")})),
            }
        }
    }
    Ok(None)
}

/// The first Property Modifiers record of the entity `graph` that changes reload. An Add of zero
/// or a Multiply by one leaves its input alone, as does any other operation.
fn reload_modifier(native: &mut Reader, graph: u32) -> Result<Option<Value>> {
    // A graph of another class holds no components.
    let Ok(entity) = native.tag(graph, Some(ENTITY)) else {
        return Ok(None);
    };
    for row in entity.array(16, 12, Some(COMPONENT_ROW))? {
        let Ok(owner) = native.tag(entity.u32(row)?, Some(OWNER)) else {
            continue;
        };
        let definition = owner.pointer(24)?;
        if definition < 4 || owner.u32(definition - 4)? != MODIFIERS {
            continue;
        }
        for record in owner.array(definition + 88, 88, Some(modifiers::SETTINGS_SCHEMA))? {
            let component = owner.u8(record + modifiers::COMPONENT_OFFSET as usize)?;
            let input = owner.u16(record + modifiers::INPUT_OFFSET as usize)?;
            if component != RELOAD_COMPONENT
                && (component != WEAPON_STATS || input != RELOAD_SPEED_INPUT)
            {
                continue;
            }
            let operation = owner.u8(record + modifiers::OPERATION_OFFSET as usize)?;
            let neutral = match i64::from(operation) {
                modifiers::OPERATION_ADD => 0.,
                modifiers::OPERATION_MULTIPLY => 1.,
                _ => continue,
            };
            let amount = f32::from_bits(owner.u32(record + modifiers::AMOUNT_OFFSET as usize)?);
            if amount != neutral {
                return Ok(Some(json!({"component":component,"input":input,
                    "operation":operation,"amount":amount})));
            }
        }
    }
    Ok(None)
}
