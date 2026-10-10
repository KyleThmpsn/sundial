//! A subclass recipe's own socket-entry list and display record: its base's, with abilities and
//! attunements from other stock subclasses swapped in, and each authored ability and path node
//! given a pool and node record of its own.
//!
//! Every stock list shares one layout, so an entry's position fixes its display hash, plug
//! source, group and prerequisites, and a swapped entry keeps all of them. Only the pool and the
//! display row's node record move. The build places the records after the private plugs, and
//! adds each authored entry's custom perks to its pool once they are compiled.
use std::collections::{BTreeMap, BTreeSet};

mod attached;

use sundial::investment::InvestmentCatalog;
use sundial::package_authoring::{PackageManager, fnv1_name_hash};
use tiger_pkg::TagHash;

use super::native::{
    self, Companion, DISPLAY_NODE_CLASS, Entry, POOL_CLASS, SOCKET_ENTRY_LIST_CLASS,
    SUPER_ENTRY_KIND, TALENT_DISPLAY_CLASS,
};
use super::tables::SubclassTables;
use super::{
    AbilitySlot, AttunementPath, EntryEdits, EntryIcon, ModifierEffect, Place, SubclassAbilities,
    layout,
};
use crate::error::{invalid, validation};
use crate::perk::{Icon, PerkRecipe};
use crate::tag_payload::read_tag;
use crate::{AuthoringResult, ItemKind};
use sundial::package_authoring::ability_bank::Modifier;
use sundial::package_authoring::ability_modifier::{
    charge_key, parameter_key, recharge_key, recharge_modifier,
};
use sundial::package_authoring::runtime::WeaponRuntimeValueOverride;

/// Sunrise keeps per-entry selection state for at most this many lists with a super lane.
const SUNRISE_SUPER_LANE_LIST_CAPACITY: usize = 32;

/// What authoring reads from the installed packages.
pub(crate) struct Sources<'a> {
    pub(crate) manager: &'a PackageManager,
    pub(crate) tables: &'a SubclassTables,
    /// Rows in the stock finished sandbox-perk table. An added perk must be one of them.
    pub(crate) stock_perks: usize,
    /// The socket-entry-list row a stock subclass's item names. It refuses an item that is not
    /// an installed subclass.
    pub(crate) list_index: &'a dyn Fn(u32, &str) -> AuthoringResult<u16>,
    /// The container of a row in the stock item icon table.
    pub(crate) icon_container: &'a dyn Fn(u16) -> AuthoringResult<TagHash>,
    /// The entity a stock ability row names, through its pattern hash and the entity assignment
    /// table, if any.
    pub(crate) entity_of: &'a dyn Fn(u8) -> AuthoringResult<Option<TagHash>>,
    /// The ability entities a stock perk's On a Specific Ability and Ends on a Specific Ability
    /// conditions name.
    pub(crate) ability_references: &'a dyn Fn(u16) -> AuthoringResult<Vec<u32>>,
    /// The bank the entity of a stock ability row reads, if it has one.
    pub(crate) ability_bank: &'a dyn Fn(u8) -> AuthoringResult<Option<AbilityBank>>,
}

/// An ability's bank as a modifier needs it: whether it takes a charge row, the script
/// parameters a row can set in it and the keys its property rows answer to.
#[derive(Clone, Debug)]
pub(crate) struct AbilityBank {
    pub(crate) tag: u32,
    pub(crate) charges: bool,
    /// Whether the bank takes a row that changes the ability's recharge rate.
    pub(crate) recharge: bool,
    pub(crate) parameters: Vec<u32>,
    pub(crate) keys: Vec<u32>,
}

/// Dawn files at most this many keys into one ability's bucket and drops the rest.
const BUCKET_KEYS: usize = 16;
/// The group of an entry Dawn selects on its own rather than among alternatives.
const NO_GROUP: u8 = 0xFF;

/// A key an authored entry applies to an ability while both are selected, the entry holding
/// the ability, that ability's stock row, and the bank row the key needs when it is not a
/// stock key of the bank.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedModifier {
    pub(crate) key: u32,
    pub(crate) target: u8,
    pub(crate) stock_row: u8,
    pub(crate) bank_row: Option<BankRow>,
}

/// A property row the build adds to a stock ability bank under a key of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BankRow {
    pub(crate) bank: u32,
    pub(crate) key: u32,
    pub(crate) modifier: Modifier,
}

/// An authored subclass's own socket-entry list and display record.
#[derive(Clone)]
pub(crate) struct ResolvedList {
    /// The base's list, whose table rows the authored ones copy.
    pub(crate) base_index: u16,
    pub(crate) hash: u32,
    template_tag: TagHash,
    display_template_tag: TagHash,
    list: Vec<u8>,
    display: Vec<u8>,
    companion: Companion,
    display_companion: Companion,
    /// Abilities and path nodes with edits of their own, which take a pool and node record each.
    pub(crate) entries: Vec<AuthoredEntry>,
    /// Paths with names of their own, which the lore tables carry.
    pub(crate) path_names: Vec<PathName>,
}

/// An ability's or path node's own pool and node record, copied from the stock entry it is
/// based on and edited.
#[derive(Clone)]
pub(crate) struct AuthoredEntry {
    /// The entry it fills in the authored list.
    pub(crate) entry: u8,
    /// How errors name it: "Grenade 1", or "Node 2 of the top attunement".
    pub(crate) label: String,
    /// What names its authored text and icon row.
    pub(crate) key: String,
    pool_template: TagHash,
    pool: Vec<u8>,
    record_template: TagHash,
    record: Vec<u8>,
    /// Artwork or a node color of its own, which takes a private icon row and container.
    pub(crate) icon: Option<NodeIcon>,
    /// Perks from the perk workbench, compiled with the private perks.
    pub(crate) custom_perks: Vec<PerkRecipe>,
    /// What it asks to change about abilities, by the entry holding each: its modifiers, its
    /// extra charges and its parameter values. Resolved once the list is whole.
    requested: Vec<(u8, ModifierEffect)>,
    /// What it changes, resolved.
    pub(crate) modifiers: Vec<ResolvedModifier>,
    /// Its stock pool's modifier records it leaves out, as key and row.
    removed_modifiers: Vec<(u32, u8)>,
    /// The ability's own entity with values changed, which the build copies under an ability
    /// row of the entry's own.
    pub(crate) entity: Option<AbilityEntity>,
    /// Stock perks its pool grants whose conditions name an ability the list copies. The build
    /// replaces each with a private copy that names the copy.
    pub(crate) retargeted: Vec<u16>,
}

