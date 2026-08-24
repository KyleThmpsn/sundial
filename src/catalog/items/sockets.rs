//! Item socket decoding, option pools, labels, and catalyst state.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::hash::{format_hash_hex, parse_hash_hex};

use super::{
    super::{
        Catalog,
        package::{array_at, u16_at, u32_at},
    },
    ItemDef,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::catalog) enum GearKind {
    Weapon,
    Armor,
    Other(u64),
}

const COSMETIC_SOCKET_TYPES_WITHOUT_MARKERS: [u16; 1] = [746];

pub(in crate::catalog) const fn gear_kind(bucket_hash: u64) -> GearKind {
    if matches!(bucket_hash, 1_498_876_634 | 2_465_295_065 | 953_998_645) {
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

pub(in crate::catalog) const ORDINARY_SOCKET_CLASS: u32 = 0x8080_77C4;

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
}

impl SocketDef {
    pub(crate) fn display_label(&self, index: usize) -> String {
        if self.label.is_empty() {
            format!("Socket {}", index + 1)
        } else {
            format!("{}. {}", index + 1, self.label)
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

    pub(crate) fn socket_type_options(&self, socket_type: u16) -> &[u64] {
        self.socket_type_options
            .get(&socket_type)
            .map(Vec::as_slice)
            .unwrap_or_default()
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

    pub(crate) fn all_plug_options(&self) -> &[u64] {
        &self.all_plug_options
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
    for item in items {
        for socket in &mut item.sockets {
            socket.allowed.sort_by_key(|hash| {
                names
                    .get(hash)
                    .map(|name| name.to_lowercase())
                    .unwrap_or_default()
            });
            socket.allowed.dedup();
            let pool = if let Some(index) = indices.get(&socket.allowed) {
                *index
            } else {
                let index = u32::try_from(pools.len())
                    .map_err(|_| "The catalog contains too many distinct socket pools")?;
                let values = std::mem::take(&mut socket.allowed);
                indices.insert(values.clone(), index);
                pools.push(values);
                index
            };
            socket.pool = pool;
            socket.allowed.clear();
        }
    }
    Ok(pools)
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
            for seed in seeds {
                let Some(category) = plug_category_by_hash.get(&seed) else {
                    continue;
                };
                if matches!(*category, 0xB134_761E | 0x8772_7F34 | 0x6C86_3692) {
                    let Some(category_items) = plug_category_items.get(category) else {
                        continue;
                    };
                    socket.allowed.extend(category_items.iter().copied());
                }
            }
            socket.allowed.sort_unstable();
            socket.allowed.dedup();
        }
    }
    infer_socket_plug_types(items, names, type_names);
    let tracker_plugs = [2_285_418_970, 2_302_094_943, 38_912_240];
    for item in items.iter_mut() {
        for socket in &mut item.sockets {
            // Kill/Crucible tracker sockets use a small synthetic plug set
            // keyed by socket type rather than a package plug-set row.
            if socket.socket_type == 518 {
                socket.allowed.extend(tracker_plugs);
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

pub(in crate::catalog) fn socket_allowed_hashes(
    item: &[u8],
    socket_base: usize,
    item_hashes: &[u64],
    plug_set_table: &[u8],
) -> Vec<u64> {
    let mut allowed = Vec::new();

    // Small reusable lists, such as a fixed shader choice, are embedded in
    // the inventory item definition.
    if let Ok((count, rows, _)) = array_at(item, socket_base + 64) {
        for index in 0..count.min(65_535) {
            if let Ok(item_index) = u32_at(item, rows + index * 32) {
                if let Some(hash) = item_hashes.get(item_index as usize) {
                    allowed.push(*hash);
                }
            }
        }
    }

    // Larger option pools use the shared DestinyPlugSetDefinition table.
    // Reusable and randomized plug sets have separate row indices at +12 and
    // +32 respectively.
    let Ok((set_count, set_rows, _)) = array_at(plug_set_table, 8) else {
        return allowed;
    };
    for set_offset in [12, 32] {
        let Ok(set_index) = u16_at(item, socket_base + set_offset) else {
            continue;
        };
        if set_index == u16::MAX || set_index as usize >= set_count {
            continue;
        }
        let descriptor = set_rows + set_index as usize * 24 + 8;
        if let Ok((count, rows, _)) = array_at(plug_set_table, descriptor) {
            for index in 0..count.min(65_535) {
                if let Ok(item_index) = u32_at(plug_set_table, rows + index * 32) {
                    if let Some(hash) = item_hashes.get(item_index as usize) {
                        allowed.push(*hash);
                    }
                }
            }
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
