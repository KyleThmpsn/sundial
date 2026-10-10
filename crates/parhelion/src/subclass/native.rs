//! A subclass's native records, as the game reads them.
//!
//! - **Item.** A subclass definition's talent-grid holder names its socket-entry list by row.
//! - **List.** Root slot 97 rows name a socket-entry list: 24 entries of 64 bytes, each with a
//!   display hash, plug source, group, kind and the pool (+56) that grants its ability.
//! - **Display record.** Globals slot 61 rows, under the same row hash, name a display record:
//!   node rows (display hash, grid position, node record) and attunement rows (plug source and
//!   the lore row that names the path).
//! - **Pool.** Groups of variants, each variant with its records, its sandbox perks (+0x20) and
//!   the flags it sets. Every stock subclass pool has one group with one variant.
//! - **Node record.** Steps of display rows, each a name, a description and an icon row.
//!
//! Lists and display records are type-16 records, each followed by a shared-tag companion that
//! lists what loads with it.
use std::collections::{BTreeMap, BTreeSet};

use sundial::package_authoring::{
    PackageManager,
    investment_schema::{
        ITEM_DEFINITION_HASH_OFFSET, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET,
        ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE, ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET,
    },
};
use tiger_pkg::TagHash;

use crate::AuthoringResult;
use crate::error::{invalid, validation};
use crate::package_profile::LOCALIZATION_DONOR_TABLE_INDEX;
use crate::shared_tag_memory::{
    SharedTagDependencies, adjacent_companion_tag, validate_shared_tag_companion_payload,
};
use crate::tag_payload::{
    append_native_array, array_at, read_i64, read_tag, read_u8, read_u16, read_u32, read_u64,
    relative_target, synchronize_payload_size, write_bytes, write_localized_reference, write_u16,
    write_u32,
};

/// A subclass definition names itself at the native hash field and again at the head of its
/// inventory block.
pub(crate) const IDENTITY_OFFSETS: [usize; 2] = [
    ITEM_DEFINITION_HASH_OFFSET,
    crate::item::ITEM_STACK_LABEL_OFFSET,
];