/// The stock ability row an entry's pool equips, the entity it names, and the values the copy
/// changes.
#[derive(Clone)]
pub(crate) struct AbilityEntity {
    pub(crate) row: u8,
    pub(crate) source: TagHash,
    pub(crate) values: Vec<WeaponRuntimeValueOverride>,
    pub(crate) palettes: Vec<super::PaletteEdit>,
    pub(crate) tints: Vec<super::TintEdit>,
    pub(crate) grade: Option<super::EffectGrade>,
    pub(crate) swaps: Vec<super::SpawnSwap>,
    pub(crate) bank_values: Vec<super::BankValue>,
    /// The damage type its damage profiles take, as the client encodes it.
    pub(crate) damage_type: Option<u8>,
    /// The HUD glyph its energy controller names in place of its own: the glyph of the ability
    /// whose icon the entry takes.
    pub(crate) hud_glyph: Option<u32>,
    /// The keys its entry applies to its row. Its bank's rows that name a glyph while one of them
    /// applies name `hud_glyph` too, since the tile shows theirs.
    pub(crate) hud_keys: Vec<u32>,
    /// A private glyph row carrying the ability color and, for a Super, its inherited theme.
    pub(crate) hud_color: Option<super::hud::Row>,
    pub(crate) attached: Vec<AttachedHud>,
}

/// One descendant's controller glyph and conditional variants, with optional private artwork.
#[derive(Clone)]
pub(crate) struct AttachedHud {
    pub(crate) graph: u32,
    /// The controller's row first, followed by distinct glyphs its bank can select.
    pub(crate) rows: Vec<super::hud::Row>,
    pub(crate) icon: Option<NodeIcon>,
}

/// An entry's own artwork, and the stock icon row it replaces, whose container the authored
/// one is built from.
#[derive(Clone)]
pub(crate) struct NodeIcon {
    pub(crate) artwork: Option<Icon>,
    pub(crate) color: Option<[u8; 3]>,
    pub(crate) source_row: u16,
    pub(crate) source_container: TagHash,
}

