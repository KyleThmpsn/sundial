//! Stored-item card rendering and plug editing.

use super::*;

#[derive(Default)]
struct InventoryItemCardRequests {
    actions: Vec<InventoryItemAction>,
    remove: bool,
    equip: Option<&'static str>,
    move_to: Option<usize>,
    swap_requested: bool,
    swap_response: Option<egui::Response>,
}

struct InventoryItemActionContext<'a> {
    snapshot: &'a InventoryItemSnapshot,
    ui_identity: InventoryItemUiId,
    editable: bool,
    valid: bool,
    equipment_target: Option<(&'static str, &'static str)>,
    target_occupied: bool,
}

struct InventoryItemPickerContext<'a> {
    snapshot: &'a InventoryItemSnapshot,
    ui_identity: InventoryItemUiId,
    editable: bool,
    class_type: u64,
    allow_cross_class_subclasses: bool,
    current_bucket: Option<u8>,
    replacing_unresolved: bool,
    transfer_destinations: &'a [CharacterTransferDestination],
    bucket_usage: &'a BucketUsage,
    equipment_target: Option<(&'static str, &'static str)>,
    key: &'a str,
    picker_anchor: &'a egui::Response,
}

impl SundialApp {
    pub(super) fn draw_inventory_item_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        editable: bool,
        class_type: u64,
        context: CharacterInventoryCardContext<'_>,
    ) -> Option<CharacterInventoryItemRequest> {
        let allow_cross_class_subclasses = self.preferences.experimental_cross_class_subclasses;
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let metadata = self
            .manifest
            .inventory_metadata(u64::from(snapshot.definition_hash))
            .copied();
        let hash_hex_text = format_hash_hex(u64::from(snapshot.definition_hash));
        let soid_text = format!("0x{:016X}", snapshot.instance_soid);
        let current_bucket = metadata
            .filter(|metadata| metadata.scope == InventoryScope::Character)
            .map(|metadata| metadata.native_bucket_id);
        let replacing_unresolved =
            metadata.is_none_or(|metadata| metadata.scope == InventoryScope::Unknown);
        let valid = resolved.as_ref().is_some_and(|definition| {
            definition.metadata.is_character_inventory_candidate()
                && definition.item.as_ref().is_some_and(|item| {
                    equipment::item_class_is_compatible(
                        item,
                        class_type,
                        self.preferences.experimental_cross_class_subclasses,
                    )
                })
        });
        let key = inventory_item_state_key(ui_identity);
        let mut requests = InventoryItemCardRequests::default();
        let transfer_destinations =
            self.character_transfer_destinations(context.transfer_targets, resolved.as_ref());
        let equipment_target = resolved
            .as_ref()
            .and_then(|definition| definition.item.as_ref())
            .and_then(|item| equipment_target_for_bucket(item.bucket_hash))
            .filter(|(slot, _)| {
                self.document
                    .equipment_slots()
                    .iter()
                    .any(|(known, _, _)| known == slot)
            });
        let target_occupied = equipment_target
            .is_some_and(|(slot, _)| context.occupied_equipment_slots.contains(&slot));

        ui.push_id(
            ("character-inventory-item", ui_identity),
            |ui| {
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = resolved.as_ref().map_or(
        DefinitionSummary::Unknown {
            hash_display_text: &hash_hex_text,
        },
                        |definition| DefinitionSummary::Known {
                            name: &definition.name,
            hash_display_text: &hash_hex_text,
                            type_name: &definition.type_name,
                        },
                    );
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        Some(u64::from(snapshot.definition_hash)),
                        Some(DefinitionInspectionContext {
                            source: format!(
                                "Character {} Inventory · Item {}",
                                snapshot.location.character_index + 1,
                                snapshot.location.item_index + 1
                            ),
                            instance_id: Some(soid_text.clone()),
                            authored_level: Some(i64::from(snapshot.level)),
                            flags: snapshot.flags,
                            quantity: Some(i64::from(snapshot.quantity)),
                            plugs: Some(match &snapshot.plugs {
                                ItemPlugs::NativeDefaults => serde_json::Value::Null,
                                ItemPlugs::Authored(plugs) => serde_json::json!(plugs),
                            }),
                            plug_count: Some(match &snapshot.plugs {
                                ItemPlugs::NativeDefaults => resolved
                                    .as_ref()
                                    .and_then(|definition| definition.item.as_ref())
                                    .map_or(0, |item| item.default_plugs.len()),
                                ItemPlugs::Authored(plugs) => plugs.len(),
                            }),
                        }),
                        ItemHeader {
                            label: None,
                            soid: Some(&soid_text),
                            definition,
                            icon: None,
                            fill: item_editor::muted_item_header_fill(ui),
                            valid,
                            invalid_message: "not valid for this character inventory",
                        },
                        |_| {},
                    );
                    self.draw_inventory_item_actions(
                        ui,
                        InventoryItemActionContext {
                            snapshot,
                            ui_identity,
                            editable,
                            valid,
                            equipment_target,
                            target_occupied,
                        },
                        &mut requests,
                    );

                    let picker_anchor = requests.swap_response.as_ref().map_or_else(
                        || header_response.clone(),
                        |swap_response| header_response.clone() | swap_response.clone(),
                    );
                    self.draw_inventory_item_picker(
                        ui,
                        InventoryItemPickerContext {
                            snapshot,
                            ui_identity,
                            editable,
                            class_type,
                            allow_cross_class_subclasses,
                            current_bucket,
                            replacing_unresolved,
                            transfer_destinations: &transfer_destinations,
                            bucket_usage: context.bucket_usage,
                            equipment_target,
                            key: &key,
                            picker_anchor: &picker_anchor,
                        },
                        &mut requests,
                    );
                    if !requests.actions
                        .iter()
                        .any(|action| matches!(action, InventoryItemAction::Remove))
                    {
                        if let Some(item) = resolved
                            .as_ref()
                            .and_then(|definition| definition.item.as_ref())
                        {
                            self.draw_inventory_plugs(
                                ui,
                                snapshot,
                                ui_identity,
                                item,
                                editable,
                                &mut requests.actions,
                            );
                        } else if matches!(snapshot.plugs, ItemPlugs::Authored(ref plugs) if !plugs.is_empty())
                        {
                            ui.label(
                                egui::RichText::new(
                                    "Plugs are preserved but cannot be guided without an installed item definition.",
                                )
                                .weak(),
                            );
                        }
                    }
                    });
            },
        );
        if requests.remove {
            return Some(CharacterInventoryItemRequest::Apply(vec![
                InventoryItemAction::Remove,
            ]));
        }
        if let Some(destination_character_index) = requests.move_to {
            return Some(CharacterInventoryItemRequest::MoveTo(
                destination_character_index,
            ));
        }
        if let Some(slot) = requests.equip {
            return Some(CharacterInventoryItemRequest::Equip(slot));
        }
        (!requests.actions.is_empty())
            .then_some(CharacterInventoryItemRequest::Apply(requests.actions))
    }

    fn draw_inventory_item_actions(
        &mut self,
        ui: &mut egui::Ui,
        context: InventoryItemActionContext<'_>,
        requests: &mut InventoryItemCardRequests,
    ) {
        let InventoryItemActionContext {
            snapshot,
            ui_identity,
            editable,
            valid,
            equipment_target,
            target_occupied,
        } = context;
        ui.add_enabled_ui(editable, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                for action in item_editor::draw_level_and_quantity(
                    ui,
                    ("character-inventory-numeric", ui_identity),
                    NumericItemFields {
                        level: Some(i64::from(snapshot.level)),
                        power_max: self
                            .manifest
                            .item_power_cap(u64::from(snapshot.definition_hash)),
                        allow_power_above_cap: self.preferences.experimental_power_above_cap,
                        quantity: None,
                        quantity_max: None,
                    },
                ) {
                    if let ItemEditorAction::SetLevel { level } = action
                        && let Ok(level) = i32::try_from(level)
                    {
                        requests.actions.push(InventoryItemAction::SetLevel(level));
                    }
                }
                ui.add_space(8.0);
                if let Some(flags) = item_editor::draw_masterwork_flag(
                    ui,
                    snapshot.flags,
                    self.document.supports_v13_account(),
                ) {
                    requests.actions.push(InventoryItemAction::SetFlags(flags));
                }
                let flags = snapshot.flags.unwrap_or_default();
                let locked = flags & INVENTORY_FLAG_LOCKED != 0;
                let lock_response = if locked {
                    item_editor::draw_lock_button(ui, true, "Unlock stored item")
                        .on_hover_text("Unlock this item")
                } else {
                    item_editor::draw_unlock_button(ui, true, "Lock stored item")
                        .on_hover_text("Lock this item")
                };
                if lock_response.clicked() {
                    requests.actions.push(InventoryItemAction::SetFlags(
                        set_inventory_locked_flag(snapshot.flags, !locked),
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    if item_editor::draw_trash_button(ui, true, "Delete stored item")
                        .on_hover_text("Delete this stored item")
                        .clicked()
                    {
                        requests.remove = true;
                    }
                    let response = ui
                        .add(egui::Button::new("Swap").small())
                        .on_hover_text("Open item picker");
                    requests.swap_requested = response.clicked();
                    requests.swap_response = Some(response);
                    if let Some((slot, slot_label)) = equipment_target {
                        let can_equip = editable && valid && snapshot.quantity == 1;
                        let tooltip = if snapshot.quantity != 1 {
                            "Only a single inventory item can be equipped at a time".to_owned()
                        } else if !valid {
                            format!("This item is not valid for the {slot_label} slot")
                        } else if target_occupied {
                            format!("Equip in the {slot_label} slot and move its current item here")
                        } else {
                            format!("Equip in the empty {slot_label} slot")
                        };
                        let response =
                            ui.add_enabled(can_equip, egui::Button::new("Equip").small());
                        let response = if can_equip {
                            response.on_hover_text(tooltip)
                        } else {
                            response.on_disabled_hover_text(tooltip)
                        };
                        if response.clicked() {
                            requests.equip = Some(slot);
                        }
                    }
                });
            });
        });
    }

    fn draw_inventory_item_picker(
        &mut self,
        ui: &mut egui::Ui,
        context: InventoryItemPickerContext<'_>,
        requests: &mut InventoryItemCardRequests,
    ) {
        let InventoryItemPickerContext {
            snapshot,
            ui_identity,
            editable,
            class_type,
            allow_cross_class_subclasses,
            current_bucket,
            replacing_unresolved,
            transfer_destinations,
            bucket_usage,
            equipment_target,
            key,
            picker_anchor,
        } = context;
        let picker_action = ui
            .add_enabled_ui(editable, |ui| {
                let manifest = &self.manifest;
                let show_dummy_items = self.show_dummy_items;
                let query = self.searches.entry(key.to_owned()).or_default();
                item_editor::draw_definition_picker_with_open_request_and_footer(
                    ui,
                    manifest,
                    ("character-inventory-definition", ui_identity),
                    query,
                    picker_height_with_transfer_destinations(transfer_destinations.len()),
                    (Some(picker_anchor), requests.swap_requested),
                    (
                        |query| DefinitionPickerChoices {
                            definitions: without_definition_groups(character_definition_choices(
                                manifest
                                    .character_inventory_candidates(
                                        query,
                                        class_type,
                                        show_dummy_items,
                                        allow_cross_class_subclasses,
                                    )
                                    .filter(|definition| {
                                        crate::account_contract::definition_available(
                                            definition.hash,
                                            self.document.supports_v13_account(),
                                        )
                                    })
                                    .filter(|definition| {
                                        bucket_has_room(
                                            definition.metadata,
                                            bucket_usage,
                                            current_bucket,
                                            replacing_unresolved,
                                        )
                                    }),
                            )),
                            existing_inventory: Vec::new(),
                            clear: None,
                            random_item_builder_hash: equipment_target
                                .filter(|(slot, _)| {
                                    WEAPON_SLOTS.contains(slot) || ARMOR_SLOTS.contains(slot)
                                })
                                .map(|_| u64::from(snapshot.definition_hash)),
                            empty_message: "No compatible items with space in this bucket"
                                .to_owned(),
                        },
                        |ui| draw_character_transfer_destinations(ui, transfer_destinations),
                    ),
                )
            })
            .inner;
        requests.move_to = picker_action.1;
        if let Some(ItemEditorAction::OpenInRandomItemBuilder { hash }) = picker_action.0 {
            equipment::request_inventory_item_builder(
                ui.ctx(),
                snapshot.location.character_index,
                hash,
                &snapshot.plugs,
            );
        }
        if let Some(ItemEditorAction::SetDefinition { hash }) = picker_action.0
            && let Ok(hash) = u32::try_from(hash)
        {
            requests
                .actions
                .push(InventoryItemAction::SetDefinitionHash(hash));
            requests
                .actions
                .push(InventoryItemAction::SetPlugs(ItemPlugs::NativeDefaults));
            if let Some(maximum) = self
                .manifest
                .inventory_metadata(u64::from(hash))
                .and_then(|metadata| metadata.max_stack_size)
                .map(|maximum| maximum.min(i32::MAX as u32) as i32)
                && snapshot.quantity > maximum
            {
                requests
                    .actions
                    .push(InventoryItemAction::SetQuantity(maximum.max(1)));
            }
            self.searches.insert(key.to_owned(), String::new());
        }
    }

    fn draw_inventory_plugs(
        &mut self,
        ui: &mut egui::Ui,
        inventory: &InventoryItemSnapshot,
        ui_identity: InventoryItemUiId,
        item: &ItemDef,
        editable: bool,
        requested: &mut Vec<InventoryItemAction>,
    ) {
        let (current_plugs, native_defaults) = displayed_inventory_plugs(inventory, item);
        if item.sockets.is_empty() && current_plugs.is_empty() {
            return;
        }
        let socket_count = item
            .sockets
            .len()
            .max(current_plugs.len())
            .min(inventory::MAX_ITEM_PLUGS);
        let title = if native_defaults {
            format!("Plugs ({socket_count}, native defaults)")
        } else {
            format!("Plugs ({socket_count})")
        };
        egui::CollapsingHeader::new(title)
            .id_salt(("character-inventory-plugs", ui_identity))
            .show(ui, |ui| {
                for socket_index in 0..socket_count {
                    let current_hash = current_plugs
                        .get(socket_index)
                        .copied()
                        .flatten()
                        .map(u64::from);
                    let native_default = native_plug_default(&item.default_plugs, socket_index);
                    let query_key = format!(
                        "{}:plug:{socket_index}",
                        inventory_item_state_key(ui_identity)
                    );
                    let mut query = self
                        .plug_searches
                        .get(&query_key)
                        .cloned()
                        .unwrap_or_default();
                    let picker_snapshot = item_editor::plug_picker_snapshot(
                        &self.manifest,
                        item,
                        socket_index,
                        current_hash,
                        current_hash.map_or_else(
                            || "None".to_owned(),
                            |hash| {
                                self.manifest
                                    .plug_label(hash, self.preferences.show_plug_hashes)
                            },
                        ),
                        native_default,
                        self.plug_selection_mode,
                    );
                    let searchable = picker_snapshot.choices.len() > 12;
                    let action = ui
                        .add_enabled_ui(editable, |ui| {
                            item_editor::draw_plug_picker(
                                ui,
                                &self.manifest,
                                ("character-inventory-plug", ui_identity, socket_index),
                                &mut query,
                                &picker_snapshot,
                                PickerHeight {
                                    min: PLUG_PICKER_MIN_HEIGHT,
                                    max: PLUG_PICKER_MAX_HEIGHT,
                                },
                            )
                        })
                        .inner;
                    if let Some(ItemEditorAction::SetPlug { socket_index, hash }) = action {
                        let mut plugs = current_plugs.clone();
                        while plugs.len() <= socket_index {
                            plugs.push(None);
                        }
                        plugs[socket_index] = hash.and_then(|hash| u32::try_from(hash).ok());
                        requested.push(InventoryItemAction::SetPlugs(ItemPlugs::Authored(plugs)));
                    }
                    if searchable {
                        self.plug_searches.insert(query_key, query);
                    } else {
                        self.plug_searches.remove(&query_key);
                    }
                }
            });
    }
}
