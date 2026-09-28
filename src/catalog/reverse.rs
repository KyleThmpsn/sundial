//! Reverse lookups from a definition to the items and records that reference it.

use std::{collections::HashMap, sync::OnceLock};

use super::{Catalog, MaterialSetUse, ObjectiveOwnerTraitDef, PlugOffer, RecordRewardUse};
use crate::hash::parse_hash_hex;

/// Reverse indexes built on first use and never serialized.
#[derive(Default)]
pub(super) struct ReverseIndexes {
    plug_offers: OnceLock<PlugOfferIndex>,
    metadata: OnceLock<MetadataIndex>,
    record_rewards: OnceLock<HashMap<u64, Vec<RecordRewardUse>>>,
    stat_groups: OnceLock<HashMap<u16, Vec<usize>>>,
}

/// Item metadata references, keyed by the referenced row and sorted by item name.
#[derive(Default)]
struct MetadataIndex {
    sandbox_perks: HashMap<u16, Vec<u64>>,
    traits: HashMap<u16, Vec<u64>>,
    plug_categories: HashMap<u64, Vec<u64>>,
    material_sets: HashMap<u16, Vec<(u64, MaterialSetUse)>>,
}

/// Socket usage behind plug offers.
///
/// Every plug's offers together run to millions of rows, so each plug's list is resolved on its
/// first request from these compact tables.
#[derive(Default)]
struct PlugOfferIndex {
    /// Sockets, as (item position, socket index), whose valid option sources use each pool.
    pool_sockets: Vec<Vec<(u32, u32)>>,
    /// The source pools holding each plug.
    pools_by_plug: HashMap<u64, Vec<u32>>,
    /// Sockets defaulting to each plug.
    defaults: HashMap<u64, Vec<(u32, u32)>>,
    offers: HashMap<u64, OnceLock<Box<[PlugOffer]>>>,
}

impl PlugOfferIndex {
    fn build(catalog: &Catalog) -> Self {
        let mut index = Self {
            pool_sockets: vec![Vec::new(); catalog.plug_pools.len()],
            ..Self::default()
        };
        for (item_position, item) in catalog.items.iter().enumerate() {
            let Ok(item_position) = u32::try_from(item_position) else {
                break;
            };
            for (socket_index, socket) in item.sockets.iter().enumerate() {
                let Ok(socket_index) = u32::try_from(socket_index) else {
                    break;
                };
                for source in socket.sources.iter().filter(|source| source.valid) {
                    let pool = source.pool as usize;
                    if catalog
                        .plug_pools
                        .get(pool)
                        .is_some_and(|members| !members.is_empty())
                        && let Some(sockets) = index.pool_sockets.get_mut(pool)
                        && sockets.last() != Some(&(item_position, socket_index))
                    {
                        sockets.push((item_position, socket_index));
                    }
                }
            }
            for (socket_index, plug) in item.default_plugs.iter().enumerate() {
                let Some(plug) = plug.as_deref().and_then(parse_hash_hex) else {
                    continue;
                };
                let Ok(socket_index) = u32::try_from(socket_index) else {
                    break;
                };
                index
                    .defaults
                    .entry(plug)
                    .or_default()
                    .push((item_position, socket_index));
            }
        }
        for (pool, (sockets, members)) in index
            .pool_sockets
            .iter()
            .zip(&catalog.plug_pools)
            .enumerate()
        {
            if sockets.is_empty() {
                continue;
            }
            let Ok(pool) = u32::try_from(pool) else {
                break;
            };
            for plug in members {
                index.pools_by_plug.entry(*plug).or_default().push(pool);
            }
        }
        index.offers = index
            .pools_by_plug
            .keys()
            .chain(index.defaults.keys())
            .map(|plug| (*plug, OnceLock::new()))
            .collect();
        index
    }
}

impl MetadataIndex {
    fn build(catalog: &Catalog) -> Self {
        let mut index = Self::default();
        for (&hash, metadata) in &catalog.item_package_metadata {
            for perk in &metadata.sandbox_perks {
                index
                    .sandbox_perks
                    .entry(perk.perk_index)
                    .or_default()
                    .push(hash);
            }
            for &trait_index in &metadata.trait_indices {
                index.traits.entry(trait_index).or_default().push(hash);
            }
            if let Some(category) = metadata.plug_category_hash {
                index
                    .plug_categories
                    .entry(category)
                    .or_default()
                    .push(hash);
            }
        }
        for (&hash, sets) in &catalog.item_material_requirement_set_indices {
            for (set, usage) in [
                (sets.insertion, MaterialSetUse::Insertion),
                (sets.enabled, MaterialSetUse::Enabled),
            ] {
                if let Some(set) = set {
                    index
                        .material_sets
                        .entry(set)
                        .or_default()
                        .push((hash, usage));
                }
            }
        }
        for hashes in index
            .sandbox_perks
            .values_mut()
            .chain(index.traits.values_mut())
            .chain(index.plug_categories.values_mut())
        {
            catalog.sort_hashes_by_name(hashes);
        }
        for uses in index.material_sets.values_mut() {
            uses.sort_by_cached_key(|&(hash, usage)| {
                (
                    catalog.name_order(hash),
                    hash,
                    usage == MaterialSetUse::Enabled,
                )
            });
        }
        index
    }
}

impl Catalog {
    /// Items whose sockets default to or offer this plug (any valid source).
    ///
    /// Each item socket appears once, sorted by item name, then hash, then socket.
    pub(crate) fn plug_offers(&self, plug_hash: u64) -> &[PlugOffer] {
        let index = self
            .reverse
            .plug_offers
            .get_or_init(|| PlugOfferIndex::build(self));
        let Some(offers) = index.offers.get(&plug_hash) else {
            return &[];
        };
        offers.get_or_init(|| self.resolve_plug_offers(index, plug_hash))
    }

