//! Single-item validation and account mutation.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn apply_candidate(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    candidate: &Candidate,
    discard_replaced: bool,
) -> Result<String, String> {
    if !account::can_mutate_equipment(document) {
        return Err("Randomizing requires a writable equipment schema".to_owned());
    }
    if candidate.plugs.len() > inventory::MAX_ITEM_PLUGS {
        return Err("The generated item contains too many authored plugs".to_owned());
    }
    let item = catalog
        .item(candidate.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The generated base item is no longer available")?;
    let (slot, slot_label) = slot_for_bucket(item.bucket_hash)
        .ok_or("The generated item does not belong to a weapon or armor slot")?;
    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }

    let equipped_item = equipped_item_row(document, character_index, slot)?;
    let mut updated = document.clone();
    let mut previous_item_preserved = false;
    let mut previous_item_discarded = false;
    if let Some(equipped_item) = equipped_item.as_ref() {
        if let Some(reason) = equipped_item_preservation_blocker(
            document,
            catalog,
            character_index,
            slot,
            equipped_item,
        ) {
            if !discard_replaced {
                return Err(format!(
                    "The currently equipped item cannot be moved to inventory: {reason}"
                ));
            }
            previous_item_discarded = true;
        } else {
            account::move_equipment_item_to_inventory(&mut updated, character_index, slot)
                .map_err(|error| {
                    format!("The currently equipped item could not be moved to inventory: {error}")
                })?;
            previous_item_preserved = true;
        }
    }
    account::equip_definition(
        &mut updated,
        character_index,
        slot,
        item.hash,
        &item.default_plugs,
    )?;
    for (socket_index, hash) in candidate.plugs.iter().copied().enumerate() {
        account::set_equipment_item_plug(
            &mut updated,
            character_index,
            slot,
            socket_index,
            &item.default_plugs,
            hash,
        )?;
    }
    settings::validate_workspace_document(&updated)
        .map_err(|error| format!("The generated item did not pass validation: {error}"))?;
    *document = updated;
    let result = if previous_item_preserved {
        format!(
            "Equipped {} in {slot_label}; moved the previous item to character inventory",
            item.name
        )
    } else if previous_item_discarded {
        format!(
            "Equipped {} in {slot_label}; deleted the previous equipped item",
            item.name
        )
    } else {
        format!("Equipped {} in {slot_label}", item.name)
    };
    Ok(result)
}

pub(super) fn equip_replacement_warning(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    candidate: Option<&Candidate>,
) -> Option<EquipReplacementWarning> {
    let candidate = candidate?;
    let candidate_item = catalog.item(candidate.item_hash)?;
    let (slot, slot_label) = slot_for_bucket(candidate_item.bucket_hash)?;
    let equipped_item = equipped_item_row(document, character_index, slot)
        .ok()
        .flatten()?;
    let current_item_name = equipped_item
        .definition_hash
        .and_then(|hash| catalog.item(hash).map(|item| item.name.clone()))
        .unwrap_or_else(|| format!("The currently equipped {slot_label} item"));
    let reason = equipped_item_preservation_blocker(
        document,
        catalog,
        character_index,
        slot,
        &equipped_item,
    )?;
    Some(EquipReplacementWarning {
        current_item_name,
        candidate_item_name: candidate_item.name.clone(),
        reason,
    })
}

pub(super) fn equipped_item_row(
    document: &account::WorkspaceDocument,
    character_index: usize,
    slot: &str,
) -> Result<Option<EquippedItemSnapshot>, String> {
    Ok(account::equipped_item_snapshots(document, character_index)?
        .into_iter()
        .find(|item| item.slot == slot))
}

pub(super) fn equipped_item_preservation_blocker(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    slot: &str,
    equipped_item: &EquippedItemSnapshot,
) -> Option<String> {
    let Some(definition_hash) = equipped_item.definition_hash else {
        return Some("Its definition hash is unreadable".to_owned());
    };
    if let Some(reason) = definition_inventory_add_blocker(
        document,
        catalog,
        character_index,
        definition_hash,
        "The equipped item cannot be stored in character inventory",
    ) {
        return Some(reason);
    }

    let mut preview = document.clone();
    account::move_equipment_item_to_inventory(&mut preview, character_index, slot)
        .err()
        .map(|error| error.to_string())
}

pub(super) fn inventory_add_blocker(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    candidate: Option<&Candidate>,
) -> Option<String> {
    let candidate = candidate?;
    let Some(item) = catalog.item(candidate.item_hash) else {
        return Some("The generated base item is unavailable".to_owned());
    };
    definition_inventory_add_blocker(
        document,
        catalog,
        character_index,
        item.hash,
        "This definition cannot be added to character inventory",
    )
}

pub(super) fn definition_inventory_add_blocker(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    definition_hash: u64,
    unavailable_message: &str,
) -> Option<String> {
    if !account::can_mutate_character_inventory(document) {
        return Some("Requires writable character inventory".to_owned());
    }
    let inventory = match account::character_inventory(document, character_index) {
        Ok(inventory) => inventory,
        Err(error) => return Some(format!("Character inventory is unavailable: {error}")),
    };
    if inventory
        .as_ref()
        .is_some_and(|items| items.len() >= account::character_inventory_capacity(document))
    {
        return Some("Character inventory is full".to_owned());
    }
    let Some(metadata) = catalog
        .inventory_metadata(definition_hash)
        .filter(|metadata| metadata.is_character_inventory_candidate())
    else {
        return Some(unavailable_message.to_owned());
    };
    if let Some(reason) = character_bucket_add_blocker(
        document,
        catalog,
        character_index,
        inventory.as_deref().unwrap_or_default(),
        metadata,
    ) {
        return Some(reason);
    }
    None
}