/// Removes only the supported donor-class equip condition. The ability grid still keeps its
/// class-base pool. Keep the empty array's native header and all other equipment fields.
pub(crate) fn allow_every_class(definition: &mut [u8]) -> AuthoringResult<()> {
    use sundial::package_authoring::investment_schema::{
        ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET, subclass_equipment_class,
    };
    if subclass_equipment_class(definition)
        .map_err(invalid)?
        .is_none()
    {
        return Ok(());
    }
    let equipment = relative_target(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    let (_, header, _, _) = array_at(definition, equipment)?;
    crate::tag_payload::set_array_count(definition, equipment, header, 0)
}

/// Points only the supported donor-class equip condition at `class`: 0 Titan, 1 Hunter,
/// 2 Warlock. The ability grid still keeps its class-base pool.
pub(crate) fn set_class(definition: &mut [u8], class: u8) -> AuthoringResult<()> {
    sundial::package_authoring::investment_schema::set_subclass_equipment_class(definition, class)
        .map_err(invalid)
}

/// Gives a subclass's item strings `damage`, whose icon it shows beside its name. Its strings
/// hold the type in the resource their stat group pointer names, as the native damage enum.
pub(crate) fn set_damage_type(
    strings: &mut [u8],
    damage: crate::recipe::RecipeDamageType,
) -> AuthoringResult<()> {
    use crate::recipe::RecipeDamageType;
    use sundial::package_authoring::ability_damage::{ARC, KINETIC, SOLAR, VOID};
    use sundial::package_authoring::investment_schema::{
        ITEM_STRING_DAMAGE_TYPE_OFFSET, ITEM_STRING_STAT_GROUP_POINTER_OFFSET,
        ITEM_STRING_STAT_GROUP_RESOURCE_CLASS,
    };
    let resource = relative_target(strings, ITEM_STRING_STAT_GROUP_POINTER_OFFSET)?;
    if resource < 4 || read_u32(strings, resource - 4)? != ITEM_STRING_STAT_GROUP_RESOURCE_CLASS {
        return Err(invalid("The subclass's strings hold no damage type"));
    }
    let field = resource + ITEM_STRING_DAMAGE_TYPE_OFFSET;
    if read_u32(strings, field)? > u32::from(VOID) {
        return Err(invalid(
            "The subclass's strings hold an unknown damage type",
        ));
    }
    let code = match damage {
        RecipeDamageType::Kinetic => KINETIC,
        RecipeDamageType::Solar => SOLAR,
        RecipeDamageType::Arc => ARC,
        RecipeDamageType::Void => VOID,
    };
    write_u32(strings, field, u32::from(code))
}

const TALENT_GRID_HOLDER_CLASS: u32 = 0x8080_77B7;
pub(super) const SOCKET_ENTRY_LIST_TABLE_CLASS: u32 = 0x8080_7A78;
pub(super) const SOCKET_ENTRY_LIST_ROW_CLASS: u32 = 0x8080_7A7E;
pub(super) const SOCKET_ENTRY_LIST_CLASS: u32 = 0x8080_7A80;
const SOCKET_ENTRY_CLASS: u32 = 0x8080_7A86;
const SOCKET_ENTRY_ARRAY: usize = 0x10;
const SOCKET_ENTRY_SIZE: usize = 64;
const ENTRY_DISPLAY_HASH: usize = 0;
const ENTRY_PLUG_SOURCE: usize = 8;
const ENTRY_GROUP: usize = 12;
const ENTRY_KIND: usize = 13;
const ENTRY_POOL: usize = 56;
/// The entry kind of the super lane, which is what makes a list a subclass's.
pub(super) const SUPER_ENTRY_KIND: u8 = 34;
pub(super) const TALENT_DISPLAY_TABLE_CLASS: u32 = 0x8080_5C3C;
pub(super) const TALENT_DISPLAY_ROW_CLASS: u32 = 0x8080_5C40;
pub(super) const TALENT_DISPLAY_CLASS: u32 = 0x8080_5C42;
/// Display rows: an entry's display hash, its grid position and its node record.
const DISPLAY_NODE_ARRAY: usize = 0x08;
const DISPLAY_NODE_ROW_CLASS: u32 = 0x8080_5C46;
const DISPLAY_NODE_ROW_SIZE: usize = 24;
const DISPLAY_NODE_TAG: usize = 16;
pub(super) const DISPLAY_NODE_CLASS: u32 = 0x8080_5C49;
/// Attunement rows: an attunement's plug source and the lore row the client shows it by.
const DISPLAY_PATH_ARRAY: usize = 0x30;
const DISPLAY_PATH_ROW_CLASS: u32 = 0x8080_5C45;
const DISPLAY_PATH_ROW_SIZE: usize = 8;
pub(super) const POOL_CLASS: u32 = 0x8080_7A8B;
const POOL_GROUP_ARRAY: usize = 0x08;
const POOL_GROUP_CLASS: u32 = 0x8080_7A8D;
const POOL_GROUP_SIZE: usize = 144;
const POOL_VARIANT_CLASS: u32 = 0x8080_7A97;
const POOL_VARIANT_SIZE: usize = 88;
const VARIANT_PERK_ARRAY: usize = 0x20;
const VARIANT_PERK_CLASS: u32 = 0x8080_7C5F;
/// A variant's records sit behind its first descriptor. A record's +0 hash is a key in the ability
/// bank's property rows. The ability itself is a row of the ability definition table: +0xB the row
/// the record equips, +4 the row its key applies to, 0xFF for none.
const POOL_RECORD_SIZE: usize = 16;
/// Every stock pool's record arrays carry this class, 168 of 168.
const POOL_RECORD_CLASS: u32 = 0x8080_7AA2;
const RECORD_KEY: usize = 0x00;
const RECORD_KEY_ROW: usize = 0x04;
const RECORD_EQUIPPED_ROW: usize = 0x0B;
const NO_ROW: u8 = 0xFF;
/// The hash a record holds when it files no key, FNV-1's offset basis: the empty name.
const NO_KEY: u32 = 0x811C_9DC5;
/// A node record's steps, each with display rows that start with a name and a description and
/// hold the node's row in the item icon table.
const NODE_STEP_ARRAY: usize = 0x08;
const NODE_STEP_CLASS: u32 = 0x8080_2B16;
const NODE_STEP_SIZE: usize = 0x20;
const NODE_DISPLAY_CLASS: u32 = 0x8080_5C4B;
const NODE_DISPLAY_SIZE: usize = 0x20;
const NODE_NAME: usize = 0x00;
const NODE_DESCRIPTION: usize = 0x08;
const NODE_ICON: usize = 0x18;

/// A type-16 record's shared-tag companion, which lists what loads with the record. For an
/// authored record, the dependencies leave out the record and companion, whose tags the build
/// assigns.
#[derive(Clone)]
pub(crate) struct Companion {
    pub(crate) tag: TagHash,
    pub(crate) payload: Vec<u8>,
    pub(crate) dependencies: SharedTagDependencies,
}

impl Companion {
    pub(super) fn read(manager: &PackageManager, owner: TagHash) -> AuthoringResult<Self> {
        let tag = adjacent_companion_tag(owner)?;
        if manager
            .get_entry(tag)
            .is_none_or(|entry| entry.reference != crate::format::SHARED_TAG_COMPANION_CLASS)
        {
            return Err(invalid(format!(
                "Subclass record {owner} has no shared-tag companion"
            )));
        }
        let payload = read_tag(manager, tag, "subclass shared-tag companion")?;
        let dependencies = validate_shared_tag_companion_payload(&payload, tag, owner)?;
        Ok(Self {
            tag,
            payload,
            dependencies,
        })
    }

    /// The same dependencies for a copy of `owner`, less the stock record and its companion.
    pub(super) fn for_copy(&self, owner: TagHash, added: impl IntoIterator<Item = u32>) -> Self {
        let mut dependencies = self.dependencies.clone();
        dependencies.remove(&u32::from(owner));
        dependencies.remove(&u32::from(self.tag));
        dependencies.extend(added);
        Self {
            tag: self.tag,
            payload: self.payload.clone(),
            dependencies,
        }
    }

    /// The companion payload for an authored record at `owner`, with this companion at `tag`,
    /// loading `added` with it too.
    pub(crate) fn payload_for(
        &self,
        owner: TagHash,
        tag: TagHash,
        added: impl IntoIterator<Item = u32>,
    ) -> AuthoringResult<Vec<u8>> {
        let mut dependencies = self.dependencies.clone();
        dependencies.insert(u32::from(owner));
        dependencies.insert(u32::from(tag));
        dependencies.extend(added);
        crate::shared_tag_memory::build_shared_tag_companion_payload(
            &self.payload,
            tag,
            owner,
            &dependencies,
        )
    }
}

/// One socket entry: what its position fixes, and its pool.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) display_hash: u32,
    pub(crate) plug_source: u32,
    pub(crate) group: u8,
    pub(crate) kind: u8,
    pub(crate) pool: u32,
}

