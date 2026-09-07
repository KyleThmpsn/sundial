//! Package and account input collection for armor-stat previews.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn refresh_input(
    state: &mut State,
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
    plug_mode: PlugSelectionMode,
    allow_inventory_swaps: bool,
) {
    let key = source_key(document, character_index, plug_mode, allow_inventory_swaps);
    if state.source_key.as_ref() == Some(&key) {
        return;
    }
    state.input = Some(build_input(catalog, &key));
    state.source_key = Some(key);
    state.preview = None;
    state.preview_task = None;
    if state.preserve_feedback_once {
        state.preserve_feedback_once = false;
    } else {
        state.feedback = None;
    }
}

pub(super) fn refresh_preview(state: &mut State, context: &egui::Context) {
    if let Some(task) = state.preview_task.take() {
        if state.source_key.as_ref() != Some(&task.source_key) || state.targets != task.targets {
            // The worker can finish in the background; its stale result is intentionally dropped.
        } else {
            match task.receiver.try_recv() {
                Ok(solution) => {
                    state.preview = Some(solution);
                    state.preview_due_at = None;
                }
                Err(TryRecvError::Empty) => {
                    state.preview_task = Some(task);
                    context.request_repaint_after(Duration::from_millis(16));
                }
                Err(TryRecvError::Disconnected) => {
                    state.preview_due_at = None;
                }
            }
        }
    }

    if state.preview.is_some()
        || state.preview_task.is_some()
        || !state.targets.iter().any(|target| *target > 0)
    {
        return;
    }

    let now = context.input(|input| input.time);
    if let Some(due_at) = state.preview_due_at {
        if now < due_at {
            context.request_repaint_after(Duration::from_secs_f64(due_at - now));
            return;
        }
        state.preview_due_at = None;
    }

    let (Some(input), Some(source_key)) = (state.input.clone(), state.source_key.clone()) else {
        return;
    };
    let targets = state.targets;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(solve(&input, targets));
    });
    state.preview_task = Some(PreviewTask {
        source_key,
        targets,
        receiver,
    });
    context.request_repaint_after(Duration::from_millis(16));
}

pub(super) fn source_key(
    document: &account::WorkspaceDocument,
    character_index: usize,
    plug_mode: PlugSelectionMode,
    allow_inventory_swaps: bool,
) -> SourceKey {
    SourceKey {
        character_index,
        plug_mode,
        allow_inventory_swaps,
        class_type: account::character_metadata(document, character_index)
            .ok()
            .map(|metadata| u64::from(metadata.class_type))
            .unwrap_or(99),
        equipment: account::equipped_item_snapshots(document, character_index).unwrap_or_default(),
        inventory: account::character_inventory(document, character_index)
            .ok()
            .flatten()
            .unwrap_or_default(),
    }
}