/// An attunement path with a name of its own. The display record shows a path by a lore row,
/// so the path takes a row of its own, copied from the one it had for its icon.
#[derive(Clone)]
pub(crate) struct PathName {
    /// The display record's attunement row, named by its path's lead plug source.
    pub(crate) plug_source: u32,
    /// The stock lore row the path showed before.
    pub(crate) template_row: u32,
    pub(crate) row_hash: u32,
    pub(crate) name_hash: u32,
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

impl StockList {
    fn read(sources: &Sources<'_>, item_hash: u32, role: &str) -> AuthoringResult<Self> {
        let index = (sources.list_index)(item_hash, role)?;
        let (tag, display_tag) = sources.tables.row_tags(index)?;
        for (tag, class, label) in [
            (tag, SOCKET_ENTRY_LIST_CLASS, "Subclass socket-entry list"),
            (display_tag, TALENT_DISPLAY_CLASS, "Subclass display record"),
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
        let stock = Self {
            index,
            tag,
            list: read_tag(sources.manager, tag, "subclass socket-entry list")?,
            display_tag,
            display: read_tag(sources.manager, display_tag, "subclass display record")?,
            companion: Companion::read(sources.manager, tag)?,
            display_companion: Companion::read(sources.manager, display_tag)?,
        };
        if stock.entries()?.len() != layout::ENTRY_COUNT
            || stock.entry(layout::SUPER)?.kind != SUPER_ENTRY_KIND
        {
            return Err(invalid(format!(
                "{role} item 0x{item_hash:08X} does not use the stock subclass layout"
            )));
        }
        Ok(stock)
    }

    fn entries(&self) -> AuthoringResult<Vec<Entry>> {
        native::entries(&self.list)
    }

    fn entry(&self, index: u8) -> AuthoringResult<Entry> {
        self.entries()?
            .get(usize::from(index))
            .copied()
            .ok_or_else(|| invalid(format!("Subclass entry {index} is outside its list")))
    }

    /// The node record the entry at `index` shows.
    fn node_record(&self, index: u8) -> AuthoringResult<TagHash> {
        native::node_tag(&self.display, self.entry(index)?.display_hash)
    }
}

/// Every stock subclass a recipe's abilities read: the sources of its abilities, attunements
/// and nodes, and of the icons they take.
fn source_subclasses(abilities: &SubclassAbilities) -> BTreeSet<u32> {
    let edits = abilities.choices.iter().map(|choice| &choice.edits).chain(
        abilities
            .attunements
            .iter()
            .flat_map(|attunement| attunement.nodes.iter().map(|node| &node.edits)),
    );
    abilities
        .choices
        .iter()
        .map(|choice| choice.source)
        .chain(
            abilities
                .attunements
                .iter()
                .map(|attunement| attunement.source),
        )
        .chain(
            abilities
                .attunements
                .iter()
                .flat_map(|attunement| attunement.nodes.iter().map(|node| node.source)),
        )
        .chain(edits.flat_map(|edits| {
            edits
                .icon
                .iter()
                .chain(
                    edits
                        .attached_abilities
                        .iter()
                        .filter_map(|edit| edit.icon.as_ref()),
                )
                .filter_map(|icon| match icon {
                    EntryIcon::Ability { subclass, .. } => Some(*subclass),
                    _ => None,
                })
        }))
        .collect()
}

/// The base's list and display record with each chosen entry's pool, node record and
/// attunement value taken from its source, and each authored entry given its own.
pub(crate) fn author_list(
    sources: &Sources<'_>,
    namespace: &str,
    base_hash: u32,
    abilities: &SubclassAbilities,
) -> AuthoringResult<ResolvedList> {
    abilities.validate().map_err(invalid)?;
    let mut abilities = abilities.clone();
    super::hud::include_entries(&mut abilities, base_hash);
    let base = StockList::read(sources, base_hash, "Base subclass")?;
    let base_entries = base.entries()?;
    let mut stock = BTreeMap::<u32, StockList>::new();
    for hash in source_subclasses(&abilities) {
        let source = StockList::read(sources, hash, "Ability source")?;
        if source
            .entries()?
            .iter()
            .zip(&base_entries)
            .any(|(source, base)| source.position() != base.position())
        {
            return Err(invalid(format!(
                "Subclass 0x{hash:08X} does not share its base's layout"
            )));
        }
        stock.insert(hash, source);
    }
    let mut list = base.list.clone();
    let mut display = base.display.clone();
    let mut entries = Vec::new();
    for choice in &abilities.choices {
        if AbilitySlot::of_entry(choice.entry).is_none()
            && !layout::FOUNDATIONS.contains(&choice.entry)
        {
            return Err(invalid(format!(
                "Entry {} is not an editable ability",
                choice.entry
            )));
        }
        let source = &stock[&choice.source];
        copy_entry(
            &mut list,
            &mut display,
            &base,
            source,
            choice.entry,
            choice.source_entry,
        )?;
        let color = abilities.hud_color;
        if !choice.edits.is_empty() || color.is_some() {
            entries.push(author_entry(
                sources,
                (&stock, namespace),
                Place::Ability(choice.entry),
                &choice.edits,
                color,
                (source, choice.entry, choice.source_entry),
            )?);
        }
    }
    let mut path_names = Vec::new();
    for attunement in &abilities.attunements {
        let source = &stock[&attunement.source];
        for (target, from) in attunement
            .path
            .entries()
            .into_iter()
            .zip(attunement.source_path.entries())
        {
            copy_entry(&mut list, &mut display, &base, source, target, from)?;
        }
        let place = base.entry(attunement.path.entries()[0])?.plug_source;
        let from = source
            .entry(attunement.source_path.entries()[0])?
            .plug_source;
        let value = native::path_value(&source.display, from)?;
        native::set_path_value(&mut display, place, value)?;
        for node in &attunement.nodes {
            let target = attunement.path.entries()[usize::from(node.position)];
            let node_source = &stock[&node.source];
            copy_entry(
                &mut list,
                &mut display,
                &base,
                node_source,
                target,
                node.source_entry(),
            )?;
            let color = abilities.hud_color;
            if !node.edits.is_empty() || color.is_some() {
                entries.push(author_entry(
                    sources,
                    (&stock, namespace),
                    Place::Node(attunement.path, node.position),
                    &node.edits,
                    color,
                    (node_source, target, node.source_entry()),
                )?);
            }
        }
        if attunement.name.is_some() {
            path_names.push(PathName {
                plug_source: place,
                template_row: value,
                row_hash: crate::presentation::text_hash(
                    namespace,
                    &format!("attunement-{}-row", attunement.path.key()),
                ),
                name_hash: path_name_hash(namespace, attunement.path),
            });
        }
    }
    resolve_modifiers(sources, &list, &mut entries)?;
    check_buckets(sources, &list, &entries)?;
    carry_copies(sources, (&list, &display), &mut entries)?;
    check_choices(
        &native::entries(&list)?,
        &entries.iter().map(|entry| entry.entry).collect(),
    )?;
    // Every display row still names a node record, now some of them another subclass's.
    let node_tags = native::node_tags(&display)?;
    for node in &node_tags {
        if sources
            .manager
            .get_entry(*node)
            .is_none_or(|entry| entry.reference != DISPLAY_NODE_CLASS)
        {
            return Err(invalid(format!(
                "Subclass display row names {node}, which is not a node record"
            )));
        }
    }
    // The swapped-in pools and node records load with the authored list and display record.
    let companion = base.companion.for_copy(
        base.tag,
        native::entries(&list)?.iter().map(|entry| entry.pool),
    );
    let display_companion = base
        .display_companion
        .for_copy(base.display_tag, node_tags.iter().map(|tag| tag.0));
    Ok(ResolvedList {
        base_index: base.index,
        hash: fnv1_name_hash(&format!("parhelion/{namespace}/socket_entry_list")),
        template_tag: base.tag,
        display_template_tag: base.display_tag,
        list,
        display,
        companion,
        display_companion,
        entries,
        path_names,
    })
}

/// How errors name an ability or node: "Grenade 1", or "Node 2 of the top attunement".
fn label(place: Place) -> String {
    match place {
        Place::Ability(entry) => AbilitySlot::of_entry(entry)
            .map_or_else(|| place.label(), |slot| slot.entry_label(entry)),
        Place::Node(path, position) => format!(
            "Node {} of the {} attunement",
            position + 1,
            path.label().to_lowercase()
        ),
    }
}

/// The HUD glyph of the Super a subclass on `base` with `abilities` equips, whose row colors its
/// ability tiles: the one its icon takes from another ability, else its own.
pub(crate) fn super_glyph(
    sources: &Sources<'_>,
    base: u32,
    abilities: Option<&SubclassAbilities>,
) -> AuthoringResult<Option<u32>> {
    let choice = abilities.map_or_else(
        || super::SubclassChoice::stock(layout::SUPER, base, layout::SUPER),
        |abilities| abilities.ability(base, layout::SUPER),
    );
    if let Some(EntryIcon::Ability { subclass, entry }) = &choice.edits.icon {
        let owner = StockList::read(sources, *subclass, "Super icon source")?;
        if let Some(glyph) = entry_glyph(sources, &owner, *entry)? {
            return Ok(Some(glyph));
        }
    }
    let source = StockList::read(sources, choice.source, "Super source")?;
    entry_glyph(sources, &source, choice.source_entry)
}

/// The HUD glyph entity `tag` shows while `keys` apply: the last row of its bank naming one that
/// they apply, as dodges and melees name each variant's, else the glyph its energy controller
/// names. `None` for an entity with neither.
fn entity_glyph(sources: &Sources<'_>, tag: TagHash, keys: &[u32]) -> AuthoringResult<Option<u32>> {
    use sundial::package_authoring::{ability_hud, ability_modifier};
    let entity = read_tag(sources.manager, tag, "ability entity")?;
    // A bank without a HUD controller cannot supply a tile glyph. Its native gates may also
    // be outside the row editor's contract, while its graph remains safe to copy unchanged.
    let Some(site) = ability_hud::glyph_site(sources.manager, &entity).map_err(invalid)? else {
        return Ok(None);
    };
    if let Some(bank) = ability_modifier::entity_bank(&entity).map_err(invalid)? {
        let bank = read_tag(sources.manager, TagHash(bank), "ability bank")?;
        if let Some(variant) = ability_hud::variant_glyphs(&bank, keys)
            .map_err(invalid)?
            .last()
        {
            return Ok(Some(variant.glyph));
        }
    }
    Ok(Some(site.key))
}

/// The keys `pool` applies to ability row `row`.
fn row_keys(pool: &[u8], row: u8) -> AuthoringResult<Vec<u32>> {
    Ok(native::modifiers(pool)?
        .into_iter()
        .filter(|(_, applied)| *applied == row)
        .map(|(key, _)| key)
        .collect())
}

/// The HUD glyph of the ability `list`'s entry `entry` equips, through the first equipped row
/// with an entity, as the entry's own keys select it. `None` for an entry that equips no
/// ability, such as a passive node.
fn entry_glyph(sources: &Sources<'_>, list: &StockList, entry: u8) -> AuthoringResult<Option<u32>> {
    let pool = read_tag(
        sources.manager,
        TagHash(list.entry(entry)?.pool),
        "subclass pool",
    )?;
    for row in native::equipped_rows(&pool)? {
        if let Some(entity) = (sources.entity_of)(row)? {
            return entity_glyph(sources, entity, &row_keys(&pool, row)?);
        }
    }
    Ok(None)
}

/// An ability or path node with edits of its own: its source entry's pool with the perks edited,
/// and its node record with the text and icon pointed at the recipe's.
fn author_entry(
    sources: &Sources<'_>,
    (stock, namespace): (&BTreeMap<u32, StockList>, &str),
    place: Place,
    edits: &EntryEdits,
    theme: Option<[u8; 3]>,
    (source, target, from): (&StockList, u8, u8),
) -> AuthoringResult<AuthoredEntry> {
    let (label, key) = (label(place), place.key());
    let entry = source.entry(from)?;
    let pool_template = TagHash(entry.pool);
    if sources
        .manager
        .get_entry(pool_template)
        .is_none_or(|entry| entry.reference != POOL_CLASS)
    {
        return Err(invalid(format!(
            "{label} starts from {pool_template}, which is not an ability pool"
        )));
    }
    if let Some(perk) = edits
        .added_perks
        .iter()
        .find(|perk| usize::from(**perk) >= sources.stock_perks)
    {
        return Err(invalid(format!(
            "{label} adds sandbox perk {perk}, which is not an installed perk"
        )));
    }
    let pool = read_tag(sources.manager, pool_template, "subclass pool")?;
    let pool = native::edit_pool_perks(&pool, &edits.added_perks, &edits.removed_perks)
        .map_err(|error| error.context(label.clone()))?;
    let record_template = native::node_tag(&source.display, entry.display_hash)?;
    let record = read_tag(sources.manager, record_template, "subclass node record")?;
    let text = |field: &str| crate::presentation::text_hash(namespace, &format!("{key}-{field}"));
    let mut record = native::set_node_text(
        &record,
        edits.name.as_ref().map(|_| text("name")),
        edits.description.as_ref().map(|_| text("description")),
    )
    .map_err(|error| error.context(label.clone()))?;
    let mut artwork = None;
    // The HUD draws an ability's glyph, not its node icon, so an icon from another ability brings
    // that ability's glyph too. An icon with no glyph, a passive node's or the entry's own
    // artwork, is drawn in a glyph row of the ability's own from the icon's primary layer.
    let mut icon_glyph = None;
    let mut icon_art = None;
    match &edits.icon {
        Some(EntryIcon::Ability { subclass, entry }) => {
            let owner = stock.get(subclass).ok_or_else(|| {
                validation(format!("{label} takes an icon from an unread subclass"))
            })?;
            let icon_record = read_tag(
                sources.manager,
                owner.node_record(*entry)?,
                "subclass node record",
            )?;
            let icon_row = native::node_icon(&icon_record)?;
            record = native::set_node_icon(&record, icon_row)?;
            icon_glyph = entry_glyph(sources, owner, *entry)?;
            if icon_glyph.is_none() {
                let container = read_tag(
                    sources.manager,
                    (sources.icon_container)(icon_row)?,
                    "icon container",
                )?;
                icon_art = Some(super::hud::Art::Layer(super::hud::primary_layer(
                    sources.manager,
                    &container,
                    false,
                )?));
            }
        }
        Some(EntryIcon::Artwork { artwork: icon }) => {
            artwork = Some(icon.clone());
            icon_art = Some(super::hud::Art::OwnIcon);
        }
        None => {}
    }
    let color = edits.color.or(theme);
    let icon = if artwork.is_some() || color.is_some() {
        let source_row = native::node_icon(&record)?;
        // Foundation records can deliberately have no menu icon. A color applies only where
        // an icon exists, independently of the ability's HUD glyph and its other edits.
        if source_row == u16::MAX && artwork.is_none() {
            None
        } else {
            Some(NodeIcon {
                artwork,
                color,
                source_row,
                source_container: (sources.icon_container)(source_row)
                    .map_err(|error| error.context(format!("{label} has no icon to replace")))?,
            })
        }
    } else {
        None
    };
    let hud_color = edits
        .color
        .or(theme.filter(|_| super::hud::is_super(place)));
    let entity = if edits.keeps_entity()
        && icon_glyph.is_none()
        && icon_art.is_none()
        && hud_color.is_none()
    {
        None
    } else {
        // The page scopes each value to the graph it loaded: the ability's entity, or a graph it
        // spawns. The copy is of the row whose entity a value names, or of the first row with an
        // entity when only spawned graphs or effect colors change. The build refuses a graph the
        // entity does not reach.
        let scopes = edits
            .ability_values
            .iter()
            .filter_map(|value| value.locator.graph_tag.map(|tag| tag.get()))
            .chain(edits.spawn_swaps.iter().map(|swap| swap.graph))
            .collect::<BTreeSet<_>>();
        let mut first = None;
        let mut named = None;
        for row in native::equipped_rows(&pool)? {
            if let Some(source) = (sources.entity_of)(row)? {
                first.get_or_insert((row, source));
                if scopes.contains(&source.0) {
                    named = Some((row, source));
                    break;
                }
            }
        }
        match named.or(first) {
            // A node with no entity of its own shows its icon on the Subclass screen alone.
            None if edits.keeps_entity() => None,
            None => {
                return Err(invalid(format!(
                    "{label} has no entity of its own to change"
                )));
            }
            Some((row, source)) => {
                // The keys the entry still applies to its row choose the glyph the tile shows.
                let hud_keys = row_keys(&pool, row)?
                    .into_iter()
                    .filter(|key| {
                        !edits
                            .removed_modifiers
                            .iter()
                            .any(|stock| (stock.key, stock.row) == (*key, row))
                    })
                    .collect::<Vec<_>>();
                let own_glyph = entity_glyph(sources, source, &hud_keys)?;
                // A row of its own for a color, or for an icon that no glyph row holds.
                let color_row = own_glyph
                    .map(|own| icon_glyph.unwrap_or(own))
                    .filter(|_| hud_color.is_some() || icon_art.is_some())
                    .map(|glyph| -> AuthoringResult<_> {
                        Ok(super::hud::Row {
                            key: crate::presentation::text_hash(
                                namespace,
                                &format!("{key}-hud-color"),
                            ),
                            source: glyph,
                            rgb: hud_color,
                            theme: if super::hud::is_super(place) {
                                theme.or(super::hud::row_color(sources.manager, glyph)?)
                            } else {
                                None
                            },
                            art: icon_art,
                        })
                    })
                    .transpose()?;
                let hud_glyph = match (icon_glyph, own_glyph) {
                    (Some(glyph), Some(own)) if glyph != own => Some(glyph),
                    _ => None,
                };
                let hud_glyph = color_row.map(|row| row.key).or(hud_glyph);
                let attached = attached::resolve(
                    sources,
                    stock,
                    (namespace, &key),
                    source,
                    &edits.attached_abilities,
                    native::node_icon(&record)?,
                )?;
                (!edits.keeps_entity() || hud_glyph.is_some()).then(|| AbilityEntity {
                    row,
                    source,
                    values: edits.ability_values.clone(),
                    palettes: edits.palettes.clone(),
                    tints: edits.tints.clone(),
                    grade: edits.grade,
                    swaps: edits.spawn_swaps.clone(),
                    bank_values: edits.bank_values.clone(),
                    damage_type: edits.damage_mode(),
                    hud_glyph,
                    hud_keys,
                    hud_color: color_row,
                    attached,
                })
            }
        }
    };
    let removed_modifiers = edits
        .removed_modifiers
        .iter()
        .map(|stock| (stock.key, stock.row))
        .collect::<Vec<_>>();
    let stock_modifiers = native::modifiers(&pool)?;
    if let Some((key, row)) = removed_modifiers
        .iter()
        .find(|removed| !stock_modifiers.contains(removed))
    {
        return Err(invalid(format!(
            "{label} applies no key 0x{key:08X} to ability row {row}"
        )));
    }
    let mut requested = edits
        .modifiers
        .iter()
        .map(|modifier| (modifier.target, modifier.effect))
        .collect::<Vec<_>>();
    if edits.extra_charges > 0 {
        requested.push((
            target,
            ModifierEffect::Charges {
                count: edits.extra_charges,
            },
        ));
    }
    if edits.recharge().is_some() {
        requested.push((
            target,
            ModifierEffect::Recharge {
                multiplier_bits: edits.recharge_bits,
            },
        ));
    }
    requested.extend(edits.parameters.iter().map(|parameter| {
        (
            target,
            ModifierEffect::Parameter {
                parameter: parameter.parameter,
                value_bits: parameter.value_bits,
                add: false,
            },
        )
    }));
    Ok(AuthoredEntry {
        entry: target,
        label,
        key,
        pool_template,
        pool,
        record_template,
        record,
        icon,
        custom_perks: edits.custom_perks.clone(),
        requested,
        modifiers: Vec::new(),
        removed_modifiers,
        entity,
        retargeted: Vec::new(),
    })
}

impl AuthoredEntry {
    /// The sandbox perks it compiles to: one per custom perk effect.
    pub(crate) fn effect_count(&self) -> usize {
        self.custom_perks
            .iter()
            .map(|perk| perk.effects.len())
            .sum::<usize>()
    }
}

/// A list's records as the build places them: the list and display record, then each authored
/// entry's pool and node record, each paired with the record after it.
pub(crate) struct PlacedList {
    pub(crate) list_tag: TagHash,
    pub(crate) display_tag: TagHash,
    /// Each pair's template and payload, in host tag order.
    pub(crate) pairs: Vec<[(TagHash, Vec<u8>); 2]>,
}

/// What the build compiled for one authored entry: the finished sandbox perks of its custom
/// perks' effects, the icon row of its artwork, the ability row of its entity copy, and each
/// retargeted stock perk with the private copy that replaces it.
#[derive(Clone, Debug, Default)]
pub(crate) struct CompiledEntry {
    pub(crate) perks: Vec<u16>,
    pub(crate) icon: Option<u16>,
    pub(crate) row: Option<u8>,
    pub(crate) retargeted: Vec<(u16, u16)>,
}

impl ResolvedList {
    /// Records needing a host tag pair: the list and display record, and each authored entry.
    pub(crate) fn tag_pairs(&self) -> usize {
        2 + self.entries.len()
    }

