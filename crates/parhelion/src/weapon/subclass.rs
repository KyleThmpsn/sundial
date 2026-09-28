//! Subclass authoring.
//!
//! A subclass recipe changes identity, text and icon the way other gear does. It has no
//! Collections entry, as no stock subclass has one. Its abilities and attunements may come from
//! other stock subclasses, which gives it a socket-entry list of its own: its base's list with
//! those entries' pools swapped in, and a display record with their names and icons swapped in.
//! Every stock list shares one layout, so an entry's position fixes its display hash, plug
//! source, group and prerequisites, and a swapped entry keeps all of them.
//!
//! Lists and display records are type-16 records, each followed by a shared-tag companion that
//! lists what loads with it: the pools, and the node records. An authored one's companion keeps
//! its base's dependencies and adds the pools and node records it takes from other subclasses.
use super::donors::DonorItem;
use super::resolve::ResolvedWeapon;
use super::sources::ProjectSources;
use super::*;
use crate::shared_tag_memory::{
    SharedTagDependencies, adjacent_companion_tag, validate_shared_tag_companion_payload,
};
use crate::subclass::{AbilitySlot, AttunementPath, SubclassAbilities, SubclassPathNode, layout};
use sundial::package_authoring::investment_schema::ITEM_SOCKET_ENTRY_LIST_BLOCK_SIZE;

/// A subclass definition names itself at the native hash field and again at the head of its
/// inventory block.
pub(super) const IDENTITY_OFFSETS: [usize; 2] = [ITEM_DEFINITION_HASH_OFFSET, 0xB0];

const TALENT_GRID_HOLDER_CLASS: u32 = 0x8080_77B7;
const SOCKET_ENTRY_LIST_TABLE_CLASS: u32 = 0x8080_7A78;
const SOCKET_ENTRY_LIST_ROW_CLASS: u32 = 0x8080_7A7E;
const SOCKET_ENTRY_LIST_CLASS: u32 = 0x8080_7A80;
const SOCKET_ENTRY_CLASS: u32 = 0x8080_7A86;
const SOCKET_ENTRY_ARRAY: usize = 0x10;
const SOCKET_ENTRY_SIZE: usize = 64;
const ENTRY_DISPLAY_HASH: usize = 0;
const ENTRY_PLUG_SOURCE: usize = 8;
const ENTRY_GROUP: usize = 12;
const ENTRY_KIND: usize = 13;
const ENTRY_POOL: usize = 56;
/// The entry kind of the super lane, which is what makes a list a subclass's.
const SUPER_ENTRY_KIND: u8 = 34;
const TALENT_DISPLAY_TABLE_CLASS: u32 = 0x8080_5C3C;
const TALENT_DISPLAY_ROW_CLASS: u32 = 0x8080_5C40;
const TALENT_DISPLAY_CLASS: u32 = 0x8080_5C42;
/// Display rows: an entry's display hash, its grid position and its node record.
const DISPLAY_NODE_ARRAY: usize = 0x08;
const DISPLAY_NODE_ROW_CLASS: u32 = 0x8080_5C46;
const DISPLAY_NODE_ROW_SIZE: usize = 24;
const DISPLAY_NODE_TAG: usize = 16;
const DISPLAY_NODE_CLASS: u32 = 0x8080_5C49;
/// Attunement rows: an attunement's plug source and the value the client shows it by.
const DISPLAY_PATH_ARRAY: usize = 0x30;
const DISPLAY_PATH_ROW_CLASS: u32 = 0x8080_5C45;
const DISPLAY_PATH_ROW_SIZE: usize = 8;
/// Sunrise keeps per-entry selection state for at most this many lists with a super lane.
const SUNRISE_SUPER_LANE_LIST_CAPACITY: usize = 32;
/// An entry's pool: groups of variants, each variant with its records, its sandbox perks and the
/// flags it sets. Every stock subclass pool has one group with one variant.
const POOL_CLASS: u32 = 0x8080_7A8B;
const POOL_GROUP_ARRAY: usize = 0x08;
const POOL_GROUP_CLASS: u32 = 0x8080_7A8D;
const POOL_GROUP_SIZE: usize = 144;
const POOL_VARIANT_CLASS: u32 = 0x8080_7A97;
const POOL_VARIANT_SIZE: usize = 88;
const VARIANT_PERK_ARRAY: usize = 0x20;
const VARIANT_PERK_CLASS: u32 = 0x8080_7C5F;
/// A node record's steps, each with display rows that start with a name and a description.
const NODE_STEP_ARRAY: usize = 0x08;
const NODE_STEP_CLASS: u32 = 0x8080_2B16;
const NODE_STEP_SIZE: usize = 0x20;
const NODE_DISPLAY_CLASS: u32 = 0x8080_5C4B;
const NODE_DISPLAY_SIZE: usize = 0x20;
const NODE_NAME: usize = 0x00;
const NODE_DESCRIPTION: usize = 0x08;

