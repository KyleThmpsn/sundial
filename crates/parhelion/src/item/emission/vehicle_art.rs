//! The inventory model of a Sparrow that summons another vehicle. The inventory and inspect
//! screens draw the item's gear art, apart from the graph it summons, so a Pike summon still shows
//! a Sparrow there. The item takes a private gear-art row copied from its own, with every
//! assignment rewritten:
//!
//! - None: each names a stock part whose relation names no entity, which draws nothing.
//! - Vehicle: each names a private copy of its part whose model owner is the summoned vehicle's.
//!   The copy's typed pointers to the owner move to the same blocks of the vehicle's owner, and
//!   the vehicle's graph, which the summon enrolls, keeps loading the model.
//!
//! Parts with no model, such as marker sets alone, keep their stock assignments.
use super::*;
use crate::tag_payload::{read_u64, relative_target, write_u64};
use crate::vehicle::{InventoryModel, Sparrow, Summon};
use linking::{Companion, Companions, Node};
use reskin::{array, owner_slots, read};

const ENTITY_CLASS: u32 = 0x8080_9C0F;
const RESOURCE_CLASS: u32 = 0x8080_9C36;
const RELATION_CLASS: u32 = 0x8080_744A;
const MODEL_OWNER_HEADER: u32 = 0x8080_72B8;
const MODEL_OWNER_DATA: u32 = 0x8080_72BD;
/// An entity's component rows, owner tag first.
const COMPONENTS: usize = 0x10;
const COMPONENT_ROW: usize = 12;

pub(super) fn apply(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    weapons: &[WeaponCloneSpec],
    replacements: &mut Vec<ReplacementSpec>,
) -> AuthoringResult<()> {
    let mut companions = Companions::new();
    for spec in weapons {
        let Some(sparrow) = spec.overrides.sparrow.as_ref() else {
            continue;
        };
        if sparrow.summon == Summon::Sparrow || sparrow.inventory_model == InventoryModel::Sparrow {
            continue;
        }
        let item = spec.identity.item_hash;
        model(
            directory,
            manager,
            emission,
            (&mut companions, &mut *replacements),
            (item, sparrow),
        )
        .map_err(|error| {
            error.context(format!("Sparrow {:?}: its inventory model", spec.text.name))
        })?;
    }
    Ok(())
}

fn model(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    (companions, replacements): (&mut Companions, &mut Vec<ReplacementSpec>),
    (item, sparrow): (u32, &Sparrow),
) -> AuthoringResult<()> {
    let ordinal = definition_ordinal(emission, item)?;
    let art_rows = weapon_art_arrangements(&emission.host_new_tags[ordinal].payload)?;
    let indices = art_rows
        .iter()
        .map(|row| row.arrangement)
        .collect::<BTreeSet<_>>();
    let [source_row] = indices.into_iter().collect::<Vec<_>>()[..] else {
        return Err(invalid(
            "A changed inventory model needs one gear-art row for every class",
        ));
    };
    let source_row = usize::from(source_row);
    let (count, _, rows, _) = array(&emission.item_metadata, 8)?;
    if source_row >= count {
        return Err(invalid("The gear-art row is outside the metadata table"));
    }
    let keys = art::row_keys(&emission.item_metadata, rows + source_row * 32)?;
    if keys.is_empty() {
        return Err(invalid("The gear art lists no parts"));
    }
    let table = match replacements
        .iter()
        .find(|replacement| replacement.tag == art::ASSIGNMENT_TABLE)
    {
        Some(replacement) => replacement.payload.clone(),
        None => read(manager, art::ASSIGNMENT_TABLE.0)?,
    };
    let map = assignments(&table)?;
    let (substitutions, entries) = match sparrow.inventory_model {
        InventoryModel::Sparrow => return Ok(()),
        InventoryModel::Empty => {
            let empty = empty_part(manager, &map)?;
            let substitutions: BTreeMap<u32, u32> = keys.iter().map(|key| (*key, empty)).collect();
            (substitutions, Vec::new())
        }
        InventoryModel::Vehicle => vehicle_parts(
            directory,
            manager,
            emission,
            companions,
            (&map, &keys),
            (item, &sparrow.summon),
        )?,
    };
    if substitutions.is_empty() {
        return Err(invalid("No part of the gear art has a model to change"));
    }
    let row_index = art::prepare_row_at(emission, item, source_row)?;
    let (_, _, rows, _) = array(&emission.item_metadata, 8)?;
    art::rewrite(
        &mut emission.item_metadata,
        rows + row_index * 32,
        rows + source_row * 32,
        |singles, slots| {
            for key in singles
                .iter_mut()
                .chain(slots.iter_mut().flat_map(|(_, keys)| keys))
            {
                if let Some(private) = substitutions.get(key) {
                    *key = *private;
                }
            }
            Ok(())
        },
    )?;
    let arrangement =
        u16::try_from(row_index).map_err(|_| invalid("Gear-art row index overflow"))?;
    let updated = art_rows
        .iter()
        .map(|row| WeaponArtArrangementOverride {
            character_class: row.character_class,
            arrangement,
        })
        .collect::<Vec<_>>();
    set_weapon_art_arrangements(&mut emission.host_new_tags[ordinal].payload, &updated)?;
    if !entries.is_empty() {
        let replacement = art::insert_assignments(&table, &entries)?;
        replacements.retain(|existing| existing.tag != art::ASSIGNMENT_TABLE);
        replacements.push(replacement);
    }
    Ok(())
}