impl Entry {
    /// What the entry's position fixes in every stock list.
    pub(super) const fn position(self) -> (u32, u32, u8, u8) {
        (self.display_hash, self.plug_source, self.group, self.kind)
    }
}

pub(crate) fn entries(list: &[u8]) -> AuthoringResult<Vec<Entry>> {
    let (count, _, rows, class) = array_at(list, SOCKET_ENTRY_ARRAY)?;
    if class != SOCKET_ENTRY_CLASS {
        return Err(invalid(format!(
            "Subclass socket entries have class 0x{class:08X}"
        )));
    }
    (0..count)
        .map(|index| {
            let row = rows + index * SOCKET_ENTRY_SIZE;
            Ok(Entry {
                display_hash: read_u32(list, row + ENTRY_DISPLAY_HASH)?,
                plug_source: read_u32(list, row + ENTRY_PLUG_SOURCE)?,
                group: read_u8(list, row + ENTRY_GROUP)?,
                kind: read_u8(list, row + ENTRY_KIND)?,
                pool: read_u32(list, row + ENTRY_POOL)?,
            })
        })
        .collect()
}

/// Points `entry` of the list at `pool`.
pub(super) fn set_entry_pool(list: &mut [u8], entry: u8, pool: u32) -> AuthoringResult<()> {
    let (_, _, rows, _) = array_at(list, SOCKET_ENTRY_ARRAY)?;
    write_u32(
        list,
        rows + usize::from(entry) * SOCKET_ENTRY_SIZE + ENTRY_POOL,
        pool,
    )
}

