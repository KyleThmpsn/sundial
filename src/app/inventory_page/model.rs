//! View models shared by the profile and character inventory pages.

use std::{collections::HashMap, sync::Arc};

use crate::catalog::{InventoryMetadata, InventoryScope, ItemDef};

use super::super::{
    equipment::EquippedItemSnapshot,
    inventory::{InventoryItemAction, InventoryItemSnapshot},
};

pub(super) const BUCKET_HEADER_SIZE_DELTA: f32 = 1.0;
pub(super) const TRANSFER_DESTINATION_ROW_HEIGHT: f32 = 28.0;
pub(super) const TRANSFER_DESTINATION_ROW_SPACING: f32 = 2.0;
pub(super) const TRANSFER_FOOTER_CHROME_HEIGHT: f32 = 28.0;
pub(super) const TRANSFER_PICKER_MIN_LIST_HEIGHT: f32 = 176.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum ProfileInventorySection {
    #[default]
    SharedItems,
    DismantleRewards,
    PendingRewards,
}

#[derive(Clone)]
pub(super) struct ResolvedDefinition {
    pub(super) name: String,
    pub(super) type_name: String,
    pub(super) metadata: InventoryMetadata,
    pub(super) item: Option<Arc<ItemDef>>,
}

pub(super) struct BucketUsage {
    pub(super) counts: HashMap<u8, usize>,
    pub(super) unresolved_count: usize,
    pub(super) occupancy_complete: bool,
}

pub(super) struct CharacterTransferTarget {
    pub(super) character_index: usize,
    pub(super) label: String,
    pub(super) class_type: u64,
    pub(super) stored_count: Option<usize>,
    pub(super) usage: Option<BucketUsage>,
    pub(super) unavailable_reason: Option<String>,
}

pub(super) struct CharacterInventoryCardContext<'a> {
    pub(super) bucket_usage: &'a BucketUsage,
    pub(super) transfer_targets: &'a [CharacterTransferTarget],
    pub(super) occupied_equipment_slots: &'a [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BucketKey {
    pub(super) scope: InventoryScope,
    pub(super) native_id: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::app) struct InventoryItemUiId {
    pub(super) character_index: usize,
    pub(super) instance_soid: u64,
    pub(super) duplicate_ordinal: Option<usize>,
}

#[cfg(test)]
impl InventoryItemUiId {
    pub(in crate::app) const fn new(
        character_index: usize,
        instance_soid: u64,
        duplicate_ordinal: Option<usize>,
    ) -> Self {
        Self {
            character_index,
            instance_soid,
            duplicate_ordinal,
        }
    }
}

pub(in crate::app) struct CharacterInventoryEditorContext {
    pub(super) editable: bool,
    pub(super) class_type: u64,
}

impl CharacterInventoryEditorContext {
    pub(in crate::app) const fn editable(&self) -> bool {
        self.editable
    }

    pub(in crate::app) const fn class_type(&self) -> u64 {
        self.class_type
    }
}

pub(super) struct ItemBucket<T> {
    pub(super) key: BucketKey,
    pub(super) label: String,
    pub(super) capacity: Option<u16>,
    pub(super) addable: bool,
    pub(super) items: Vec<T>,
}

#[derive(Clone)]
pub(super) enum CharacterInventoryEntry {
    Equipped(EquippedItemSnapshot),
    Stored {
        snapshot: InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
    },
}

pub(super) enum CharacterInventoryItemRequest {
    Apply(Vec<InventoryItemAction>),
    Equip(&'static str),
    MoveTo(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CharacterTransferDestination {
    pub(super) character_index: usize,
    pub(super) label: String,
    pub(super) detail: String,
    pub(super) enabled: bool,
    pub(super) tooltip: String,
}

impl CharacterInventoryEntry {
    pub(super) fn definition_hash(&self) -> Option<u64> {
        match self {
            Self::Equipped(snapshot) => snapshot.definition_hash,
            Self::Stored { snapshot, .. } => Some(u64::from(snapshot.definition_hash)),
        }
    }

    pub(super) const fn is_stored(&self) -> bool {
        matches!(self, Self::Stored { .. })
    }

    pub(super) fn level(&self) -> i64 {
        match self {
            Self::Equipped(snapshot) => snapshot.level.unwrap_or_default(),
            Self::Stored { snapshot, .. } => snapshot.level as i64,
        }
    }

    pub(super) fn locked(&self) -> bool {
        let flags = match self {
            Self::Equipped(snapshot) => snapshot.flags.unwrap_or_default(),
            Self::Stored { snapshot, .. } => snapshot.flags.unwrap_or_default(),
        };
        flags & super::super::inventory::INVENTORY_FLAG_LOCKED != 0
    }
}
