use crate::app::account_workspace as account;

use super::*;
use sundial_account as account_domain;

use crate::persistence::json_account::{JsonAccountError, JsonCharacterAdapter};

/// Returns present equipment rows in the order supported by the JSON schema.
///
/// A malformed row is represented by an [`EquippedItemSnapshot`] with issues instead
/// of being discarded. Errors are reserved for an unusable character/equipment path.
pub(in crate::app) fn equipped_item_snapshots(
    document: &Value,
    character_index: usize,
) -> Result<Vec<EquippedItemSnapshot>, String> {
    let characters = document
        .pointer("/state/characters")
        .ok_or("Missing /state/characters")?
        .as_array()
        .ok_or("/state/characters must be an array")?;
    let character = characters
        .get(character_index)
        .ok_or_else(|| format!("Missing character at index {character_index}"))?
        .as_object()
        .ok_or_else(|| format!("Character {character_index} must be an object"))?;
    let Some(equipment_value) = character.get("equipment") else {
        return Ok(Vec::new());
    };
    let equipment = equipment_value
        .as_object()
        .ok_or_else(|| format!("Character {character_index} equipment must be an object"))?;
    let mode = super::inventory::schema_mode(document);
    let allow_unknown_members = mode.is_future();

    Ok(mode
        .equipment_slots()
        .iter()
        .filter_map(|&(slot, slot_label, bucket_hash)| {
            let value = equipment.get(slot)?;
            (!value.is_null()).then(|| {
                equipped_item_snapshot(
                    slot,
                    slot_label,
                    bucket_hash,
                    value,
                    allow_unknown_members,
                    mode.item_flag_mask(),
                )
            })
        })
        .collect())
}

