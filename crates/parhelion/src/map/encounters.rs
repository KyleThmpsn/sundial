//! A converted mission's own squads: an encounter object per area whose squads stand where the
//! level's squads do, built the way a stock mission's are.
//!
//! A stock encounter object (`80809462`) lists, in a handle group for every bubble, an object list
//! of combatant spawn points and one spawn rule per squad, and in a group for its bubble one AI
//! spawner per squad. A spawner's placed record names its primary component at header `+0x10` and
//! its secondary at `+0x18`. The secondary is also its keyed slot descriptor: the object's key at
//! `+0x30`, slot type 1 at `+0x34` and its slot index at `+0x36`, its name at `+0x50`, the raw
//! object-slot references of its spawn rules at `+0x98` and `+0xA0` (the object's key, slot type 66 at bit 32 and
//! the rule's index at bit 48), and its members at `+0xA8`. A member's six candidate arrays, one
//! per difficulty, point at inline placement rows whose entity is the member's actor. A spawn
//! rule's secondary is its keyed descriptor (type 66), and its point arrays name the spawn point
//! row it spawns at by that row's 64-bit placement identity. Each record's name is its last
//! content, a `80800065` blob of a `u64` length and the bytes.
//!
//! The donor's squads are templates. For each squad of the level and each character among its
//! spawn points, the copy clones the donor squad whose actor the character's entry in
//! `combatants.json` prefers, gives it the level squad's name, its own slot indices and keys, and
//! a spawn point row of its own at the level squad's first spawn point of that character. The
//! squad's member count is the number of those spawn points, which a mission script places.
use std::collections::BTreeMap;

use parhelion_import::halo_reach::level::squads::Squad;
use sundial::package_authoring::{PackageManager, fnv1_name_hash};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, read_i64, read_tag, read_u32, read_u64,
        synchronize_payload_size, write_u16, write_u32, write_u64,
    },
};

use super::{
    chain,
    placed::{Placed, clone_placed, keyed, read as placed, renamed, set_slot},
};

mod point;

const OBJECT_KEY: usize = 0x0C;
/// The slot counts of an object's two handle groups, its slot table and its handle groups.
const OBJECT_SLOT_COUNTS: usize = 0x18;
const OBJECT_SLOTS: usize = 0x20;
const OBJECT_GROUPS: usize = 0x38;
const SLOT_CLASS: u32 = 0x8080_9B3A;
const GROUP_SIZE: usize = 24;
const GROUP_BUBBLE: usize = 0x00;
const GROUP_HANDLES: usize = 0x08;
const HANDLE_ROW_CLASS: u32 = 0x8080_9466;
/// A handle names a list at `+0x08`, or none, and its redirects at `+0x10`.
const HANDLE_CLASS: u32 = 0x8080_9468;
const HANDLE_LIST: usize = 0x08;
const LIST_CLASS: u32 = 0x8080_99D6;
/// A placed record's header pointers to its primary and secondary components.
const RECORD_PRIMARY: usize = 0x10;
/// Keyed descriptor fields, at the secondary component.
const DESCRIPTOR_CLASS: usize = 0x04;
const DESCRIPTOR_SLOT_INDEX: usize = 0x36;
const SPAWNER_CLASS: u32 = 0x8080_9A3B;
const RULE_CLASS: u32 = 0x8080_94CF;
const SQUAD_SLOT: u16 = 1;
const RULE_SLOT: u16 = 66;
/// The spawner secondary's two spawn-rule references and members, and a member's candidate arrays.
const SPAWNER_OTHER_RULE: usize = 0x98;
const SPAWNER_RULE: usize = 0xA0;
const NULL_SLOT: u64 = 0xffff_00ff_811c_9dc5;
const SPAWNER_MEMBERS: usize = 0xA8;
const MEMBER_CANDIDATES: usize = 0x08;
const CANDIDATE_ARRAYS: usize = 6;
const CANDIDATE_ARRAY_SIZE: usize = 16;
/// A spawn rule primary's point array: 72-byte rows whose placement identity is their first field.
const RULE_POINTS: usize = 0x1A0;
const RULE_POINT_IDENTITY: usize = 0x00;

const COMBATANTS: &str = include_str!("encounters/combatants.json");

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Combatants {
    schema: u32,
    /// Additional authored encounter objects providing actors absent from the activity donor.
    #[serde(default)]
    encounters: Vec<String>,
    /// Per Reach character, the actor classes that stand in for it, most fitting first.
    characters: BTreeMap<String, Vec<String>>,
}

