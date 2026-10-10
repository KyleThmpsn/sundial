//! Item socket decoding, option pools, labels, and catalyst state.

use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
};

use serde::{Deserialize, Serialize};

use crate::{
    hash::{format_hash_hex, parse_hash_hex},
    investment::schema::{
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

/// The item's subtype as the game prints it under its name, such as Sidearm, or a stand-in.
pub(crate) fn item_subtype_label(item: &ItemDef) -> String {
    let type_name = item.type_name.trim();
    if type_name.is_empty() {
        "Item Subtype".to_owned()
    } else {
        type_name.to_owned()
    }
}

/// The item's type in Bungie's sense, Weapon or Armor, or a stand-in for anything else.
pub(crate) fn item_type_label(item: &ItemDef) -> &'static str {
    match gear_kind(item.bucket_hash) {
        GearKind::Weapon => "Weapon",
        GearKind::Armor => "Armor",
        GearKind::Other(_) => "Item Type",
    }
}

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

/// Whether a plug is cosmetic by what it is, not by the pool it sits in: shaders, ornaments,
/// transmat effects, projections and trackers. An ornament socket without a marker plug
/// otherwise leaks its ornaments into every wider scope.
fn cosmetic_plug(name: Option<&str>, type_name: Option<&str>) -> bool {
    let type_name = type_name.map(str::to_ascii_lowercase).unwrap_or_default();
    ["shader", "ornament", "transmat", "projection", "tracker"]
        .iter()
        .any(|kind| type_name.contains(kind))
        || name.is_some_and(|name| cosmetic_marker(name) || tracker_marker(name))
}

/// Every non-cosmetic plug used anywhere on a gear type (the item's type name, such as a
/// sidearm) and on a gear kind (all weapons or all armor), plus the cosmetic pools.
pub(in crate::catalog) fn build_gear_type_options(
    items: &[ItemDef],
    plug_pools: &[Vec<u64>],
    names: &HashMap<u64, String>,
    type_names: &HashMap<u64, String>,
) -> GearTypeOptions {
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
    let mut cosmetic_hashes = cosmetic_pools
        .iter()
        .filter_map(|pool| plug_pools.get(*pool as usize))
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    cosmetic_hashes.extend(plug_pools.iter().flatten().copied().filter(|hash| {
        cosmetic_plug(
            names.get(hash).map(String::as_str),
            type_names.get(hash).map(String::as_str),
        )
    }));

    let mut type_options = HashMap::<String, Vec<u64>>::new();
    let mut class_options = HashMap::<GearKind, Vec<u64>>::new();
    for item in items {
        let gear_type = item_gear_type(item).into_owned();
        let gear_kind = gear_kind(item.bucket_hash);
        for socket in &item.sockets {
            if let Some(pool) = plug_pools.get(socket.pool as usize) {
                let plugs = pool.iter().filter(|hash| !cosmetic_hashes.contains(hash));
                type_options
                    .entry(gear_type.clone())
                    .or_default()
                    .extend(plugs.clone());
                class_options.entry(gear_kind).or_default().extend(plugs);
            }
        }
    }
    for values in type_options.values_mut() {
        sort_plug_options(values, names);
    }
    for values in class_options.values_mut() {
        sort_plug_options(values, names);
    }
    (type_options, class_options, cosmetic_pools)
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

/// Who carries a socket type among the items of one gear type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SocketCarriers {
    /// How many differently named items carry it, so a reissue of one piece counts once.
    pub items: usize,
    /// The plug it most often starts with, the lowest hash among equals.
    pub default_plug: Option<u64>,
}

pub(in crate::catalog) fn build_socket_type_carriers(
    items: &[ItemDef],
) -> HashMap<String, HashMap<u16, SocketCarriers>> {
    let mut tallies = HashMap::<String, HashMap<u16, (HashSet<&str>, HashMap<u64, usize>)>>::new();
    for item in items {
        let by_type = tallies
            .entry(item_gear_type(item).into_owned())
            .or_default();
        for (index, socket) in item.sockets.iter().enumerate() {
            let (names, defaults) = by_type.entry(socket.socket_type).or_default();
            names.insert(item.name.trim());
            if let Some(plug) = item
                .default_plugs
                .get(index)
                .and_then(Option::as_deref)
                .and_then(parse_hash_hex)
            {
                *defaults.entry(plug).or_default() += 1;
            }
        }
    }
    tallies
        .into_iter()
        .map(|(gear_type, by_type)| {
            let by_type = by_type
                .into_iter()
                .map(|(socket_type, (names, defaults))| {
                    let default_plug = defaults
                        .into_iter()
                        .max_by(|(left, left_count), (right, right_count)| {
                            left_count.cmp(right_count).then_with(|| right.cmp(left))
                        })
                        .map(|(plug, _)| plug);
                    (
                        socket_type,
                        SocketCarriers {
                            items: names.len(),
                            default_plug,
                        },
                    )
                })
                .collect();
            (gear_type, by_type)
        })
        .collect()
}

pub(in crate::catalog) use crate::investment::schema::ITEM_ORDINARY_SOCKET_ROW_CLASS as ORDINARY_SOCKET_CLASS;

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
    /// Native row order is essential. Picker pools are sorted and cannot encode ownership bits.
    pub(crate) fn ordered_randomized_choices(&self) -> &[u64] {
        self.sources
            .iter()
            .find(|s| s.valid && matches!(s.kind, SocketOptionSourceKind::RandomizedSet { .. }))
            .map_or(&[], |s| s.ordered_members.as_slice())
    }

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

    /// Who carries `socket_type` among the items of `item`'s gear type.
    pub(crate) fn socket_type_carriers(&self, item: &ItemDef, socket_type: u16) -> SocketCarriers {
        self.socket_type_carriers
            .get(item_gear_type(item).as_ref())
            .and_then(|carriers| carriers.get(&socket_type))
            .copied()
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

    /// Plugs used anywhere on this item's gear type, such as every sidearm.
    pub(crate) fn gear_type_options(&self, item: &ItemDef, socket_index: usize) -> Vec<u64> {
        let Some(socket) = item.sockets.get(socket_index) else {
            return Vec::new();
        };
        let mut options = self
            .gear_type_options
            .get(item_gear_type(item).as_ref())
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
            .get(item_gear_type(item).as_ref())
            .cloned()
            .unwrap_or_default();
        if self.cosmetic_socket_types.contains(&socket_type) {
            options.extend(self.socket_type_options(socket_type));
            sort_plug_options(&mut options, &self.names);
        }
        options
    }

    /// Plugs used anywhere on this item's gear kind, all weapons or all armor.
    pub(crate) fn gear_kind_options(&self, item: &ItemDef, socket_index: usize) -> Vec<u64> {
        let Some(socket) = item.sockets.get(socket_index) else {
            return Vec::new();
        };
        let mut options = self
            .gear_kind_options
            .get(&gear_kind(item.bucket_hash))
            .cloned()
            .unwrap_or_default();
        if self.cosmetic_socket_pools.contains(&socket.pool) {
            options.extend(self.socket_type_options(socket.socket_type));
            sort_plug_options(&mut options, &self.names);
        }
        options
    }

    pub(crate) fn gear_kind_options_for_type(&self, item: &ItemDef, socket_type: u16) -> Vec<u64> {
        let mut options = self
            .gear_kind_options
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

    /// How many shared reusable plug sets offer each plug, across installed items' sockets.
    pub(crate) fn reusable_set_counts(&self) -> HashMap<u64, usize> {
        let mut sets = HashMap::<u64, HashSet<u16>>::new();
        for socket in self.items.iter().flat_map(|item| &item.sockets) {
            for source in &socket.sources {
                if let SocketOptionSourceKind::ReusableSet { index } = source.kind {
                    for &plug in &source.ordered_members {
                        sets.entry(plug).or_default().insert(index);
                    }
                }
            }
        }
        sets.into_iter()
            .map(|(plug, sets)| (plug, sets.len()))
            .collect()
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
        // The pool holds these values now, so the socket's own list gives its space back.
        *values = Vec::new();
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
            if let Some(Some(default)) = item.default_plugs.get(socket_index)
                && let Some(hash) = parse_hash_hex(default)
            {
                seeds.push(hash);
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

type GearTypeOptions = (
    HashMap<String, Vec<u64>>,
    HashMap<GearKind, Vec<u64>>,
    HashSet<u32>,
);

#[cfg(test)]
mod tests;
