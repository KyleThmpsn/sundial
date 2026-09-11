//! Item socket decoding, option pools, labels, and catalyst state.

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
};

use serde::{Deserialize, Serialize};

use crate::{
    hash::{format_hash_hex, parse_hash_hex},
    investment_schema::{
        ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET, ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS,
        ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE, ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET,
        ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
    },
    package_payload::{array_at, u16_at, u64_at},
};

use super::{super::Catalog, ItemDef, damage::is_weapon_bucket};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::catalog) enum GearKind {
    Weapon,
    Armor,
    Other(u64),
}

type SocketOptionsByType = HashMap<u16, Vec<u64>>;
type SocketOptionsByGearType = HashMap<String, SocketOptionsByType>;

fn item_gear_type(item: &ItemDef) -> Cow<'_, str> {
    let type_name = item.type_name.trim();
    if type_name.is_empty() {
        Cow::Owned(format!("Bucket {}", item.bucket_hash))
    } else {
        Cow::Borrowed(type_name)
    }
}

const COSMETIC_SOCKET_TYPES_WITHOUT_MARKERS: [u16; 1] = [746];

pub(in crate::catalog) const fn gear_kind(bucket_hash: u64) -> GearKind {
    if is_weapon_bucket(bucket_hash) {
        GearKind::Weapon
    } else if matches!(
        bucket_hash,
        3_448_274_439 | 3_551_918_588 | 14_239_492 | 20_886_954 | 1_585_787_867
    ) {
        GearKind::Armor
    } else {
        GearKind::Other(bucket_hash)
    }
}

fn cosmetic_marker(name: &str) -> bool {
    matches!(
        name,
        "Default Shader" | "Default Ornament" | "Tracker Disabled"
    ) || name.contains("Kill Tracker")
}

fn tracker_marker(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    name == "tracker disabled" || name.contains("kill tracker")
}

pub(in crate::catalog) fn build_gear_type_options(
    items: &[ItemDef],
    plug_pools: &[Vec<u64>],
    names: &HashMap<u64, String>,
) -> (HashMap<GearKind, Vec<u64>>, HashSet<u32>) {
    let mut cosmetic_pools = plug_pools
        .iter()
        .enumerate()
        .filter(|(_, pool)| {
            pool.iter()
                .filter_map(|hash| names.get(hash))
                .any(|name| cosmetic_marker(name))
        })
        .filter_map(|(index, _)| u32::try_from(index).ok())
        .collect::<HashSet<_>>();
    for socket in items.iter().flat_map(|item| &item.sockets) {
        if COSMETIC_SOCKET_TYPES_WITHOUT_MARKERS.contains(&socket.socket_type) {
            cosmetic_pools.insert(socket.pool);
        }
    }
    let cosmetic_hashes = cosmetic_pools
        .iter()
        .filter_map(|pool| plug_pools.get(*pool as usize))
        .flatten()
        .copied()
        .collect::<HashSet<_>>();

    let mut options = HashMap::<GearKind, Vec<u64>>::new();
    for item in items {
        let gear_kind = gear_kind(item.bucket_hash);
        for socket in &item.sockets {
            if let Some(pool) = plug_pools.get(socket.pool as usize) {
                options
                    .entry(gear_kind)
                    .or_default()
                    .extend(pool.iter().filter(|hash| !cosmetic_hashes.contains(hash)));
            }
        }
    }
    for values in options.values_mut() {
        sort_plug_options(values, names);
    }
    (options, cosmetic_pools)
}

pub(in crate::catalog) fn build_socket_type_options(
    items: &[ItemDef],
    plug_pools: &[Vec<u64>],
    names: &HashMap<u64, String>,
) -> (SocketOptionsByType, SocketOptionsByGearType) {
    let mut socket_type_options = SocketOptionsByType::new();
    let mut socket_and_gear_type_options = SocketOptionsByGearType::new();
    for item in items {
        let gear_type = item_gear_type(item).into_owned();
        for socket in &item.sockets {
            if let Some(pool) = plug_pools.get(socket.pool as usize) {
                socket_type_options
                    .entry(socket.socket_type)
                    .or_default()
                    .extend(pool.iter().copied());
                socket_and_gear_type_options
                    .entry(gear_type.clone())
                    .or_default()
                    .entry(socket.socket_type)
                    .or_default()
                    .extend(pool.iter().copied());
            }
        }
    }
    for options in socket_type_options.values_mut() {
        sort_plug_options(options, names);
    }
    // Shader plugs are cosmetic and shared across weapon families. Exotic-only
    // families may have no stock shader socket, but an authored replacement can
    // use the installed shader category without admitting other families' traits.
    if let Some(shaders) = socket_type_options
        .get(&180)
        .filter(|pool| !pool.is_empty())
    {
        for item in items
            .iter()
            .filter(|item| is_weapon_bucket(item.bucket_hash))
        {
            socket_and_gear_type_options
                .entry(item_gear_type(item).into_owned())
                .or_default()
                .entry(180)
                .or_insert_with(|| shaders.clone());
        }
    }
    for options_by_socket in socket_and_gear_type_options.values_mut() {
        for options in options_by_socket.values_mut() {
            sort_plug_options(options, names);
        }
    }
    (socket_type_options, socket_and_gear_type_options)
}