/// The actor classes `character` takes, most fitting first: its own entry, else the entry of the
/// name with its last `_` part removed, and so on.
fn actors(table: &Combatants, character: &str) -> AuthoringResult<Vec<u32>> {
    let mut name = character;
    loop {
        if let Some(found) = table.characters.get(name) {
            return found
                .iter()
                .map(|text| {
                    u32::from_str_radix(text.trim_start_matches("0x"), 16)
                        .map_err(|_| invalid(format!("Combatant {name} names {text}")))
                })
                .collect();
        }
        let Some((shorter, _)) = name.rsplit_once('_') else {
            return Ok(Vec::new());
        };
        name = shorter;
    }
}

/// One donor squad: its spawner, its rule and the placement identity of its spawn point.
#[derive(Clone)]
struct Template {
    actor: u32,
    spawner: Placed,
    rule: Placed,
    identity: u64,
    dependencies: Vec<TagHash>,
}

/// The donor encounter the copy's encounters are built from.
#[derive(Clone)]
pub(super) struct Templates {
    object: (TagHash, Vec<u8>),
    /// The handle naming the object's spawn point list, and the list.
    list_handle: (TagHash, Vec<u8>),
    list: (TagHash, Vec<u8>),
    /// The list row the first squad spawns at, which the copy's points are copies of.
    row: usize,
    squads: Vec<Template>,
}

/// One squad of the copy's encounter.
struct Native {
    name: String,
    source: String,
    template: usize,
    anchor: parhelion_import::halo_reach::level::spawn::Spawn,
    /// How many of the level squad's spawn points take this character, the members a mission
    /// script places.
    count: usize,
}

/// An area's encounter: the donor templates and its squads.
pub(super) struct Encounter {
    templates: Templates,
    squads: Vec<Native>,
    key: u32,
}

fn class(manager: &PackageManager, tag: u32) -> Option<u32> {
    manager.get_entry(TagHash(tag)).map(|entry| entry.reference)
}

fn target(payload: &[u8], field: usize) -> AuthoringResult<usize> {
    let relative = read_i64(payload, field)?;
    field
        .checked_add_signed(relative as isize)
        .filter(|&at| relative != 0 && at < payload.len())
        .ok_or_else(|| invalid(format!("Pointer at 0x{field:X} leaves its record")))
}

/// The `u32` rows of the array at `descriptor`, none when it is empty.
fn words(payload: &[u8], descriptor: usize) -> AuthoringResult<Vec<u32>> {
    if read_u64(payload, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, _) = array_at(payload, descriptor)?;
    (0..count)
        .map(|index| read_u32(payload, rows + index * 4))
        .collect()
}

/// A spawner's actor: the entity of its first member's first candidate placement.
fn spawner_actor(record: &[u8]) -> AuthoringResult<Option<u32>> {
    let secondary = keyed(record)?;
    if read_u64(record, secondary + SPAWNER_MEMBERS)? == 0 {
        return Ok(None);
    }
    let (_, _, members, _) = array_at(record, secondary + SPAWNER_MEMBERS)?;
    for array in 0..CANDIDATE_ARRAYS {
        let field = members + MEMBER_CANDIDATES + array * CANDIDATE_ARRAY_SIZE;
        if read_u64(record, field)? == 0 {
            continue;
        }
        let (_, _, candidates, _) = array_at(record, field)?;
        let placement = target(record, candidates)?;
        return Ok(Some(read_u32(record, placement)?));
    }
    Ok(None)
}

/// The placement identity of a spawn rule's first point.
fn rule_identity(record: &[u8]) -> AuthoringResult<Option<u64>> {
    let primary = target(record, RECORD_PRIMARY)?;
    if read_u64(record, primary + RULE_POINTS)? == 0 {
        return Ok(None);
    }
    let (_, _, points, _) = array_at(record, primary + RULE_POINTS)?;
    Ok(Some(read_u64(record, points + RULE_POINT_IDENTITY)?))
}