    /// The property rows its modifiers need in stock ability banks, each once.
    pub(crate) fn bank_rows(&self) -> Vec<BankRow> {
        let mut rows = Vec::<BankRow>::new();
        for row in self
            .entries
            .iter()
            .flat_map(|entry| &entry.modifiers)
            .filter_map(|modifier| modifier.bank_row)
        {
            if !rows
                .iter()
                .any(|existing| existing.bank == row.bank && existing.key == row.key)
            {
                rows.push(row);
            }
        }
        rows
    }

    /// The list's records at their host tags. `tag(n)` names the list's n-th tag: the list and
    /// its companion, the display record and its companion, then each authored entry's pool and
    /// node record. `compiled` holds each authored entry's perks and icon, in `entries` order.
    pub(crate) fn place(
        &self,
        tag: &dyn Fn(usize) -> AuthoringResult<TagHash>,
        compiled: &[CompiledEntry],
    ) -> AuthoringResult<PlacedList> {
        if compiled.len() != self.entries.len() {
            return Err(validation(
                "Authored subclass entries were not all compiled",
            ));
        }
        let (list_tag, display_tag) = (tag(0)?, tag(2)?);
        let mut list = self.list.clone();
        let mut display = self.display.clone();
        let entries = native::entries(&list)?;
        // Each copied ability's stock row, its own row, and the entry that equips it. The owner's
        // records move to the copy's row. An entry Dawn can select beside the owner keeps its
        // records for the stock row and takes a duplicate for the copy's, and an alternative of
        // the owner keeps its own, since it holds the stock ability whenever it is selected.
        let moved = self
            .entries
            .iter()
            .zip(compiled)
            .filter_map(|(authored, compiled)| {
                Some((authored.entity.as_ref()?.row, compiled.row?, authored.entry))
            })
            .collect::<Vec<_>>();
        let mut entry_pairs = Vec::with_capacity(self.entries.len());
        let mut pools = Vec::with_capacity(self.entries.len());
        let mut records = Vec::with_capacity(self.entries.len());
        for (index, (authored, compiled)) in self.entries.iter().zip(compiled).enumerate() {
            if compiled.perks.len() != authored.effect_count()
                || compiled.icon.is_some() != authored.icon.is_some()
                || compiled.row.is_some() != authored.entity.is_some()
                || compiled
                    .retargeted
                    .iter()
                    .map(|(stock, _)| *stock)
                    .ne(authored.retargeted.iter().copied())
            {
                return Err(validation(format!(
                    "{} was not compiled with its custom perks, icon and entity",
                    authored.label
                )));
            }
            let (pool_tag, record_tag) = (tag(4 + 2 * index)?, tag(5 + 2 * index)?);
            let mut pool = native::edit_pool_perks(&authored.pool, &compiled.perks, &[])
                .and_then(|pool| native::replace_pool_perks(&pool, &compiled.retargeted))
                .map_err(|error| error.context(authored.label.clone()))?;
            // Stock modifiers it leaves out go first, so a copy does not duplicate them.
            pool = native::edit_pool_modifiers(&pool, &authored.removed_modifiers, &[])
                .map_err(|error| error.context(authored.label.clone()))?;
            let here = entry_at(&entries, authored.entry)?;
            for &(from, to, owner) in &moved {
                let next = if owner == authored.entry {
                    native::move_row(&pool, from, to, true)
                } else if exclusive(here, entry_at(&entries, owner)?) {
                    continue;
                } else {
                    native::duplicate_row(&pool, from, to)
                };
                pool = next.map_err(|error| error.context(authored.label.clone()))?;
            }
            // Its own modifiers apply to the ability each targets: its copy's row when it has one.
            let added = authored
                .modifiers
                .iter()
                .map(|modifier| {
                    let row = moved
                        .iter()
                        .find(|(_, _, owner)| *owner == modifier.target)
                        .map_or(modifier.stock_row, |(_, to, _)| *to);
                    (modifier.key, row)
                })
                .collect::<Vec<_>>();
            pool = native::edit_pool_modifiers(&pool, &[], &added)
                .map_err(|error| error.context(authored.label.clone()))?;
            let record = match compiled.icon {
                Some(icon) => native::set_node_icon(&authored.record, icon)?,
                None => authored.record.clone(),
            };
            let display_hash = entries
                .get(usize::from(authored.entry))
                .ok_or_else(|| invalid(format!("{} is outside its list", authored.label)))?
                .display_hash;
            native::set_entry_pool(&mut list, authored.entry, pool_tag.0)?;
            native::set_node_tag(&mut display, display_hash, record_tag)?;
            pools.push(pool_tag.0);
            records.push(record_tag.0);
            entry_pairs.push([
                (authored.pool_template, pool),
                (authored.record_template, record),
            ]);
        }
        let list_companion = self.companion.payload_for(list_tag, tag(1)?, pools)?;
        let display_companion =
            self.display_companion
                .payload_for(display_tag, tag(3)?, records)?;
        let mut pairs = vec![
            [
                (self.template_tag, list),
                (self.companion.tag, list_companion),
            ],
            [
                (self.display_template_tag, display),
                (self.display_companion.tag, display_companion),
            ],
        ];
        pairs.extend(entry_pairs);
        Ok(PlacedList {
            list_tag,
            display_tag,
            pairs,
        })
    }
}

fn path_name_hash(namespace: &str, path: AttunementPath) -> u32 {
    crate::presentation::text_hash(namespace, &format!("attunement-{}", path.key()))
}

/// The text a subclass recipe authors for its abilities and attunements, by localized hash.
pub(crate) fn authored_text<'a>(
    namespace: &str,
    abilities: Option<&'a SubclassAbilities>,
) -> Vec<(u32, &'a str)> {
    let Some(abilities) = abilities else {
        return Vec::new();
    };
    let field = |key: &str, field: &str| {
        crate::presentation::text_hash(namespace, &format!("{key}-{field}"))
    };
    let mut entries = abilities
        .choices
        .iter()
        .map(|choice| (Place::Ability(choice.entry).key(), &choice.edits))
        .collect::<Vec<_>>();
    let mut text = Vec::new();
    for attunement in &abilities.attunements {
        if let Some(name) = &attunement.name {
            text.push((path_name_hash(namespace, attunement.path), name.as_str()));
        }
        entries.extend(attunement.nodes.iter().map(|node| {
            (
                Place::Node(attunement.path, node.position).key(),
                &node.edits,
            )
        }));
    }
    for (key, edits) in entries {
        for (name, value) in [("name", &edits.name), ("description", &edits.description)] {
            if let Some(value) = value {
                text.push((field(&key, name), value.as_str()));
            }
        }
    }
    text
}

/// A copied ability takes an ability row and an entity of its own. Records elsewhere in the list
/// that apply their keys to its stock row follow the row, and stock perks whose conditions name
/// its stock entity follow the entity through private copies. Each entry holding either takes a
/// pool of its own, as an authored entry with no edits.
fn carry_copies(
    sources: &Sources<'_>,
    (list, display): (&[u8], &[u8]),
    authored: &mut Vec<AuthoredEntry>,
) -> AuthoringResult<()> {
    let list_entries = native::entries(list)?;
    let copied = authored
        .iter()
        .filter_map(|entry| Some((entry.entity.as_ref()?, entry.entry)))
        .map(|(entity, owner)| (entity.row, entity.source.0, owner))
        .collect::<Vec<_>>();
    // Two copies of one row must be for alternatives Dawn never selects together, or both would
    // take the keys meant for the row at once.
    for (index, &(row, _, owner)) in copied.iter().enumerate() {
        for &(other_row, _, other) in &copied[..index] {
            if other_row == row
                && !exclusive(
                    entry_at(&list_entries, owner)?,
                    entry_at(&list_entries, other)?,
                )
            {
                return Err(invalid(
                    "Two abilities with values of their own copy the same ability",
                ));
            }
        }
    }
    if copied.is_empty() {
        return Ok(());
    }
    let mut naming = Naming {
        sources,
        entities: copied.iter().map(|(_, entity, _)| *entity).collect(),
        known: BTreeMap::new(),
    };
    for entry in authored.iter_mut() {
        entry.retargeted = naming
            .perks(&entry.pool)
            .map_err(|error| error.context(entry.label.clone()))?;
    }
    let held = authored
        .iter()
        .map(|entry| entry.entry)
        .collect::<BTreeSet<_>>();
    for (index, &entry) in list_entries.iter().enumerate() {
        let index =
            u8::try_from(index).map_err(|_| invalid("Subclass entry index does not fit 8 bits"))?;
        let pool_template = TagHash(entry.pool);
        if held.contains(&index)
            || sources
                .manager
                .get_entry(pool_template)
                .is_none_or(|entry| entry.reference != POOL_CLASS)
        {
            continue;
        }
        let pool = read_tag(sources.manager, pool_template, "subclass pool")?;
        let label = format!("Subclass entry {index}");
        let mut applies = false;
        for &(row, _, owner) in &copied {
            if !exclusive(entry, entry_at(&list_entries, owner)?) {
                applies |= native::applies_to_row(&pool, row)?;
            }
        }
        let retargeted = naming
            .perks(&pool)
            .map_err(|error| error.context(label.clone()))?;
        if !applies && retargeted.is_empty() {
            continue;
        }
        // Sundial reads an authored list's class from its class node's pool, which must stay
        // the stock one.
        if index == layout::CLASS_BASE {
            return Err(invalid(
                "The class node names an ability with values of its own, and it must stay stock",
            ));
        }
        let record_template = native::node_tag(display, entry.display_hash)?;
        authored.push(AuthoredEntry {
            entry: index,
            label,
            key: format!("entry-{index}"),
            pool_template,
            pool,
            record_template,
            record: read_tag(sources.manager, record_template, "subclass node record")?,
            icon: None,
            custom_perks: Vec::new(),
            requested: Vec::new(),
            modifiers: Vec::new(),
            removed_modifiers: Vec::new(),
            entity: None,
            retargeted,
        });
    }
    Ok(())
}

/// Whether two entries are alternatives Dawn never selects together: one group, different plug
/// sources. A path's nodes share their plug source and are selected together.
fn exclusive(a: Entry, b: Entry) -> bool {
    a.group != NO_GROUP && a.group == b.group && a.plug_source != b.plug_source
}

fn entry_at(entries: &[Entry], entry: u8) -> AuthoringResult<Entry> {
    entries
        .get(usize::from(entry))
        .copied()
        .ok_or_else(|| invalid(format!("Subclass entry {entry} is outside its list")))
}

/// The first ability row the pool of list entry `entry` equips, if it equips one.
fn equipped_row(
    sources: &Sources<'_>,
    entries: &[Entry],
    entry: u8,
) -> AuthoringResult<Option<u8>> {
    let pool = TagHash(entry_at(entries, entry)?.pool);
    if sources
        .manager
        .get_entry(pool)
        .is_none_or(|entry| entry.reference != POOL_CLASS)
    {
        return Ok(None);
    }
    let pool = read_tag(sources.manager, pool, "subclass pool")?;
    Ok(native::equipped_rows(&pool)?.first().copied())
}

/// Resolves each authored entry's requests against the whole list: the stock row of each
/// target's ability, its bank, and the bank row a key needs. Refuses a request the target's
/// bank cannot take.
fn resolve_modifiers(
    sources: &Sources<'_>,
    list: &[u8],
    authored: &mut [AuthoredEntry],
) -> AuthoringResult<()> {
    let entries = native::entries(list)?;
    let mut rows = BTreeMap::<u8, Option<u8>>::new();
    let mut banks = BTreeMap::<u8, Option<AbilityBank>>::new();
    for entry in authored.iter_mut() {
        let mut modifiers = Vec::with_capacity(entry.requested.len());
        for &(target, effect) in &entry.requested {
            let target_label = super::entry_place(target)
                .map_or_else(|| format!("Subclass entry {target}"), label);
            let row = match rows.get(&target) {
                Some(row) => *row,
                None => {
                    let row = equipped_row(sources, &entries, target)?;
                    rows.insert(target, row);
                    row
                }
            };
            let Some(stock_row) = row else {
                return Err(invalid(format!(
                    "{}: {target_label} equips no ability",
                    entry.label
                )));
            };
            let bank = match banks.get(&stock_row) {
                Some(bank) => bank.clone(),
                None => {
                    let bank = (sources.ability_bank)(stock_row)?;
                    banks.insert(stock_row, bank.clone());
                    bank
                }
            };
            let Some(bank) = bank else {
                return Err(invalid(format!(
                    "{}: {target_label} has no ability bank to change",
                    entry.label
                )));
            };
            let (key, bank_row) = match effect {
                ModifierEffect::Key { key } => {
                    if !bank.keys.contains(&key) {
                        return Err(invalid(format!(
                            "{}: {target_label} has no key 0x{key:08X}",
                            entry.label
                        )));
                    }
                    (key, None)
                }
                ModifierEffect::Charges { count } => {
                    if !bank.charges {
                        return Err(invalid(format!(
                            "{}: {target_label} takes no extra charges",
                            entry.label
                        )));
                    }
                    let key = charge_key(count);
                    let modifier = Modifier::Charges(i64::from(count));
                    (key, Some((key, modifier)))
                }
                ModifierEffect::Parameter {
                    parameter,
                    value_bits,
                    add,
                } => {
                    if !bank.parameters.contains(&parameter) {
                        return Err(invalid(format!(
                            "{}: {target_label} has no settable parameter 0x{parameter:08X}",
                            entry.label
                        )));
                    }
                    let key = parameter_key(bank.tag, parameter, value_bits, add);
                    let modifier = Modifier::Parameter {
                        name: parameter,
                        applied: f32::from_bits(value_bits),
                        add,
                    };
                    (key, Some((key, modifier)))
                }
                ModifierEffect::Recharge { multiplier_bits } => {
                    if !bank.recharge {
                        return Err(invalid(format!(
                            "{}: {target_label} takes no recharge change",
                            entry.label
                        )));
                    }
                    let key = recharge_key(multiplier_bits);
                    let modifier = recharge_modifier(f32::from_bits(multiplier_bits));
                    (key, Some((key, modifier)))
                }
            };
            modifiers.push(ResolvedModifier {
                key,
                target,
                stock_row,
                bank_row: bank_row.map(|(key, modifier)| BankRow {
                    bank: bank.tag,
                    key,
                    modifier,
                }),
            });
        }
        entry.modifiers = modifiers;
    }
    Ok(())
}

/// Refuses a list that would file more than 16 keys into one ability's bucket, the most Dawn
/// keeps. For each ability row it counts the most keys the entries can file at once: an entry
/// Dawn selects on its own always, and of each group's alternatives the one that files most.
fn check_buckets(
    sources: &Sources<'_>,
    list: &[u8],
    authored: &[AuthoredEntry],
) -> AuthoringResult<()> {
    let entries = native::entries(list)?;
    let mut always = BTreeMap::<u8, usize>::new();
    let mut groups = BTreeMap::<u8, BTreeMap<u32, BTreeMap<u8, usize>>>::new();
    for (index, entry) in entries.iter().enumerate() {
        let index =
            u8::try_from(index).map_err(|_| invalid("Subclass entry index does not fit 8 bits"))?;
        let counts = match authored.iter().find(|authored| authored.entry == index) {
            Some(authored) => {
                let mut counts = native::keys_per_row(&authored.pool)?;
                for (_, row) in &authored.removed_modifiers {
                    if let Some(count) = counts.get_mut(row) {
                        *count = count.saturating_sub(1);
                    }
                }
                for modifier in &authored.modifiers {
                    *counts.entry(modifier.stock_row).or_default() += 1;
                }
                counts
            }
            None => {
                let pool = TagHash(entry.pool);
                if sources
                    .manager
                    .get_entry(pool)
                    .is_none_or(|entry| entry.reference != POOL_CLASS)
                {
                    continue;
                }
                native::keys_per_row(&read_tag(sources.manager, pool, "subclass pool")?)?
            }
        };
        let target = if entry.group == NO_GROUP {
            &mut always
        } else {
            groups
                .entry(entry.group)
                .or_default()
                .entry(entry.plug_source)
                .or_default()
        };
        for (row, count) in counts {
            *target.entry(row).or_default() += count;
        }
    }
    let mut totals = always;
    for alternatives in groups.values() {
        let mut most = BTreeMap::<u8, usize>::new();
        for counts in alternatives.values() {
            for (row, count) in counts {
                let entry = most.entry(*row).or_default();
                *entry = (*entry).max(*count);
            }
        }
        for (row, count) in most {
            *totals.entry(row).or_default() += count;
        }
    }
    if let Some((row, count)) = totals.iter().find(|(_, count)| **count > BUCKET_KEYS) {
        return Err(invalid(format!(
            "{count} keys would apply to ability row {row} at once, and Dawn keeps {BUCKET_KEYS}"
        )));
    }
    Ok(())
}

/// Which stock perks name a copied ability's entity, each perk read once.
struct Naming<'a, 'b> {
    sources: &'a Sources<'b>,
    entities: BTreeSet<u32>,
    known: BTreeMap<u16, bool>,
}

impl Naming<'_, '_> {
    /// The perks a pool grants whose conditions name a copied ability.
    fn perks(&mut self, pool: &[u8]) -> AuthoringResult<Vec<u16>> {
        let mut named = Vec::new();
        for perk in native::granted_perks(pool)? {
            let names = if let Some(names) = self.known.get(&perk).copied() {
                names
            } else {
                let names = (self.sources.ability_references)(perk)?
                    .iter()
                    .any(|entity| self.entities.contains(entity));
                self.known.insert(perk, names);
                names
            };
            if names {
                named.push(perk);
            }
        }
        Ok(named)
    }
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
    native::set_entry_pool(list, target, source.entry(from)?.pool)?;
    native::set_node_tag(
        display,
        base.entry(target)?.display_hash,
        source.node_record(from)?,
    )
}

/// Sunrise reads a group of up to four entries as alternatives, one of them chosen. Two choices
/// with one pool would offer the same ability twice. An `authored` entry is one of its own even
/// when it starts from another's pool.
fn check_choices(entries: &[Entry], authored: &BTreeSet<u8>) -> AuthoringResult<()> {
    let identity = |entry: u8| {
        (
            entries[usize::from(entry)].pool,
            authored.contains(&entry).then_some(entry),
        )
    };
    for slot in AbilitySlot::ALL {
        let choices = slot
            .entries()
            .iter()
            .map(|&entry| identity(entry))
            .collect::<Vec<_>>();
        if choices.iter().collect::<BTreeSet<_>>().len() != choices.len() {
            return Err(invalid(format!(
                "Two {} choices are the same ability",
                slot.label().to_lowercase()
            )));
        }
    }
    let attunements = AttunementPath::ALL.map(|path| path.entries().map(&identity));
    if attunements.iter().collect::<BTreeSet<_>>().len() != attunements.len() {
        return Err(invalid("Two attunements are the same"));
    }
    Ok(())
}

/// A subclass recipe's settings the talent grid replaces, which it cannot take: rarity, stats,
/// sockets and the rest. Its lore uses the shared writer, since a stock subclass carries the
/// same lore block as a weapon.
pub(crate) fn unsupported_setting(overrides: &crate::WeaponCloneOverrides) -> Option<&'static str> {
    [
        (overrides.rarity.is_some(), "a rarity"),
        (
            !overrides.investment_stats.is_empty()
                || !overrides.removed_investment_stats.is_empty(),
            "stats",
        ),
        (
            !overrides.socket_columns.is_empty() || !overrides.socket_plug_variants.is_empty(),
            "sockets or socket perks",
        ),
        (overrides.render_dye_rows.is_some(), "dye rows"),
        (overrides.badge.is_some(), "a badge"),
    ]
    .into_iter()
    .find_map(|(set, setting)| set.then_some(setting))
}