/// The assignment map's rows: gear-art key to relation tag.
fn assignments(table: &[u8]) -> AuthoringResult<BTreeMap<u32, u32>> {
    let (count, _, rows, _) = array(table, 8)?;
    (0..count)
        .map(|i| {
            Ok((
                read_u32(table, rows + i * 8)?,
                read_u32(table, rows + i * 8 + 4)?,
            ))
        })
        .collect()
}

/// A stock assignment whose relation names no entity. Stock gear art lists such empty parts,
/// and they draw nothing.
fn empty_part(
    manager: &sundial::package_authoring::PackageManager,
    map: &BTreeMap<u32, u32>,
) -> AuthoringResult<u32> {
    for (&key, &relation) in map {
        if matches!(key, 0 | u32::MAX | 0x811C_9DC5)
            || manager
                .get_entry(TagHash(relation))
                .is_none_or(|entry| entry.reference != RELATION_CLASS)
        {
            continue;
        }
        if read_u32(&read(manager, relation)?, 0x10)? == u32::MAX {
            return Ok(key);
        }
    }
    Err(invalid("No stock gear-art part is empty"))
}

/// Each source key's substitute, and the assignments the private copies take.
type VehicleParts = (BTreeMap<u32, u32>, Vec<(u32, TagHash)>);

/// Private copies of the parts `keys` name, each drawing `vehicle`'s model.
fn vehicle_parts(
    directory: &Path,
    manager: &sundial::package_authoring::PackageManager,
    emission: &mut PackageEmission,
    companions: &mut Companions,
    (map, keys): (&BTreeMap<u32, u32>, &[u32]),
    (item, vehicle): (u32, &Summon),
) -> AuthoringResult<VehicleParts> {
    let entity = vehicle
        .entity()
        .map_err(invalid)?
        .ok_or_else(|| invalid("The Sparrow summons itself"))?;
    let (vehicle_owner, vehicle_bytes) = model_owner(manager, &read(manager, entity)?)?
        .ok_or_else(|| invalid(format!("Vehicle 0x{entity:08X} owns no model")))?;
    let vehicle_blocks = blocks(&vehicle_bytes)?;
    let mut nodes = Vec::new();
    let mut parts = Vec::new();
    for key in keys {
        let relation_tag = *map
            .get(key)
            .ok_or_else(|| invalid(format!("Gear-art key 0x{key:08X} has no assignment")))?;
        let relation = read(manager, relation_tag)?;
        let part_tag = read_u32(&relation, 0x10)?;
        // An empty part names no entity and draws nothing, so its stock key can stay.
        if part_tag == u32::MAX {
            continue;
        }
        if manager
            .get_entry(TagHash(part_tag))
            .is_none_or(|entry| entry.reference != ENTITY_CLASS)
        {
            return Err(invalid(format!(
                "Gear-art relation 0x{relation_tag:08X} does not name an entity"
            )));
        }
        let mut part = read(manager, part_tag)?;
        let Some((owner_tag, owner_bytes)) = model_owner(manager, &part)? else {
            continue;
        };
        retarget_model(
            &mut part,
            (owner_tag, &owner_bytes),
            (vehicle_owner, vehicle_blocks),
        )
        .map_err(|error| error.context(format!("Gear part 0x{part_tag:08X}")))?;
        let prefix = format!("part{}", parts.len());
        let entity_symbol = format!("{prefix}-entity");
        let parent_symbol = format!("{prefix}-parent");
        nodes.push(Node::new(entity_symbol.clone(), part_tag, part));
        let mut parent = Node::new(parent_symbol.clone(), relation_tag, relation);
        parent.patch(0x10, entity_symbol)?;
        nodes.push(parent);
        let mut companion = Node::new(format!("{prefix}-parent-companion"), 0, Vec::new());
        companion.companion = Some(Companion::new(parent_symbol.clone(), relation_tag));
        nodes.push(companion);
        parts.push((*key, private_key(item, *key), parent_symbol));
    }
    if parts.is_empty() {
        return Ok((BTreeMap::new(), Vec::new()));
    }
    let linked = linking::link(
        directory,
        emission,
        manager,
        nodes,
        "part0-parent",
        companions,
        0,
        |_, _| Ok(()),
        None,
    )?;
    let mut substitutions = BTreeMap::new();
    let mut entries = Vec::with_capacity(parts.len());
    for (key, private, symbol) in parts {
        let tag = *linked
            .symbols
            .get(&symbol)
            .ok_or_else(|| invalid("Parent symbol missing"))?;
        substitutions.insert(key, private);
        entries.push((private, tag));
    }
    Ok((substitutions, entries))
}