/// The socket-entry-list row a subclass definition's talent-grid holder names.
pub(crate) fn list_index(definition: &[u8]) -> AuthoringResult<u16> {
    let block = holder(definition)?;
    read_u16(definition, block + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET)
}

fn holder(definition: &[u8]) -> AuthoringResult<usize> {
    if read_i64(definition, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)? == 0 {
        return Err(invalid("The base subclass has no talent-grid holder"));
    }
    let block = relative_target(definition, ITEM_SOCKET_ENTRY_LIST_BLOCK_POINTER_OFFSET)?;
    if block < 4
        || block + ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE > definition.len()
        || read_u32(definition, block - 4)? != TALENT_GRID_HOLDER_CLASS
    {
        return Err(invalid(
            "The base subclass's talent-grid holder is not the native one",
        ));
    }
    Ok(block)
}

/// Points an authored subclass at its own socket-entry list, changing nothing else.
pub(crate) fn set_list_index(definition: &mut [u8], index: u16) -> AuthoringResult<()> {
    let field = holder(definition)? + ITEM_SOCKET_ENTRY_LIST_INDEX_OFFSET;
    let before = definition.to_vec();
    write_u16(definition, field, index)?;
    let mut normalized = definition.to_vec();
    write_bytes(&mut normalized, field, &before[field..field + 2])?;
    if list_index(definition)? != index || normalized != before {
        return Err(validation(
            "Subclass list authoring changed bytes outside the talent-grid holder",
        ));
    }
    Ok(())
}

/// The pool of a list's class-base entry, the base melee each attunement links to. It differs
/// by class, and an authored list keeps its base's, so it names the class of a subclass whose
/// item strings do not.
pub(crate) fn class_base_pool(list: &[u8]) -> AuthoringResult<u32> {
    entries(list)?
        .get(usize::from(super::layout::CLASS_BASE))
        .map(|entry| entry.pool)
        .ok_or_else(|| invalid("The subclass list has no class-base entry"))
}

fn node_row(display: &[u8], display_hash: u32) -> AuthoringResult<usize> {
    let (count, _, rows, class) = array_at(display, DISPLAY_NODE_ARRAY)?;
    if class != DISPLAY_NODE_ROW_CLASS {
        return Err(invalid(format!(
            "Subclass display rows have class 0x{class:08X}"
        )));
    }
    (0..count)
        .map(|index| rows + index * DISPLAY_NODE_ROW_SIZE)
        .find(|&row| read_u32(display, row).ok() == Some(display_hash))
        .ok_or_else(|| {
            invalid(format!(
                "The subclass display record has no row for 0x{display_hash:08X}"
            ))
        })
}

/// The node record a display record shows for the entry with `display_hash`.
pub(super) fn node_tag(display: &[u8], display_hash: u32) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(
        display,
        node_row(display, display_hash)? + DISPLAY_NODE_TAG,
    )?))
}

/// Shows `record` for the entry with `display_hash`.
pub(super) fn set_node_tag(
    display: &mut [u8],
    display_hash: u32,
    record: TagHash,
) -> AuthoringResult<()> {
    let row = node_row(display, display_hash)?;
    write_u32(display, row + DISPLAY_NODE_TAG, record.0)
}

