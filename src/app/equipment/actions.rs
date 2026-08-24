use super::*;

impl SundialApp {
    pub(super) fn equipment_mutation_allowed(&mut self) -> bool {
        if super::inventory::schema_mode(&self.document).can_mutate_equipment() {
            true
        } else {
            self.set_status(
                "Equipment editing is disabled for this settings schema",
                true,
            );
            false
        }
    }

    pub(super) fn equipment_flags_mutation_allowed(&mut self) -> bool {
        if super::inventory::schema_mode(&self.document).can_mutate_equipment_flags() {
            true
        } else {
            self.set_status(
                format!(
                    "Equipment lock-state editing requires a writable settings schema {} or newer",
                    super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
                ),
                true,
            );
            false
        }
    }

    pub(super) fn select_item(&mut self, character: usize, slot: &str, item: &ItemDef) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match equip_definition(
            &mut self.document,
            character,
            slot,
            item.hash,
            &item.default_plugs,
        ) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(format!("Equipped {}", item.name), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(in crate::app) fn equip_stored_item(
        &mut self,
        location: super::inventory::InventoryItemLocation,
        slot: &str,
    ) -> bool {
        if !self.equipment_mutation_allowed() {
            return false;
        }
        if !super::inventory::schema_mode(&self.document).can_mutate_character_inventory() {
            self.set_status("Equipping a stored item requires settings schema 6", true);
            return false;
        }

        let snapshot =
            match super::inventory::character_inventory(&self.document, location.character_index) {
                Ok(Some(items)) => items.into_iter().find(|item| item.location == location),
                Ok(None) => None,
                Err(error) => {
                    self.set_status(error.to_string(), true);
                    return false;
                }
            };
        let Some(snapshot) = snapshot else {
            self.set_status("The selected inventory item no longer exists", true);
            return false;
        };
        let Some((_, _, bucket)) = SLOTS
            .iter()
            .find(|(known_slot, _, _)| *known_slot == slot)
            .copied()
        else {
            self.set_status(format!("Unknown equipment slot: {slot}"), true);
            return false;
        };
        let Some(item) = self
            .manifest
            .item_handle_for_bucket(u64::from(snapshot.definition_hash), bucket)
        else {
            self.set_status(
                format!(
                    "The selected inventory item is not valid for the {} slot",
                    equipment_slot_label(slot)
                ),
                true,
            );
            return false;
        };
        let item_name = item.name.clone();
        match equip_inventory_item(&mut self.document, location, slot, &item) {
            Ok(replaced_item) => {
                self.dirty = true;
                let slot_label = equipment_slot_label(slot);
                self.set_status(
                if replaced_item {
                    format!(
                        "Equipped {item_name}; moved the previous {slot_label} item to inventory"
                    )
                } else {
                    format!("Equipped {item_name} in the empty {slot_label} slot")
                },
                false,
            );
                for id_scope in ["characters-equipment", "character-inventory-equipped"] {
                    self.searches.insert(
                        format!("{id_scope}:{}:{slot}", location.character_index),
                        String::new(),
                    );
                    let plug_prefix = format!(
                        "plug-search:{id_scope}:{}:{slot}:",
                        location.character_index
                    );
                    self.plug_searches
                        .retain(|key, _| !key.starts_with(&plug_prefix));
                }
                true
            }
            Err(error) => {
                self.set_status(error, true);
                false
            }
        }
    }

    pub(super) fn select_subclass_item(&mut self, character: usize, item: &ItemDef) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match equip_subclass_with_default_abilities(&mut self.document, character, item) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(format!("Equipped {}", item.name), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn empty_weapon(&mut self, character: usize, slot: &str) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match set_weapon_slot_empty(&mut self.document, character, slot) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Set the {} slot to empty", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn unequip_weapon(&mut self, character: usize, slot: &str) {
        if !WEAPON_SLOTS.contains(&slot) {
            self.set_status(
                format!(
                    "The {} slot cannot be unequipped",
                    equipment_slot_label(slot)
                ),
                true,
            );
            return;
        }
        if !self.equipment_mutation_allowed() {
            return;
        }
        if !super::inventory::schema_mode(&self.document).can_mutate_character_inventory() {
            self.set_status("Unequipping to inventory requires settings schema 6", true);
            return;
        }

        match super::inventory::move_equipment_item_to_inventory(
            &mut self.document,
            character,
            slot,
        ) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Moved the {} item to inventory", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error.to_string(), true),
        }
    }

    pub(super) fn select_plug(
        &mut self,
        character: usize,
        slot: &str,
        socket_index: usize,
        socket_label: &str,
        default_plugs: &[Option<String>],
        hash: Option<u64>,
    ) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match set_equipment_item_plug(
            &mut self.document,
            character,
            slot,
            socket_index,
            default_plugs,
            hash,
        ) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(format!("Updated {slot} {socket_label}"), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn select_equipment_level(&mut self, character: usize, slot: &str, level: i64) {
        if !self.equipment_mutation_allowed() {
            return;
        }
        match set_equipment_item_level(&mut self.document, character, slot, level) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Updated {} power", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }

    pub(super) fn select_equipment_flags(
        &mut self,
        character: usize,
        slot: &str,
        flags: Option<u8>,
    ) {
        if !self.equipment_flags_mutation_allowed() {
            return;
        }
        match set_equipment_item_flags(&mut self.document, character, slot, flags) {
            Ok(()) => {
                self.dirty = true;
                self.set_status(
                    format!("Updated {} item state", equipment_slot_label(slot)),
                    false,
                );
            }
            Err(error) => self.set_status(error, true),
        }
    }
}