pub(super) fn character_bucket_add_blocker(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    inventory: &[inventory::InventoryItemSnapshot],
    candidate: &InventoryMetadata,
) -> Option<String> {
    let Some(capacity) = candidate.authored_row_capacity().map(usize::from) else {
        return Some("This inventory bucket has no safe capacity".to_owned());
    };
    let equipment = match account::equipped_item_snapshots(document, character_index) {
        Ok(equipment) => equipment,
        Err(_) => return Some("Character equipment is unavailable".to_owned()),
    };
    let mut occupied = 0usize;
    let mut unresolved = 0usize;
    let mut count = |hash: Option<u64>| match hash.and_then(|hash| catalog.inventory_metadata(hash))
    {
        Some(metadata)
            if metadata.scope == InventoryScope::Character
                && metadata.native_bucket_id == candidate.native_bucket_id =>
        {
            occupied += 1;
        }
        Some(metadata) if metadata.scope == InventoryScope::Character => {}
        Some(_) | None => unresolved += 1,
    };
    for item in equipment {
        count(item.definition_hash);
    }
    for item in inventory {
        count(Some(u64::from(item.definition_hash)));
    }

    let bucket_label = candidate.bucket_label();
    if occupied >= capacity {
        Some(format!("{bucket_label} is full"))
    } else if bucket_has_room_for_add(occupied, unresolved, capacity) {
        None
    } else {
        Some(format!(
            "Cannot verify room in {bucket_label} because existing inventory placement is unresolved"
        ))
    }
}

pub(super) const fn bucket_has_room_for_add(
    occupied: usize,
    unresolved: usize,
    capacity: usize,
) -> bool {
    occupied.saturating_add(unresolved) < capacity
}

pub(super) fn add_candidate_to_inventory(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    candidate: &Candidate,
) -> Result<String, String> {
    if let Some(reason) = inventory_add_blocker(document, catalog, character_index, Some(candidate))
    {
        return Err(reason);
    }
    if candidate.plugs.len() > inventory::MAX_ITEM_PLUGS {
        return Err("The generated item contains too many authored plugs".to_owned());
    }
    let item = catalog
        .item(candidate.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The generated base item is no longer available")?;
    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    if item.class_type != 3 && item.class_type != class_type {
        return Err(format!(
            "{} is not compatible with {}",
            item.name,
            class_name(class_type)
        ));
    }
    let definition_hash = u32::try_from(item.hash)
        .map_err(|_| format!("{} has an invalid definition hash", item.name))?;
    let item_level = capped_inventory_item_level(catalog, item)?;
    let plugs = candidate
        .plugs
        .iter()
        .copied()
        .map(|hash| {
            hash.map(|hash| {
                u32::try_from(hash).map_err(|_| format!("{} generated an invalid plug", item.name))
            })
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut updated = document.clone();
    let location = account::add_inventory_item(
        &mut updated,
        character_index,
        inventory::NewInventoryItem::single(definition_hash, item_level),
    )
    .map_err(|error| error.to_string())?;
    account::apply_inventory_item_action(
        &mut updated,
        location,
        inventory::InventoryItemAction::SetPlugs(inventory::ItemPlugs::Authored(plugs)),
    )
    .map_err(|error| error.to_string())?;
    settings::validate_workspace_document(&updated)
        .map_err(|error| format!("The generated item did not pass validation: {error}"))?;
    *document = updated;
    Ok(format!("Added {} to character inventory", item.name))
}

pub(super) fn validate_loadout_request(
    document: &account::WorkspaceDocument,
    character_index: usize,
    options: LoadoutOptions,
) -> Result<u64, String> {
    if !options.any() {
        return Err("Select at least one loadout section".to_owned());
    }
    if !account::can_mutate_equipment(document) {
        return Err("Randomizing a loadout requires writable equipment".to_owned());
    }
    if !account::can_mutate_character_inventory(document) {
        return Err("Randomizing a loadout requires writable character inventory".to_owned());
    }
    let class_type = character_class(document, character_index);
    if class_type > 2 {
        return Err(format!(
            "Character {} has no valid class",
            character_index + 1
        ));
    }
    account::character_inventory(document, character_index).map_err(|error| error.to_string())?;
    Ok(class_type)
}

pub(super) fn has_locked_exotic(
    equipped_items: &[EquippedItemSnapshot],
    slots: &[&str],
    keep_locked_items: bool,
    catalog: &Catalog,
) -> bool {
    keep_locked_items
        && equipped_items.iter().any(|item| {
            slots.contains(&item.slot)
                && loadout_item_is_locked(item.flags)
                && item
                    .definition_hash
                    .and_then(|hash| catalog.item(hash))
                    .is_some_and(|item| is_exotic(catalog, item))
        })
}

pub(super) fn held_item_counts(
    held_inventory: &[inventory::InventoryItemSnapshot],
    catalog: &Catalog,
) -> (usize, HashMap<u64, usize>) {
    let mut by_bucket = HashMap::new();
    for item in held_inventory {
        if let Some(definition) = catalog.item(u64::from(item.definition_hash)) {
            *by_bucket.entry(definition.bucket_hash).or_default() += 1;
        }
    }
    (held_inventory.len(), by_bucket)
}
