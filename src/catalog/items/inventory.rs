//! Installed inventory placement metadata and safe candidate queries.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tiger_pkg::{PackageManager, TagHash};

use super::{
    super::{
        Catalog, CatalogSearchQuery,
        package::{i32_at, u32_at},
    },
    ItemDef,
};

const INVENTORY_BUCKET_TABLE_SLOT: usize = 17;
const INVENTORY_BUCKET_COUNT_OFFSET: usize = 140;
const INVENTORY_BUCKET_FIRST_DESCRIPTOR: usize = 144;
const INVENTORY_BUCKET_DESCRIPTOR_SIZE: usize = 36;
const INVENTORY_BUCKET_FIRST_SLOT_OFFSET: usize = 4;
const INVENTORY_BUCKET_SLOT_COUNT_OFFSET: usize = 8;
const INVENTORY_BUCKET_SCOPE_OFFSET: usize = 24;
const INVENTORY_MAX_STACK_SIZE_OFFSET: usize = 180;
const INVENTORY_BUCKET_ID_OFFSET: usize = 184;
const INVENTORY_INSTANCED_OFFSET: usize = 187;

/// Native inventory array selected by an installed bucket descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InventoryScope {
    #[default]
    Unknown,
    Character,
    Profile,
    SmallProfile,
}

impl InventoryScope {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Character => "Character",
            Self::Profile => "Profile",
            Self::SmallProfile => "Small profile",
        }
    }

    /// Fixed capacity of the native array selected by this scope.
    pub(crate) const fn array_capacity(self) -> Option<u16> {
        match self {
            Self::Unknown => None,
            Self::Character => Some(350),
            Self::Profile => Some(701),
            Self::SmallProfile => Some(6),
        }
    }
}

/// Quantity policy declared by the installed item definition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemStackability {
    #[default]
    Unknown,
    Stackable,
    Instanced,
}

impl ItemStackability {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Stackable => "Stackable",
            Self::Instanced => "Instanced",
        }
    }
}

/// Installed item and bucket fields needed to place an authored inventory row safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InventoryMetadata {
    pub scope: InventoryScope,
    pub native_bucket_id: u8,
    pub stackability: ItemStackability,
    pub max_stack_size: Option<u32>,
    /// Number of rows owned by this bucket, not the capacity of the whole scope array.
    pub bucket_capacity: Option<u16>,
}

impl Default for InventoryMetadata {
    fn default() -> Self {
        Self {
            scope: InventoryScope::Unknown,
            native_bucket_id: u8::MAX,
            stackability: ItemStackability::Unknown,
            max_stack_size: None,
            bucket_capacity: None,
        }
    }
}

impl InventoryMetadata {
    /// Maximum number of settings-authored rows that can occupy this native bucket.
    ///
    /// Character callers count both present equipment and unequipped inventory against
    /// this value. Empty equipment slots therefore leave their row available, matching
    /// Sunrise's runtime placement order.
    pub(crate) const fn authored_row_capacity(self) -> Option<u16> {
        match (self.scope, self.bucket_capacity) {
            (InventoryScope::Unknown, _) | (_, None) => None,
            (_, Some(capacity)) => Some(capacity),
        }
    }

    pub(crate) const fn is_profile_items_candidate(self) -> bool {
        matches!(self.scope, InventoryScope::Profile)
            && matches!(self.stackability, ItemStackability::Stackable)
            && matches!(self.max_stack_size, Some(size) if size > 0)
            && matches!(self.authored_row_capacity(), Some(size) if size > 0)
    }

    pub(crate) const fn is_character_inventory_candidate(self) -> bool {
        matches!(self.scope, InventoryScope::Character)
            && !matches!(self.stackability, ItemStackability::Unknown)
            && matches!(self.max_stack_size, Some(size) if size > 0)
            && matches!(self.authored_row_capacity(), Some(size) if size > 0)
    }

    /// Human-facing name for the installed Shadowkeep bucket represented by this metadata.
    pub(crate) fn bucket_label(self) -> String {
        inventory_bucket_name(self.scope, self.native_bucket_id).map_or_else(
            || match self.scope {
                InventoryScope::Unknown => "Unknown bucket".to_owned(),
                scope => format!("{} bucket {}", scope.label(), self.native_bucket_id),
            },
            str::to_owned,
        )
    }
}