/// The one component owner of `entity` whose header names a model, with its payload.
fn model_owner(
    manager: &sundial::package_authoring::PackageManager,
    entity: &[u8],
) -> AuthoringResult<Option<(u32, Vec<u8>)>> {
    let (count, _, rows, _) = array(entity, COMPONENTS)?;
    let mut found = None;
    for i in 0..count {
        let tag = read_u32(entity, rows + i * COMPONENT_ROW)?;
        if manager
            .get_entry(TagHash(tag))
            .is_none_or(|entry| entry.reference != RESOURCE_CLASS)
        {
            continue;
        }
        let bytes = read(manager, tag)?;
        let header = relative_target(&bytes, 0x10)?;
        if header < 4 || read_u32(&bytes, header - 4)? != MODEL_OWNER_HEADER {
            continue;
        }
        if found.as_ref().is_some_and(|(seen, _)| *seen != tag) {
            return Err(invalid("The entity owns several models"));
        }
        found = Some((tag, bytes));
    }
    Ok(found)
}

/// Where a model owner's header and data blocks begin.
fn blocks(owner: &[u8]) -> AuthoringResult<(usize, usize)> {
    let header = relative_target(owner, 0x10)?;
    let data = relative_target(owner, 0x18)?;
    if header < 4
        || data < 4
        || read_u32(owner, header - 4)? != MODEL_OWNER_HEADER
        || read_u32(owner, data - 4)? != MODEL_OWNER_DATA
    {
        return Err(invalid("The model owner has an unsupported layout"));
    }
    Ok((header, data))
}

/// Points `part`'s model owner at `vehicle`'s: each word naming the owner names the vehicle's,
/// and each typed pointer to one of the owner's blocks points at the same block of the
/// vehicle's owner. A pointer anywhere else is refused.
fn retarget_model(
    part: &mut [u8],
    (owner_tag, owner): (u32, &[u8]),
    (vehicle_tag, (vehicle_header, vehicle_data)): (u32, (usize, usize)),
) -> AuthoringResult<()> {
    let (header, data) = blocks(owner)?;
    let (count, _, rows, _) = array(part, COMPONENTS)?;
    let roots = (0..count)
        .map(|i| rows + i * COMPONENT_ROW)
        .collect::<BTreeSet<_>>();
    for slot in owner_slots(part, owner, owner_tag)? {
        write_u32(part, slot, vehicle_tag)?;
        if roots.contains(&slot) {
            continue;
        }
        let class = read_u32(part, slot + 4)?;
        let target = usize::try_from(read_u64(part, slot + 8)?)
            .map_err(|_| invalid("A model owner pointer is out of range"))?;
        let moved = match (class, target) {
            (MODEL_OWNER_HEADER, at) if at == header => vehicle_header,
            (MODEL_OWNER_DATA, at) if at == data => vehicle_data,
            _ => {
                return Err(invalid(format!(
                    "A pointer into the model owner names class 0x{class:08X} at {target:#x}"
                )));
            }
        };
        write_u64(part, slot + 8, moved as u64)?;
    }
    Ok(())
}

/// A stable private assignment key that cannot collide with the sentinels.
fn private_key(item: u32, source: u32) -> u32 {
    let hash = sundial::package_authoring::fnv1_name_hash(&format!(
        "parhelion/vehicle-art/{item:08X}/{source:08X}"
    ));
    if matches!(hash, 0 | u32::MAX | 0x811C_9DC5) {
        hash ^ 0x10000
    } else {
        hash
    }
}
