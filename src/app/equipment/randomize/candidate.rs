//! Base-item selection and randomized plug candidates.

use super::*;

pub(super) fn roll_family(
    catalog: &Catalog,
    class_type: u64,
    show_dummy_items: bool,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    family: ItemFamily,
) -> Result<(), String> {
    let selected_slot = match family {
        ItemFamily::Weapon => state.weapon_slot,
        ItemFamily::Armor => state.armor_slot,
    }
    .and_then(|index| family.slots().get(index).copied());
    let candidates = family
        .slots()
        .iter()
        .copied()
        .filter(|slot| selected_slot.is_none_or(|selected| selected == *slot));
    let candidates = random_item_candidates(catalog, class_type, show_dummy_items, candidates)
        .into_iter()
        .filter(|item| state.filter(family).matches(catalog, item))
        .collect::<Vec<_>>();
    let item = Rng::from_clock()
        .pick(&candidates)
        .copied()
        .ok_or_else(|| {
            format!(
                "No usable {} definitions match the selection and filters",
                family.label()
            )
        })?;
    state.candidate = Some(rolled_candidate(catalog, item, plug_mode)?);
    state.selected_socket = 0;
    state.socket_query.clear();
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

pub(super) fn select_base(
    catalog: &Catalog,
    plug_mode: PlugSelectionMode,
    state: &mut WorkspaceState,
    hash: u64,
) -> Result<(), String> {
    let item = catalog
        .item(hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("The selected base item cannot be authored safely")?;
    state.candidate = Some(rolled_candidate(catalog, item, plug_mode)?);
    state.selected_socket = 0;
    state.socket_query.clear();
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

pub(super) fn open_builder_request(
    catalog: &Catalog,
    state: &mut WorkspaceState,
    request: &ItemBuilderRequest,
) -> Result<(), String> {
    let (family, slot_index, candidate) = candidate_from_builder_request(catalog, request)?;
    state.active_family = family;
    match family {
        ItemFamily::Weapon => state.weapon_slot = Some(slot_index),
        ItemFamily::Armor => state.armor_slot = Some(slot_index),
    }
    state.base_query.clear();
    state.socket_query.clear();
    state.selected_socket = 0;
    state.candidate = Some(candidate);
    state.pending_destructive_equip = None;
    state.feedback = None;
    state.armor_stat_allocation.clear_feedback();
    Ok(())
}

pub(super) fn candidate_from_builder_request(
    catalog: &Catalog,
    request: &ItemBuilderRequest,
) -> Result<(ItemFamily, usize, Candidate), String> {
    let item = catalog
        .item(request.item_hash)
        .filter(|item| item_can_be_authored(item))
        .ok_or("This item cannot be opened safely in Random Item Builder")?;
    let (slot, _) = slot_for_bucket(item.bucket_hash)
        .ok_or("Random Item Builder only supports weapons and armor")?;
    let (family, slot_index) = if let Some(index) = WEAPON_SLOTS
        .iter()
        .position(|candidate_slot| *candidate_slot == slot)
    {
        (ItemFamily::Weapon, index)
    } else if let Some(index) = ARMOR_SLOTS
        .iter()
        .position(|candidate_slot| *candidate_slot == slot)
    {
        (ItemFamily::Armor, index)
    } else {
        return Err("Random Item Builder only supports weapons and armor".to_owned());
    };

    let mut candidate = default_candidate(item)?;
    if let Some(authored_plugs) = request.authored_plugs.as_ref() {
        let (plug_values, _) = displayed_plugs(Some(authored_plugs), &item.default_plugs);
        let plugs = plug_values
            .iter()
            .map(|value| {
                if value.is_null() {
                    Ok(None)
                } else {
                    parse_unsigned_value(value)
                        .filter(|hash| valid_definition_hash(*hash))
                        .map(Some)
                        .ok_or_else(|| "This item's authored plugs are malformed".to_owned())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if plugs.len() > inventory::MAX_ITEM_PLUGS {
            return Err("This item's authored plugs exceed the supported socket limit".to_owned());
        }
        let authored_count = item
            .sockets
            .len()
            .max(plugs.len())
            .min(inventory::MAX_ITEM_PLUGS);
        candidate.plugs = plugs;
        candidate.plugs.resize(authored_count, None);
    }
    Ok((family, slot_index, candidate))
}

pub(super) fn rolled_candidate(
    catalog: &Catalog,
    item: &ItemDef,
    plug_mode: PlugSelectionMode,
) -> Result<Candidate, String> {
    let mut candidate = default_candidate(item)?;
    let mut rng = Rng::from_clock();
    for socket_index in 0..item.sockets.len().min(inventory::MAX_ITEM_PLUGS) {
        if let Some(hash) = random_plug(catalog, item, socket_index, plug_mode, &mut rng) {
            candidate.plugs[socket_index] = Some(hash);
        }
    }
    Ok(candidate)
}

pub(super) fn default_candidate(item: &ItemDef) -> Result<Candidate, String> {
    let mut plugs = item
        .default_plugs
        .iter()
        .map(|plug| {
            plug.as_deref()
                .map(|text| {
                    parse_hash_hex(text)
                        .filter(|hash| valid_definition_hash(*hash))
                        .ok_or_else(|| {
                            format!("{} contains an invalid package-default plug", item.name)
                        })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let authored_count = item
        .sockets
        .len()
        .max(plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    plugs.resize(authored_count, None);
    Ok(Candidate {
        item_hash: item.hash,
        plugs,
    })
}

pub(super) fn default_plug(item: &ItemDef, socket_index: usize) -> Result<Option<u64>, String> {
    item.default_plugs
        .get(socket_index)
        .and_then(|plug| plug.as_deref())
        .map(|text| {
            parse_hash_hex(text)
                .filter(|hash| valid_definition_hash(*hash))
                .ok_or_else(|| format!("{} contains an invalid package-default plug", item.name))
        })
        .transpose()
}

pub(super) fn random_plug(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    plug_mode: PlugSelectionMode,
    rng: &mut Rng,
) -> Option<u64> {
    let socket = item.sockets.get(socket_index)?;
    match plug_mode {
        PlugSelectionMode::Supported => rng.pick_valid_hash(catalog.socket_options(socket)),
        PlugSelectionMode::SocketAndGearType => {
            let options = catalog.socket_and_gear_type_options(item, socket_index);
            rng.pick_valid_hash(options)
        }
        PlugSelectionMode::MatchingSocketType => {
            rng.pick_valid_hash(catalog.socket_type_options(socket.socket_type))
        }
        PlugSelectionMode::GearType => {
            let options = catalog.gear_type_options(item, socket_index);
            rng.pick_valid_hash(&options)
        }
        PlugSelectionMode::AnyPlug => rng.pick_valid_hash(catalog.all_plug_options()),
    }
}