const fn inventory_bucket_name(scope: InventoryScope, bucket: u8) -> Option<&'static str> {
    match (scope, bucket) {
        (InventoryScope::Character, 0) => Some("Kinetic weapons"),
        (InventoryScope::Character, 1) => Some("Energy weapons"),
        (InventoryScope::Character, 2) => Some("Power weapons"),
        (InventoryScope::Character, 3) => Some("Helmets"),
        (InventoryScope::Character, 4) => Some("Gauntlets"),
        (InventoryScope::Character, 5) => Some("Chest armor"),
        (InventoryScope::Character, 6) => Some("Leg armor"),
        (InventoryScope::Character, 7) => Some("Class items"),
        (InventoryScope::Character, 8) => Some("Ghost shells"),
        (InventoryScope::Character, 9) => Some("Vehicles"),
        (InventoryScope::Character, 10) => Some("Ships"),
        (InventoryScope::Character, 12) => Some("Emote collection"),
        (InventoryScope::Character, 16) => Some("Subclasses"),
        (InventoryScope::Character, 17) => Some("Clan banners"),
        (InventoryScope::Character, 27) => Some("Emblems"),
        (InventoryScope::Character, 31) => Some("Engrams"),
        (InventoryScope::Character, 33) => Some("Quest steps"),
        (InventoryScope::Character, 37) => Some("General inventory"),
        (InventoryScope::Character, 40) => Some("Quests and bounties"),
        (InventoryScope::Character, 41) => Some("Emotes"),
        (InventoryScope::Character, 47) => Some("Finishers"),
        (InventoryScope::Character, 49) => Some("Seasonal artifacts"),
        (InventoryScope::Profile, 13) => Some("Modifications"),
        (InventoryScope::Profile, 14) => Some("Shaders"),
        (InventoryScope::Profile, 15) => Some("Consumables"),
        (InventoryScope::Profile, 21) => Some("Glimmer"),
        (InventoryScope::Profile, 22) => Some("Legendary Shards"),
        (InventoryScope::Profile, 23) => Some("Silver"),
        (InventoryScope::Profile, 24) => Some("Bright Dust"),
        (InventoryScope::Profile, 42) => Some("General profile items"),
        _ => None,
    }
}

/// A displayable inventory definition, including profile-only definitions that are not equipment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InventoryDefinition<'a> {
    pub hash: u64,
    pub name: &'a str,
    pub type_name: &'a str,
    pub metadata: &'a InventoryMetadata,
    /// Present when the definition is also part of the existing equipment catalog.
    pub item: Option<&'a ItemDef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::catalog) struct InventoryBucketDescriptor {
    scope: InventoryScope,
    capacity: u16,
}

impl Catalog {
    pub(crate) fn character_inventory_candidate_buckets(
        &self,
        class_type: u64,
        show_dummy_items: bool,
    ) -> &[InventoryMetadata] {
        self.character_inventory_candidate_buckets
            .get(class_type, show_dummy_items)
    }

    pub(crate) fn inventory_metadata(&self, hash: u64) -> Option<&InventoryMetadata> {
        self.inventory_metadata.get(&hash)
    }

    /// Resolves both equipment definitions and profile-only inventory definitions.
    pub(crate) fn inventory_definition(&self, hash: u64) -> Option<InventoryDefinition<'_>> {
        let metadata = self.inventory_metadata(hash)?;
        let item = self.item(hash);
        let name = self
            .names
            .get(&hash)
            .map(String::as_str)
            .or_else(|| item.map(|definition| definition.name.as_str()))?;
        let type_name = self
            .type_names
            .get(&hash)
            .map(String::as_str)
            .or_else(|| item.map(|definition| definition.type_name.as_str()))
            .unwrap_or_default();
        Some(InventoryDefinition {
            hash,
            name,
            type_name,
            metadata,
            item,
        })
    }

    /// Returns deterministic, safe profile-item matches.
    pub(crate) fn profile_item_candidates(
        &self,
        text: &str,
    ) -> impl Iterator<Item = InventoryDefinition<'_>> + '_ {
        let query = CatalogSearchQuery::new(text);
        self.inventory_hashes
            .iter()
            .filter_map(|hash| self.inventory_definition(*hash))
            .filter(move |definition| {
                definition.metadata.is_profile_items_candidate()
                    && !crate::dummy_items::contains(definition.hash)
                    && query.matches(
                        self,
                        definition.hash,
                        &[definition.name, definition.type_name],
                    )
            })
    }

    /// Returns deterministic, equippable character-inventory matches.
    pub(crate) fn character_inventory_candidates(
        &self,
        text: &str,
        class_type: u64,
        show_dummy_items: bool,
    ) -> impl Iterator<Item = InventoryDefinition<'_>> + '_ {
        let query = CatalogSearchQuery::new(text);
        self.inventory_hashes
            .iter()
            .filter_map(|hash| self.inventory_definition(*hash))
            .filter(move |definition| {
                definition.item.is_some_and(|item| {
                    (item.class_type == 3 || item.class_type == class_type)
                        && (show_dummy_items || !crate::dummy_items::contains(item.hash))
                }) && definition.metadata.is_character_inventory_candidate()
                    && query.matches(
                        self,
                        definition.hash,
                        &[definition.name, definition.type_name],
                    )
            })
    }
}