fn build_input(catalog: &Catalog, source: &SourceKey) -> LoadoutInput {
    let mut pieces = Vec::with_capacity(ARMOR_SLOTS.len());
    let mut candidates = Vec::with_capacity(ARMOR_SLOTS.len());

    for slot in ARMOR_SLOTS.iter().copied() {
        let (label, bucket_hash) = SLOTS
            .iter()
            .find_map(|(known, label, bucket)| (*known == slot).then_some((*label, *bucket)))
            .unwrap_or((slot, 0));
        let snapshot = source.equipment.iter().find(|item| item.slot == slot);
        let mut equipped_piece = read_snapshot_piece(catalog, slot, label, bucket_hash, snapshot);
        if let Some(item) = equipped_piece.item.as_ref() {
            equipped_piece.current_totals = cap_u16_totals(armor_stat_allocation::selected_totals(
                catalog,
                item,
                &equipped_piece.current_plugs,
            ));
        }
        let preserve_slot = equipped_piece.locked;
        let mut slot_candidates = vec![candidate_from_piece(
            catalog,
            equipped_piece.clone(),
            ArmorOrigin::Equipped,
            source.plug_mode,
        )];
        if source.allow_inventory_swaps && !preserve_slot && source.class_type <= 2 {
            for snapshot in &source.inventory {
                if snapshot.quantity != 1
                    || snapshot.flags.unwrap_or_default() & inventory::INVENTORY_FLAG_LOCKED != 0
                {
                    continue;
                }
                let Some(item) = catalog
                    .item_handle_for_bucket(u64::from(snapshot.definition_hash), bucket_hash)
                else {
                    continue;
                };
                if item.class_type != 3 && item.class_type != source.class_type {
                    continue;
                }
                let piece = read_inventory_piece(catalog, slot, label, item, snapshot);
                slot_candidates.push(candidate_from_piece(
                    catalog,
                    piece,
                    ArmorOrigin::Inventory {
                        instance_soid: snapshot.instance_soid,
                        definition_hash: snapshot.definition_hash,
                    },
                    source.plug_mode,
                ));
            }
        }
        pieces.push(equipped_piece);
        candidates.push(slot_candidates);
    }

    let mut current_totals = [0_u16; 6];
    for piece in &pieces {
        for (total, value) in current_totals.iter_mut().zip(piece.current_totals) {
            *total = total.saturating_add(value);
        }
    }
    LoadoutInput {
        pieces,
        candidates,
        current_totals: cap_u16_totals(current_totals),
    }
}

