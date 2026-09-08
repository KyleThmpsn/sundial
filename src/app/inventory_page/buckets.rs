//! Inventory bucket grouping, ordering, capacity policy, and shared bucket chrome.

use std::collections::HashMap;

use eframe::egui;

use crate::catalog::{InventoryMetadata, InventoryScope};

use super::super::{SundialApp, inventory::ProfileItemSnapshot};
use super::model::{
    BUCKET_HEADER_SIZE_DELTA, BucketKey, BucketUsage, ItemBucket, ResolvedDefinition,
};

impl SundialApp {
    pub(super) fn profile_bucket_usage(&self, items: &[ProfileItemSnapshot]) -> BucketUsage {
        let mut counts = HashMap::new();
        let mut unresolved_count = 0;
        let mut occupancy_complete = true;
        for item in items {
            match self
                .manifest
                .inventory_metadata(u64::from(item.definition_hash))
            {
                Some(metadata) if metadata.scope == InventoryScope::Profile => {
                    *counts.entry(metadata.native_bucket_id).or_default() += 1;
                }
                Some(metadata) if metadata.scope != InventoryScope::Unknown => {
                    unresolved_count += 1;
                    occupancy_complete = false;
                }
                Some(_) | None => unresolved_count += 1,
            }
        }
        BucketUsage {
            counts,
            unresolved_count,
            occupancy_complete,
        }
    }

    pub(super) fn resolve_inventory_definition(&self, hash: u32) -> Option<ResolvedDefinition> {
        self.manifest
            .inventory_definition(u64::from(hash))
            .map(|definition| ResolvedDefinition {
                name: definition.name.to_owned(),
                type_name: definition.type_name.to_owned(),
                metadata: *definition.metadata,
                item: self.manifest.item_handle(u64::from(hash)),
            })
    }

    pub(super) fn group_items_by_bucket<T>(
        &self,
        items: Vec<T>,
        definition_hash: impl Fn(&T) -> Option<u64>,
        expected_scope: InventoryScope,
    ) -> Vec<ItemBucket<T>> {
        let mut groups = Vec::<ItemBucket<T>>::new();
        for item in items {
            let metadata = self
                .manifest
                .inventory_metadata(definition_hash(&item).unwrap_or_default())
                .copied()
                .unwrap_or_default();
            let key = BucketKey {
                scope: metadata.scope,
                native_id: metadata.native_bucket_id,
            };
            if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
                group.items.push(item);
            } else {
                groups.push(ItemBucket {
                    key,
                    label: metadata.bucket_label(),
                    capacity: metadata.authored_row_capacity(),
                    addable: false,
                    items: vec![item],
                });
            }
        }
        groups.sort_by_cached_key(|group| {
            (
                page_bucket_display_rank(expected_scope, group.key),
                group.label.to_lowercase(),
                group.key.native_id,
            )
        });
        groups
    }
}
pub(super) fn distinct_candidate_buckets(
    metadata: impl IntoIterator<Item = InventoryMetadata>,
) -> Vec<InventoryMetadata> {
    let mut buckets = Vec::new();
    for metadata in metadata {
        if !buckets.iter().any(|stored: &InventoryMetadata| {
            stored.scope == metadata.scope && stored.native_bucket_id == metadata.native_bucket_id
        }) {
            buckets.push(metadata);
        }
    }
    buckets
}
pub(super) fn add_candidate_buckets<T>(
    groups: &mut Vec<ItemBucket<T>>,
    candidates: impl IntoIterator<Item = InventoryMetadata>,
    expected_scope: InventoryScope,
) {
    for metadata in candidates {
        let key = BucketKey {
            scope: metadata.scope,
            native_id: metadata.native_bucket_id,
        };
        if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
            group.addable = true;
            group.capacity = metadata.authored_row_capacity();
            group.label = metadata.bucket_label();
        } else {
            groups.push(ItemBucket {
                key,
                label: metadata.bucket_label(),
                capacity: metadata.authored_row_capacity(),
                addable: true,
                items: Vec::new(),
            });
        }
    }
    groups.sort_by_cached_key(|group| {
        (
            page_bucket_display_rank(expected_scope, group.key),
            group.label.to_lowercase(),
            group.key.native_id,
        )
    });
}