fn equipped_item_snapshot(
    slot: &'static str,
    slot_label: &'static str,
    bucket_hash: u64,
    value: &Value,
    allow_unknown_members: bool,
    flag_mask: u8,
) -> EquippedItemSnapshot {
    let raw_item_text = compact_json_text(value);
    let Some(item) = value.as_object() else {
        return EquippedItemSnapshot {
            slot,
            slot_label,
            bucket_hash,
            raw_item_text: raw_item_text.clone(),
            definition_hash: None,
            definition_text: "<missing>".to_owned(),
            instance_soid: None,
            instance_soid_text: "<missing>".to_owned(),
            level: None,
            quantity: None,
            flags: None,
            plugs: EquippedItemPlugs::Malformed(raw_item_text),
            issues: vec!["equipment row must be an object".to_owned()],
        };
    };

    let mut issues = Vec::new();
    if !allow_unknown_members {
        for member in item.keys() {
            if !super::inventory::KNOWN_ITEM_MEMBERS.contains(&member.as_str()) {
                issues.push(format!("unknown item member {member}"));
            }
        }
    }

    let definition_value = item.get("definition_hash");
    let definition_hash = definition_value.and_then(parse_unsigned_value);
    let definition_text =
        definition_hash.map_or_else(|| field_display_text(definition_value), format_hash_hex);
    match (definition_value, definition_hash) {
        (None, _) => issues.push("missing definition_hash".to_owned()),
        (Some(_), None) => {
            issues.push("definition_hash must be an unsigned integer or a 0x hex string".to_owned())
        }
        (_, Some(hash)) if u32::try_from(hash).is_err() => {
            issues.push("definition_hash must fit in an unsigned 32-bit value".to_owned());
        }
        (_, Some(hash)) if hash == u64::from(account_domain::NO_DEFINITION_HASH.get()) => {
            issues.push("definition_hash is the engine no-definition sentinel".to_owned());
        }
        _ => {}
    }

    let soid_value = item.get("instance_soid");
    let instance_soid = soid_value.and_then(parse_unsigned_value);
    let instance_soid_text = instance_soid.map_or_else(
        || field_display_text(soid_value),
        |soid| format!("0x{soid:016X}"),
    );
    match (soid_value, instance_soid) {
        (None, _) => issues.push("missing instance_soid".to_owned()),
        (Some(_), None) => {
            issues.push("instance_soid must be an unsigned integer or a 0x hex string".to_owned());
        }
        (_, Some(0)) => issues.push("instance_soid must not be zero".to_owned()),
        _ => {}
    }

    let level = item.get("level").and_then(Value::as_i64);
    match item.get("level") {
        None => issues.push("missing level".to_owned()),
        Some(_) if level.is_none() => {
            issues.push("level must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !level.is_some_and(|value| (0..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("level must be a non-negative signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let quantity = item.get("quantity").and_then(Value::as_i64);
    match item.get("quantity") {
        None => issues.push("missing quantity".to_owned()),
        Some(_) if quantity.is_none() => {
            issues.push("quantity must be a signed 32-bit integer".to_owned());
        }
        Some(_) if !quantity.is_some_and(|value| (1..=i64::from(i32::MAX)).contains(&value)) => {
            issues.push("quantity must be a positive signed 32-bit integer".to_owned());
        }
        _ => {}
    }

    let plugs = equipped_item_plugs(item.get("plugs"), &mut issues);

    let flags = item
        .get("flags")
        .and_then(parse_unsigned_value)
        .and_then(|flags| u8::try_from(flags).ok());
    if let Some(flags_value) = item.get("flags")
        && parse_unsigned_value(flags_value).is_none_or(|flags| flags > u64::from(flag_mask))
    {
        issues.push(format!("flags must be between 0 and {flag_mask}"));
    }

    EquippedItemSnapshot {
        slot,
        slot_label,
        bucket_hash,
        raw_item_text,
        definition_hash,
        definition_text,
        instance_soid,
        instance_soid_text,
        level,
        quantity,
        flags,
        plugs,
        issues,
    }
}

fn equipped_item_plugs(value: Option<&Value>, issues: &mut Vec<String>) -> EquippedItemPlugs {
    let Some(value) = value else {
        issues.push("missing plugs".to_owned());
        return EquippedItemPlugs::Missing;
    };
    if value.is_null() {
        return EquippedItemPlugs::NativeDefaults;
    }
    let Some(plugs) = value.as_array() else {
        let raw = compact_json_text(value);
        issues.push("plugs must be null or an array".to_owned());
        return EquippedItemPlugs::Malformed(raw);
    };
    if plugs.len() > super::inventory::MAX_ITEM_PLUGS {
        issues.push(format!(
            "plugs cannot contain more than {} entries",
            super::inventory::MAX_ITEM_PLUGS
        ));
    }
    let values = plugs
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if value.is_null() {
                return EquippedPlugValue::Empty;
            }
            let Some(hash) = parse_unsigned_value(value) else {
                let raw = compact_json_text(value);
                issues.push(format!(
                    "plug {index} must be null, an unsigned integer, or a 0x hex string"
                ));
                return EquippedPlugValue::Malformed(raw);
            };
            if u32::try_from(hash).is_err() {
                issues.push(format!(
                    "plug {index} hash must fit in an unsigned 32-bit value"
                ));
            } else if hash == u64::from(account_domain::NO_DEFINITION_HASH.get()) {
                issues.push(format!(
                    "plug {index} hash is the engine no-definition sentinel"
                ));
            }
            EquippedPlugValue::Hash(hash)
        })
        .collect();
    EquippedItemPlugs::Authored(values)
}

pub(super) fn field_display_text(value: Option<&Value>) -> String {
    match value {
        None => "<missing>".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(value) => compact_json_text(value),
    }
}

fn compact_json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

fn default_plug_values(defaults: &[Option<String>]) -> Vec<Value> {
    defaults
        .iter()
        .map(|plug| plug.clone().map_or(Value::Null, Value::String))
        .collect()
}

pub(in crate::app) fn equipment_slot_label(slot: &str) -> &str {
    crate::account_contract::ALL_EQUIPMENT_SLOTS
        .iter()
        .find_map(|(name, label, _)| (*name == slot).then_some(*label))
        .unwrap_or(slot)
}

#[cfg(test)]
fn next_instance_soid(document: &Value) -> Option<u64> {
    super::inventory::allocate_instance_soid(document).ok()
}

fn domain_default_plugs(
    default_plugs: &[Option<String>],
) -> Result<account_domain::ItemPlugs, String> {
    default_plugs
        .iter()
        .map(|plug| {
            plug.as_deref()
                .map(|hash| {
                    parse_hash_hex(hash)
                        .and_then(|hash| u32::try_from(hash).ok())
                        .map(account_domain::DefinitionHash::new)
                        .ok_or_else(|| format!("Invalid equipment default plug hash: {hash}"))
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .map(account_domain::ItemPlugs::Authored)
}

fn equipment_character_id(
    adapter: &JsonCharacterAdapter,
    character_index: usize,
) -> Result<account_domain::EntityId, String> {
    adapter
        .character_id_at_index(character_index)
        .ok_or_else(|| format!("Character {} does not exist", character_index + 1))
}

fn equipment_item<'a>(
    adapter: &'a JsonCharacterAdapter,
    character_id: account_domain::EntityId,
    slot: &str,
) -> Option<&'a account_domain::ItemInstance> {
    adapter
        .state()
        .characters()
        .iter()
        .find(|character| character.id == character_id)
        .and_then(|character| {
            character
                .equipment
                .get(&account_domain::EquipmentSlot::new(slot))
        })
        .and_then(Option::as_ref)
}

fn equipment_item_target_error(slot: &str) -> String {
    format!(
        "The {} slot must contain an item object before it can be edited",
        equipment_slot_label(slot)
    )
}

fn load_existing_equipment_item(
    document: &Value,
    character_index: usize,
    slot: &str,
    require_valid_plugs: bool,
    map_load_error: impl FnOnce(JsonAccountError) -> String,
) -> Result<(JsonCharacterAdapter, account_domain::EntityId), String> {
    if !super::inventory::schema_mode(document)
        .equipment_slots()
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
    {
        return Err(format!("Unknown equipment slot: {slot}"));
    }
    let adapter = if require_valid_plugs {
        JsonCharacterAdapter::load_equipment_plug_patch_slot(document, character_index, slot)
    } else {
        JsonCharacterAdapter::load_equipment_patch_slot(document, character_index, slot)
    }
    .map_err(map_load_error)?;
    let character_id = equipment_character_id(&adapter, character_index)
        .map_err(|_| equipment_item_target_error(slot))?;
    equipment_item(&adapter, character_id, slot)
        .ok_or_else(|| equipment_item_target_error(slot))?;
    Ok((adapter, character_id))
}

fn apply_equipment_command(
    document: &mut Value,
    adapter: JsonCharacterAdapter,
    command: account_domain::CharacterCommand,
) -> Result<(), String> {
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| error.to_string())?;
    *document = candidate;
    Ok(())
}

pub(in crate::app) fn inferred_item_level(document: &Value, character_index: usize) -> i64 {
    document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .and_then(|equipment| {
            equipment
                .values()
                .filter_map(|item| {
                    item.get("level")
                        .and_then(Value::as_i64)
                        .filter(|level| (1..=i64::from(i32::MAX)).contains(level))
                })
                .max()
        })
        .unwrap_or(106)
}

pub(in crate::app) fn equip_definition(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    definition_hash: u64,
    default_plugs: &[Option<String>],
) -> Result<(), String> {
    if !super::inventory::schema_mode(document)
        .equipment_slots()
        .iter()
        .any(|(known, _, _)| *known == slot)
    {
        return Err(format!("Unknown equipment slot: {slot}"));
    }
    if !crate::account_contract::definition_available(
        definition_hash,
        super::inventory::schema_mode(document).supports_v13(),
    ) {
        return Err("The emote wheel requires JSON schema 13 or newer".into());
    }
    let definition_hash = u32::try_from(definition_hash).map_err(|_| {
        format!(
            "Cannot equip an invalid definition hash in the {} slot",
            equipment_slot_label(slot)
        )
    })?;
    let plugs = domain_default_plugs(default_plugs)?;
    let adapter =
        JsonCharacterAdapter::load_for_equipment_definition(document, character_index, slot)
            .map_err(|error| {
                let slot_path = format!("/state/characters/{character_index}/equipment/{slot}");
                if error.path() == Some(slot_path.as_str()) {
                    format!(
                        "The {} slot must be an object or null before it can be changed",
                        equipment_slot_label(slot)
                    )
                } else {
                    error.to_string()
                }
            })?;
    let character_id = equipment_character_id(&adapter, character_index)?;
    let domain_slot = account_domain::EquipmentSlot::new(slot);
    let command = if equipment_item(&adapter, character_id, slot).is_some() {
        account_domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: domain_slot,
            update: account_domain::ItemUpdate::SetDefinitionAndPlugs {
                definition_hash: account_domain::DefinitionHash::new(definition_hash),
                plugs,
            },
        }
    } else {
        let first_instance_soid = account_domain::InstanceSoid::try_from_u64(
            super::inventory::GENERATED_INSTANCE_SOID_START,
        )
        .expect("the generated instance SOID range starts at a nonzero value");
        let instance_soid = adapter
            .state()
            .next_available_instance_soid(first_instance_soid)
            .map_err(|_| "Could not allocate a unique instance SOID for the selected item")?;
        account_domain::CharacterCommand::SetEquipmentItem {
            character_id,
            slot: domain_slot,
            item: Some(account_domain::ItemInstance {
                id: adapter.next_entity_id(),
                instance_soid,
                definition_hash: account_domain::DefinitionHash::new(definition_hash),
                level: i32::try_from(inferred_item_level(document, character_index))
                    .expect("inferred item levels are valid signed 32-bit values"),
                quantity: 1,
                plugs,
                flags: None,
            }),
        }
    };
    apply_equipment_command(document, adapter, command)
}

/// Equips a subclass and resets the character's coordinated ability fields as one edit.
///
/// Work is performed on a clone so a malformed equipment or character path cannot leave
/// the subclass and ability selections out of sync.
pub(in crate::app) fn equip_subclass_with_default_abilities(
    document: &mut super::super::account_workspace::WorkspaceDocument,
    character_index: usize,
    item: &ItemDef,
    allow_cross_class_subclasses: bool,
) -> Result<(), String> {
    let subclass_bucket = SLOTS
        .iter()
        .find_map(|(slot, _, bucket)| (*slot == "subclass").then_some(*bucket))
        .expect("SLOTS must contain the subclass slot");
    if item.bucket_hash != subclass_bucket {
        return Err("The selected definition is not a subclass".to_owned());
    }
    let class_type = u64::from(account::character_metadata(document, character_index)?.class_type);
    if !item_class_is_compatible(item, class_type, allow_cross_class_subclasses) {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let mut candidate = document.clone();
    account::equip_definition(
        &mut candidate,
        character_index,
        "subclass",
        item.hash,
        &item.default_plugs,
    )?;
    set_default_subclass_abilities(&mut candidate, character_index, class_type, item)?;
    *document = candidate;
    Ok(())
}

/// Equips one exact stored instance, moving the previously equipped instance back to inventory.
/// Subclass swaps also reset the coordinated character ability entries just like the definition
/// picker does.
pub(in crate::app) fn equip_inventory_item(
    document: &mut super::super::account_workspace::WorkspaceDocument,
    location: super::inventory::InventoryItemLocation,
    slot: &str,
    item: &ItemDef,
    allow_cross_class_subclasses: bool,
) -> Result<bool, String> {
    let expected_bucket = document
        .equipment_slots()
        .iter()
        .find_map(|(known_slot, _, bucket)| (*known_slot == slot).then_some(*bucket))
        .ok_or_else(|| format!("Unknown equipment slot: {slot}"))?;
    if item.bucket_hash != expected_bucket {
        return Err(format!(
            "{} is not valid for the {} slot",
            item.name,
            equipment_slot_label(slot)
        ));
    }

    let inventory = account::character_inventory(document, location.character_index)
        .map_err(|error| error.to_string())?
        .ok_or("The selected character has no inventory array")?;
    let snapshot = inventory
        .iter()
        .find(|snapshot| snapshot.location == location)
        .ok_or("The selected inventory item no longer exists")?;
    if u64::from(snapshot.definition_hash) != item.hash {
        return Err("The selected inventory item changed before it could be equipped".to_owned());
    }
    if snapshot.quantity != 1 {
        return Err("Only a single inventory item can be equipped at a time".to_owned());
    }

    let class_type =
        u64::from(account::character_metadata(document, location.character_index)?.class_type);
    if !item_class_is_compatible(item, class_type, allow_cross_class_subclasses) {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let persisted_subclass_abilities = (slot == "subclass")
        .then(|| account::persisted_inventory_item_abilities(document, location))
        .flatten();
    let mut candidate = document.clone();
    let replaced_item = account::swap_inventory_item_with_equipment(&mut candidate, location, slot)
        .map_err(|error| error.to_string())?;
    if slot == "subclass" && (!candidate.uses_json_account() || !candidate.supports_v13_account()) {
        if let Some(abilities) = persisted_subclass_abilities
            .filter(|abilities| subclass_abilities_are_supported(item, *abilities))
        {
            account::apply_character_updates(
                &mut candidate,
                location.character_index,
                vec![account_domain::CharacterMetadataUpdate::SetAbilities(
                    abilities,
                )],
            )?;
        } else {
            set_default_subclass_abilities(
                &mut candidate,
                location.character_index,
                class_type,
                item,
            )?;
        }
    }
    *document = candidate;
    Ok(replaced_item)
}

/// Restores the authored armor state from another character while retaining destination SOIDs.
pub(in crate::app) fn restore_class_armor_from_character(
    document: &mut Value,
    source_character_index: usize,
    destination_character_index: usize,
) -> Result<bool, String> {
    if source_character_index == destination_character_index {
        return Ok(false);
    }
    let adapter = JsonCharacterAdapter::load_equipment_copy(
        document,
        source_character_index,
        destination_character_index,
        ARMOR_SLOTS,
    )
    .map_err(|error| error.to_string())?;
    let source_character_id = equipment_character_id(&adapter, source_character_index)?;
    let destination_character_id = equipment_character_id(&adapter, destination_character_index)?;
    let command = account_domain::CharacterCommand::CopyEquipmentItems {
        source_character_id,
        destination_character_id,
        slots: ARMOR_SLOTS
            .iter()
            .map(|slot| account_domain::EquipmentSlot::new(*slot))
            .collect(),
    };
    let (_, candidate, _) = adapter
        .apply(document, command)
        .map_err(|error| error.to_string())?;
    let changed = candidate != *document;
    if changed {
        *document = candidate;
    }
    Ok(changed)
}

fn set_default_subclass_abilities(
    document: &mut super::super::account_workspace::WorkspaceDocument,
    character_index: usize,
    class_type: u64,
    item: &ItemDef,
) -> Result<(), String> {
    if document.uses_json_account() && document.supports_v13_account() {
        return Ok(());
    }
    let defaults = default_ability_values(
        class_type,
        &item.abilities,
        game_settings::schema_version(document.json()),
    );
    account::apply_character_updates(
        document,
        character_index,
        vec![account_domain::CharacterMetadataUpdate::SetAbilities(
            account_domain::CharacterAbilities {
                movement: u8::try_from(defaults.0)
                    .expect("catalog movement ability entries fit in u8"),
                grenade: u8::try_from(defaults.1)
                    .expect("catalog grenade ability entries fit in u8"),
                super_ability: u8::try_from(defaults.2)
                    .expect("catalog super ability entries fit in u8"),
                melee: u8::try_from(defaults.3).expect("catalog melee ability entries fit in u8"),
                class_ability: u8::try_from(defaults.4)
                    .expect("catalog class ability entries fit in u8"),
            },
        )],
    )?;
    Ok(())
}

fn subclass_abilities_are_supported(
    item: &ItemDef,
    selection: account_domain::CharacterAbilities,
) -> bool {
    const MAX_ABILITY_ENTRY: u8 = 63;
    if [
        selection.movement,
        selection.grenade,
        selection.super_ability,
        selection.melee,
        selection.class_ability,
    ]
    .into_iter()
    .any(|entry| entry > MAX_ABILITY_ENTRY)
    {
        return false;
    }

    let supports = |choices: &[AbilityChoice], entry: u8| {
        choices.is_empty()
            || choices
                .iter()
                .any(|choice| choice.entry == u64::from(entry))
    };
    let pair_is_supported = if item.abilities.attunements.is_empty() {
        supports(&item.abilities.super_ability, selection.super_ability)
            && supports(&item.abilities.melee, selection.melee)
    } else {
        item.abilities.attunements.iter().any(|attunement| {
            attunement.melee.entry == u64::from(selection.melee)
                && attunement
                    .super_abilities
                    .iter()
                    .any(|choice| choice.entry == u64::from(selection.super_ability))
        })
    };

    supports(&item.abilities.movement, selection.movement)
        && supports(&item.abilities.grenade, selection.grenade)
        && supports(&item.abilities.class_ability, selection.class_ability)
        && pair_is_supported
}

pub(in crate::app) fn set_equipment_item_level(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    level: i64,
) -> Result<(), String> {
    if !(0..=i64::from(i32::MAX)).contains(&level) {
        return Err("Equipment level must be a non-negative signed 32-bit integer".to_owned());
    }
    let (adapter, character_id) =
        load_existing_equipment_item(document, character_index, slot, false, |_| {
            equipment_item_target_error(slot)
        })?;
    apply_equipment_command(
        document,
        adapter,
        account_domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: account_domain::EquipmentSlot::new(slot),
            update: account_domain::ItemUpdate::SetLevel(
                i32::try_from(level).expect("equipment level was range checked"),
            ),
        },
    )
}

pub(in crate::app) fn set_equipment_item_plug(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    socket_index: usize,
    default_plugs: &[Option<String>],
    hash: Option<u64>,
) -> Result<(), String> {
    if socket_index >= super::inventory::MAX_ITEM_PLUGS {
        return Err(format!(
            "Equipment socket index must be below {}",
            super::inventory::MAX_ITEM_PLUGS
        ));
    }
    if hash.is_some_and(|hash| u32::try_from(hash).is_err()) {
        return Err("Equipment plug hash must fit in an unsigned 32-bit integer".to_owned());
    }
    let (adapter, character_id) =
        load_existing_equipment_item(document, character_index, slot, true, |adapter_error| {
            let plugs_path = format!("/state/characters/{character_index}/equipment/{slot}/plugs");
            if adapter_error.path() == Some(plugs_path.as_str())
                && adapter_error.detail() == "plugs is missing"
            {
                format!("Missing plugs value for {slot}")
            } else if adapter_error
                .path()
                .is_some_and(|path| path.starts_with(&plugs_path))
            {
                format!("Invalid plugs value for {slot}")
            } else {
                equipment_item_target_error(slot)
            }
        })?;
    let account_domain::ItemPlugs::Authored(default_plugs) = domain_default_plugs(default_plugs)?
    else {
        unreachable!()
    };
    let plug = hash.map(|hash| {
        u32::try_from(hash)
            .map(account_domain::DefinitionHash::new)
            .expect("equipment plug hash was range checked")
    });
    apply_equipment_command(
        document,
        adapter,
        account_domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: account_domain::EquipmentSlot::new(slot),
            update: account_domain::ItemUpdate::SetPlug {
                index: socket_index,
                plug,
                default_plugs,
            },
        },
    )
}

pub(in crate::app) fn set_equipment_item_flags(
    document: &mut Value,
    character_index: usize,
    slot: &str,
    flags: Option<u8>,
) -> Result<(), String> {
    if !super::inventory::schema_mode(document).can_mutate_equipment_flags() {
        return Err(format!(
            "Equipment flags require a writable settings schema {} or newer",
            super::inventory::EQUIPMENT_FLAGS_SCHEMA_VERSION
        ));
    }
    let flag_mask = super::inventory::schema_mode(document).item_flag_mask();
    if flags.is_some_and(|flags| flags > flag_mask) {
        return Err(format!("Equipment flags must be between 0 and {flag_mask}"));
    }
    let (adapter, character_id) =
        load_existing_equipment_item(document, character_index, slot, false, |_| {
            equipment_item_target_error(slot)
        })?;
    apply_equipment_command(
        document,
        adapter,
        account_domain::CharacterCommand::UpdateEquipmentItem {
            character_id,
            slot: account_domain::EquipmentSlot::new(slot),
            update: account_domain::ItemUpdate::SetFlags(flags.map(u32::from)),
        },
    )
}

#[cfg(test)]
fn equipment_item_object_mut<'a>(
    document: &'a mut Value,
    character_index: usize,
    slot: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    if !super::inventory::schema_mode(document)
        .equipment_slots()
        .iter()
        .any(|(known_slot, _, _)| *known_slot == slot)
    {
        return Err(format!("Unknown equipment slot: {slot}"));
    }
    document
        .pointer_mut("/state/characters")
        .and_then(Value::as_array_mut)
        .and_then(|characters| characters.get_mut(character_index))
        .and_then(|character| character.get_mut("equipment"))
        .and_then(Value::as_object_mut)
        .and_then(|equipment| equipment.get_mut(slot))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!(
                "The {} slot must contain an item object before it can be edited",
                equipment_slot_label(slot)
            )
        })
}