/// Every node record a display record shows, in row order.
pub(super) fn node_tags(display: &[u8]) -> AuthoringResult<Vec<TagHash>> {
    let (count, _, rows, _) = array_at(display, DISPLAY_NODE_ARRAY)?;
    (0..count)
        .map(|index| {
            read_u32(
                display,
                rows + index * DISPLAY_NODE_ROW_SIZE + DISPLAY_NODE_TAG,
            )
            .map(TagHash)
        })
        .collect()
}

fn path_row(display: &[u8], plug_source: u32) -> AuthoringResult<usize> {
    let (count, _, rows, class) = array_at(display, DISPLAY_PATH_ARRAY)?;
    if class != DISPLAY_PATH_ROW_CLASS {
        return Err(invalid(format!(
            "Subclass attunement rows have class 0x{class:08X}"
        )));
    }
    (0..count)
        .map(|index| rows + index * DISPLAY_PATH_ROW_SIZE)
        .find(|&row| read_u32(display, row).ok() == Some(plug_source))
        .ok_or_else(|| {
            invalid(format!(
                "The subclass display record has no attunement 0x{plug_source:08X}"
            ))
        })
}

/// The lore row that names the attunement led by `plug_source`.
pub(super) fn path_value(display: &[u8], plug_source: u32) -> AuthoringResult<u32> {
    read_u32(display, path_row(display, plug_source)? + 4)
}

pub(crate) fn set_path_value(
    display: &mut [u8],
    plug_source: u32,
    value: u32,
) -> AuthoringResult<()> {
    let row = path_row(display, plug_source)?;
    write_u32(display, row + 4, value)
}

/// The pool with each variant's sandbox perks edited: `removed` taken out and `added` put after
/// the rest. Every perk in `removed` must be one of the pool's.
pub(crate) fn edit_pool_perks(
    pool: &[u8],
    added: &[u16],
    removed: &[u16],
) -> AuthoringResult<Vec<u8>> {
    let mut payload = pool.to_vec();
    if added.is_empty() && removed.is_empty() {
        return Ok(payload);
    }
    let mut found = BTreeSet::new();
    for descriptor in perk_descriptors(pool)? {
        let perks = pool_perks(pool, descriptor)?;
        found.extend(perks.iter().copied().filter(|perk| removed.contains(perk)));
        let mut edited = perks
            .iter()
            .copied()
            .filter(|perk| !removed.contains(perk))
            .collect::<Vec<_>>();
        edited.extend(added.iter().copied().filter(|perk| !perks.contains(perk)));
        if edited != perks {
            let rows = edited
                .iter()
                .flat_map(|perk| perk.to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                descriptor,
                VARIANT_PERK_CLASS,
                edited.len(),
                &rows,
            )?;
        }
    }
    if let Some(missing) = removed.iter().find(|perk| !found.contains(*perk)) {
        return Err(invalid(format!(
            "Sandbox perk {missing} is not one of the node's perks"
        )));
    }
    synchronize_payload_size(payload)
}

/// The sandbox perks a pool grants across its variants, each once.
pub(super) fn granted_perks(pool: &[u8]) -> AuthoringResult<Vec<u16>> {
    let mut granted = Vec::new();
    for descriptor in perk_descriptors(pool)? {
        for perk in pool_perks(pool, descriptor)? {
            if !granted.contains(&perk) {
                granted.push(perk);
            }
        }
    }
    Ok(granted)
}

/// The pool with each `(from, to)` perk replaced where it stands, in every variant that grants
/// it. Every `from` must be one of the pool's perks.
pub(super) fn replace_pool_perks(
    pool: &[u8],
    replacements: &[(u16, u16)],
) -> AuthoringResult<Vec<u8>> {
    let mut payload = pool.to_vec();
    let mut found = BTreeSet::new();
    for descriptor in perk_descriptors(pool)? {
        let perks = pool_perks(pool, descriptor)?;
        if perks.is_empty() {
            continue;
        }
        let (_, _, rows, _) = array_at(pool, descriptor)?;
        for (index, perk) in perks.into_iter().enumerate() {
            if let Some(&(from, to)) = replacements.iter().find(|(from, _)| *from == perk) {
                write_u16(&mut payload, rows + index * 2, to)?;
                found.insert(from);
            }
        }
    }
    if let Some((missing, _)) = replacements.iter().find(|(from, _)| !found.contains(from)) {
        return Err(invalid(format!(
            "Sandbox perk {missing} is not one of the node's perks"
        )));
    }
    Ok(payload)
}