pub(in crate::catalog) use crate::investment_schema::ITEM_ORDINARY_SOCKET_ROW_CLASS as ORDINARY_SOCKET_CLASS;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct SocketDef {
    pub socket_type: u16,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub pool: u32,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub allowed: Vec<u64>,
    #[serde(default)]
    pub sources: Vec<SocketOptionSource>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SocketOptionSource {
    pub kind: SocketOptionSourceKind,
    pub pool: u32,
    pub valid: bool,
    /// Members in their original package order.
    ///
    /// `pool` points at a normalized, name-sorted picker pool. Keeping this separate prevents
    /// catalog presentation ordering from erasing the authored order of embedded and shared
    /// package lists.
    #[serde(default)]
    pub ordered_members: Vec<u64>,
    #[serde(default)]
    #[serde(skip_serializing)]
    pub allowed: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum SocketOptionSourceKind {
    Embedded,
    ReusableSet { index: u16 },
    RandomizedSet { index: u16 },
    CategoryExpansion { category_hash: u32 },
    SyntheticTracker,
}

impl SocketDef {
    pub(crate) fn display_label(&self, index: usize) -> String {
        if self.label.is_empty() {
            format!("Socket {}", index + 1)
        } else {
            format!("{}. {}", index + 1, self.label)
        }
    }

    /// Returns the complete native embedded list in its authored package order.
    ///
    /// This is deliberately distinct from the socket option pool, which is normalized and
    /// sorted for browsing. Invalid or partially decoded lists are not safe authoring seeds.
    pub(crate) fn ordered_embedded_choices(&self) -> &[u64] {
        self.sources
            .iter()
            .find(|source| source.valid && source.kind == SocketOptionSourceKind::Embedded)
            .map_or(&[], |source| source.ordered_members.as_slice())
    }

    pub(crate) fn reusable_set_index(&self) -> Option<u16> {
        self.sources.iter().find_map(|source| match source.kind {
            SocketOptionSourceKind::ReusableSet { index } => Some(index),
            _ => None,
        })
    }

    pub(crate) fn randomized_set_index(&self) -> Option<u16> {
        self.sources.iter().find_map(|source| match source.kind {
            SocketOptionSourceKind::RandomizedSet { index } => Some(index),
            _ => None,
        })
    }
}

impl SocketOptionSource {
    pub(crate) fn label(&self) -> String {
        match self.kind {
            SocketOptionSourceKind::Embedded => "Embedded Plug List".into(),
            SocketOptionSourceKind::ReusableSet { index } => format!("Reusable Plug Set #{index}"),
            SocketOptionSourceKind::RandomizedSet { index } => {
                format!("Randomized Plug Set #{index}")
            }
            SocketOptionSourceKind::CategoryExpansion { category_hash } => {
                format!("Category expansion 0x{category_hash:08X}")
            }
            SocketOptionSourceKind::SyntheticTracker => "Derived Tracker Set".into(),
        }
    }

    pub(crate) const fn origin_label(&self) -> &'static str {
        match self.kind {
            SocketOptionSourceKind::Embedded
            | SocketOptionSourceKind::ReusableSet { .. }
            | SocketOptionSourceKind::RandomizedSet { .. } => "Package",
            SocketOptionSourceKind::CategoryExpansion { .. } => "Derived from Category",
            SocketOptionSourceKind::SyntheticTracker => "Derived from Item Definitions",
        }
    }
}

impl Catalog {
    pub(crate) fn socket_options(&self, socket: &SocketDef) -> &[u64] {
        self.plug_pools
            .get(socket.pool as usize)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn socket_source_options(&self, source: &SocketOptionSource) -> &[u64] {
        self.plug_pools
            .get(source.pool as usize)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn socket_type_options(&self, socket_type: u16) -> &[u64] {
        self.socket_type_options
            .get(&socket_type)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn socket_and_gear_type_options(
        &self,
        item: &ItemDef,
        socket_index: usize,
    ) -> &[u64] {
        let Some(socket) = item.sockets.get(socket_index) else {
            return &[];
        };
        self.socket_and_gear_type_options_for_type(item, socket.socket_type)
    }

    pub(crate) fn socket_and_gear_type_options_for_type(
        &self,
        item: &ItemDef,
        socket_type: u16,
    ) -> &[u64] {
        self.socket_and_gear_type_options
            .get(item_gear_type(item).as_ref())
            .and_then(|options| options.get(&socket_type))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn socket_type_is_known_for_item(&self, item: &ItemDef, socket_type: u16) -> bool {
        self.socket_and_gear_type_options
            .get(item_gear_type(item).as_ref())
            .is_some_and(|options| options.contains_key(&socket_type))
    }

    pub(crate) fn socket_type_label_for_item(&self, item: &ItemDef, socket_type: u16) -> String {
        infer_socket_label(
            socket_type,
            None,
            self.socket_and_gear_type_options_for_type(item, socket_type),
            &self.names,
            &self.type_names,
        )
    }

    pub(crate) fn socket_and_gear_type_option_counts(&self, item: &ItemDef) -> Vec<(u16, usize)> {
        let mut rows = self
            .socket_and_gear_type_options
            .get(item_gear_type(item).as_ref())
            .into_iter()
            .flat_map(|options| options.iter())
            .map(|(&socket_type, options)| (socket_type, options.len()))
            .collect::<Vec<_>>();
        rows.sort_unstable_by_key(|(socket_type, _)| *socket_type);
        rows
    }

    pub(crate) fn gear_type_options(&self, item: &ItemDef, socket_index: usize) -> Vec<u64> {
        let Some(socket) = item.sockets.get(socket_index) else {
            return Vec::new();
        };
        let mut options = self
            .gear_type_options
            .get(&gear_kind(item.bucket_hash))
            .cloned()
            .unwrap_or_default();
        if self.cosmetic_socket_pools.contains(&socket.pool) {
            options.extend(self.socket_type_options(socket.socket_type));
            sort_plug_options(&mut options, &self.names);
        }
        options
    }

    pub(crate) fn gear_type_options_for_type(&self, item: &ItemDef, socket_type: u16) -> Vec<u64> {
        let mut options = self
            .gear_type_options
            .get(&gear_kind(item.bucket_hash))
            .cloned()
            .unwrap_or_default();
        if self.cosmetic_socket_types.contains(&socket_type) {
            options.extend(self.socket_type_options(socket_type));
            sort_plug_options(&mut options, &self.names);
        }
        options
    }

    pub(crate) fn all_plug_options(&self) -> &[u64] {
        &self.all_plug_options
    }

    /// Returns whether a hash is installed as either a selectable plug or an
    /// item's native socket default.
    pub(crate) fn contains_plug(&self, hash: u64) -> bool {
        self.plug_hashes.contains(&hash)
    }
}

pub(in crate::catalog) fn format_plug_label(name: &str, hash: u64, include_hash: bool) -> String {
    if include_hash {
        format!("{name}  ({})", format_hash_hex(hash))
    } else {
        name.to_owned()
    }
}

pub(in crate::catalog) fn sort_plug_options(options: &mut Vec<u64>, names: &HashMap<u64, String>) {
    options.sort_unstable();
    options.dedup();
    options.sort_by_cached_key(|hash| {
        let name = names.get(hash).map_or("", String::as_str).trim();
        (name.is_empty(), name.to_lowercase(), *hash)
    });
}

pub(in crate::catalog) fn intern_socket_pools(
    items: &mut [ItemDef],
    names: &HashMap<u64, String>,
) -> Result<Vec<Vec<u64>>, String> {
    let mut pools = vec![Vec::new()];
    let mut indices = HashMap::<Vec<u64>, u32>::new();
    indices.insert(Vec::new(), 0);
    for item in items.iter_mut() {
        for socket in &mut item.sockets {
            socket.pool = intern_socket_pool(&mut socket.allowed, names, &mut pools, &mut indices)?;
        }
    }
    for item in items.iter_mut() {
        for source in item
            .sockets
            .iter_mut()
            .flat_map(|socket| &mut socket.sources)
        {
            source.pool = intern_socket_pool(&mut source.allowed, names, &mut pools, &mut indices)?;
        }
    }
    Ok(pools)
}

fn intern_socket_pool(
    values: &mut Vec<u64>,
    names: &HashMap<u64, String>,
    pools: &mut Vec<Vec<u64>>,
    indices: &mut HashMap<Vec<u64>, u32>,
) -> Result<u32, String> {
    sort_plug_options(values, names);
    if let Some(index) = indices.get(values) {
        values.clear();
        return Ok(*index);
    }
    let index = u32::try_from(pools.len())
        .map_err(|_| "The catalog contains too many distinct socket pools")?;
    let values = std::mem::take(values);
    indices.insert(values.clone(), index);
    pools.push(values);
    Ok(index)
}

pub(in crate::catalog) fn build_socket_choices(
    items: &mut [ItemDef],
    plug_category_by_hash: &HashMap<u64, u32>,
    plug_category_items: &HashMap<u32, Vec<u64>>,
    names: &HashMap<u64, String>,
    type_names: &mut HashMap<u64, String>,
) {
    for item in items.iter_mut() {
        for (socket_index, socket) in item.sockets.iter_mut().enumerate() {
            let mut seeds = socket.allowed.clone();
            if let Some(Some(default)) = item.default_plugs.get(socket_index) {
                if let Some(hash) = parse_hash_hex(default) {
                    seeds.push(hash);
                }
            }
            let mut expanded_categories = BTreeSet::new();
            for seed in seeds {
                let Some(category) = plug_category_by_hash.get(&seed) else {
                    continue;
                };
                if matches!(*category, 0xB134_761E | 0x8772_7F34 | 0x6C86_3692) {
                    expanded_categories.insert(*category);
                }
            }
            for category_hash in expanded_categories {
                let Some(category_items) = plug_category_items.get(&category_hash) else {
                    continue;
                };
                socket.allowed.extend(category_items.iter().copied());
                socket.sources.push(SocketOptionSource {
                    kind: SocketOptionSourceKind::CategoryExpansion { category_hash },
                    pool: 0,
                    valid: true,
                    ordered_members: Vec::new(),
                    allowed: category_items.clone(),
                });
            }
            socket.allowed.sort_unstable();
            socket.allowed.dedup();
        }
    }
    infer_socket_plug_types(items, names, type_names);
    let mut tracker_plugs = names
        .iter()
        .filter_map(|(&hash, name)| tracker_marker(name).then_some(hash))
        .collect::<Vec<_>>();
    tracker_plugs.extend(items.iter().flat_map(|item| {
        item.sockets
            .iter()
            .enumerate()
            .filter(|(_, socket)| socket.socket_type == 518)
            .filter_map(|(socket_index, _)| {
                item.default_plugs
                    .get(socket_index)
                    .and_then(Option::as_deref)
                    .and_then(parse_hash_hex)
            })
    }));
    tracker_plugs.sort_unstable();
    tracker_plugs.dedup();
    for item in items.iter_mut() {
        for socket in &mut item.sockets {
            // Kill/Crucible tracker sockets use a small synthetic plug set
            // keyed by socket type rather than a package plug-set row. Derive its
            // members from installed item definitions and native defaults.
            if socket.socket_type == 518 && !tracker_plugs.is_empty() {
                socket.allowed.extend(&tracker_plugs);
                socket.sources.push(SocketOptionSource {
                    kind: SocketOptionSourceKind::SyntheticTracker,
                    pool: 0,
                    valid: true,
                    ordered_members: Vec::new(),
                    allowed: tracker_plugs.clone(),
                });
                socket.allowed.sort_unstable();
                socket.allowed.dedup();
            }
        }
    }
    for item in items.iter_mut() {
        for (socket_index, socket) in item.sockets.iter_mut().enumerate() {
            let default = item
                .default_plugs
                .get(socket_index)
                .and_then(Option::as_deref)
                .and_then(parse_hash_hex);
            socket.label = infer_socket_label(
                socket.socket_type,
                default,
                &socket.allowed,
                names,
                type_names,
            );
        }
    }
}

pub(in crate::catalog) fn infer_socket_label(
    socket_type: u16,
    default: Option<u64>,
    allowed: &[u64],
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> String {
    if let Some(label) = verified_socket_label(socket_type) {
        return label.into();
    }

    if let Some(label) = default.and_then(|hash| socket_label_for_plug(hash, names, type_names)) {
        return label;
    }

    let mut counts = HashMap::<String, usize>::new();
    for &hash in allowed {
        if let Some(label) = socket_label_for_plug(hash, names, type_names) {
            *counts.entry(label).or_default() += 1;
        }
    }
    if let Some((label, count)) =
        counts
            .iter()
            .max_by(|(left_label, left_count), (right_label, right_count)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right_label.cmp(left_label))
            })
        && count.saturating_mul(2) >= counts.values().sum()
    {
        return label.clone();
    }

    String::new()
}

fn verified_socket_label(socket_type: u16) -> Option<&'static str> {
    match socket_type {
        29..=43 => Some("Armor Masterwork"),
        // Shadowkeep's public manifest categorizes these as GHOST SHELL PERKS;
        // individual perk definitions use the overly generic type "Intrinsic".
        51 => Some("Ghost Perk"),
        62 => Some("Sparrow Perk"),
        483 => Some("Weapon Masterwork"),
        518 => Some("Kill Tracker"),
        520 => Some("Armor Tier"),
        676 => Some("Stat Allocation"),
        678 | 679 => Some("Armor Energy Upgrade"),
        760 | 761 => Some("Top Stat Allocation"),
        762 | 763 => Some("Bottom Stat Allocation"),
        _ => None,
    }
}

fn inferred_plug_type_for_socket(socket_type: u16) -> Option<&'static str> {
    match socket_type {
        29..=43 => Some("Armor Masterwork"),
        51 => Some("Ghost Perk"),
        520 => Some("Armor Tier"),
        678 | 679 => Some("Armor Energy"),
        760 | 761 => Some("Top Stat Allocation"),
        762 | 763 => Some("Bottom Stat Allocation"),
        _ => None,
    }
}

pub(in crate::catalog) fn infer_socket_plug_types(
    items: &[ItemDef],
    names: &HashMap<u64, String>,
    type_names: &mut HashMap<u64, String>,
) {
    for item in items {
        for (socket_index, socket) in item.sockets.iter().enumerate() {
            let Some(type_name) = inferred_plug_type_for_socket(socket.socket_type) else {
                continue;
            };
            let default = item
                .default_plugs
                .get(socket_index)
                .and_then(Option::as_deref)
                .and_then(parse_hash_hex);
            for hash in socket.allowed.iter().copied().chain(default) {
                if socket.socket_type == 51 {
                    // "Intrinsic" is not useful here and is inconsistent with the
                    // manifest's Ghost Shell Perks socket category.
                    type_names.insert(hash, type_name.into());
                } else {
                    type_names.entry(hash).or_insert_with(|| type_name.into());
                }
            }
        }
    }

    for (&hash, name) in names {
        if name.trim().eq_ignore_ascii_case("Empty Mod Socket") {
            type_names.entry(hash).or_insert_with(|| "Armor Mod".into());
        }
    }
}

pub(in crate::catalog) fn socket_label_for_plug(
    hash: u64,
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> Option<String> {
    let name = names.get(&hash).map_or("", String::as_str).trim();
    let lower_name = name.to_ascii_lowercase();
    if lower_name == "default shader" {
        return Some("Shader".into());
    }
    if lower_name.contains("ornament") {
        return Some("Ornament".into());
    }
    if lower_name == "no projection" {
        return Some("Ghost Projection".into());
    }
    if lower_name == "default effect" {
        return Some("Transmat Effect".into());
    }
    if lower_name.contains("tracker") {
        return Some("Kill Tracker".into());
    }
    if lower_name.contains("catalyst") {
        return Some("Catalyst".into());
    }
    if lower_name.starts_with("tier ") && lower_name.ends_with(" weapon")
        || lower_name.starts_with("masterwork:")
    {
        return Some("Weapon Masterwork".into());
    }
    if lower_name.starts_with("tier ") && lower_name.ends_with(" armor") {
        return Some("Armor Tier".into());
    }
    if matches!(lower_name.as_str(), "upgrade armor" | "change energy type") {
        return Some("Armor Energy Upgrade".into());
    }

    let type_name = type_names.get(&hash).map_or("", String::as_str).trim();
    if type_name.is_empty() || type_name == "Restore Defaults" {
        return None;
    }
    if type_name.contains("Ornament") {
        Some("Ornament".into())
    } else {
        Some(type_name.into())
    }
}

pub(in crate::catalog) fn socket_package_sources(
    item: &[u8],
    socket_base: usize,
    item_hashes: &[u64],
    plug_set_table: &[u8],
) -> Vec<SocketOptionSource> {
    const PLUG_SET_TABLE_DESCRIPTOR: usize = 8;
    const PLUG_SET_ROW_STRIDE: usize = 24;
    const PLUG_SET_MEMBERS_OFFSET: usize = 8;

    let mut sources = Vec::new();

    // Small reusable lists, such as a fixed shader choice, are embedded in
    // the inventory item definition.
    let Some(embedded_descriptor) =
        socket_base.checked_add(ITEM_ORDINARY_SOCKET_EMBEDDED_PLUGS_OFFSET)
    else {
        return sources;
    };
    if u64_at(item, embedded_descriptor).is_ok_and(|count| count != 0) {
        let decoded = plug_member_hashes(item, embedded_descriptor, item_hashes);
        let ordered_members = decoded
            .as_ref()
            .map_or_else(Vec::new, |members| members.values.clone());
        sources.push(SocketOptionSource {
            kind: SocketOptionSourceKind::Embedded,
            pool: 0,
            valid: decoded.as_ref().is_some_and(|members| members.complete),
            ordered_members,
            allowed: decoded.map_or_else(Vec::new, |members| members.values),
        });
    }

    // Larger option pools use the shared DestinyPlugSetDefinition table.
    // Reusable and randomized plug sets have separate row indices at +12 and
    // +32 respectively.
    let sets = array_at(plug_set_table, PLUG_SET_TABLE_DESCRIPTOR)
        .ok()
        .filter(|(_, rows, _)| *rows <= plug_set_table.len());
    for (set_offset, randomized) in [
        (ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET, false),
        (ITEM_ORDINARY_SOCKET_RANDOMIZED_PLUG_SET_OFFSET, true),
    ] {
        let Some(set_index_offset) = socket_base.checked_add(set_offset) else {
            continue;
        };
        let Ok(set_index) = u16_at(item, set_index_offset) else {
            continue;
        };
        if set_index == u16::MAX {
            continue;
        }
        let kind = if randomized {
            SocketOptionSourceKind::RandomizedSet { index: set_index }
        } else {
            SocketOptionSourceKind::ReusableSet { index: set_index }
        };
        let decoded = sets.and_then(|(set_count, set_rows, _)| {
            if usize::from(set_index) >= set_count {
                return None;
            }
            let descriptor = set_rows
                .checked_add(usize::from(set_index).checked_mul(PLUG_SET_ROW_STRIDE)?)?
                .checked_add(PLUG_SET_MEMBERS_OFFSET)?;
            match u64_at(plug_set_table, descriptor).ok()? {
                0 => Some(DecodedPlugMembers {
                    values: Vec::new(),
                    complete: true,
                }),
                _ => plug_member_hashes(plug_set_table, descriptor, item_hashes),
            }
        });
        let ordered_members = decoded
            .as_ref()
            .map_or_else(Vec::new, |members| members.values.clone());
        sources.push(SocketOptionSource {
            kind,
            pool: 0,
            valid: decoded.as_ref().is_some_and(|members| members.complete),
            ordered_members,
            allowed: decoded.map_or_else(Vec::new, |members| members.values),
        });
    }
    sources
}

struct DecodedPlugMembers {
    values: Vec<u64>,
    complete: bool,
}

fn plug_member_hashes(
    data: &[u8],
    descriptor: usize,
    item_hashes: &[u64],
) -> Option<DecodedPlugMembers> {
    const MAXIMUM_PLUG_MEMBER_COUNT: usize = 65_535;

    let (count, rows, row_class) = array_at(data, descriptor).ok()?;
    if row_class != ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_CLASS {
        return Some(DecodedPlugMembers {
            values: Vec::new(),
            complete: false,
        });
    }
    if rows > data.len() {
        return Some(DecodedPlugMembers {
            values: Vec::new(),
            complete: false,
        });
    }
    let mut values = Vec::new();
    let mut complete = count <= MAXIMUM_PLUG_MEMBER_COUNT;
    for index in 0..count.min(MAXIMUM_PLUG_MEMBER_COUNT) {
        let Some(row) = index
            .checked_mul(ITEM_ORDINARY_SOCKET_PLUG_MEMBER_ROW_SIZE)
            .and_then(|offset| rows.checked_add(offset))
        else {
            complete = false;
            break;
        };
        let Ok(item_index) = u16_at(data, row) else {
            complete = false;
            break;
        };
        let Some(hash) = item_hashes.get(usize::from(item_index)).copied() else {
            complete = false;
            continue;
        };
        values.push(hash);
    }
    Some(DecodedPlugMembers { values, complete })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plug_member_array(row_class: u32, item_index: u16, reserved: [u8; 6]) -> Vec<u8> {
        let mut data = vec![0_u8; 72];
        data[0..8].copy_from_slice(&1_u64.to_le_bytes());
        data[8..16].copy_from_slice(&16_i64.to_le_bytes());
        data[24..32].copy_from_slice(&1_u64.to_le_bytes());
        data[32..36].copy_from_slice(&row_class.to_le_bytes());
        data[40..42].copy_from_slice(&item_index.to_le_bytes());
        data[42..48].copy_from_slice(&reserved);
        data
    }

    #[test]
    fn plug_member_indices_are_u16_and_require_the_native_row_class() {
        let item_hashes = [0x1111_u64, 0x2222];
        let data = plug_member_array(0x8080_2E03, 1, [0xA5; 6]);
        let decoded = plug_member_hashes(&data, 0, &item_hashes).unwrap();
        assert!(decoded.complete);
        assert_eq!(decoded.values, vec![0x2222]);

        let wrong_class = plug_member_array(0xDEAD_BEEF, 1, [0; 6]);
        let decoded = plug_member_hashes(&wrong_class, 0, &item_hashes).unwrap();
        assert!(!decoded.complete);
        assert!(decoded.values.is_empty());
    }

    fn item(bucket_hash: u64, sockets: Vec<SocketDef>) -> ItemDef {
        ItemDef {
            hash: bucket_hash,
            name: String::new(),
            type_name: String::new(),
            bucket_hash,
            class_type: 3,
            default_plugs: Vec::new(),
            sockets,
            abilities: Default::default(),
        }
    }

    fn typed_item(bucket_hash: u64, type_name: &str, sockets: Vec<SocketDef>) -> ItemDef {
        let mut item = item(bucket_hash, sockets);
        item.type_name = type_name.to_owned();
        item
    }

    #[test]
    fn gear_type_options_pool_live_socket_data_without_cosmetic_plugs() {
        let pools = vec![vec![1, 2], vec![10, 11], vec![3], vec![20]];
        let names = HashMap::from([
            (1, "Arrowhead Brake".to_owned()),
            (2, "Rampage".to_owned()),
            (3, "Kill Clip".to_owned()),
            (10, "Default Shader".to_owned()),
            (11, "Golden Trace Shader".to_owned()),
            (20, "Mobility Mod".to_owned()),
        ]);
        let items = vec![
            item(
                1_498_876_634,
                vec![
                    SocketDef {
                        socket_type: 100,
                        pool: 0,
                        ..SocketDef::default()
                    },
                    SocketDef {
                        socket_type: 200,
                        pool: 1,
                        ..SocketDef::default()
                    },
                ],
            ),
            item(
                2_465_295_065,
                vec![SocketDef {
                    socket_type: 101,
                    pool: 2,
                    ..SocketDef::default()
                }],
            ),
            item(
                3_448_274_439,
                vec![SocketDef {
                    socket_type: 300,
                    pool: 3,
                    ..SocketDef::default()
                }],
            ),
        ];

        let (options, cosmetic_pools) = build_gear_type_options(&items, &pools, &names);

        assert_eq!(options.get(&GearKind::Weapon).unwrap(), &vec![1, 3, 2]);
        assert_eq!(options.get(&GearKind::Armor).unwrap(), &vec![20]);
        assert!(cosmetic_pools.contains(&1));
        assert!(
            !options
                .get(&GearKind::Weapon)
                .unwrap()
                .iter()
                .any(|hash| matches!(hash, 10 | 11))
        );
    }

    #[test]
    fn socket_and_gear_type_options_require_both_dimensions_to_match() {
        let pools = vec![vec![1], vec![2], vec![3], vec![4], vec![5]];
        let names = HashMap::from([
            (1, "Weapon A".to_owned()),
            (2, "Armor A".to_owned()),
            (3, "Weapon B".to_owned()),
            (4, "Weapon C".to_owned()),
            (5, "Weapon D".to_owned()),
        ]);
        let items = vec![
            typed_item(
                1_498_876_634,
                "Hand Cannon",
                vec![SocketDef {
                    socket_type: 100,
                    pool: 0,
                    ..SocketDef::default()
                }],
            ),
            typed_item(
                3_448_274_439,
                "Helmet",
                vec![SocketDef {
                    socket_type: 100,
                    pool: 1,
                    ..SocketDef::default()
                }],
            ),
            typed_item(
                2_465_295_065,
                "Auto Rifle",
                vec![
                    SocketDef {
                        socket_type: 200,
                        pool: 2,
                        ..SocketDef::default()
                    },
                    SocketDef {
                        socket_type: 100,
                        pool: 3,
                        ..SocketDef::default()
                    },
                ],
            ),
            typed_item(
                2_465_295_065,
                "Hand Cannon",
                vec![SocketDef {
                    socket_type: 100,
                    pool: 4,
                    ..SocketDef::default()
                }],
            ),
        ];

        let (socket_options, socket_and_gear_options) =
            build_socket_type_options(&items, &pools, &names);

        assert_eq!(socket_options.get(&100).unwrap(), &vec![2, 1, 4, 5]);
        assert_eq!(
            socket_and_gear_options
                .get("Hand Cannon")
                .and_then(|options| options.get(&100))
                .unwrap(),
            &vec![1, 5]
        );
        assert_eq!(
            socket_and_gear_options
                .get("Helmet")
                .and_then(|options| options.get(&100))
                .unwrap(),
            &vec![2]
        );
        assert_eq!(
            socket_and_gear_options
                .get("Auto Rifle")
                .and_then(|options| options.get(&200))
                .unwrap(),
            &vec![3]
        );
        assert_eq!(
            socket_and_gear_options
                .get("Auto Rifle")
                .and_then(|options| options.get(&100))
                .unwrap(),
            &vec![4]
        );
    }

    #[test]
    fn installed_shaders_are_available_to_exotic_only_weapon_families() {
        let pools = vec![vec![11], vec![22]];
        let names = HashMap::from([(11, "Shader".into()), (22, "Trait".into())]);
        let items = vec![
            typed_item(
                1_498_876_634,
                "Auto Rifle",
                vec![
                    SocketDef {
                        socket_type: 180,
                        pool: 0,
                        ..Default::default()
                    },
                    SocketDef {
                        socket_type: 92,
                        pool: 1,
                        ..Default::default()
                    },
                ],
            ),
            typed_item(2_465_295_065, "Trace Rifle", vec![]),
            typed_item(3_448_274_439, "Helmet", vec![]),
        ];
        let (_, families) = build_socket_type_options(&items, &pools, &names);
        assert_eq!(families["Trace Rifle"][&180], vec![11]);
        assert!(!families["Trace Rifle"].contains_key(&92));
        assert!(!families.contains_key("Helmet"));
        let (_, without_shader) = build_socket_type_options(&items[1..], &pools, &names);
        assert!(!without_shader.contains_key("Trace Rifle"));
    }

    #[test]
    fn package_sources_keep_embedded_reusable_and_randomized_members_separate() {
        let mut item = vec![0_u8; 240];
        write_u16(&mut item, 12, 0);
        write_u16(&mut item, 32, 1);
        write_plug_member_array(&mut item, 64, 2, 128);
        write_u32(&mut item, 144, 3);
        write_u32(&mut item, 176, 1);

        let mut plug_sets = vec![0_u8; 320];
        write_array_descriptor(&mut plug_sets, 8, 2, 64);
        write_plug_member_array(&mut plug_sets, 88, 1, 160);
        write_u32(&mut plug_sets, 176, 2);
        write_plug_member_array(&mut plug_sets, 112, 2, 224);
        write_u32(&mut plug_sets, 240, 4);
        write_u32(&mut plug_sets, 272, 5);

        let sources = socket_package_sources(&item, 0, &[100, 101, 102, 103, 104, 105], &plug_sets);

        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
        assert_eq!(sources[0].allowed, vec![103, 101]);
        assert_eq!(sources[0].ordered_members, vec![103, 101]);
        assert!(sources[0].valid);
        assert_eq!(
            sources[1].kind,
            SocketOptionSourceKind::ReusableSet { index: 0 }
        );
        assert_eq!(sources[1].allowed, vec![102]);
        assert_eq!(sources[1].ordered_members, vec![102]);
        assert!(sources[1].valid);
        assert_eq!(
            sources[2].kind,
            SocketOptionSourceKind::RandomizedSet { index: 1 }
        );
        assert_eq!(sources[2].allowed, vec![104, 105]);
        assert_eq!(sources[2].ordered_members, vec![104, 105]);
        assert!(sources[2].valid);
    }

    #[test]
    fn invalid_shared_set_reference_is_retained_without_unsafe_members() {
        let mut item = vec![0_u8; 96];
        write_u16(&mut item, 12, 7);
        write_u16(&mut item, 32, u16::MAX);
        let mut plug_sets = vec![0_u8; 128];
        write_array_descriptor(&mut plug_sets, 8, 1, 64);

        let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].kind,
            SocketOptionSourceKind::ReusableSet { index: 7 }
        );
        assert!(!sources[0].valid);
        assert!(sources[0].allowed.is_empty());
    }

    #[test]
    fn referenced_shared_set_decodes_when_unreferenced_tail_rows_are_missing() {
        let mut item = vec![0_u8; 96];
        write_u16(&mut item, 12, 0);
        write_u16(&mut item, 32, u16::MAX);
        let mut plug_sets = vec![0_u8; 160];
        write_array_descriptor(&mut plug_sets, 8, 10, 48);
        write_plug_member_array(&mut plug_sets, 72, 1, 112);
        write_u32(&mut plug_sets, 128, 0);

        let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].allowed, vec![100]);
        assert!(sources[0].valid);
    }

    #[test]
    fn partially_invalid_list_keeps_members_that_decode_safely() {
        let mut item = vec![0_u8; 208];
        write_u16(&mut item, 12, u16::MAX);
        write_u16(&mut item, 32, u16::MAX);
        write_plug_member_array(&mut item, 64, 2, 128);
        write_u32(&mut item, 144, 0);
        write_u32(&mut item, 176, 7);

        let sources = socket_package_sources(&item, 0, &[100], &[]);

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
        assert_eq!(sources[0].allowed, vec![100]);
        assert_eq!(sources[0].ordered_members, vec![100]);
        assert!(!sources[0].valid);
    }

    #[test]
    fn derived_sources_are_separate_but_feed_the_same_combined_pool() {
        let category = 0xB134_761E;
        let mut items = vec![item(
            1_498_876_634,
            vec![SocketDef {
                socket_type: 518,
                allowed: vec![1],
                sources: vec![SocketOptionSource {
                    kind: SocketOptionSourceKind::Embedded,
                    pool: 0,
                    valid: true,
                    ordered_members: vec![1],
                    allowed: vec![1],
                }],
                ..SocketDef::default()
            }],
        )];
        let names = HashMap::from([
            (2_285_418_970, "Tracker Disabled".to_owned()),
            (2_302_094_943, "Crucible Kill Tracker".to_owned()),
            (38_912_240, "Vanguard Kill Tracker".to_owned()),
        ]);
        build_socket_choices(
            &mut items,
            &HashMap::from([(1, category)]),
            &HashMap::from([(category, vec![1, 2])]),
            &names,
            &mut HashMap::new(),
        );
        let pools = intern_socket_pools(&mut items, &names).unwrap();
        let socket = &items[0].sockets[0];

        assert_eq!(socket.sources.len(), 3);
        assert!(socket.sources.iter().any(|source| matches!(
            source.kind,
            SocketOptionSourceKind::CategoryExpansion { category_hash }
                if category_hash == category
        )));
        assert!(
            socket
                .sources
                .iter()
                .any(|source| source.kind == SocketOptionSourceKind::SyntheticTracker)
        );
        assert!(pools[socket.pool as usize].contains(&2));
        assert!(pools[socket.pool as usize].contains(&2_285_418_970));
        for source in &socket.sources {
            assert!(source.allowed.is_empty());
            assert!(source.pool < pools.len() as u32);
        }
    }

    #[test]
    fn socket_source_cache_rows_keep_pool_refs_and_drop_scan_scratch() {
        let socket = SocketDef {
            sources: vec![SocketOptionSource {
                kind: SocketOptionSourceKind::RandomizedSet { index: 42 },
                pool: 7,
                valid: true,
                ordered_members: vec![200, 100],
                allowed: vec![100, 200],
            }],
            ..SocketDef::default()
        };

        let encoded = serde_json::to_value(&socket).unwrap();
        assert!(encoded["sources"][0].get("allowed").is_none());
        let decoded: SocketDef = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.sources[0].pool, 7);
        assert_eq!(
            decoded.sources[0].kind,
            SocketOptionSourceKind::RandomizedSet { index: 42 }
        );
        assert_eq!(decoded.sources[0].ordered_members, vec![200, 100]);
        assert!(decoded.sources[0].allowed.is_empty());
    }

    #[test]
    fn interning_sorts_picker_pool_without_erasing_package_member_order() {
        let mut items = vec![item(
            1_498_876_634,
            vec![SocketDef {
                allowed: vec![100, 200],
                sources: vec![SocketOptionSource {
                    kind: SocketOptionSourceKind::Embedded,
                    pool: 0,
                    valid: true,
                    ordered_members: vec![200, 100],
                    allowed: vec![200, 100],
                }],
                ..SocketDef::default()
            }],
        )];
        let names = HashMap::from([(100, "Alpha".to_owned()), (200, "Zulu".to_owned())]);

        let pools = intern_socket_pools(&mut items, &names).unwrap();
        let source = &items[0].sockets[0].sources[0];

        assert_eq!(pools[source.pool as usize], vec![100, 200]);
        assert_eq!(source.ordered_members, vec![200, 100]);
        assert!(source.allowed.is_empty());
    }

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i64(data: &mut [u8], offset: usize, value: i64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn write_array_descriptor(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
        write_u64(data, descriptor, count);
        let pointer = descriptor + 8;
        write_i64(data, pointer, i64::try_from(header - pointer).unwrap());
        write_u64(data, header, count);
    }

    fn write_plug_member_array(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
        write_array_descriptor(data, descriptor, count, header);
        write_u32(data, header + 8, 0x8080_2E03);
    }
}