pub(in crate::catalog) fn scan_inventory_bucket_descriptors(
    manager: &PackageManager,
    root: &[u8],
) -> Result<HashMap<u8, InventoryBucketDescriptor>, String> {
    let slot = 8 + INVENTORY_BUCKET_TABLE_SLOT * 16;
    let table_tag = TagHash(u32_at(root, slot)?);
    let table = manager
        .read_tag(table_tag)
        .map_err(|error| format!("Could not read inventory bucket table: {error}"))?;
    parse_inventory_bucket_descriptors(&table)
}

fn parse_inventory_bucket_descriptors(
    table: &[u8],
) -> Result<HashMap<u8, InventoryBucketDescriptor>, String> {
    if table.len() < INVENTORY_BUCKET_FIRST_DESCRIPTOR {
        return Err("The inventory bucket table is truncated".into());
    }
    let count = i32_at(table, INVENTORY_BUCKET_COUNT_OFFSET)?;
    let count = usize::try_from(count)
        .ok()
        .filter(|count| (1..=u8::MAX as usize).contains(count))
        .ok_or("The inventory bucket table has an invalid descriptor count")?;
    let rows_size = count
        .checked_mul(INVENTORY_BUCKET_DESCRIPTOR_SIZE)
        .ok_or("The inventory bucket table size overflowed")?;
    let end = INVENTORY_BUCKET_FIRST_DESCRIPTOR
        .checked_add(rows_size)
        .ok_or("The inventory bucket table extent overflowed")?;
    if end > table.len() {
        return Err("The inventory bucket descriptors are truncated".into());
    }

    let mut descriptors = HashMap::with_capacity(count);
    for index in 0..count {
        let base = INVENTORY_BUCKET_FIRST_DESCRIPTOR + index * INVENTORY_BUCKET_DESCRIPTOR_SIZE;
        let bucket_id = table[base];
        if bucket_id == u8::MAX {
            return Err("The inventory bucket table contains an unavailable bucket id".into());
        }
        let scope = match table[base + INVENTORY_BUCKET_SCOPE_OFFSET] {
            0 => InventoryScope::Character,
            1 => InventoryScope::Profile,
            2 => InventoryScope::SmallProfile,
            value => {
                return Err(format!(
                    "The inventory bucket table contains unknown scope {value}"
                ));
            }
        };
        let first_slot = i32_at(table, base + INVENTORY_BUCKET_FIRST_SLOT_OFFSET)?;
        let capacity = i32_at(table, base + INVENTORY_BUCKET_SLOT_COUNT_OFFSET)?;
        let first_slot = u16::try_from(first_slot)
            .map_err(|_| "An inventory bucket has an invalid first slot")?;
        let capacity = u16::try_from(capacity)
            .ok()
            .filter(|capacity| *capacity > 0)
            .ok_or("An inventory bucket has an invalid capacity")?;
        let array_capacity = scope
            .array_capacity()
            .ok_or("An inventory bucket has no native array capacity")?;
        if first_slot > array_capacity || capacity > array_capacity - first_slot {
            return Err("An inventory bucket range exceeds its native array".into());
        }
        if descriptors
            .insert(bucket_id, InventoryBucketDescriptor { scope, capacity })
            .is_some()
        {
            return Err(format!(
                "The inventory bucket table repeats bucket {bucket_id}"
            ));
        }
    }
    Ok(descriptors)
}

pub(in crate::catalog) fn item_inventory_metadata(
    item: &[u8],
    descriptors: &HashMap<u8, InventoryBucketDescriptor>,
) -> Option<InventoryMetadata> {
    let native_bucket_id = *item.get(INVENTORY_BUCKET_ID_OFFSET)?;
    let descriptor = descriptors.get(&native_bucket_id)?;
    let max_stack_size = i32_at(item, INVENTORY_MAX_STACK_SIZE_OFFSET)
        .ok()
        .and_then(|size| u32::try_from(size).ok())
        .filter(|size| *size > 0);
    let stackability = if *item.get(INVENTORY_INSTANCED_OFFSET)? == 0 {
        ItemStackability::Stackable
    } else {
        ItemStackability::Instanced
    };
    Some(InventoryMetadata {
        scope: descriptor.scope,
        native_bucket_id,
        stackability,
        max_stack_size,
        bucket_capacity: Some(descriptor.capacity),
    })
}