/// An authored subclass's own socket-entry list and display record.
#[derive(Clone)]
pub(super) struct ResolvedSubclassList {
    /// The base's list, whose table rows the authored ones copy.
    pub(super) base_index: u16,
    pub(super) template_tag: TagHash,
    pub(super) display_template_tag: TagHash,
    pub(super) hash: u32,
    pub(super) list: Vec<u8>,
    pub(super) display: Vec<u8>,
    pub(super) companion: Companion,
    pub(super) display_companion: Companion,
    /// Path nodes with text or perks of their own, which take a pool and node record each.
    pub(super) nodes: Vec<AuthoredNode>,
    /// Paths with names of their own, which the lore tables carry.
    pub(super) path_names: Vec<PathName>,
}

/// A path node's own pool and node record, copied from its source node and edited.
#[derive(Clone)]
pub(super) struct AuthoredNode {
    /// The entry it fills in the authored list.
    pub(super) entry: u8,
    pub(super) pool_template: TagHash,
    pub(super) pool: Vec<u8>,
    pub(super) record_template: TagHash,
    pub(super) record: Vec<u8>,
}

/// An attunement path with a name of its own. The display record shows a path by a lore row,
/// so the path takes a row of its own, copied from the one it had for its icon.
#[derive(Clone)]
pub(super) struct PathName {
    /// The display record's attunement row, named by its path's lead plug source.
    pub(super) plug_source: u32,
    /// The stock lore row the path showed before.
    pub(super) template_row: u32,
    pub(super) row_hash: u32,
    pub(super) name_hash: u32,
}

/// A type-16 record's shared-tag companion, which lists what loads with the record. For an
/// authored record, the dependencies leave out the record and companion, whose tags the build
/// assigns.
#[derive(Clone)]
pub(super) struct Companion {
    pub(super) tag: TagHash,
    pub(super) payload: Vec<u8>,
    pub(super) dependencies: SharedTagDependencies,
}