impl Templates {
    pub(super) fn supplement(&mut self, manager: &PackageManager) -> AuthoringResult<()> {
        let table: Combatants = serde_json::from_str(COMBATANTS)
            .map_err(|error| invalid(format!("combatants.json: {error}")))?;
        for text in table.encounters {
            let tag = u32::from_str_radix(text.trim_start_matches("0x"), 16)
                .map_err(|_| invalid("Invalid encounter template identity"))?;
            if class(manager, tag) != Some(0x8080_9462) {
                return Err(invalid(format!(
                    "Encounter template {tag:08X} has the wrong class"
                )));
            }
            let payload = read_tag(manager, TagHash(tag), "additional encounter template")?;
            let mut squads = Self::find(manager, tag, &payload)?
                .map_or_else(Vec::new, |template| template.squads);
            let rule = self
                .squads
                .first()
                .ok_or_else(|| invalid("No native spawn rule template"))?;
            squads.extend(unbound(manager, &payload, rule)?);
            if squads.is_empty() {
                return Err(invalid(format!(
                    "Encounter template {tag:08X} has no usable squads"
                )));
            }
            let actors = squads
                .iter()
                .map(|squad| squad.actor)
                .collect::<std::collections::BTreeSet<_>>();
            let dependencies =
                sundial::package_authoring::sandbox_perk::entity::residency::additions(
                    manager, actors,
                )
                .map_err(|error| {
                    invalid(format!("Additional encounter actor dependencies: {error}"))
                })?
                .into_iter()
                .map(TagHash)
                .collect::<Vec<_>>();
            for squad in &mut squads {
                squad.dependencies.extend(dependencies.iter().copied());
                for placed in [&squad.spawner, &squad.rule] {
                    squad.dependencies.extend(sundial::package_authoring::sandbox_perk::entity::residency::owner_requirements(
                        manager, placed.tags[2].0, &placed.payloads[2])
                        .map_err(invalid)?.into_iter().map(|requirement| TagHash(requirement.tag)));
                }
            }
            self.squads.extend(squads);
        }
        Ok(())
    }

    /// Adds `other`'s squads, so actors from every donor encounter object can stand in.
    pub(super) fn merge(&mut self, other: Self) {
        self.squads.extend(other.squads);
    }

    /// The donor encounter object `object` as templates, when it lists spawners whose rules name
    /// spawn points in its object list.
    pub(super) fn find(
        manager: &PackageManager,
        object: u32,
        payload: &[u8],
    ) -> AuthoringResult<Option<Self>> {
        if read_u64(payload, OBJECT_GROUPS)? == 0 {
            return Ok(None);
        }
        let (count, _, groups, _) = array_at(payload, OBJECT_GROUPS)?;
        let mut rules = BTreeMap::new();
        let mut spawners = Vec::new();
        let mut list = None;
        for group in 0..count {
            for handle in words(payload, groups + group * GROUP_SIZE + GROUP_HANDLES)? {
                if class(manager, handle) == Some(HANDLE_CLASS) && list.is_none() {
                    let handle_payload = read_tag(manager, TagHash(handle), "object handle")?;
                    let named = read_u32(&handle_payload, HANDLE_LIST)?;
                    if named != u32::MAX && class(manager, named) == Some(LIST_CLASS) {
                        let list_payload = read_tag(manager, TagHash(named), "object list")?;
                        list = Some((
                            (TagHash(handle), handle_payload),
                            (TagHash(named), list_payload),
                        ));
                        continue;
                    }
                }
                let Some(placed) = placed(manager, handle)? else {
                    continue;
                };
                let record = &placed.payloads[2];
                let Ok(descriptor) = keyed(record) else {
                    continue;
                };
                match read_u32(record, descriptor + DESCRIPTOR_CLASS)? {
                    SPAWNER_CLASS => spawners.push(placed),
                    RULE_CLASS => {
                        rules.insert(
                            read_u16_at(record, descriptor + DESCRIPTOR_SLOT_INDEX)?,
                            placed,
                        );
                    }
                    _ => {}
                }
            }
        }
        let Some((list_handle, list)) = list else {
            return Ok(None);
        };
        let rows = chain::table_rows(&list.1)?;
        let mut identities = BTreeMap::new();
        for &row in &rows {
            identities.insert(chain::row_identity(&list.1, row)?, row);
        }
        let mut squads = Vec::new();
        let mut first_row = None;
        for spawner in spawners {
            let record = &spawner.payloads[2];
            let secondary = keyed(record)?;
            let reference = read_u64(record, secondary + SPAWNER_RULE)?;
            if (reference >> 32) & 0xFF != u64::from(RULE_SLOT) {
                continue;
            }
            let Some(rule) = rules.get(&((reference >> 48) as u16)) else {
                continue;
            };
            let (Some(actor), Some(identity)) =
                (spawner_actor(record)?, rule_identity(&rule.payloads[2])?)
            else {
                continue;
            };
            let Some(&row) = identities.get(&identity) else {
                continue;
            };
            first_row.get_or_insert(row);
            squads.push(Template {
                actor,
                dependencies: dependencies(manager, &spawner)?,
                spawner: spawner.clone(),
                rule: rule.clone(),
                identity,
            });
        }
        let Some(row) = first_row else {
            return Ok(None);
        };
        Ok(Some(Self {
            object: (TagHash(object), payload.to_vec()),
            list_handle,
            list,
            row,
            squads,
        }))
    }
}