    /// Items whose package metadata carries this sandbox perk index.
    pub(crate) fn items_with_sandbox_perk(&self, perk_index: u16) -> &[u64] {
        self.metadata_index()
            .sandbox_perks
            .get(&perk_index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// An item trait definition by hash, with its index into trait_definitions().
    pub(crate) fn item_trait(&self, hash: u64) -> Option<(u16, &ObjectiveOwnerTraitDef)> {
        let (index, definition) = self
            .trait_definitions
            .iter()
            .enumerate()
            .find(|(_, definition)| definition.hash == hash)?;
        Some((u16::try_from(index).ok()?, definition))
    }

    /// Items whose package metadata lists this trait index.
    pub(crate) fn items_with_trait(&self, trait_index: u16) -> &[u64] {
        self.metadata_index()
            .traits
            .get(&trait_index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Plug items whose plug_category_hash equals this hash.
    pub(crate) fn plugs_in_category(&self, category_hash: u64) -> &[u64] {
        self.metadata_index()
            .plug_categories
            .get(&category_hash)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Items naming this material requirement set as their insertion or enabled set.
    pub(crate) fn items_using_material_requirement_set(
        &self,
        set_index: u16,
    ) -> &[(u64, MaterialSetUse)] {
        self.metadata_index()
            .material_sets
            .get(&set_index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Records whose rewards or interval items grant this item, sorted by record name.
    pub(crate) fn records_rewarding_item(&self, item_hash: u64) -> &[RecordRewardUse] {
        self.reverse
            .record_rewards
            .get_or_init(|| self.record_reward_index())
            .get(&item_hash)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Indices into the item stat groups whose scaled stats include this stat definition index.
    pub(crate) fn stat_groups_with_stat(&self, definition_index: u16) -> &[usize] {
        self.reverse
            .stat_groups
            .get_or_init(|| {
                let mut groups = HashMap::<u16, Vec<usize>>::new();
                for (group_index, group) in self.item_stat_groups.iter().enumerate() {
                    for stat in &group.scaled_stats {
                        let entry = groups.entry(stat.definition_index).or_default();
                        if entry.last() != Some(&group_index) {
                            entry.push(group_index);
                        }
                    }
                }
                groups
            })
            .get(&definition_index)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn metadata_index(&self) -> &MetadataIndex {
        self.reverse
            .metadata
            .get_or_init(|| MetadataIndex::build(self))
    }

    fn resolve_plug_offers(&self, index: &PlugOfferIndex, plug_hash: u64) -> Box<[PlugOffer]> {
        let defaults = index
            .defaults
            .get(&plug_hash)
            .into_iter()
            .flatten()
            .map(|&(item, socket)| (item, socket, true));
        let offered = index
            .pools_by_plug
            .get(&plug_hash)
            .into_iter()
            .flatten()
            .filter_map(|&pool| index.pool_sockets.get(pool as usize))
            .flatten()
            .map(|&(item, socket)| (item, socket, false));
        let mut sockets = defaults.chain(offered).collect::<Vec<_>>();
        // The default row sorts first within its socket, so dedup keeps it.
        sockets.sort_unstable_by_key(|&(item, socket, is_default)| (item, socket, !is_default));
        sockets.dedup_by_key(|entry| (entry.0, entry.1));
        let mut offers = sockets
            .into_iter()
            .filter_map(|(item, socket, is_default)| {
                Some(PlugOffer {
                    item_hash: self.items.get(item as usize)?.hash,
                    socket_index: socket as usize,
                    is_default,
                })
            })
            .collect::<Vec<_>>();
        offers.sort_by_cached_key(|offer| {
            (
                self.name_order(offer.item_hash),
                offer.item_hash,
                offer.socket_index,
            )
        });
        offers.into_boxed_slice()
    }

    fn record_reward_index(&self) -> HashMap<u64, Vec<RecordRewardUse>> {
        let mut rewards = HashMap::<u64, Vec<RecordRewardUse>>::new();
        for (record_index, record) in self.records.iter().flatten().enumerate() {
            let Some(runtime) = &record.runtime else {
                continue;
            };
            let completion = runtime
                .rewards
                .iter()
                .map(|&(item, quantity)| (item, None, quantity));
            let intervals = runtime
                .interval_items
                .iter()
                .enumerate()
                .filter_map(|(interval, &item)| item.map(|item| (item, Some(interval), 1)));
            for (item, interval, quantity) in completion.chain(intervals) {
                if let Some(item_hash) = self.item_hash_for_index(item) {
                    rewards.entry(item_hash).or_default().push(RecordRewardUse {
                        record_index,
                        interval,
                        quantity,
                    });
                }
            }
        }
        let records = self.records().unwrap_or_default();
        for uses in rewards.values_mut() {
            uses.sort_by_cached_key(|usage| {
                let record = records.get(usage.record_index);
                let name = record.map_or("", |record| record.name.trim());
                (
                    name.is_empty(),
                    name.to_lowercase(),
                    record.map_or(0, |record| record.hash),
                    usage.record_index,
                    usage.interval,
                )
            });
        }
        rewards
    }

    /// Sort key placing named definitions first, alphabetically.
    fn name_order(&self, hash: u64) -> (bool, String) {
        let name = self
            .display_name(hash)
            .or_else(|| self.package_item_name(hash))
            .unwrap_or_default()
            .trim();
        (name.is_empty(), name.to_lowercase())
    }

    fn sort_hashes_by_name(&self, hashes: &mut Vec<u64>) {
        hashes.sort_unstable();
        hashes.dedup();
        hashes.sort_by_cached_key(|&hash| (self.name_order(hash), hash));
    }
}