pub(super) fn candidate_from_piece(
    catalog: &Catalog,
    piece: ArmorPiece,
    origin: ArmorOrigin,
    plug_mode: PlugSelectionMode,
) -> ArmorCandidate {
    let Some(item) = piece.item.as_ref() else {
        return ArmorCandidate {
            piece,
            origin,
            fixed_totals: [0; 6],
            sockets: Vec::new(),
            exotic: false,
        };
    };
    let mut fixed_totals = catalog.armor_stat_values(item.hash);
    let mut sockets = Vec::new();
    let mut intrinsic_counted = false;
    for socket_index in 0..piece.current_plugs.len() {
        let current = piece.current_plugs[socket_index];
        let kind = if socket_is_preserved_masterwork(item, socket_index) {
            SocketKind::Masterwork
        } else {
            SocketKind::Stat
        };
        let choices = if piece.issue.is_none() && !piece.locked {
            match kind {
                SocketKind::Stat
                    if socket_is_adjustable_stat_socket(catalog, item, socket_index) =>
                {
                    socket_choices(catalog, item, socket_index, current, plug_mode)
                }
                SocketKind::Masterwork if !piece.masterworked => {
                    masterwork_choices(catalog, item, socket_index, current)
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let mutable = choices.len() > 1
            && choices.iter().any(|choice| {
                choice.hash != current && choice.values.iter().any(|value| *value > 0)
            });
        if mutable {
            sockets.push(MutableSocket {
                socket_index,
                current,
                choices,
                kind,
            });
        } else if let Some(hash) = current {
            if armor_stat_allocation::is_intrinsic_plug(catalog, hash) {
                if intrinsic_counted {
                    continue;
                }
                intrinsic_counted = true;
            }
            for (total, value) in
                fixed_totals
                    .iter_mut()
                    .zip(armor_stat_allocation::socket_stat_values(
                        catalog,
                        item,
                        socket_index,
                        hash,
                    ))
            {
                *total = total.saturating_add(value);
            }
        }
    }
    let exotic = catalog.item_rarity(item.hash) == ItemRarity::Exotic;
    ArmorCandidate {
        piece,
        origin,
        fixed_totals,
        sockets,
        exotic,
    }
}

pub(super) fn masterwork_choices(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
) -> Vec<SocketChoice> {
    let Some(socket) = item.sockets.get(socket_index) else {
        return Vec::new();
    };
    let current_choice = SocketChoice {
        hash: current,
        values: current.map_or([0; 6], |hash| {
            armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash)
        }),
    };
    let best = catalog
        .socket_options(socket)
        .iter()
        .copied()
        .filter(|hash| valid_masterwork_plug(catalog, *hash))
        .map(|hash| SocketChoice {
            hash: Some(hash),
            values: armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash),
        })
        .filter(|choice| choice.values.iter().any(|value| *value > 0))
        .max_by_key(|choice| (choice.values.iter().sum::<i32>(), Reverse(choice.hash)));
    let mut choices = vec![current_choice];
    if let Some(best) = best
        && best.hash != current
    {
        choices.push(best);
    }
    choices
}

pub(super) fn valid_masterwork_plug(catalog: &Catalog, hash: u64) -> bool {
    catalog.display_name(hash).is_some_and(|name| {
        let name = name.trim();
        name.contains("Masterwork")
            || name.ends_with(" Energy 10")
            || name.starts_with("Tier 10 Armor")
    })
}

pub(super) fn read_inventory_piece(
    catalog: &Catalog,
    slot: &'static str,
    label: &'static str,
    item: Arc<ItemDef>,
    snapshot: &inventory::InventoryItemSnapshot,
) -> ArmorPiece {
    let mut current_plugs = match &snapshot.plugs {
        inventory::ItemPlugs::NativeDefaults => item
            .default_plugs
            .iter()
            .map(|plug| plug.as_deref().and_then(parse_hash_hex))
            .collect::<Vec<_>>(),
        inventory::ItemPlugs::Authored(plugs) => plugs
            .iter()
            .map(|plug| plug.map(u64::from))
            .collect::<Vec<_>>(),
    };
    let socket_count = item
        .sockets
        .len()
        .max(item.default_plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    current_plugs.resize(socket_count, None);
    let flags = snapshot.flags.unwrap_or_default();
    let masterworked = piece_is_masterworked(catalog, &item, &current_plugs);
    let current_totals = cap_u16_totals(armor_stat_allocation::selected_totals(
        catalog,
        &item,
        &current_plugs,
    ));
    ArmorPiece {
        slot,
        label,
        name: item.name.clone(),
        item: Some(item),
        current_plugs,
        current_totals,
        locked: flags & inventory::INVENTORY_FLAG_LOCKED != 0,
        masterworked,
        issue: None,
    }
}

pub(super) fn socket_is_adjustable_stat_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
) -> bool {
    armor_stat_allocation::is_allocation_socket(catalog, item, socket_index)
        || is_armor_stat_mod_socket(catalog, item, socket_index)
}

pub(super) fn socket_is_preserved_masterwork(item: &ItemDef, socket_index: usize) -> bool {
    item.sockets.get(socket_index).is_some_and(|socket| {
        matches!(socket.socket_type, 29..=43 | 520 | 678 | 679) || {
            let label = socket.label.to_ascii_lowercase();
            label.contains("masterwork")
                || label.contains("armor tier")
                || label.contains("armor energy")
        }
    })
}

pub(super) fn piece_is_masterworked(
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &[Option<u64>],
) -> bool {
    plugs
        .iter()
        .copied()
        .enumerate()
        .any(|(socket_index, hash)| {
            let Some(hash) = hash else {
                return false;
            };
            if !socket_is_preserved_masterwork(item, socket_index) {
                return false;
            }
            catalog.display_name(hash).is_some_and(|name| {
                let name = name.trim();
                name.contains("Masterwork")
                    || name.ends_with(" Energy 10")
                    || name.starts_with("Tier 10 Armor")
            })
        })
}

pub(super) fn read_snapshot_piece(
    catalog: &Catalog,
    slot: &'static str,
    label: &'static str,
    bucket_hash: u64,
    snapshot: Option<&EquippedItemSnapshot>,
) -> ArmorPiece {
    let Some(snapshot) = snapshot else {
        return unavailable_piece(slot, label, "No equipped armor");
    };
    let Some(definition_hash) = snapshot.definition_hash else {
        return unavailable_piece(slot, label, "Invalid definition");
    };
    let Some(item) = catalog.item_handle_for_bucket(definition_hash, bucket_hash) else {
        return unavailable_piece(slot, label, "Definition unavailable");
    };
    let mut issue = snapshot.issues.first().cloned();
    let mut current_plugs = match &snapshot.plugs {
        EquippedItemPlugs::NativeDefaults => item
            .default_plugs
            .iter()
            .map(|plug| plug.as_deref().and_then(parse_hash_hex))
            .collect(),
        EquippedItemPlugs::Authored(plugs) => plugs
            .iter()
            .map(|plug| match plug {
                EquippedPlugValue::Empty => None,
                EquippedPlugValue::Hash(hash) => Some(*hash),
                EquippedPlugValue::Malformed(_) => {
                    issue = Some("Invalid authored plug".to_owned());
                    None
                }
            })
            .collect(),
        EquippedItemPlugs::Missing | EquippedItemPlugs::Malformed(_) => {
            issue = Some("Plugs unavailable".to_owned());
            Vec::new()
        }
    };
    let socket_count = item
        .sockets
        .len()
        .max(item.default_plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    current_plugs.resize(socket_count, None);
    let masterworked = piece_is_masterworked(catalog, &item, &current_plugs);
    ArmorPiece {
        slot,
        label,
        name: item.name.clone(),
        item: Some(item),
        current_plugs,
        current_totals: [0; 6],
        locked: snapshot.flags.unwrap_or_default() & inventory::INVENTORY_FLAG_LOCKED != 0,
        masterworked,
        issue,
    }
}

pub(super) fn unavailable_piece(
    slot: &'static str,
    label: &'static str,
    issue: &str,
) -> ArmorPiece {
    ArmorPiece {
        slot,
        label,
        name: "Unavailable".to_owned(),
        item: None,
        current_plugs: Vec::new(),
        current_totals: [0; 6],
        locked: false,
        masterworked: false,
        issue: Some(issue.to_owned()),
    }
}

pub(super) fn socket_choices(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
    mode: PlugSelectionMode,
) -> Vec<SocketChoice> {
    if item.sockets.get(socket_index).is_none() {
        return Vec::new();
    }
    let allocation_socket =
        armor_stat_allocation::is_allocation_socket(catalog, item, socket_index);
    let armor_mod_socket = is_armor_stat_mod_socket(catalog, item, socket_index);
    if !allocation_socket && !armor_mod_socket {
        return Vec::new();
    }
    let mut hashes =
        crate::investment::plug_selection::candidates_for_socket(catalog, item, socket_index, mode);
    if let Some(current) = current {
        hashes.push(current);
    }
    hashes.sort_unstable();
    hashes.dedup();

    let mut by_values = HashMap::<[i32; 6], Option<u64>>::new();
    if current.is_none() || armor_mod_socket {
        by_values.insert([0; 6], None);
    }
    for hash in hashes {
        if hash == u64::from(NO_DEFINITION_HASH.get()) || u32::try_from(hash).is_err() {
            continue;
        }
        if allocation_socket && !armor_stat_allocation::is_allocation_plug(catalog, hash) {
            continue;
        }
        if armor_mod_socket && !is_armor_stat_mod_plug(catalog, hash) {
            continue;
        }
        let values = armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash);
        if values.iter().all(|value| *value == 0) && Some(hash) != current {
            continue;
        }
        by_values
            .entry(values)
            .and_modify(|stored| {
                if Some(hash) == current
                    || stored.is_some_and(|old| Some(old) != current && hash < old)
                {
                    *stored = Some(hash);
                }
            })
            .or_insert(Some(hash));
    }

    let mut choices = by_values
        .into_iter()
        .map(|(values, hash)| SocketChoice { hash, values })
        .collect::<Vec<_>>();
    choices.sort_by_key(|choice| (choice.hash != current, choice.values, choice.hash));
    choices
}
