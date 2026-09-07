//! Full-loadout planning, validation, and installation.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn randomize_full_loadout(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    plug_mode: PlugSelectionMode,
    show_dummy_items: bool,
    options: LoadoutOptions,
) -> Result<String, String> {
    let class_type = validate_loadout_request(document, character_index, options)?;

    let mut updated = document.clone();
    if options.replace_held_inventory {
        clear_selected_inventory(&mut updated, catalog, character_index, options)?;
    }
    let equipped_items = account::equipped_item_snapshots(&updated, character_index)
        .map_err(|error| error.to_string())?;
    let mut rng = Rng::from_clock();
    let locked_equipped = |slot: &str| {
        options.keep_locked_items.then(|| {
            equipped_items
                .iter()
                .find(|item| item.slot == slot && loadout_item_is_locked(item.flags))
        })?
    };
    let mut exotic_weapon_equipped = has_locked_exotic(
        &equipped_items,
        WEAPON_SLOTS,
        options.keep_locked_items,
        catalog,
    );
    let mut exotic_armor_equipped = has_locked_exotic(
        &equipped_items,
        ARMOR_SLOTS,
        options.keep_locked_items,
        catalog,
    );
    let mut generated_items = 0usize;
    let mut generated_slots = 0usize;
    let held_inventory = account::character_inventory(&updated, character_index)
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let (mut held_items, mut held_items_by_bucket) = held_item_counts(&held_inventory, catalog);

    for &(slot, _, bucket_hash) in document.equipment_slots() {
        let scope = loadout_scope_for_slot(slot);
        if !options.includes(scope) {
            continue;
        }
        let candidates = catalog
            .browse(bucket_hash, class_type, show_dummy_items, false)
            .into_iter()
            .filter(|item| {
                item_can_be_authored(item)
                    && crate::account_contract::definition_available(
                        item.hash,
                        document.supports_v13_account(),
                    )
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let mut used_hashes = Vec::new();
        let mut slot_changed = false;
        if let Some(locked) = locked_equipped(slot) {
            if let Some(hash) = locked.definition_hash {
                used_hashes.push(hash);
            }
        } else {
            let ordinary = candidates
                .iter()
                .copied()
                .filter(|item| !is_exotic(catalog, item))
                .collect::<Vec<_>>();
            let equipped_candidates = if (WEAPON_SLOTS.contains(&slot) && exotic_weapon_equipped)
                || (ARMOR_SLOTS.contains(&slot) && exotic_armor_equipped)
            {
                ordinary.as_slice()
            } else {
                candidates.as_slice()
            };
            let equipped =
                pick_avoiding(&mut rng, equipped_candidates, &used_hashes).ok_or_else(|| {
                    format!("No usable non-exotic item is available for the {slot} slot")
                })?;
            if slot == SUBCLASS_SLOT {
                equip_subclass_with_default_abilities(
                    &mut updated,
                    character_index,
                    equipped,
                    false,
                )?;
            } else {
                install_random_equipped(
                    &mut updated,
                    catalog,
                    character_index,
                    slot,
                    equipped,
                    plug_mode,
                    &mut rng,
                )?;
            }
            if WEAPON_SLOTS.contains(&slot) && is_exotic(catalog, equipped) {
                exotic_weapon_equipped = true;
            }
            if ARMOR_SLOTS.contains(&slot) && is_exotic(catalog, equipped) {
                exotic_armor_equipped = true;
            }
            used_hashes.push(equipped.hash);
            generated_items += 1;
            slot_changed = true;
        }

        let held_target = if !options.replace_held_inventory
            || matches!(slot, SUBCLASS_SLOT | CLAN_BANNER_SLOT)
        {
            0
        } else {
            HELD_ITEMS_PER_SLOT
                .saturating_sub(
                    held_items_by_bucket
                        .get(&bucket_hash)
                        .copied()
                        .unwrap_or_default(),
                )
                .min(inventory::CHARACTER_INVENTORY_CAPACITY.saturating_sub(held_items))
        };
        for _ in 0..held_target {
            let held = pick_avoiding(&mut rng, &candidates, &used_hashes)
                .ok_or_else(|| format!("No usable held item is available for the {slot} slot"))?;
            install_random_held(
                &mut updated,
                catalog,
                character_index,
                held,
                plug_mode,
                &mut rng,
            )?;
            used_hashes.push(held.hash);
            held_items += 1;
            *held_items_by_bucket.entry(bucket_hash).or_default() += 1;
            generated_items += 1;
            slot_changed = true;
        }
        if slot_changed {
            generated_slots += 1;
        }
    }

    if generated_slots == 0 {
        return Err("No usable item definitions were found for this character".to_owned());
    }
    settings::validate_workspace_document(&updated)
        .map_err(|error| format!("The generated loadout did not pass validation: {error}"))?;
    *document = updated;
    Ok(format!(
        "Randomized {generated_items} items across {generated_slots} slots"
    ))
}

pub(super) fn clear_selected_inventory(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    options: LoadoutOptions,
) -> Result<(), String> {
    let inventory = account::character_inventory(document, character_index)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Character {} inventory must be an array",
                character_index + 1
            )
        })?;
    let removed_indices = inventory.into_iter().filter_map(|item| {
        if options.keep_locked_items && loadout_item_is_locked(item.flags) {
            return None;
        }
        // An unavailable definition cannot safely be assigned to a selected category.
        let definition = catalog.item(u64::from(item.definition_hash))?;
        let scope = loadout_scope_for_bucket(definition.bucket_hash)?;
        options.includes(scope).then_some(item.location.item_index)
    });
    account::remove_character_inventory_items(document, character_index, removed_indices)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn loadout_scope_for_bucket(bucket_hash: u64) -> Option<LoadoutScope> {
    SLOTS.iter().find_map(|(slot, _, bucket)| {
        (*bucket == bucket_hash).then(|| loadout_scope_for_slot(slot))
    })
}