pub(super) fn prepare_character_buckets<T>(groups: &mut Vec<ItemBucket<T>>, v13_account: bool) {
    if !v13_account {
        return;
    }
    // The collection owns the emote equipment slot on current Sunrise. Keep native keys
    // and capacities so display grouping cannot change placement or hide an overflow.
    groups.retain(|group| {
        group.key.scope != InventoryScope::Character
            || group.key.native_id != 41
            || !group.items.is_empty()
    });
    for group in groups {
        if group.key.scope == InventoryScope::Character {
            match group.key.native_id {
                12 => group.label = "Emotes".into(),
                41 => group.label = "Individual Emotes".into(),
                _ => {}
            }
        }
    }
}

pub(super) fn bucket_header_label<T>(
    group: &ItemBucket<T>,
    usage: &BucketUsage,
    expected_scope: InventoryScope,
) -> String {
    if group.key.scope == expected_scope
        && let Some(capacity) = group.capacity
    {
        let occupied = usage
            .counts
            .get(&group.key.native_id)
            .copied()
            .unwrap_or_default();
        return format!("{} · {occupied} / {capacity}", group.label);
    }
    format!("{} · {}", group.label, item_count_label(group.items.len()))
}

pub(super) fn bucket_header_text(ui: &egui::Ui, text: &str) -> egui::RichText {
    let size = egui::TextStyle::Body.resolve(ui.style()).size + BUCKET_HEADER_SIZE_DELTA;
    egui::RichText::new(text).strong().size(size)
}

pub(super) fn bucket_key_has_room(
    key: BucketKey,
    capacity: Option<u16>,
    usage: &BucketUsage,
) -> bool {
    capacity.is_some_and(|capacity| {
        let occupied = usage
            .counts
            .get(&key.native_id)
            .copied()
            .unwrap_or_default();
        occupied.saturating_add(usage.unresolved_count) < usize::from(capacity)
    })
}

pub(super) fn bucket_add_tooltip(
    can_add: bool,
    editable: bool,
    target_ready: bool,
    array_has_room: bool,
    occupancy_complete: bool,
    bucket_has_room: bool,
    bucket_label: &str,
) -> String {
    if can_add {
        return format!("Add an item to {bucket_label}");
    }
    if !editable {
        "Adding items is disabled for this schema".to_owned()
    } else if !target_ready {
        "The inventory target is missing or invalid".to_owned()
    } else if !array_has_room {
        "The inventory array is full".to_owned()
    } else if !occupancy_complete {
        "Bucket occupancy cannot be established until malformed or unsupported rows are repaired"
            .to_owned()
    } else if !bucket_has_room {
        format!("{bucket_label} is at capacity")
    } else {
        "This bucket cannot accept another item".to_owned()
    }
}

pub(super) const fn profile_bucket_rank(bucket: u8) -> u16 {
    match bucket {
        21 => 0, // Glimmer
        22 => 1, // Legendary Shards
        24 => 2, // Bright Dust
        15 => 3, // Consumables and materials
        23 => 4, // Silver
        14 => 5, // Shaders
        13 => 6, // Modifications
        42 => 7, // General profile items
        _ => 8,
    }
}

pub(super) const fn character_bucket_rank(bucket: u8) -> u16 {
    match bucket {
        0 => 0,   // Kinetic weapons
        1 => 1,   // Energy weapons
        2 => 2,   // Power weapons
        3 => 3,   // Helmets
        4 => 4,   // Gauntlets
        5 => 5,   // Chest armor
        6 => 6,   // Leg armor
        7 => 7,   // Class items
        8 => 8,   // Ghost shells
        9 => 9,   // Vehicles
        10 => 10, // Ships
        16 => 11, // Subclasses
        17 => 12, // Clan banners
        27 => 13, // Emblems
        12 => 14, // Emote collection
        41 => 15, // Individual emotes
        47 => 16, // Finishers
        49 => 17, // Seasonal artifacts
        _ => 100 + bucket as u16,
    }
}

