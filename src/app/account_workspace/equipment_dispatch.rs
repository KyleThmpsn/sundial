//! Equipment operations routed to the selected account backend.

use super::*;

impl WorkspaceDocument {
    /// The JSON version does not upgrade the independent SQLite account contract.
    pub(in crate::app) fn supports_v13_account(&self) -> bool {
        self.uses_json_account() && crate::app::inventory::schema_mode(self.json()).supports_v13()
    }

    pub(in crate::app) fn equipment_slots(
        &self,
    ) -> &'static [crate::account_contract::EquipmentSlotContract] {
        if self.uses_json_account() {
            crate::app::inventory::schema_mode(self.json()).equipment_slots()
        } else {
            crate::account_contract::EQUIPMENT_SLOTS
        }
    }
}

pub(in crate::app) fn equipped_item_snapshots(
    document: &WorkspaceDocument,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    match &document.account {
        AccountDocument::Json(_) => {
            crate::app::equipment::equipped_item_snapshots(&document.json, character_index)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::equipped_item_snapshots(document, character_index)
        }
        AccountDocument::Blocked(_) => Err(blocked_string(document)),
    }
}

pub(in crate::app) fn equip_definition(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if !document
        .equipment_slots()
        .iter()
        .any(|(known, _, _)| *known == slot)
    {
        return Err(format!(
            "Unknown equipment slot for the active account source: {slot}"
        ));
    }
    if !crate::account_contract::definition_available(
        definition_hash,
        document.supports_v13_account(),
    ) {
        return Err("The emote wheel requires a v13+ JSON account".into());
    }
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::equipment::equip_definition(
            &mut document.json,
            character_index,
            slot,
            definition_hash,
            default_plugs,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::equip_definition(
            document,
            character_index,
            slot,
            definition_hash,
            default_plugs,
        ),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(in crate::app) fn set_equipment_item_level(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
    level: i64,
) -> Result<(), String> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::equipment::set_equipment_item_level(
            &mut document.json,
            character_index,
            slot,
            level,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::set_equipment_item_level(document, character_index, slot, level)
        }
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(in crate::app) fn set_equipment_item_flags(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::equipment::set_equipment_item_flags(
            &mut document.json,
            character_index,
            slot,
            flags,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::set_equipment_item_flags(document, character_index, slot, flags)
        }
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn set_equipment_item_plug(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
    socket_index: usize,
    default_plugs: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::equipment::set_equipment_item_plug(
            &mut document.json,
            character_index,
            slot,
            socket_index,
            default_plugs,
            hash,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::set_equipment_item_plug(
            document,
            character_index,
            slot,
            socket_index,
            default_plugs,
            hash,
        ),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(in crate::app) fn set_weapon_slot_empty(
    document: &mut WorkspaceDocument,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    match &mut document.account {
        AccountDocument::Json(_) => {
            crate::app::equipment::set_weapon_slot_empty(&mut document.json, character_index, slot)
        }
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => {
            sqlite::set_weapon_slot_empty(document, character_index, slot)
        }
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}

pub(in crate::app) fn restore_class_armor(
    document: &mut WorkspaceDocument,
    source_character_index: usize,
    destination_character_index: usize,
) -> Result<bool, String> {
    match &mut document.account {
        AccountDocument::Json(_) => crate::app::equipment::restore_class_armor_from_character(
            &mut document.json,
            source_character_index,
            destination_character_index,
        ),
        #[cfg(feature = "sqlite-account")]
        AccountDocument::Sqlite(document) => sqlite::restore_class_armor(
            document,
            source_character_index,
            destination_character_index,
        ),
        AccountDocument::Blocked(reason) => Err(reason.clone()),
    }
}