/// Every pool variant: the offset of each 88-byte variant, whose record array descriptor sits
/// at its start.
fn variants(pool: &[u8]) -> AuthoringResult<Vec<usize>> {
    let (groups, _, group_rows, group_class) = array_at(pool, POOL_GROUP_ARRAY)?;
    if group_class != POOL_GROUP_CLASS {
        return Err(invalid(format!(
            "Subclass pool groups have class 0x{group_class:08X}"
        )));
    }
    let mut variants = Vec::new();
    for group in (0..groups).map(|index| group_rows + index * POOL_GROUP_SIZE) {
        if read_u64(pool, group)? == 0 {
            continue;
        }
        let (count, _, variant_rows, variant_class) = array_at(pool, group)?;
        if variant_class != POOL_VARIANT_CLASS {
            return Err(invalid(format!(
                "Subclass pool variants have class 0x{variant_class:08X}"
            )));
        }
        variants.extend((0..count).map(|index| variant_rows + index * POOL_VARIANT_SIZE));
    }
    Ok(variants)
}

/// Every pool variant's perk array descriptor.
fn perk_descriptors(pool: &[u8]) -> AuthoringResult<Vec<usize>> {
    Ok(variants(pool)?
        .into_iter()
        .map(|variant| variant + VARIANT_PERK_ARRAY)
        .collect())
}

/// Every pool variant's records: the offset of each 16-byte record.
fn pool_records(pool: &[u8]) -> AuthoringResult<Vec<usize>> {
    let mut records = Vec::new();
    for variant in variants(pool)? {
        if read_u64(pool, variant)? == 0 {
            continue;
        }
        let (count, _, rows, _) = array_at(pool, variant)?;
        records.extend((0..count).map(|index| rows + index * POOL_RECORD_SIZE));
    }
    Ok(records)
}

/// One variant's records, whole.
fn variant_records(pool: &[u8], variant: usize) -> AuthoringResult<Vec<[u8; POOL_RECORD_SIZE]>> {
    if read_u64(pool, variant)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(pool, variant)?;
    if class != POOL_RECORD_CLASS {
        return Err(invalid(format!(
            "Subclass pool records have class 0x{class:08X}"
        )));
    }
    (0..count)
        .map(|index| {
            let at = rows + index * POOL_RECORD_SIZE;
            pool.get(at..at + POOL_RECORD_SIZE)
                .and_then(|record| record.try_into().ok())
                .ok_or_else(|| invalid("A subclass pool record runs past its pool"))
        })
        .collect()
}

/// A record's key and the row it applies the key to, when it is a modifier: it applies a key
/// and equips nothing. An ability's own record, which equips its row, is never one.
fn record_modifier(record: &[u8; POOL_RECORD_SIZE]) -> Option<(u32, u8)> {
    let key = u32::from_le_bytes(record[RECORD_KEY..RECORD_KEY + 4].try_into().ok()?);
    let row = record[RECORD_KEY_ROW];
    (record[RECORD_EQUIPPED_ROW] == NO_ROW
        && row != NO_ROW
        && !matches!(key, 0 | u32::MAX | NO_KEY))
    .then_some((key, row))
}

/// A modifier record, laid out as the stock ones are: the key, the row, then no links, kind or
/// destination.
fn modifier_record((key, row): (u32, u8)) -> [u8; POOL_RECORD_SIZE] {
    let mut record = [0; POOL_RECORD_SIZE];
    record[RECORD_KEY..RECORD_KEY + 4].copy_from_slice(&key.to_le_bytes());
    record[RECORD_KEY_ROW] = row;
    record[0x08..=0x0C].fill(NO_ROW);
    record
}

