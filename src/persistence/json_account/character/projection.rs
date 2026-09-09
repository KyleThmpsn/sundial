//! Lossless projection: preserve raw rows and rewrite only recorded changes.
use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sundial_account::{Character, CharacterMetadata, EquipmentSlot, ItemInstance, ItemPlugs};

use super::{
    EquipmentOrigin, ItemFieldChanges, JsonAccountError, JsonCharacterAdapter, JsonCharacterResult,
    MetadataFieldChanges, PlugProjection,
};

impl JsonCharacterAdapter {
    pub(super) fn project(&self, document: &Value) -> JsonCharacterResult<Value> {
        if document.get("version").and_then(Value::as_u64) != self.source_schema_version {
            return Err(JsonAccountError::format(
                "/version",
                "the JSON schema changed after the character projection was loaded",
            ));
        }
        let mut candidate = document.clone();
        let characters = candidate_characters_mut(&mut candidate)?;
        for character in self.state.characters() {
            let character_index = *self.character_indices.get(&character.id).ok_or_else(|| {
                JsonAccountError::format(
                    "/state/characters",
                    "a loaded character lost its JSON index",
                )
            })?;
            let row = characters
                .get_mut(character_index)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| {
                    JsonAccountError::format(
                        format!("/state/characters/{character_index}"),
                        "character must be an object",
                    )
                })?;

            if let Some(changes) = self.metadata_changes.get(&character.id) {
                let metadata = character.metadata.ok_or_else(|| {
                    JsonAccountError::format(
                        format!("/state/characters/{character_index}"),
                        "character metadata was not loaded",
                    )
                })?;
                project_metadata(row, metadata, *changes);
            }

            let inventory_ids = character
                .inventory
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>();
            let inventory_changed = self
                .inventory_origins
                .get(&character.id)
                .is_none_or(|origin| *origin != inventory_ids)
                || inventory_ids
                    .iter()
                    .any(|item_id| self.item_changes.contains_key(item_id));
            if inventory_changed {
                row.insert(
                    "inventory".into(),
                    Value::Array(
                        character
                            .inventory
                            .iter()
                            .map(|item| Value::Object(self.project_item(item)))
                            .collect(),
                    ),
                );
            }

            let slots = self.character_slots(character);
            if slots.iter().any(|slot| self.slot_changed(character, slot)) {
                let equipment = row
                    .get_mut("equipment")
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| {
                        JsonAccountError::format(
                            format!("/state/characters/{character_index}/equipment"),
                            "equipment must be an object",
                        )
                    })?;
                for slot in slots {
                    if !self.slot_changed(character, &slot) {
                        continue;
                    }
                    match character.equipment.get(&slot) {
                        None => {
                            equipment.remove(slot.as_str());
                        }
                        Some(None) => {
                            equipment.insert(slot.as_str().into(), Value::Null);
                        }
                        Some(Some(item)) => {
                            equipment.insert(
                                slot.as_str().into(),
                                Value::Object(self.project_item(item)),
                            );
                        }
                    }
                }
            }
        }
        Ok(candidate)
    }

    fn character_slots(&self, character: &Character) -> BTreeSet<EquipmentSlot> {
        character
            .equipment
            .keys()
            .cloned()
            .chain(
                self.equipment_origins
                    .keys()
                    .filter(|(character_id, _)| *character_id == character.id)
                    .map(|(_, slot)| slot.clone()),
            )
            .collect()
    }

    fn slot_changed(&self, character: &Character, slot: &EquipmentSlot) -> bool {
        let origin = self
            .equipment_origins
            .get(&(character.id, slot.clone()))
            .copied();
        let current = character.equipment.get(slot).map(|item| match item {
            Some(item) => EquipmentOrigin::Item(item.id),
            None => EquipmentOrigin::Null,
        });
        origin != current
            || current.is_some_and(|origin| match origin {
                EquipmentOrigin::Item(item_id) => {
                    self.item_changes.contains_key(&item_id)
                        || self.item_copies.contains_key(&item_id)
                }
                EquipmentOrigin::Null => false,
            })
    }

    pub(super) fn project_item(&self, item: &ItemInstance) -> Map<String, Value> {
        let mut row = self.item_rows.get(&item.id).cloned().unwrap_or_default();
        if let Some(source) = self.item_copies.get(&item.id) {
            // Absent known fields are part of the copied state too. Preserve only
            // destination identity and unrelated opaque fields before overlaying.
            for key in ["definition_hash", "level", "quantity", "plugs", "flags"] {
                row.remove(key);
            }
            for (key, value) in source {
                if key != "instance_soid" {
                    row.insert(key.clone(), value.clone());
                }
            }
        }
        let changes = self.item_changes.get(&item.id).copied().unwrap_or_else(|| {
            if self.item_rows.contains_key(&item.id) {
                ItemFieldChanges::default()
            } else {
                ItemFieldChanges::ALL
            }
        });
        if !self.item_rows.contains_key(&item.id) {
            row.insert(
                "instance_soid".into(),
                Value::String(format!("0x{:016X}", item.instance_soid.get())),
            );
        }
        if changes.definition_hash {
            row.insert(
                "definition_hash".into(),
                Value::String(format!("0x{:08X}", item.definition_hash.get())),
            );
        }
        if changes.level {
            row.insert("level".into(), Value::from(item.level));
        }
        if changes.quantity {
            row.insert("quantity".into(), Value::from(item.quantity));
        }
        if changes.plugs {
            let encoded = encode_plugs(&item.plugs);
            if let Some(PlugProjection::Patch(indices)) = self.plug_projections.get(&item.id)
                && let Some(raw) = row.get_mut("plugs").and_then(Value::as_array_mut)
                && let Some(encoded) = encoded.as_array()
            {
                // A socket edit must not normalize unrelated plug representations.
                for &index in indices {
                    if raw.len() <= index {
                        raw.resize(index + 1, Value::Null);
                    }
                    raw[index] = encoded.get(index).cloned().unwrap_or(Value::Null);
                }
            } else {
                row.insert("plugs".into(), encoded);
            }
        }
        if changes.flags {
            if let Some(flags) = item.flags {
                row.insert("flags".into(), Value::from(flags));
            } else {
                row.remove("flags");
            }
        }
        row
    }
}