/// Checks the base and every source against the installed catalog: each is a subclass. A source
/// may be of any class.
pub(crate) fn validate_sources(
    catalog: &InvestmentCatalog,
    base: u32,
    abilities: Option<&SubclassAbilities>,
) -> AuthoringResult<()> {
    let is_subclass = |hash: u32| {
        catalog.gear_donor(hash).is_some_and(|donor| {
            ItemKind::from_bucket_hash(donor.summary.bucket_hash) == Some(ItemKind::Subclass)
        })
    };
    if !is_subclass(base) {
        return Err(invalid(format!(
            "Base item 0x{base:08X} is not an installed subclass"
        )));
    }
    let Some(abilities) = abilities else {
        return Ok(());
    };
    abilities.validate().map_err(invalid)?;
    if let Some(source) = source_subclasses(abilities)
        .into_iter()
        .find(|source| !is_subclass(*source))
    {
        return Err(invalid(format!(
            "Ability source 0x{source:08X} is not an installed subclass"
        )));
    }
    Ok(())
}

/// The class a stock subclass is for, as the pool of its class base entry, which every subclass
/// of one class shares and an authored list keeps.
pub(crate) fn class_key(sources: &Sources<'_>, base: u32) -> AuthoringResult<u32> {
    Ok(StockList::read(sources, base, "Base subclass")?
        .entry(layout::CLASS_BASE)?
        .pool)
}

/// Checks a project against the limit Sunrise reads subclasses with: a list with a super lane
/// for at most 32 subclasses. How their classes group is `grouping`'s, which the build applies
/// before its items take their indices.
pub(crate) fn validate_project(sources: &Sources<'_>, lists: usize) -> AuthoringResult<()> {
    if sources.tables.super_lane_lists + lists > SUNRISE_SUPER_LANE_LIST_CAPACITY {
        return Err(invalid(format!(
            "A build can give at most {} subclasses abilities of their own",
            SUNRISE_SUPER_LANE_LIST_CAPACITY - sources.tables.super_lane_lists
        )));
    }
    Ok(())
}