pub(super) fn page_bucket_display_rank(expected_scope: InventoryScope, key: BucketKey) -> u16 {
    if key.scope == expected_scope {
        return match expected_scope {
            InventoryScope::Character => character_bucket_rank(key.native_id),
            InventoryScope::Profile => profile_bucket_rank(key.native_id),
            InventoryScope::SmallProfile => key.native_id as u16,
            InventoryScope::Unknown => u16::MAX,
        };
    }
    match key.scope {
        InventoryScope::Unknown => u16::MAX,
        _ => 10_000 + u16::from(scope_id(key.scope)) * 256 + key.native_id as u16,
    }
}

pub(super) fn item_count_label(count: usize) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

pub(super) const fn scope_id(scope: InventoryScope) -> u8 {
    match scope {
        InventoryScope::Unknown => 0,
        InventoryScope::Character => 1,
        InventoryScope::Profile => 2,
        InventoryScope::SmallProfile => 3,
    }
}

pub(super) fn draw_bucket_details<T>(
    ui: &mut egui::Ui,
    group: &ItemBucket<T>,
    usage: &BucketUsage,
    expected_scope: InventoryScope,
) {
    if let Some(message) = bucket_overflow_message(group, usage, expected_scope) {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
    if group.key.scope == InventoryScope::Unknown {
        ui.label(
            egui::RichText::new(
                "No installed bucket metadata is available; these rows remain in their original order.",
            )
            .weak(),
        );
        return;
    }
    if group.key.scope != expected_scope {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{} scope · native bucket {} · wrong scope for this page, so this row is not counted toward valid bucket occupancy",
                group.key.scope.label(),
                group.key.native_id
            ),
        );
    }
}

pub(super) fn bucket_overflow_message<T>(
    group: &ItemBucket<T>,
    usage: &BucketUsage,
    expected_scope: InventoryScope,
) -> Option<String> {
    if group.key.scope != expected_scope {
        return None;
    }
    let capacity = usize::from(group.capacity?);
    let occupied = usage
        .counts
        .get(&group.key.native_id)
        .copied()
        .unwrap_or_default();
    (occupied > capacity).then(|| format!(
        "{occupied} items exceed the installed limit of {capacity}. Equipped and stored items share this limit. Remove {} extra items.",
        occupied - capacity,
    ))
}

pub(super) fn bucket_has_room(
    metadata: &InventoryMetadata,
    usage: &BucketUsage,
    current_bucket: Option<u8>,
    replacing_unresolved: bool,
) -> bool {
    if current_bucket == Some(metadata.native_bucket_id) {
        return true;
    }
    metadata.authored_row_capacity().is_some_and(|capacity| {
        let known = usage
            .counts
            .get(&metadata.native_bucket_id)
            .copied()
            .unwrap_or_default();
        let unresolved = usage
            .unresolved_count
            .saturating_sub(usize::from(replacing_unresolved));
        known.saturating_add(unresolved) < usize::from(capacity)
    })
}

pub(super) fn profile_swap_candidate(
    metadata: &InventoryMetadata,
    current_bucket: Option<u8>,
    quantity: i32,
    usage: &BucketUsage,
    replacing_unresolved: bool,
) -> bool {
    current_bucket.is_none_or(|bucket| {
        metadata.scope == InventoryScope::Profile && metadata.native_bucket_id == bucket
    }) && metadata
        .max_stack_size
        .is_some_and(|maximum| maximum >= quantity as u32)
        && bucket_has_room(metadata, usage, current_bucket, replacing_unresolved)
}