/// A stock squad may omit its spawn rule and carry inline placement. An authored
/// squad supplies its own point and rule, so that spawner can use an existing checked rule
/// template. Only the native null object-slot value takes this path.
fn unbound(
    manager: &PackageManager,
    payload: &[u8],
    rule: &Template,
) -> AuthoringResult<Vec<Template>> {
    let (count, _, groups, _) = array_at(payload, OBJECT_GROUPS)?;
    let mut result = Vec::new();
    for group in 0..count {
        for handle in words(payload, groups + group * GROUP_SIZE + GROUP_HANDLES)? {
            let Some(spawner) = placed(manager, handle)? else {
                continue;
            };
            let record = &spawner.payloads[2];
            let secondary = keyed(record)?;
            if read_u32(record, secondary + DESCRIPTOR_CLASS)? != SPAWNER_CLASS
                || read_u64(record, secondary + SPAWNER_RULE)? != NULL_SLOT
            {
                continue;
            }
            let Some(actor) = spawner_actor(record)? else {
                continue;
            };
            result.push(Template {
                actor,
                dependencies: dependencies(manager, &spawner)?,
                spawner,
                rule: rule.rule.clone(),
                identity: rule.identity,
            });
        }
    }
    Ok(result)
}

fn dependencies(manager: &PackageManager, spawner: &Placed) -> AuthoringResult<Vec<TagHash>> {
    // Supplemental spawners can name stock resources outside both the activity donor and
    // the actor graph. Their retained declarations still execute in the authored copy.
    let mut required =
        sundial::package_authoring::sandbox_perk::entity::residency::resource_additions(
            manager,
            [spawner.tags[2].0],
        )
        .map_err(|error| invalid(format!("Encounter spawner dependencies: {error}")))?;
    required.extend(
        point::dependencies(manager, spawner)?
            .into_iter()
            .map(|tag| tag.0),
    );
    for tag in spawner.tags {
        required.remove(&tag.0);
    }
    Ok(required.into_iter().map(TagHash).collect())
}

fn read_u16_at(payload: &[u8], at: usize) -> AuthoringResult<u16> {
    crate::tag_payload::read_u16(payload, at)
}

