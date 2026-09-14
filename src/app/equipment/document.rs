use crate::app::account_workspace as account;

use super::*;
use sundial_account as account_domain;

use crate::persistence::json_account::JsonCharacterAdapter;
pub(in crate::app) use crate::persistence::json_account::equipment::*;

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
    if slot == "subclass" && !candidate.uses_subclass_plug_abilities() {
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
    if document.uses_subclass_plug_abilities() {
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