impl Companion {
    fn read(manager: &PackageManager, owner: TagHash) -> AuthoringResult<Self> {
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
    fn for_copy(&self, owner: TagHash, added: impl IntoIterator<Item = u32>) -> Self {
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
    pub(super) fn payload_for(
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

/// One stock subclass's list and display record.
struct StockList {
    index: u16,
    tag: TagHash,
    list: Vec<u8>,
    display_tag: TagHash,
    display: Vec<u8>,
    companion: Companion,
    display_companion: Companion,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Entry {
    display_hash: u32,
    plug_source: u32,
    group: u8,
    kind: u8,
    pool: u32,
}

impl StockList {
    fn read(sources: &ProjectSources, item_hash: u32, role: &str) -> AuthoringResult<Self> {
        let item = resolve_donor_item(sources, item_hash, role)?;
        let definition = read_tag(&sources.manager, item.definition_tag, role)?;
        gear::native_slot(&definition, ItemKind::Subclass).map_err(|_| {
            invalid(format!(
                "{role} item 0x{item_hash:08X} is not an installed subclass"
            ))
        })?;
        let index = list_index(&definition)?;
        let (tag, display_tag) = sources.subclass_tables.row_tags(index)?;
        let list = read_tag(&sources.manager, tag, "subclass socket-entry list")?;
        let display = read_tag(&sources.manager, display_tag, "subclass display record")?;
        let stock = Self {
            index,
            tag,
            list,
            display_tag,
            display,
            companion: Companion::read(&sources.manager, tag)?,
            display_companion: Companion::read(&sources.manager, display_tag)?,
        };
        stock.check_classes(sources)?;
        if stock.entries()?.len() != layout::ENTRY_COUNT
            || stock.entry(layout::SUPER)?.kind != SUPER_ENTRY_KIND
        {
            return Err(invalid(format!(
                "{role} item 0x{item_hash:08X} does not use the stock subclass layout"
            )));
        }
        Ok(stock)
    }

    fn check_classes(&self, sources: &ProjectSources) -> AuthoringResult<()> {
        for (tag, class, label) in [
            (
                self.tag,
                SOCKET_ENTRY_LIST_CLASS,
                "Subclass socket-entry list",
            ),
            (
                self.display_tag,
                TALENT_DISPLAY_CLASS,
                "Subclass display record",
            ),
        ] {
            let entry = sources
                .manager
                .get_entry(tag)
                .ok_or_else(|| invalid(format!("{label} {tag} is not live")))?;
            if entry.reference != class {
                return Err(invalid(format!(
                    "{label} {tag} has class 0x{:08X}, expected 0x{class:08X}",
                    entry.reference
                )));
            }
        }
        Ok(())
    }

    fn entries(&self) -> AuthoringResult<Vec<Entry>> {
        entries(&self.list)
    }

    fn entry(&self, index: u8) -> AuthoringResult<Entry> {
        entries(&self.list)?
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| invalid(format!("Subclass entry {index} is outside its list")))
    }
}

fn entries(list: &[u8]) -> AuthoringResult<Vec<Entry>> {
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

/// The tag of the socket-entry list at `index` in the stock table.
pub(crate) fn list_tag(table: &[u8], index: u16) -> AuthoringResult<TagHash> {
    let (count, _, rows) = terminal_index_table_layout(
        table,
        SOCKET_ENTRY_LIST_ROW_CLASS,
        "socket-entry-list table",
    )?;
    let index = usize::from(index);
    if index >= count {
        return Err(invalid(format!(
            "Socket-entry list {index} is outside the table"
        )));
    }
    Ok(TagHash(read_u32(table, rows + index * ITEM_ROW_SIZE + 16)?))
}

/// The pool of a list's class-base entry, the base melee each attunement links to. It differs
/// by class, and an authored list keeps its base's, so it names the class of a subclass whose
/// item strings do not.
pub(crate) fn class_base_pool(list: &[u8]) -> AuthoringResult<u32> {
    entries(list)?
        .get(usize::from(layout::CLASS_BASE))
        .map(|entry| entry.pool)
        .ok_or_else(|| invalid("The subclass list has no class-base entry"))
}

/// Points an authored subclass at its own socket-entry list, changing nothing else.
pub(super) fn set_list_index(definition: &mut [u8], index: u16) -> AuthoringResult<()> {
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

/// The stock socket-entry-list and display tables, which grow by a row for each subclass whose
/// abilities come from other subclasses.
#[derive(Clone)]
pub(super) struct SubclassTables {
    pub(super) list_table_tag: TagHash,
    pub(super) display_table_tag: TagHash,
    pub(super) lists: Vec<u8>,
    pub(super) displays: Vec<u8>,
    /// Stock lists that carry a super lane, which Sunrise keeps selection state for.
    pub(super) super_lane_lists: usize,
}

impl SubclassTables {
    pub(super) fn load(
        manager: &PackageManager,
        list_table_tag: TagHash,
        display_table_tag: TagHash,
    ) -> AuthoringResult<Self> {
        for (tag, class, label) in [
            (
                list_table_tag,
                SOCKET_ENTRY_LIST_TABLE_CLASS,
                "Socket-entry-list table",
            ),
            (
                display_table_tag,
                TALENT_DISPLAY_TABLE_CLASS,
                "Subclass display table",
            ),
        ] {
            let entry = manager
                .get_entry(tag)
                .ok_or_else(|| invalid(format!("{label} {tag} is not live")))?;
            if entry.reference != class {
                return Err(invalid(format!(
                    "{label} {tag} has class 0x{:08X}, expected 0x{class:08X}",
                    entry.reference
                )));
            }
        }
        let lists = read_tag(manager, list_table_tag, "socket-entry-list table")?;
        let displays = read_tag(manager, display_table_tag, "subclass display table")?;
        let (list_count, _, _) = terminal_index_table_layout(
            &lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        let (display_count, _, _) = terminal_index_table_layout(
            &displays,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if list_count != display_count {
            return Err(invalid(format!(
                "The socket-entry-list table has {list_count} rows and its display table {display_count}"
            )));
        }
        let mut tables = Self {
            list_table_tag,
            display_table_tag,
            lists,
            displays,
            super_lane_lists: 0,
        };
        for index in 0..list_count {
            let (tag, _) = tables.row_tags(u16::try_from(index).unwrap_or(u16::MAX))?;
            if manager
                .get_entry(tag)
                .is_none_or(|entry| entry.reference != SOCKET_ENTRY_LIST_CLASS)
            {
                continue;
            }
            // The empty and cut-down lists carry no super lane, and may carry no entry rows.
            let list = read_tag(manager, tag, "socket-entry list")?;
            if entries(&list)
                .is_ok_and(|entries| entries.iter().any(|entry| entry.kind == SUPER_ENTRY_KIND))
            {
                tables.super_lane_lists += 1;
            }
        }
        Ok(tables)
    }

    pub(super) fn count(&self) -> AuthoringResult<usize> {
        Ok(terminal_index_table_layout(
            &self.lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?
        .0)
    }

    /// A row's list tag and display tag. The two tables name each row by the same hash.
    fn row_tags(&self, index: u16) -> AuthoringResult<(TagHash, TagHash)> {
        let index = usize::from(index);
        let (count, _, rows) = terminal_index_table_layout(
            &self.lists,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        let (_, _, display_rows) = terminal_index_table_layout(
            &self.displays,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if index >= count {
            return Err(invalid(format!(
                "Socket-entry list {index} is outside the table"
            )));
        }
        let row = rows + index * ITEM_ROW_SIZE;
        let display_row = display_rows + index * ITEM_ROW_SIZE;
        if read_u32(&self.lists, row)? != read_u32(&self.displays, display_row)? {
            return Err(invalid(format!(
                "Socket-entry list {index} and its display row name different lists"
            )));
        }
        Ok((
            TagHash(read_u32(&self.lists, row + 16)?),
            TagHash(read_u32(&self.displays, display_row + 16)?),
        ))
    }

    /// Adds an authored list's rows, copied from its base's, and returns the list's index.
    pub(super) fn append(
        &mut self,
        list: &ResolvedSubclassList,
        list_tag: TagHash,
        display_tag: TagHash,
    ) -> AuthoringResult<u16> {
        let index = u16::try_from(self.count()?)
            .map_err(|_| invalid("Socket-entry-list index does not fit 16 bits"))?;
        self.lists = append_index_row(
            std::mem::take(&mut self.lists),
            usize::from(list.base_index),
            list.hash,
            list_tag,
            SOCKET_ENTRY_LIST_ROW_CLASS,
            "socket-entry-list table",
        )?;
        self.displays = append_index_row(
            std::mem::take(&mut self.displays),
            usize::from(list.base_index),
            list.hash,
            display_tag,
            TALENT_DISPLAY_ROW_CLASS,
            "subclass display table",
        )?;
        if self.row_tags(index)? != (list_tag, display_tag) {
            return Err(validation("Authored subclass list rows are inconsistent"));
        }
        Ok(index)
    }
}

/// Checks a subclass recipe's settings: identity, text, lore and icon, and abilities from other
/// subclasses. Rarity, stats and sockets belong to the talent grid, which a subclass has instead.
/// Lore uses the shared writer, since a stock subclass carries the same lore block as a weapon.
pub(super) fn unsupported_setting(spec: &WeaponCloneSpec) -> Option<&'static str> {
    let overrides = &spec.overrides;
    [
        (overrides.rarity.is_some(), "a rarity"),
        (
            !overrides.investment_stats.is_empty()
                || !overrides.removed_investment_stats.is_empty(),
            "stats",
        ),
        (
            !overrides.socket_columns.is_empty() || !overrides.socket_plug_variants.is_empty(),
            "sockets or custom perks",
        ),
        (overrides.render_dye_rows.is_some(), "dye rows"),
        (overrides.badge.is_some(), "a badge"),
    ]
    .into_iter()
    .find_map(|(set, setting)| set.then_some(setting))
}

/// Checks the base and every source against the installed catalog: each is a subclass. A source
/// may be of any class.
pub(super) fn validate_against_catalog(
    catalog: &InvestmentCatalog,
    spec: &WeaponCloneSpec,
) -> AuthoringResult<()> {
    let is_subclass = |hash: u32| {
        catalog.gear_donor(hash).is_some_and(|donor| {
            ItemKind::from_bucket_hash(donor.summary.bucket_hash) == Some(ItemKind::Subclass)
        })
    };
    if !is_subclass(spec.donor_item_hash) {
        return Err(invalid(format!(
            "Base item 0x{:08X} is not an installed subclass",
            spec.donor_item_hash
        )));
    }
    let Some(abilities) = &spec.overrides.subclass_abilities else {
        return Ok(());
    };
    abilities.validate().map_err(invalid)?;
    let sources = abilities
        .choices
        .iter()
        .map(|choice| {
            let slot = AbilitySlot::of_entry(choice.entry).unwrap_or(AbilitySlot::Grenade);
            (choice.source, slot.label())
        })
        .chain(
            abilities
                .attunements
                .iter()
                .map(|attunement| (attunement.source, "Attunement")),
        )
        .chain(abilities.attunements.iter().flat_map(|attunement| {
            attunement
                .nodes
                .iter()
                .map(|node| (node.source, "Attunement node"))
        }));
    for (source, label) in sources {
        if !is_subclass(source) {
            return Err(invalid(format!(
                "{label} source 0x{source:08X} is not an installed subclass"
            )));
        }
    }
    Ok(())
}

/// Resolves one subclass recipe. Its definition keeps the base's talent-grid holder unless its
/// abilities come from other subclasses, which gives it a list of its own.
pub(super) fn resolve(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    donor: DonorItem,
    definition: Vec<u8>,
    strings: Vec<u8>,
) -> AuthoringResult<ResolvedWeapon> {
    gear::native_slot(&definition, ItemKind::Subclass)?;
    if matching_u32_offsets(&definition, spec.donor_item_hash) != IDENTITY_OFFSETS
        || !matching_u32_offsets(&strings, spec.donor_item_hash).is_empty()
    {
        return Err(invalid(
            "The base subclass embeds its item identity at unsupported offsets",
        ));
    }
    let base = StockList::read(sources, spec.donor_item_hash, "Base subclass")?;
    let subclass_list = match &spec.overrides.subclass_abilities {
        Some(abilities) if !abilities.is_empty() => {
            Some(author_list(sources, spec, &base, abilities)?)
        }
        _ => None,
    };
    let donor_icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    validate_reused_stock_item_icon(&sources.stock_item_icons, &strings, donor_icon_index)?;
    let donor_icon_container =
        stock_item_icon_container(&sources.stock_item_icons, donor_icon_index)?;
    read_tag(
        &sources.manager,
        donor_icon_container,
        "base subclass icon container",
    )?;
    let icon_donor = spec
        .icon_donor
        .as_ref()
        .map(|reference| resolve_icon_donor(sources, reference))
        .transpose()?;
    let selected_icon = icon_donor.unwrap_or(ResolvedIconDonor {
        item_index: donor.item_index,
        icon_index: donor_icon_index,
        icon_container: donor_icon_container,
    });
    Ok(ResolvedWeapon {
        weapon: spec.clone(),
        donor_item_index: donor.item_index,
        collection: None,
        definition_tag: donor.definition_tag,
        string_tag: donor.string_tag,
        definition,
        strings,
        icon_template_item_index: donor.item_index,
        icon_template_container: donor_icon_container,
        donor_icon_index: selected_icon.icon_index,
        donor_icon_container: selected_icon.icon_container,
        presentation_donor: None,
        damage_carrier_source: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        runtime_pattern_source: None,
        gear_art_pattern_source: None,
        appearance_rig_donor: None,
        socket_column_indices: Vec::new(),
        has_authored_shader: false,
        subclass_list,
        dye_rows: None,
    })
}

/// The base's list and display record with each chosen entry's pool, node record and
/// attunement value taken from its source.
fn author_list(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    base: &StockList,
    abilities: &SubclassAbilities,
) -> AuthoringResult<ResolvedSubclassList> {
    abilities.validate().map_err(invalid)?;
    let base_entries = base.entries()?;
    let mut stock = BTreeMap::<u32, StockList>::new();
    let node_sources = abilities
        .attunements
        .iter()
        .flat_map(|attunement| attunement.nodes.iter().map(|node| node.source));
    for hash in abilities
        .choices
        .iter()
        .map(|choice| choice.source)
        .chain(
            abilities
                .attunements
                .iter()
                .map(|attunement| attunement.source),
        )
        .chain(node_sources)
    {
        if stock.contains_key(&hash) {
            continue;
        }
        let source = StockList::read(sources, hash, "Ability source")?;
        if source
            .entries()?
            .iter()
            .zip(&base_entries)
            .any(|(source, base)| {
                (
                    source.display_hash,
                    source.plug_source,
                    source.group,
                    source.kind,
                ) != (base.display_hash, base.plug_source, base.group, base.kind)
            })
        {
            return Err(invalid(format!(
                "Subclass 0x{hash:08X} does not share its base's layout"
            )));
        }
        stock.insert(hash, source);
    }
    let mut list = base.list.clone();
    let mut display = base.display.clone();
    for choice in &abilities.choices {
        AbilitySlot::of_entry(choice.entry)
            .ok_or_else(|| invalid(format!("Entry {} is not an ability slot", choice.entry)))?;
        let source = &stock[&choice.source];
        copy_entry(
            &mut list,
            &mut display,
            base,
            source,
            choice.entry,
            choice.source_entry,
        )?;
    }
    let mut nodes = Vec::new();
    let mut path_names = Vec::new();
    for attunement in &abilities.attunements {
        let source = &stock[&attunement.source];
        for (target, from) in attunement
            .path
            .entries()
            .into_iter()
            .zip(attunement.source_path.entries())
        {
            copy_entry(&mut list, &mut display, base, source, target, from)?;
        }
        let place = base.entry(attunement.path.entries()[0])?.plug_source;
        let from = source
            .entry(attunement.source_path.entries()[0])?
            .plug_source;
        let value = path_value(&source.display, from)?;
        set_path_value(&mut display, place, value)?;
        for node in &attunement.nodes {
            let target = attunement.path.entries()[usize::from(node.position)];
            let from = node.source_path.entries()[usize::from(node.source_position)];
            let node_source = &stock[&node.source];
            copy_entry(&mut list, &mut display, base, node_source, target, from)?;
            if node.is_authored() {
                nodes.push(author_node(
                    sources,
                    spec,
                    attunement.path,
                    node,
                    (node_source, target, from),
                )?);
            }
        }
        if attunement.name.is_some() {
            path_names.push(PathName {
                plug_source: place,
                template_row: value,
                row_hash: crate::presentation::text_hash(
                    &spec.namespace,
                    &format!("attunement-{}-row", attunement.path.key()),
                ),
                name_hash: path_name_hash(&spec.namespace, attunement.path),
            });
        }
    }
    check_choices(
        &entries(&list)?,
        &nodes.iter().map(|node| node.entry).collect(),
    )?;
    // Every display row still names a node record, now some of them another subclass's.
    let (count, _, rows, _) = array_at(&display, DISPLAY_NODE_ARRAY)?;
    for row in (0..count).map(|index| rows + index * DISPLAY_NODE_ROW_SIZE) {
        let node = TagHash(read_u32(&display, row + DISPLAY_NODE_TAG)?);
        if sources
            .manager
            .get_entry(node)
            .is_none_or(|entry| entry.reference != DISPLAY_NODE_CLASS)
        {
            return Err(invalid(format!(
                "Subclass display row names {node}, which is not a node record"
            )));
        }
    }
    // The swapped-in pools and node records load with the authored list and display record.
    let companion = base
        .companion
        .for_copy(base.tag, entries(&list)?.iter().map(|entry| entry.pool));
    let node_tags = (0..count)
        .map(|index| {
            read_u32(
                &display,
                rows + index * DISPLAY_NODE_ROW_SIZE + DISPLAY_NODE_TAG,
            )
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let display_companion = base.display_companion.for_copy(base.display_tag, node_tags);
    Ok(ResolvedSubclassList {
        base_index: base.index,
        template_tag: base.tag,
        display_template_tag: base.display_tag,
        hash: fnv1_name_hash(&format!("parhelion/{}/socket_entry_list", spec.namespace)),
        list,
        display,
        companion,
        display_companion,
        nodes,
        path_names,
    })
}

/// A path node with text or perks of its own: its source node's pool with the perks edited, and
/// its node record with the text pointed at the recipe's.
fn author_node(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    path: AttunementPath,
    node: &SubclassPathNode,
    (source, target, from): (&StockList, u8, u8),
) -> AuthoringResult<AuthoredNode> {
    let context = format!(
        "Node {} of the {} attunement",
        node.position + 1,
        path.label().to_lowercase()
    );
    let entry = source.entry(from)?;
    let pool_template = TagHash(entry.pool);
    if sources
        .manager
        .get_entry(pool_template)
        .is_none_or(|entry| entry.reference != POOL_CLASS)
    {
        return Err(invalid(format!(
            "{context} starts from {pool_template}, which is not an ability pool"
        )));
    }
    let perks =
        finished_sandbox_perk_count(&sources.stock_finished_sandbox_perks).map_err(invalid)?;
    if let Some(perk) = node
        .added_perks
        .iter()
        .find(|perk| usize::from(**perk) >= perks)
    {
        return Err(invalid(format!(
            "{context} adds sandbox perk {perk}, which is not an installed perk"
        )));
    }
    let pool = read_tag(&sources.manager, pool_template, "subclass node pool")?;
    let pool = edit_pool_perks(&pool, &node.added_perks, &node.removed_perks)
        .map_err(|error| error.context(context.clone()))?;
    let record_template = node_tag(&source.display, entry.display_hash)?;
    let record = read_tag(&sources.manager, record_template, "subclass node record")?;
    let text = |field: &str| node_text_hash(&spec.namespace, path, node.position, field);
    let record = set_node_text(
        &record,
        node.name.as_ref().map(|_| text("name")),
        node.description.as_ref().map(|_| text("description")),
    )
    .map_err(|error| error.context(context))?;
    Ok(AuthoredNode {
        entry: target,
        pool_template,
        pool,
        record_template,
        record,
    })
}

/// The pool with each variant's sandbox perks edited: `removed` taken out and `added` put after
/// the rest. Every perk in `removed` must be one of the pool's.
fn edit_pool_perks(pool: &[u8], added: &[u16], removed: &[u16]) -> AuthoringResult<Vec<u8>> {
    let mut payload = pool.to_vec();
    if added.is_empty() && removed.is_empty() {
        return Ok(payload);
    }
    let (groups, _, group_rows, group_class) = array_at(pool, POOL_GROUP_ARRAY)?;
    if group_class != POOL_GROUP_CLASS {
        return Err(invalid(format!(
            "Subclass pool groups have class 0x{group_class:08X}"
        )));
    }
    let mut found = BTreeSet::new();
    for group in (0..groups).map(|index| group_rows + index * POOL_GROUP_SIZE) {
        if read_u64(pool, group)? == 0 {
            continue;
        }
        let (variants, _, variant_rows, variant_class) = array_at(pool, group)?;
        if variant_class != POOL_VARIANT_CLASS {
            return Err(invalid(format!(
                "Subclass pool variants have class 0x{variant_class:08X}"
            )));
        }
        for variant in (0..variants).map(|index| variant_rows + index * POOL_VARIANT_SIZE) {
            let descriptor = variant + VARIANT_PERK_ARRAY;
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
                crate::tag_payload::append_native_array(
                    &mut payload,
                    descriptor,
                    VARIANT_PERK_CLASS,
                    edited.len(),
                    &rows,
                )?;
            }
        }
    }
    if let Some(missing) = removed.iter().find(|perk| !found.contains(*perk)) {
        return Err(invalid(format!(
            "Sandbox perk {missing} is not one of the node's perks"
        )));
    }
    synchronize_payload_size(payload)
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

/// The node record with each display row's name and description pointing at authored text.
fn set_node_text(
    record: &[u8],
    name: Option<u32>,
    description: Option<u32>,
) -> AuthoringResult<Vec<u8>> {
    let mut payload = record.to_vec();
    let (steps, _, step_rows, step_class) = array_at(record, NODE_STEP_ARRAY)?;
    if step_class != NODE_STEP_CLASS {
        return Err(invalid(format!(
            "Subclass node steps have class 0x{step_class:08X}"
        )));
    }
    for step in (0..steps).map(|index| step_rows + index * NODE_STEP_SIZE) {
        let (displays, _, display_rows, display_class) = array_at(record, step)?;
        if display_class != NODE_DISPLAY_CLASS {
            return Err(invalid(format!(
                "Subclass node display rows have class 0x{display_class:08X}"
            )));
        }
        for display in (0..displays).map(|index| display_rows + index * NODE_DISPLAY_SIZE) {
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
    }
    Ok(payload)
}

/// The list and display record with each authored node's pool and node record at their tags.
pub(super) fn place_nodes(
    list: &ResolvedSubclassList,
    tags: &[(TagHash, TagHash)],
) -> AuthoringResult<(Vec<u8>, Vec<u8>)> {
    let mut payload = list.list.clone();
    let mut display = list.display.clone();
    let (_, _, rows, _) = array_at(&payload, SOCKET_ENTRY_ARRAY)?;
    for (node, (pool, record)) in list.nodes.iter().zip(tags) {
        let entry = rows + usize::from(node.entry) * SOCKET_ENTRY_SIZE;
        write_u32(&mut payload, entry + ENTRY_POOL, pool.0)?;
        let row = node_row(&display, read_u32(&payload, entry + ENTRY_DISPLAY_HASH)?)?;
        write_u32(&mut display, row + DISPLAY_NODE_TAG, record.0)?;
    }
    Ok((payload, display))
}

fn path_name_hash(namespace: &str, path: AttunementPath) -> u32 {
    crate::presentation::text_hash(namespace, &format!("attunement-{}", path.key()))
}

fn node_text_hash(namespace: &str, path: AttunementPath, position: u8, field: &str) -> u32 {
    crate::presentation::text_hash(
        namespace,
        &format!("attunement-{}-node-{}-{field}", path.key(), position + 1),
    )
}

/// The text a subclass recipe authors for its attunements, by localized hash.
pub(super) fn authored_text(spec: &WeaponCloneSpec) -> Vec<(u32, &str)> {
    let Some(abilities) = &spec.overrides.subclass_abilities else {
        return Vec::new();
    };
    let mut text = Vec::new();
    for attunement in &abilities.attunements {
        if let Some(name) = &attunement.name {
            text.push((
                path_name_hash(&spec.namespace, attunement.path),
                name.as_str(),
            ));
        }
        for node in &attunement.nodes {
            for (field, value) in [("name", &node.name), ("description", &node.description)] {
                if let Some(value) = value {
                    text.push((
                        node_text_hash(&spec.namespace, attunement.path, node.position, field),
                        value.as_str(),
                    ));
                }
            }
        }
    }
    text
}

/// Puts `source`'s entry `from` into entry `target`: its pool in the list, and its node record in
/// the display row that names `target`.
fn copy_entry(
    list: &mut [u8],
    display: &mut [u8],
    base: &StockList,
    source: &StockList,
    target: u8,
    from: u8,
) -> AuthoringResult<()> {
    let (_, _, rows, _) = array_at(list, SOCKET_ENTRY_ARRAY)?;
    let pool = source.entry(from)?.pool;
    write_u32(
        list,
        rows + usize::from(target) * SOCKET_ENTRY_SIZE + ENTRY_POOL,
        pool,
    )?;
    let node = node_tag(&source.display, source.entry(from)?.display_hash)?;
    let row = node_row(display, base.entry(target)?.display_hash)?;
    write_u32(display, row + DISPLAY_NODE_TAG, node.0)
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

fn node_tag(display: &[u8], display_hash: u32) -> AuthoringResult<TagHash> {
    Ok(TagHash(read_u32(
        display,
        node_row(display, display_hash)? + DISPLAY_NODE_TAG,
    )?))
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

fn path_value(display: &[u8], plug_source: u32) -> AuthoringResult<u32> {
    read_u32(display, path_row(display, plug_source)? + 4)
}

pub(super) fn set_path_value(
    display: &mut [u8],
    plug_source: u32,
    value: u32,
) -> AuthoringResult<()> {
    let row = path_row(display, plug_source)?;
    write_u32(display, row + 4, value)
}

/// Sunrise reads a group of up to four entries as alternatives, one of them chosen. Two choices
/// with one pool would offer the same ability twice. An `authored` entry is a node of its own
/// even when it starts from another's pool.
fn check_choices(entries: &[Entry], authored: &BTreeSet<u8>) -> AuthoringResult<()> {
    for slot in AbilitySlot::ALL {
        let pools = slot
            .entries()
            .iter()
            .map(|&entry| entries[usize::from(entry)].pool)
            .collect::<Vec<_>>();
        if pools.iter().collect::<BTreeSet<_>>().len() != pools.len() {
            return Err(invalid(format!(
                "Two {} choices are the same ability",
                slot.label().to_lowercase()
            )));
        }
    }
    let attunements = AttunementPath::ALL.map(|path| {
        path.entries().map(|entry| {
            (
                entries[usize::from(entry)].pool,
                authored.contains(&entry).then_some(entry),
            )
        })
    });
    if attunements.iter().collect::<BTreeSet<_>>().len() != attunements.len() {
        return Err(invalid("Two attunements are the same"));
    }
    Ok(())
}

/// Checks the build against the limits Sunrise reads subclasses with: a list with a super lane
/// for at most 32 subclasses, and every run of three subclass items after the stock ones sharing
/// a class, since Sunrise grants a character every subclass in its equipped one's run.
pub(super) fn validate_project(
    sources: &ProjectSources,
    resolved: &[ResolvedWeapon],
) -> AuthoringResult<()> {
    let subclasses = resolved
        .iter()
        .filter(|donor| donor.weapon.kind == ItemKind::Subclass)
        .collect::<Vec<_>>();
    let lists = subclasses
        .iter()
        .filter(|donor| donor.subclass_list.is_some())
        .count();
    if sources.subclass_tables.super_lane_lists + lists > SUNRISE_SUPER_LANE_LIST_CAPACITY {
        return Err(invalid(format!(
            "A build can give at most {} subclasses abilities from other subclasses",
            SUNRISE_SUPER_LANE_LIST_CAPACITY - sources.subclass_tables.super_lane_lists
        )));
    }
    for (run, members) in subclasses.chunks(3).enumerate() {
        if members.len() < 3 {
            break;
        }
        let bases = members
            .iter()
            .map(|donor| {
                StockList::read(sources, donor.weapon.donor_item_hash, "Base subclass")
                    .and_then(|stock| Ok(stock.entry(layout::CLASS_BASE)?.pool))
            })
            .collect::<AuthoringResult<BTreeSet<_>>>()?;
        if bases.len() != 1 {
            return Err(invalid(format!(
                "Subclasses {} to {} in this project must be for one class. Sunrise gives a character all three when it equips one.",
                run * 3 + 1,
                run * 3 + 3
            ))
            .context(
                members
                    .iter()
                    .map(|donor| donor.weapon.error_context())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ));
        }
    }
    Ok(())
}
