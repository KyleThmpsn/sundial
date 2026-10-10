//! The entity graphs an entity names from its component owners: the projectiles, explosions and
//! hop-ons an ability spawns or attaches. A stock grenade names 4 to 17 of them, most from its
//! bank and its throw component. Each is found as a live entity tag in an owner's payload and
//! placed by the component binding resource it sits in, with its offset into that resource,
//! which is how a runtime resource patch names a place.
//!
//! An owner can also name an impact table, which names the graphs its impacts spawn. Suppressor
//! Grenade's projectile owner `81578A97` names table `80BFA1E2` at `+0xE10`, and the table names
//! the detonation graph `80BFA4B6` that draws Suppressor's colors, so those graphs are reached
//! only through the table.
use std::collections::{BTreeMap, BTreeSet};

use crate::entity::{
    WEAPON_ENTITY_CLASS, weapon_component_binding_hashes, weapon_component_bindings,
};
use crate::package_payload::u32_at;
use crate::package_runtime::reader::PackageManager;
use crate::sandbox_perk::entity::catalog::Catalog;

/// Where an entity graph keeps the client's object type.
const OBJECT_TYPE: usize = 0x96;
const PROJECTILE_OBJECT_TYPE: u8 = 18;

/// Components that set a spawned graph apart, in the order a name prefers them, with the words
/// the name uses. Each is a class `runtime::native_type_name` names from evidence.
const DISTINCT: [(u32, &str); 7] = [
    (0x8080_43DF, "Invisibility"),
    (0x8080_3F8B, "Incoming Damage Modifiers"),
    (0x8080_3B00, "Property Modifiers"),
    (0x8080_4BEE, "Health and Shields"),
    (0x8080_4211, "Status Icon"),
    (0x8080_3C50, "Self-Destruct Timer"),
    (0x8080_72B8, "Gear Model"),
];

/// One place an entity names another entity graph.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Spawn {
    pub graph: u32,
    /// The component owner whose payload names it.
    pub owner: u32,
    pub binding_hash: u32,
    pub resource_index: u16,
    /// Byte offset of the tag from the start of that resource.
    pub offset: u32,
}