pub(in crate::app) fn set_weapon_slot_empty(
    document: &mut Value,
    character_index: usize,
    slot: &str,
) -> Result<(), String> {
    if !WEAPON_SLOTS.contains(&slot) {
        return Err(format!(
            "Only weapon slots can be set to empty; {} was not changed",
            equipment_slot_label(slot)
        ));
    }
    let adapter = JsonCharacterAdapter::load_equipment_patch_slot(document, character_index, slot)
        .map_err(|error| {
            let slot_path = format!("/state/characters/{character_index}/equipment/{slot}");
            if error.path() == Some(slot_path.as_str()) {
                format!(
                    "The {} slot contains unexpected data and was not changed",
                    equipment_slot_label(slot)
                )
            } else {
                "The selected character has no equipment object".to_owned()
            }
        })?;
    let character_id = equipment_character_id(&adapter, character_index)
        .map_err(|_| "The selected character has no equipment object")?;
    apply_equipment_command(
        document,
        adapter,
        account_domain::CharacterCommand::SetEquipmentItem {
            character_id,
            slot: account_domain::EquipmentSlot::new(slot),
            item: None,
        },
    )
}

pub(in crate::app) fn displayed_plugs(
    plugs: Option<&Value>,
    defaults: &[Option<String>],
) -> (Vec<Value>, bool) {
    let default_plugs = || default_plug_values(defaults);
    match plugs {
        Some(Value::Array(plugs)) => {
            let native_defaults = *plugs == default_plugs();
            (plugs.clone(), native_defaults)
        }
        Some(Value::Null) => (default_plugs(), true),
        _ => (Vec::new(), false),
    }
}

#[cfg(test)]
pub(in crate::app) fn materialize_authored_plugs<'a>(
    plugs: &'a mut Value,
    defaults: &[Option<String>],
) -> Option<&'a mut Vec<Value>> {
    if plugs.is_null() {
        *plugs = Value::Array(default_plug_values(defaults));
    }
    plugs.as_array_mut()
}

pub(in crate::app) fn native_plug_default(
    defaults: &[Option<String>],
    socket_index: usize,
) -> Option<NativePlugDefault> {
    match defaults.get(socket_index)? {
        Some(hash_hex) => parse_hash_hex(hash_hex).map(NativePlugDefault::Plug),
        None => Some(NativePlugDefault::Empty),
    }
}

pub(super) fn equipped_header_label(id_scope: &str, slot_label: &str) -> String {
    if id_scope == "character-inventory-equipped" {
        "Equipped".to_owned()
    } else {
        format!("{slot_label} Slot")
    }
}

#[cfg(test)]
pub(in crate::app) mod legacy;