fn project_metadata(
    row: &mut Map<String, Value>,
    metadata: CharacterMetadata,
    changes: MetadataFieldChanges,
) {
    for (changed, field, value) in [
        (changes.race, "race", metadata.race),
        (changes.gender, "gender", metadata.gender),
        (changes.class_type, "class", metadata.class_type),
        (
            changes.movement,
            "movement_ability",
            metadata.abilities.movement,
        ),
        (
            changes.grenade,
            "grenade_ability",
            metadata.abilities.grenade,
        ),
        (
            changes.super_ability,
            "super_ability",
            metadata.abilities.super_ability,
        ),
        (changes.melee, "melee_ability", metadata.abilities.melee),
        (
            changes.class_ability,
            "class_ability",
            metadata.abilities.class_ability,
        ),
    ] {
        if changed {
            row.insert(field.into(), Value::from(value));
        }
    }
}

fn candidate_characters_mut(document: &mut Value) -> JsonCharacterResult<&mut Vec<Value>> {
    document
        .get_mut("state")
        .and_then(Value::as_object_mut)
        .and_then(|state| state.get_mut("characters"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| JsonAccountError::format("/state/characters", "characters must be an array"))
}

fn encode_plugs(plugs: &ItemPlugs) -> Value {
    match plugs {
        ItemPlugs::NativeDefaults => Value::Null,
        ItemPlugs::Authored(plugs) => Value::Array(
            plugs
                .iter()
                .map(|plug| {
                    plug.map_or(Value::Null, |hash| {
                        Value::String(format!("0x{:08X}", hash.get()))
                    })
                })
                .collect(),
        ),
    }
}