/// Every place `entity`'s component owners name another entity graph, by owner and offset.
pub fn spawns(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<Spawn>, String> {
    places(manager, entity, |value, class| {
        value != entity_tag && class == WEAPON_ENTITY_CLASS
    })
}

/// Every place `entity`'s component owners name the entity itself, as a chain that spawns itself
/// again does: Arcbolt Grenade's chain graph `80B805AA` names itself. A private copy names itself
/// there.
pub fn self_spawns(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<Spawn>, String> {
    places(manager, entity, |value, _| value == entity_tag)
}

/// The classes of impact tables. A `80808BCD` table names graphs in rows of up to three, and a
/// `80808BCB` table names other tables. The 2026-10-05 census of the stock abilities' declared
/// helper routes found 91 and 8 of the 174 resources between an ability's owners and the graphs
/// they reach that way, with these two classes.
const TABLES: [u32; 2] = [0x8080_8BCD, 0x8080_8BCB];

/// Whether `tag` is an impact table.
pub fn is_table(manager: &PackageManager, tag: u32) -> bool {
    manager
        .get_entry(tag)
        .is_some_and(|entry| entry.file_type == 8 && TABLES.contains(&entry.reference))
}

/// Every place `entity`'s component owners name an impact table, by owner and offset, with the
/// table's tag in `graph`.
pub fn tables(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<Spawn>, String> {
    let mut found = places(manager, entity, |value, class| {
        value != entity_tag && TABLES.contains(&class)
    })?;
    found.retain(|place| is_table(manager, place.graph));
    Ok(found)
}

/// The outgoing graphs, impact tables and self references found in one owner scan.
#[derive(Default)]
pub struct Links {
    pub spawns: Vec<Spawn>,
    pub tables: Vec<Spawn>,
    pub selves: Vec<Spawn>,
}

/// Collects all three kinds together when traversing or copying a graph. Each list
/// retains the owner and offset order of its individual collector.
pub fn links(manager: &PackageManager, entity_tag: u32, entity: &[u8]) -> Result<Links, String> {
    let places = places(manager, entity, |value, class| {
        value == entity_tag || class == WEAPON_ENTITY_CLASS || TABLES.contains(&class)
    })?;
    let mut found = Links::default();
    for place in places {
        if place.graph == entity_tag {
            found.selves.push(place);
        } else if manager
            .get_entry(place.graph)
            .is_some_and(|entry| entry.reference == WEAPON_ENTITY_CLASS)
        {
            found.spawns.push(place);
        } else if is_table(manager, place.graph) {
            found.tables.push(place);
        }
    }
    Ok(found)
}

/// The graphs and impact tables `table` names, each with the offset of every declared reference
/// field naming it, in the order they first appear. A `80808BCB` table can name the table above
/// it and itself, as `80C70C92` names `80C70C91` at `+0xC` and itself at `+0x10`, so the list can
/// hold `table`.
pub fn table_entries(
    manager: &PackageManager,
    table: u32,
) -> Result<Vec<(u32, Vec<usize>)>, String> {
    let mut entries = Vec::<(u32, Vec<usize>)>::new();
    for (offset, tag) in crate::package_runtime::references::declared_fields(manager, table)? {
        let Some(entry) = manager.get_entry(tag) else {
            continue;
        };
        if entry.reference != WEAPON_ENTITY_CLASS && !is_table(manager, tag) {
            continue;
        }
        match entries.iter_mut().find(|(each, _)| *each == tag) {
            Some((_, offsets)) => offsets.push(offset),
            None => entries.push((tag, vec![offset])),
        }
    }
    Ok(entries)
}

/// The graphs `table` names, through any tables it names, each once, in the order found.
pub fn table_graphs(manager: &PackageManager, table: u32) -> Result<Vec<u32>, String> {
    let mut graphs = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![table];
    while let Some(table) = pending.pop() {
        if !seen.insert(table) {
            continue;
        }
        let mut nested = Vec::new();
        for (tag, _) in table_entries(manager, table)? {
            if is_table(manager, tag) {
                nested.push(tag);
            } else if !graphs.contains(&tag) {
                graphs.push(tag);
            }
        }
        pending.extend(nested.into_iter().rev());
    }
    Ok(graphs)
}

/// The graphs `entity` names directly or through its impact tables, each once: those `spawns`
/// finds, then those its tables name.
pub fn reached_graphs(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<u32>, String> {
    let links = links(manager, entity_tag, entity)?;
    let mut graphs = Vec::new();
    for spawn in links.spawns {
        if !graphs.contains(&spawn.graph) {
            graphs.push(spawn.graph);
        }
    }
    for place in links.tables {
        for graph in table_graphs(manager, place.graph)? {
            if graph != entity_tag && !graphs.contains(&graph) {
                graphs.push(graph);
            }
        }
    }
    Ok(graphs)
}

/// Every place `entity`'s component owners name a live tag that `wanted` accepts, by the tag and its
/// class.
pub(super) fn places(
    manager: &PackageManager,
    entity: &[u8],
    wanted: impl Fn(u32, u32) -> bool,
) -> Result<Vec<Spawn>, String> {
    // Each owner's resources, by where they start in its payload.
    let mut resources = BTreeMap::<u32, Vec<(u64, u32, u16)>>::new();
    for binding in weapon_component_binding_hashes(entity)? {
        for resource in weapon_component_bindings(entity, binding)? {
            let index = u16::try_from(resource.resource_index)
                .map_err(|_| "A component binding selects too many resources".to_owned())?;
            resources.entry(resource.owner_tag).or_default().push((
                resource.resource_offset,
                binding,
                index,
            ));
        }
    }
    let mut found = Vec::new();
    for (owner, mut starts) in resources {
        starts.sort_unstable();
        starts.dedup_by_key(|(start, ..)| *start);
        let payload = manager.read_tag(owner)?;
        for at in (0..payload.len().saturating_sub(3)).step_by(4) {
            let value = u32_at(&payload, at)?;
            if value == owner || !(0x8080_0000..0x8200_0000).contains(&value) {
                continue;
            }
            if manager
                .get_entry(value)
                .is_none_or(|entry| !wanted(value, entry.reference))
            {
                continue;
            }
            let at = at as u64;
            // A word before the owner's first resource belongs to no binding a patch can name.
            let Some(&(start, binding_hash, resource_index)) =
                starts.iter().rev().find(|(start, ..)| *start <= at)
            else {
                continue;
            };
            found.push(Spawn {
                graph: value,
                owner,
                binding_hash,
                resource_index,
                offset: u32::try_from(at - start)
                    .map_err(|_| "A spawned graph sits too far into its resource".to_owned())?,
            });
        }
    }
    Ok(found)
}

/// The graphs `entity` spawns, each once, in the order `spawns` finds them, those its ability bank
/// names among them. A change below the bank gives the ability a private copy of its bank.
pub fn spawned_graphs(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<u32>, String> {
    let mut graphs = Vec::new();
    for spawn in spawns(manager, entity_tag, entity)? {
        if !graphs.contains(&spawn.graph) {
            graphs.push(spawn.graph);
        }
    }
    Ok(graphs)
}

/// What the client's object type makes a graph, in the words a list shows.
const fn kind(object_type: u8) -> &'static str {
    match object_type {
        1 => "Static Mesh",
        2..=9 => "Prop",
        11 => "Interactive Object",
        12 => "Biped",
        13 => "Creature",
        14 => "Weapon",
        15 => "Vehicle",
        16 => "Turret",
        17 => "Emitter",
        18 => "Projectile",
        19..=21 => "Pickup",
        22 => "Gear",
        23..=27 => "Hop-On",
        28 => "System",
        _ => "Entity",
    }
}

/// A spawned graph's name from its own data: its kind, from the client's object type, and the
/// one component that sets it apart, when it has one. A projectile's kind says enough.
pub fn describe(entity: &[u8]) -> Result<String, String> {
    let object_type = *entity
        .get(OBJECT_TYPE)
        .ok_or("The entity graph has no object type")?;
    let kind = kind(object_type);
    if object_type == PROJECTILE_OBJECT_TYPE {
        return Ok(kind.to_owned());
    }
    let mut classes = BTreeSet::new();
    for binding in weapon_component_binding_hashes(entity)? {
        for resource in weapon_component_bindings(entity, binding)? {
            classes.insert(resource.concrete_class);
        }
    }
    Ok(DISTINCT
        .iter()
        .find(|(class, _)| classes.contains(class))
        .map_or_else(|| kind.to_owned(), |(_, words)| format!("{kind} · {words}")))
}

/// Names for graphs an entity spawns, in order: the object catalog's native name when it has
/// one, else what `describe` reads. Names that repeat are numbered after their kind.
pub fn names(manager: &PackageManager, graphs: &[u32], objects: Option<&Catalog>) -> Vec<String> {
    let mut names = graphs
        .iter()
        .map(|&graph| {
            objects
                .and_then(|objects| objects.entries.iter().find(|entry| entry.graph == graph))
                .filter(|entry| entry.label_rank() == 0)
                .map(|entry| entry.label())
                .or_else(|| {
                    let payload = manager.read_tag(graph).ok()?;
                    describe(&payload).ok()
                })
                .unwrap_or_else(|| format!("Graph 0x{graph:08X}"))
        })
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<String, usize>::new();
    for name in &names {
        *counts.entry(name.clone()).or_default() += 1;
    }
    let mut seen = BTreeMap::<String, usize>::new();
    for name in &mut names {
        if counts.get(name.as_str()).is_some_and(|count| *count > 1) {
            let number = seen.entry(name.clone()).or_default();
            *number += 1;
            *name = match name.split_once(" · ") {
                Some((kind, rest)) => format!("{kind} {number} · {rest}"),
                None => format!("{name} {number}"),
            };
        }
    }
    names
}