/// A slot name the client hashes: lowercase ASCII letters, digits and underscores.
fn slot_name(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

impl Encounter {
    pub(super) fn bindings(&self) -> super::mission::Encounter {
        super::mission::Encounter {
            key: self.key,
            template: self.templates.object.0,
            squads: self
                .squads
                .iter()
                .map(|squad| super::mission::Squad {
                    source: squad.source.clone(),
                    name: squad.name.clone(),
                    count: squad.count,
                })
                .collect(),
        }
    }

    /// The encounter of area `area` for the level's `squads` there, from `templates`, with the
    /// squads whose characters no template stands in for left out.
    pub(super) fn new(
        templates: &Templates,
        squads: &[Squad],
        area: &str,
    ) -> AuthoringResult<Option<Self>> {
        let table: Combatants = serde_json::from_str(COMBATANTS)
            .map_err(|error| invalid(format!("combatants.json: {error}")))?;
        if table.schema != 1 {
            return Err(invalid("combatants.json has an unknown schema"));
        }
        let mut natives = Vec::new();
        for squad in squads {
            let mut groups: Vec<(String, String, Vec<usize>)> = Vec::new();
            for (index, point) in squad.points.iter().enumerate() {
                let Some(character) = &point.character else {
                    continue;
                };
                match groups
                    .iter_mut()
                    .find(|(name, point_name, _)| name == character && point_name == &point.name)
                {
                    Some((_, _, indices)) => indices.push(index),
                    None => groups.push((character.clone(), point.name.clone(), vec![index])),
                }
            }
            let several = groups.len() > 1;
            for (character, point_name, indices) in groups {
                let Some(template) = actors(&table, &character)?.into_iter().find_map(|actor| {
                    templates
                        .squads
                        .iter()
                        .position(|template| template.actor == actor)
                }) else {
                    continue;
                };
                let source = if point_name.is_empty() {
                    squad.name.clone()
                } else {
                    format!("{}/{}", squad.name, point_name)
                };
                let base = slot_name(&source);
                natives.push(Native {
                    source,
                    name: if several {
                        format!("{base}_{}", slot_name(&character))
                    } else {
                        base
                    },
                    template,
                    anchor: squad.points[indices[0]].spawn,
                    count: indices.len(),
                });
            }
        }
        if natives.is_empty() {
            return Ok(None);
        }
        // Names must be distinct within the object.
        let mut seen = BTreeMap::<String, usize>::new();
        for native in &mut natives {
            let count = seen.entry(native.name.clone()).or_default();
            if *count > 0 {
                native.name = format!("{}_{}", native.name, count);
            }
            *count += 1;
        }
        Ok(Some(Self {
            templates: templates.clone(),
            squads: natives,
            key: fnv1_name_hash(&format!("{area}/encounter")),
        }))
    }

    /// The encounter's entries at the tags `at` gives from `next` on, the first being its object:
    /// the object, its list handle and list, then each squad's spawner and rule handle, redirect
    /// and record. Returns the specs and the object's tag.
    pub(super) fn tags(
        &self,
        at: &dyn Fn(usize) -> AuthoringResult<TagHash>,
        next: &mut usize,
        rename: &dyn Fn(&mut Vec<u8>),
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<TagHash>)> {
        let spec = |template: TagHash, payload: Vec<u8>| NewTagSpec {
            template_tag: template,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        let mut take = || -> AuthoringResult<TagHash> {
            let tag = at(*next)?;
            *next += 1;
            Ok(tag)
        };
        let object = take()?;
        let list_handle = take()?;
        let list = take()?;
        let mut specs = Vec::new();
        let mut added = vec![object, list_handle, list];
        let n = self.squads.len();
        let rule_index = |index: usize| n + index;

        // The spawn point list: one row per squad, copies of the template row.
        let mut list_payload = self.templates.list.1.clone();
        let rows = chain::rebuild_rows(&mut list_payload, &vec![self.templates.row; n])?;
        let identity = |index: usize| {
            u64::from(fnv1_name_hash(&format!("{:08X}/point/{index}", self.key))) << 32
                | u64::from(self.key)
        };
        for (index, (&row, squad)) in rows.iter().zip(&self.squads).enumerate() {
            chain::place_row(
                &mut list_payload,
                row,
                squad.anchor.yaw,
                squad.anchor.position,
            )?;
            chain::set_row_identity(&mut list_payload, row, identity(index))?;
        }
        let mut handle_payload = self.templates.list_handle.1.clone();
        write_u32(&mut handle_payload, HANDLE_LIST, list.0)?;

        let mut rule_handles = Vec::with_capacity(n);
        let mut spawner_handles = Vec::with_capacity(n);
        let mut slots = vec![(0u16, 0u32); 2 * n];
        for (index, squad) in self.squads.iter().enumerate() {
            let template = &self.templates.squads[squad.template];
            added.extend(template.dependencies.iter().copied());
            let spawner_tags = [take()?, take()?, take()?];
            let rule_tags = [take()?, take()?, take()?];
            added.extend(spawner_tags);
            added.extend(rule_tags);
            let rule_name = format!("{}_spawnrule", squad.name);
            let mut spawner = clone_placed(&template.spawner, spawner_tags[1], spawner_tags[2])?;
            let mut rule = clone_placed(&template.rule, rule_tags[1], rule_tags[2])?;
            let spawner_slot = index as u16;
            let rule_slot = rule_index(index) as u16;
            set_slot(&mut spawner[2], self.key, SQUAD_SLOT, spawner_slot)?;
            set_slot(&mut rule[2], self.key, RULE_SLOT, rule_slot)?;
            let secondary = keyed(&spawner[2])?;
            let reference =
                u64::from(self.key) | u64::from(RULE_SLOT) << 32 | u64::from(rule_slot) << 48;
            // Both bound references must reach the authored point. A retained donor reference
            // leaves the SDK's spawner-rule association incomplete even when +0xA0 resolves.
            let other = read_u64(&spawner[2], secondary + SPAWNER_OTHER_RULE)?;
            if other != NULL_SLOT {
                if (other >> 32) & 0xffff != u64::from(RULE_SLOT)
                    || other >> 48 == u64::from(u16::MAX)
                    || matches!(other as u32, 0 | u32::MAX)
                {
                    return Err(invalid(
                        "Encounter spawner has an unsupported spawn-rule reference",
                    ));
                }
                write_u64(&mut spawner[2], secondary + SPAWNER_OTHER_RULE, reference)?;
            }
            write_u64(&mut spawner[2], secondary + SPAWNER_RULE, reference)?;
            replace_u64(&mut rule[2], template.identity, identity(index))?;
            point::place(&mut spawner[2], squad.anchor, identity(index))?;
            spawner[2] = renamed(&spawner[2], &squad.name)?;
            rule[2] = renamed(&rule[2], &rule_name)?;
            slots[spawner_slot as usize] = (SQUAD_SLOT, fnv1_name_hash(&squad.name));
            slots[rule_slot as usize] = (RULE_SLOT, fnv1_name_hash(&rule_name));
            for (k, payload) in spawner.into_iter().enumerate() {
                specs.push(spec(template.spawner.tags[k], payload));
            }
            for (k, payload) in rule.into_iter().enumerate() {
                specs.push(spec(template.rule.tags[k], payload));
            }
            spawner_handles.push(spawner_tags[0]);
            rule_handles.push(rule_tags[0]);
        }

        // The object: its key, slot table and handle groups, the first group for every bubble with
        // the list and the rules, the second for its bubble with the spawners.
        let mut payload = self.templates.object.1.clone();
        rename(&mut payload);
        write_u32(&mut payload, OBJECT_KEY, self.key)?;
        let n16 = u16::try_from(n).map_err(|_| invalid("An encounter has too many squads"))?;
        write_u16(&mut payload, OBJECT_SLOT_COUNTS, n16)?;
        write_u16(&mut payload, OBJECT_SLOT_COUNTS + 2, n16)?;
        let slot_rows = slots
            .iter()
            .flat_map(|&(kind, hash)| {
                let mut row = [0u8; 8];
                row[0..2].copy_from_slice(&kind.to_le_bytes());
                row[4..8].copy_from_slice(&hash.to_le_bytes());
                row
            })
            .collect::<Vec<_>>();
        append_native_array(
            &mut payload,
            OBJECT_SLOTS,
            SLOT_CLASS,
            slots.len(),
            &slot_rows,
        )?;
        let (count, _, groups, _) = array_at(&payload, OBJECT_GROUPS)?;
        if count != 2 {
            return Err(invalid(
                "The template encounter object has other than two handle groups",
            ));
        }
        let mut lists = [Vec::new(), Vec::new()];
        for (group, list) in lists.iter_mut().enumerate() {
            let bubble = read_u32(&payload, groups + group * GROUP_SIZE + GROUP_BUBBLE)? as i32;
            *list = if bubble < 0 {
                std::iter::once(list_handle)
                    .chain(rule_handles.iter().copied())
                    .collect()
            } else {
                spawner_handles.clone()
            };
        }
        for (group, handles) in lists.iter().enumerate() {
            let rows = handles
                .iter()
                .flat_map(|tag| tag.0.to_le_bytes())
                .collect::<Vec<_>>();
            let field = array_at(&payload, OBJECT_GROUPS)?.2 + group * GROUP_SIZE + GROUP_HANDLES;
            append_native_array(&mut payload, field, HANDLE_ROW_CLASS, handles.len(), &rows)?;
        }
        let payload = synchronize_payload_size(payload)?;
        let mut tags = vec![
            spec(self.templates.object.0, payload),
            spec(self.templates.list_handle.0, handle_payload),
            spec(self.templates.list.0, list_payload),
        ];
        tags.extend(specs);
        Ok((tags, object, added))
    }
}

/// Replaces every 8-aligned occurrence of `from` in `record` with `to`.
fn replace_u64(record: &mut [u8], from: u64, to: u64) -> AuthoringResult<()> {
    let mut replaced = false;
    for at in (0..record.len().saturating_sub(7)).step_by(4) {
        if read_u64(record, at)? == from {
            write_u64(record, at, to)?;
            replaced = true;
        }
    }
    if replaced {
        Ok(())
    } else {
        Err(invalid("A spawn rule does not name its point"))
    }
}