pub(super) fn loadout_item_is_locked(flags: Option<u8>) -> bool {
    flags.unwrap_or_default() & inventory::INVENTORY_FLAG_LOCKED != 0
}

pub(super) fn loadout_scope_for_slot(slot: &str) -> LoadoutScope {
    if WEAPON_SLOTS.contains(&slot) {
        LoadoutScope::Weapons
    } else if ARMOR_SLOTS.contains(&slot) {
        LoadoutScope::Armor
    } else if slot == SUBCLASS_SLOT {
        LoadoutScope::Subclass
    } else {
        LoadoutScope::EquipmentFlair
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn install_random_equipped(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    slot: &str,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Result<(), String> {
    account::equip_definition(
        document,
        character_index,
        slot,
        item.hash,
        &item.default_plugs,
    )?;
    for socket_index in 0..item.sockets.len().min(inventory::MAX_ITEM_PLUGS) {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, rng) {
            account::set_equipment_item_plug(
                document,
                character_index,
                slot,
                socket_index,
                &item.default_plugs,
                Some(hash),
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn install_random_held(
    document: &mut account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Result<(), String> {
    let definition_hash = u32::try_from(item.hash)
        .map_err(|_| format!("{} has an invalid definition hash", item.name))?;
    let item_level = capped_inventory_item_level(catalog, item)?;
    let location = account::add_inventory_item(
        document,
        character_index,
        inventory::NewInventoryItem::single(definition_hash, item_level),
    )
    .map_err(|error| error.to_string())?;
    let mut plugs = default_candidate(item)?
        .plugs
        .into_iter()
        .map(|hash| {
            hash.map(|hash| {
                u32::try_from(hash)
                    .map_err(|_| format!("{} has an invalid default plug", item.name))
            })
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (socket_index, plug) in plugs
        .iter_mut()
        .enumerate()
        .take(item.sockets.len().min(inventory::MAX_ITEM_PLUGS))
    {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, rng) {
            *plug = Some(
                u32::try_from(hash)
                    .map_err(|_| format!("{} generated an invalid plug", item.name))?,
            );
        }
    }
    account::apply_inventory_item_action(
        document,
        location,
        inventory::InventoryItemAction::SetPlugs(inventory::ItemPlugs::Authored(plugs)),
    )
    .map_err(|error| error.to_string())
}

pub(super) fn capped_inventory_item_level(
    catalog: &Catalog,
    item: &ItemDef,
) -> Result<i32, String> {
    let native_bucket_id = catalog
        .inventory_metadata(item.hash)
        .map(|metadata| metadata.native_bucket_id)
        .ok_or_else(|| format!("{} has no inventory placement metadata", item.name))?;
    i32::try_from(item_editor::new_inventory_item_level(
        native_bucket_id,
        catalog.item_power_cap(item.hash),
    ))
    .map_err(|_| format!("{} has an invalid maximum item level", item.name))
}

pub(super) fn is_exotic(catalog: &Catalog, item: &ItemDef) -> bool {
    catalog.item_rarity(item.hash) == crate::catalog::ItemRarity::Exotic
}

pub(super) fn pick_avoiding<'a>(
    rng: &mut Rng,
    candidates: &[&'a ItemDef],
    used_hashes: &[u64],
) -> Option<&'a ItemDef> {
    let unused = candidates
        .iter()
        .copied()
        .filter(|item| !used_hashes.contains(&item.hash))
        .collect::<Vec<_>>();
    rng.pick(&unused)
        .copied()
        .or_else(|| rng.pick(candidates).copied())
}

pub(super) fn matching_item_instances(
    document: &account::WorkspaceDocument,
    character_index: usize,
    matching_hashes: &HashSet<u64>,
) -> Vec<ItemInstanceChoice> {
    let mut choices = Vec::new();
    if let Ok(equipped) = account::equipped_item_snapshots(document, character_index) {
        choices.extend(equipped.into_iter().filter_map(|snapshot| {
            let item_hash = snapshot.definition_hash?;
            if !matching_hashes.contains(&item_hash) {
                return None;
            }
            Some(ItemInstanceChoice {
                request: ItemBuilderRequest {
                    item_hash,
                    authored_plugs: Some(equipped_plugs_value(&snapshot.plugs)?),
                },
                location: format!("Equipped · {}", snapshot.slot_label),
            })
        }));
    }
    if let Ok(Some(stored)) = account::character_inventory(document, character_index) {
        choices.extend(stored.into_iter().filter_map(|snapshot| {
            let item_hash = u64::from(snapshot.definition_hash);
            matching_hashes
                .contains(&item_hash)
                .then(|| ItemInstanceChoice {
                    request: ItemBuilderRequest {
                        item_hash,
                        authored_plugs: Some(inventory_plugs_value(&snapshot.plugs)),
                    },
                    location: format!("Inventory · item {}", snapshot.location.item_index + 1),
                })
        }));
    }
    choices
}

pub(super) fn equipped_plugs_value(plugs: &EquippedItemPlugs) -> Option<Value> {
    match plugs {
        EquippedItemPlugs::NativeDefaults => Some(Value::Null),
        EquippedItemPlugs::Authored(plugs) => plugs
            .iter()
            .map(|plug| match plug {
                EquippedPlugValue::Empty => Some(Value::Null),
                EquippedPlugValue::Hash(hash) if valid_definition_hash(*hash) => {
                    Some(Value::from(*hash))
                }
                EquippedPlugValue::Hash(_) | EquippedPlugValue::Malformed(_) => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        EquippedItemPlugs::Missing | EquippedItemPlugs::Malformed(_) => None,
    }
}

pub(super) fn matching_items<'a>(
    catalog: &'a Catalog,
    class_type: u64,
    show_dummy_items: bool,
    query: &str,
    family: ItemFamily,
    filter: &item_editor::ItemFilter,
) -> Vec<&'a ItemDef> {
    if query.is_empty() || class_type > 2 {
        return Vec::new();
    }
    let mut items = family
        .slots()
        .iter()
        .copied()
        .filter_map(slot_definition)
        .flat_map(|(_, _, bucket)| {
            catalog.search(query, bucket, class_type, show_dummy_items, false)
        })
        .filter(|item| item_can_be_authored(item))
        .filter(|item| filter.matches(catalog, item))
        .collect::<Vec<_>>();
    items.sort_by_cached_key(|item| (item.name.to_ascii_lowercase(), item.hash));
    items.dedup_by_key(|item| item.hash);
    items
}

pub(super) fn random_item_candidates(
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    slots: impl Iterator<Item = &'static str>,
) -> Vec<&ItemDef> {
    if class_type > 2 {
        return Vec::new();
    }
    let mut items = slots
        .filter_map(slot_definition)
        .flat_map(|(_, _, bucket)| catalog.browse(bucket, class_type, show_dummy_items, false))
        .filter(|item| item_can_be_authored(item))
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.hash);
    items.dedup_by_key(|item| item.hash);
    items
}

pub(super) fn slot_definition(slot: &str) -> Option<(&'static str, &'static str, u64)> {
    crate::account_contract::ALL_EQUIPMENT_SLOTS
        .iter()
        .find(|(known_slot, _, _)| *known_slot == slot)
        .copied()
}

pub(super) fn slot_for_bucket(bucket_hash: u64) -> Option<(&'static str, &'static str)> {
    SLOTS
        .iter()
        .filter(|(slot, _, _)| WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot))
        .find_map(|(slot, label, bucket)| (*bucket == bucket_hash).then_some((*slot, *label)))
}

pub(super) fn character_class(
    document: &account::WorkspaceDocument,
    character_index: usize,
) -> u64 {
    account::character_metadata(document, character_index)
        .ok()
        .map(|metadata| u64::from(metadata.class_type))
        .unwrap_or(99)
}

pub(super) fn item_can_be_authored(item: &ItemDef) -> bool {
    // This singleton container has four selections, not the ordinary held-emote semantics
    // used by bulk randomization. It remains available in the guided equipment picker.
    item.hash != crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH
        && valid_definition_hash(item.hash)
        && item.default_plugs.len() <= inventory::MAX_ITEM_PLUGS
        && item.default_plugs.iter().all(|plug| {
            plug.as_deref()
                .is_none_or(|text| parse_hash_hex(text).is_some_and(valid_definition_hash))
        })
}

pub(super) fn valid_definition_hash(hash: u64) -> bool {
    hash != u64::from(NO_DEFINITION_HASH.get()) && u32::try_from(hash).is_ok()
}