/// The pool with each variant's records rewritten by `edit`, which sees them whole.
fn edit_records(
    pool: &[u8],
    mut edit: impl FnMut(&[[u8; POOL_RECORD_SIZE]]) -> AuthoringResult<Vec<[u8; POOL_RECORD_SIZE]>>,
) -> AuthoringResult<Vec<u8>> {
    let mut payload = pool.to_vec();
    let mut edited = false;
    for variant in variants(pool)? {
        let records = variant_records(pool, variant)?;
        let next = edit(&records)?;
        if next != records {
            append_native_array(
                &mut payload,
                variant,
                POOL_RECORD_CLASS,
                next.len(),
                &next.concat(),
            )?;
            edited = true;
        }
    }
    if edited {
        synchronize_payload_size(payload)
    } else {
        Ok(payload)
    }
}

/// The keys a pool's modifier records apply and the rows they apply them to, each once.
pub(crate) fn modifiers(pool: &[u8]) -> AuthoringResult<Vec<(u32, u8)>> {
    let mut modifiers = Vec::new();
    for variant in variants(pool)? {
        for record in variant_records(pool, variant)? {
            if let Some(modifier) = record_modifier(&record)
                && !modifiers.contains(&modifier)
            {
                modifiers.push(modifier);
            }
        }
    }
    Ok(modifiers)
}

/// The most modifier records of one variant that apply their keys to each row, which is how
/// many keys the pool files into that row's bucket at once.
pub(super) fn keys_per_row(pool: &[u8]) -> AuthoringResult<BTreeMap<u8, usize>> {
    let mut most = BTreeMap::<u8, usize>::new();
    for variant in variants(pool)? {
        let mut here = BTreeMap::<u8, usize>::new();
        for record in variant_records(pool, variant)? {
            if let Some((_, row)) = record_modifier(&record) {
                *here.entry(row).or_default() += 1;
            }
        }
        for (row, count) in here {
            let entry = most.entry(row).or_default();
            *entry = (*entry).max(count);
        }
    }
    Ok(most)
}

/// The pool with the modifier records `removed` names taken out of every variant, then one for
/// each of `added` after the rest. Every removed one must be one of the pool's.
pub(super) fn edit_pool_modifiers(
    pool: &[u8],
    removed: &[(u32, u8)],
    added: &[(u32, u8)],
) -> AuthoringResult<Vec<u8>> {
    if removed.is_empty() && added.is_empty() {
        return Ok(pool.to_vec());
    }
    let mut found = BTreeSet::new();
    let payload = edit_records(pool, |records| {
        let mut next = Vec::with_capacity(records.len() + added.len());
        for record in records {
            match record_modifier(record) {
                Some(modifier) if removed.contains(&modifier) => {
                    found.insert(modifier);
                }
                _ => next.push(*record),
            }
        }
        for record in added.iter().map(|modifier| modifier_record(*modifier)) {
            if !next.contains(&record) {
                next.push(record);
            }
        }
        Ok(next)
    })?;
    if let Some((key, row)) = removed.iter().find(|modifier| !found.contains(*modifier)) {
        return Err(invalid(format!(
            "The node applies no key 0x{key:08X} to ability row {row}"
        )));
    }
    Ok(payload)
}

/// The pool with, in each variant, a record for row `to` after every modifier record that
/// applies its key to row `from`, so an ability copied from row `from` takes the same keys
/// while the stock one keeps them.
pub(super) fn duplicate_row(pool: &[u8], from: u8, to: u8) -> AuthoringResult<Vec<u8>> {
    edit_records(pool, |records| {
        let mut next = records.to_vec();
        for record in records {
            if let Some((key, row)) = record_modifier(record)
                && row == from
            {
                let copy = modifier_record((key, to));
                if !next.contains(&copy) {
                    next.push(copy);
                }
            }
        }
        Ok(next)
    })
}