pub(in crate::catalog) const fn bucket_hash(bucket: u8) -> Option<u64> {
    Some(match bucket {
        0 => 1_498_876_634,
        1 => 2_465_295_065,
        2 => 953_998_645,
        3 => 3_448_274_439,
        4 => 3_551_918_588,
        5 => 14_239_492,
        6 => 20_886_954,
        7 => 1_585_787_867,
        8 => 4_023_194_814,
        9 => 2_025_709_351,
        10 => 284_967_655,
        16 => 3_284_755_031,
        17 => 4_292_445_962,
        27 => 4_274_335_291,
        41 => 2_401_704_334,
        47 => 3_683_254_069,
        49 => 0x59CA_1EA2,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_descriptors_validate_scope_capacity_and_identity() {
        let mut table = vec![0_u8; INVENTORY_BUCKET_FIRST_DESCRIPTOR + 2 * 36];
        table[INVENTORY_BUCKET_COUNT_OFFSET..INVENTORY_BUCKET_COUNT_OFFSET + 4]
            .copy_from_slice(&2_i32.to_le_bytes());
        for (index, bucket, first_slot, capacity, scope) in
            [(0, 14, 0_i32, 40_i32, 1_u8), (1, 49, 40, 20, 0)]
        {
            let base = INVENTORY_BUCKET_FIRST_DESCRIPTOR + index * 36;
            table[base] = bucket;
            table[base + INVENTORY_BUCKET_FIRST_SLOT_OFFSET
                ..base + INVENTORY_BUCKET_FIRST_SLOT_OFFSET + 4]
                .copy_from_slice(&first_slot.to_le_bytes());
            table[base + INVENTORY_BUCKET_SLOT_COUNT_OFFSET
                ..base + INVENTORY_BUCKET_SLOT_COUNT_OFFSET + 4]
                .copy_from_slice(&capacity.to_le_bytes());
            table[base + INVENTORY_BUCKET_SCOPE_OFFSET] = scope;
        }

        let descriptors = parse_inventory_bucket_descriptors(&table).unwrap();
        assert_eq!(
            descriptors[&14],
            InventoryBucketDescriptor {
                scope: InventoryScope::Profile,
                capacity: 40,
            }
        );

        let second = INVENTORY_BUCKET_FIRST_DESCRIPTOR + 36;
        table[second] = 14;
        assert!(parse_inventory_bucket_descriptors(&table).is_err());
        table[second] = 49;
        table[second + INVENTORY_BUCKET_FIRST_SLOT_OFFSET
            ..second + INVENTORY_BUCKET_FIRST_SLOT_OFFSET + 4]
            .copy_from_slice(&340_i32.to_le_bytes());
        assert!(parse_inventory_bucket_descriptors(&table).is_err());
    }

    #[test]
    fn metadata_uses_native_item_quantity_fields() {
        let descriptors = HashMap::from([(
            42,
            InventoryBucketDescriptor {
                scope: InventoryScope::Profile,
                capacity: 17,
            },
        )]);
        let mut item = vec![0_u8; INVENTORY_INSTANCED_OFFSET + 1];
        item[INVENTORY_MAX_STACK_SIZE_OFFSET..INVENTORY_MAX_STACK_SIZE_OFFSET + 4]
            .copy_from_slice(&999_i32.to_le_bytes());
        item[INVENTORY_BUCKET_ID_OFFSET] = 42;

        let metadata = item_inventory_metadata(&item, &descriptors).unwrap();
        assert_eq!(
            metadata,
            InventoryMetadata {
                scope: InventoryScope::Profile,
                native_bucket_id: 42,
                stackability: ItemStackability::Stackable,
                max_stack_size: Some(999),
                bucket_capacity: Some(17),
            }
        );
        assert_eq!(metadata.scope.label(), "Profile");
        assert_eq!(metadata.scope.array_capacity(), Some(701));
        assert_eq!(metadata.authored_row_capacity(), Some(17));
        assert!(metadata.is_profile_items_candidate());
        item[INVENTORY_INSTANCED_OFFSET] = 1;
        let metadata = item_inventory_metadata(&item, &descriptors).unwrap();
        assert_eq!(metadata.stackability, ItemStackability::Instanced);
        assert!(!metadata.is_profile_items_candidate());
    }

    #[test]
    fn bucket_labels_cover_known_and_unknown_native_ids() {
        let metadata = |scope, native_bucket_id| InventoryMetadata {
            scope,
            native_bucket_id,
            ..InventoryMetadata::default()
        };
        assert_eq!(
            metadata(InventoryScope::Profile, 14).bucket_label(),
            "Shaders"
        );
        assert_eq!(
            metadata(InventoryScope::Character, 0).bucket_label(),
            "Kinetic weapons"
        );
        assert_eq!(
            metadata(InventoryScope::Profile, 99).bucket_label(),
            "Profile bucket 99"
        );
        assert_eq!(
            InventoryMetadata::default().bucket_label(),
            "Unknown bucket"
        );
        assert_eq!(bucket_hash(49), Some(0x59CA_1EA2));
    }
}