/// The ability rows a pool's records equip, in order, each once.
pub(super) fn equipped_rows(pool: &[u8]) -> AuthoringResult<Vec<u8>> {
    let mut rows = Vec::new();
    for record in pool_records(pool)? {
        let row = read_u8(pool, record + RECORD_EQUIPPED_ROW)?;
        if row != NO_ROW && !rows.contains(&row) {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Whether any of a pool's modifier records applies its key to ability row `row`.
pub(super) fn applies_to_row(pool: &[u8], row: u8) -> AuthoringResult<bool> {
    Ok(modifiers(pool)?.iter().any(|(_, applied)| *applied == row))
}

/// The pool with ability row `from` replaced by `to` wherever a record applies its key to it,
/// and, when `equipped`, wherever a record equips it. Never the +0 key.
pub(super) fn move_row(pool: &[u8], from: u8, to: u8, equipped: bool) -> AuthoringResult<Vec<u8>> {
    let mut payload = pool.to_vec();
    let mut moved = false;
    for record in pool_records(pool)? {
        for (field, applies) in [(RECORD_KEY_ROW, true), (RECORD_EQUIPPED_ROW, equipped)] {
            if applies && read_u8(pool, record + field)? == from {
                write_bytes(&mut payload, record + field, &[to])?;
                moved = true;
            }
        }
    }
    if equipped && !moved {
        return Err(validation(format!(
            "The subclass pool names no ability row {from}"
        )));
    }
    Ok(payload)
}

/// One pool variant's sandbox perks.
fn pool_perks(pool: &[u8], descriptor: usize) -> AuthoringResult<Vec<u16>> {
    if read_u64(pool, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = array_at(pool, descriptor)?;
    if class != VARIANT_PERK_CLASS {
        return Err(invalid(format!(
            "Subclass pool perks have class 0x{class:08X}"
        )));
    }
    (0..count)
        .map(|index| read_u16(pool, rows + index * 2))
        .collect()
}

/// Every display row of a node record, across its steps.
fn node_displays(record: &[u8]) -> AuthoringResult<Vec<usize>> {
    let (steps, _, step_rows, step_class) = array_at(record, NODE_STEP_ARRAY)?;
    if step_class != NODE_STEP_CLASS {
        return Err(invalid(format!(
            "Subclass node steps have class 0x{step_class:08X}"
        )));
    }
    let mut rows = Vec::new();
    for step in (0..steps).map(|index| step_rows + index * NODE_STEP_SIZE) {
        let (displays, _, display_rows, display_class) = array_at(record, step)?;
        if display_class != NODE_DISPLAY_CLASS {
            return Err(invalid(format!(
                "Subclass node display rows have class 0x{display_class:08X}"
            )));
        }
        rows.extend((0..displays).map(|index| display_rows + index * NODE_DISPLAY_SIZE));
    }
    Ok(rows)
}

/// The node record with each display row's name and description pointing at authored text.
pub(super) fn set_node_text(
    record: &[u8],
    name: Option<u32>,
    description: Option<u32>,
) -> AuthoringResult<Vec<u8>> {
    let mut payload = record.to_vec();
    for display in node_displays(record)? {
        for (field, hash) in [(NODE_NAME, name), (NODE_DESCRIPTION, description)] {
            if let Some(hash) = hash {
                write_localized_reference(
                    &mut payload,
                    display + field,
                    LOCALIZATION_DONOR_TABLE_INDEX as u32,
                    hash,
                )?;
            }
        }
    }
    Ok(payload)
}

/// The row in the item icon table a node record shows, from its first display row.
pub(super) fn node_icon(record: &[u8]) -> AuthoringResult<u16> {
    let display = *node_displays(record)?
        .first()
        .ok_or_else(|| invalid("The subclass node record has no display row"))?;
    read_u16(record, display + NODE_ICON)
}

/// The node record with every display row showing icon row `icon`.
pub(crate) fn set_node_icon(record: &[u8], icon: u16) -> AuthoringResult<Vec<u8>> {
    let mut payload = record.to_vec();
    for display in node_displays(record)? {
        write_u16(&mut payload, display + NODE_ICON, icon)?;
    }
    Ok(payload)
}
