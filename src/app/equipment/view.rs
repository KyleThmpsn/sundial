use super::*;

struct EquipmentPlugEditor<'a> {
    id_scope: &'static str,
    character_index: usize,
    slot: &'static str,
    item: &'a ItemDef,
    authored_plugs: Option<&'a Value>,
    guided_editable: bool,
}

impl SundialApp {
    pub(in crate::app) fn draw_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        match self.character_inventory_layout {
            super::super::CharacterInventoryLayout::Cards => {
                self.draw_sundial_equipment(ui, character_index);
            }
            super::super::CharacterInventoryLayout::Panoptes => {
                self.draw_panoptes_equipment(ui, character_index);
            }
        }
    }

    fn draw_sundial_equipment(&mut self, ui: &mut egui::Ui, character_index: usize) {
        let schema_mode = super::inventory::schema_mode(&self.document);
        let editable = schema_mode.can_mutate_equipment();
        let inventory_editable = schema_mode.can_mutate_character_inventory();
        let class_type = self
            .characters()
            .and_then(|chars| chars.get(character_index))
            .and_then(|ch| ch.get("class"))
            .and_then(Value::as_u64)
            .unwrap_or(0);

        ui.add_space(14.0);
        let randomize_request = ui
            .horizontal(|ui| {
                ui.heading("Equipped loadout");
                let randomize_request = randomize::draw_menu(ui, editable, inventory_editable);
                if armor_stats_adjuster::draw_entry_button(ui, editable).clicked() {
                    self.armor_stats_adjuster.open(character_index);
                }
                if ui
                    .add(egui::Button::new("Inventory ›").small())
                    .on_hover_text("Open this character's stored inventory")
                    .clicked()
                {
                    self.select_view(super::ViewMode::CharacterInventory);
                }
                randomize_request
            })
            .inner;
        randomize::draw_dialogs(self, ui.ctx(), character_index, randomize_request);
        armor_stats_adjuster::draw_window(self, ui.ctx(), character_index);
        self.draw_equipped_armor_stat_row(ui, character_index);
        ui.add_enabled_ui(editable, |ui| self.draw_item_safety_controls(ui));
        ui.add_space(6.0);

        let slots = SLOTS
            .iter()
            .copied()
            .filter(|(slot, _, _)| *slot != "subclass")
            .collect::<Vec<_>>();
        let (minimum_card_width, maximum_card_width) = self.item_card_width.dimensions();
        item_editor::draw_responsive_item_cards(
            ui,
            &slots,
            minimum_card_width,
            maximum_card_width,
            |ui, &(slot, label, bucket)| {
                self.draw_equipment_slot_card(
                    ui,
                    character_index,
                    EquipmentSlotCard {
                        id_scope: "characters-equipment",
                        slot,
                        label,
                        bucket_hash: bucket,
                        class_type,
                        editable,
                        header_fill: None,
                        snapshot: None,
                    },
                );
            },
        );
    }

    pub(in crate::app) fn draw_equipped_armor_stat_row(
        &self,
        ui: &mut egui::Ui,
        character_index: usize,
    ) {
        let totals =
            armor_stats_adjuster::equipped_totals(&self.document, &self.manifest, character_index);
        ui.add_space(3.0);
        ui.separator();
        ui.add_space(3.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (index, (name, value)) in armor_stat_allocation::STAT_NAMES
                .into_iter()
                .zip(totals)
                .enumerate()
            {
                if index > 0 {
                    ui.add_space(10.0);
                }
                if let Some(icon) = self.manifest.armor_stat_icon_texture(ui.ctx(), name) {
                    ui.add(egui::Image::new((icon.id(), egui::vec2(15.0, 15.0))));
                }
                ui.label(name);
                ui.strong(value.to_string());
            }
        })
        .response
        .on_hover_text("Current totals from all equipped armor and stat plugs");
        ui.add_space(3.0);
        ui.separator();
        ui.add_space(3.0);
    }

    pub(in crate::app) fn draw_equipment_slot_card(
        &mut self,
        ui: &mut egui::Ui,
        character_index: usize,
        card: EquipmentSlotCard<'_>,
    ) {
        let EquipmentSlotCard {
            id_scope,
            slot,
            label,
            bucket_hash: bucket,
            class_type,
            editable,
            header_fill,
            snapshot,
        } = card;
        let (
            is_empty,
            current_level,
            current_flags,
            current_hash,
            current_hash_display_text,
            current_soid_text,
            authored_plugs,
        ) = {
            let equipped = self
                .characters()
                .and_then(|characters| characters.get(character_index))
                .and_then(|character| character.get("equipment"))
                .and_then(Value::as_object)
                .and_then(|equipment| equipment.get(slot));
            let hash_value = equipped.and_then(|item| item.get("definition_hash"));
            let hash = hash_value.and_then(parse_unsigned_value);
            (
                equipped.is_some_and(Value::is_null),
                equipped
                    .and_then(|item| item.get("level"))
                    .and_then(Value::as_i64),
                equipped
                    .and_then(|item| item.get("flags"))
                    .and_then(parse_unsigned_value)
                    .and_then(|flags| u8::try_from(flags).ok()),
                hash,
                hash.map_or_else(
                    || {
                        hash_value
                            .and_then(Value::as_str)
                            .unwrap_or("<missing>")
                            .to_owned()
                    },
                    format_hash_hex,
                ),
                equipped
                    .and_then(|item| item.get("instance_soid"))
                    .map(|value| {
                        parse_unsigned_value(value).map_or_else(
                            || field_display_text(Some(value)),
                            |soid| format!("0x{soid:016X}"),
                        )
                    }),
                equipped.and_then(|item| item.get("plugs")).cloned(),
            )
        };
        let current =
            current_hash.and_then(|hash| self.manifest.item_handle_for_bucket(hash, bucket));
        let definition_valid = is_empty
            || current.as_ref().is_some_and(|item| {
                item.bucket_hash == bucket
                    && (item.class_type == 3 || item.class_type == class_type)
            });
        let snapshot_valid = snapshot.is_none_or(|snapshot| snapshot.issues.is_empty());
        let valid = definition_valid && snapshot_valid;
        let guided_editable = editable && snapshot_valid;
        let flags_editable =
            super::inventory::schema_mode(&self.document).can_mutate_equipment_flags();
        let inventory_editable =
            super::inventory::schema_mode(&self.document).can_mutate_character_inventory();
        let equipped_label = equipped_header_label(id_scope, label);
        let header_soid = snapshot
            .map(|snapshot| snapshot.instance_soid_text.as_str())
            .or(current_soid_text.as_deref());
        let existing_inventory = equipment_inventory_choices(
            &self.document,
            &self.manifest,
            character_index,
            bucket,
            class_type,
        );
        ui.push_id((id_scope, character_index, slot), |ui| {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::ZERO)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let definition = if is_empty {
                        DefinitionSummary::Empty
                    } else if let Some(item) = &current {
                        DefinitionSummary::Known {
                            name: &item.name,
                            hash_display_text: &current_hash_display_text,
                            type_name: &item.type_name,
                        }
                    } else {
                        DefinitionSummary::Unknown {
                            hash_display_text: &current_hash_display_text,
                        }
                    };
                    let header = ItemHeader {
                        label: Some(&equipped_label),
                        soid: header_soid,
                        definition,
                        icon: None,
                        fill: header_fill
                            .unwrap_or_else(|| item_editor::muted_item_header_fill(ui)),
                        valid,
                        invalid_message: if definition_valid {
                            "invalid equipped item"
                        } else {
                            "invalid for slot/class"
                        },
                    };
                    let header_response = item_editor::draw_catalog_item_header_with_trailing(
                        ui,
                        &self.manifest,
                        current_hash,
                        header,
                        |_| {},
                    );

                    if let Some(snapshot) = snapshot {
                        if !snapshot.issues.is_empty() {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                snapshot.issues.join(" · "),
                            )
                            .on_hover_text(format!("Authored item: {}", snapshot.raw_item_text));
                            ui.label(
                                egui::RichText::new(
                                    "Guided edits are disabled for this malformed equipped item.",
                                )
                                .weak(),
                            );
                        }
                    }

                    let mut swap_requested = false;
                    let mut empty_requested = false;
                    let mut unequip_requested = false;
                    let swap_response = ui.horizontal(|ui| {
                            ui.add_space(4.0);
                            if !is_empty {
                                ui.add_enabled_ui(guided_editable, |ui| {
                                    if let Some(level) = current_level {
                                        for action in item_editor::draw_level_and_quantity(
                                            ui,
                                            ("equipment-numeric", character_index, slot),
                                            NumericItemFields {
                                                level: Some(level),
                                                power_max: current_hash
                                                    .and_then(|hash| self.manifest.item_power_cap(hash)),
                                                allow_power_above_cap: self
                                                    .experimental_power_above_cap,
                                                quantity: None,
                                                quantity_max: None,
                                            },
                                        ) {
                                            if let ItemEditorAction::SetLevel { level } = action {
                                                self.select_equipment_level(
                                                    character_index,
                                                    slot,
                                                    level,
                                                );
                                            }
                                        }
                                    } else {
                                        ui.label("Power");
                                        ui.label(
                                            egui::RichText::new("<invalid or missing>").weak(),
                                        );
                                    }
                                });

                                ui.add_space(8.0);
                                ui.add_enabled_ui(guided_editable && flags_editable, |ui| {
                                    let locked = current_flags.unwrap_or_default()
                                        & super::inventory::INVENTORY_FLAG_LOCKED
                                        != 0;
                                    let lock_response = if locked {
                                        super::item_editor::draw_lock_button(
                                            ui,
                                            true,
                                            "Unlock equipped item",
                                        )
                                        .on_hover_text("Unlock this item")
                                    } else {
                                        super::item_editor::draw_unlock_button(
                                            ui,
                                            true,
                                            "Lock equipped item",
                                        )
                                        .on_hover_text("Lock this item")
                                    };
                                    if lock_response.clicked() {
                                        self.select_equipment_flags(
                                            character_index,
                                            slot,
                                            super::inventory::set_inventory_locked_flag(
                                                current_flags,
                                                !locked,
                                            ),
                                        );
                                    }
                                });
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(4.0);
                                if !is_empty && WEAPON_SLOTS.contains(&slot) {
                                    let response = item_editor::draw_trash_button(
                                        ui,
                                        guided_editable,
                                        "Delete equipped item",
                                    )
                                    .on_hover_text(format!(
                                        "Delete this item and set the {} slot to empty. This does not move it to inventory (use Unequip).",
                                        equipment_slot_label(slot)
                                    ));
                                    empty_requested = response.clicked();
                                }
                                let response = ui
                                    .add_enabled(guided_editable, egui::Button::new("Swap").small())
                                    .on_hover_text("Open item picker");
                                swap_requested = response.clicked();
                                if !is_empty && WEAPON_SLOTS.contains(&slot) {
                                    let tooltip = if inventory_editable {
                                        format!(
                                            "Move the {} item to character inventory",
                                            equipment_slot_label(slot)
                                        )
                                    } else {
                                        "Unequipping to inventory requires settings schema 6"
                                            .to_owned()
                                    };
                                    let response = ui
                                        .add_enabled(
                                            guided_editable && inventory_editable,
                                            egui::Button::new("Unequip").small(),
                                        )
                                        .on_hover_text(tooltip);
                                    unequip_requested = response.clicked();
                                }
                                response
                            })
                            .inner
                        }).inner;
                    let picker_anchor = header_response.clone() | swap_response;
                    let key = format!("{id_scope}:{character_index}:{slot}");
                    if empty_requested {
                        self.empty_weapon(character_index, slot);
                        self.searches.insert(key.clone(), String::new());
                    }
                    if unequip_requested {
                        self.unequip_weapon(character_index, slot);
                        self.searches.insert(key.clone(), String::new());
                    }
                    let picker_action = {
                        let manifest = &self.manifest;
                        let show_dummy_items = self.show_dummy_items;
                        let query = self.searches.entry(key.clone()).or_default();
                        ui.add_enabled_ui(guided_editable, |ui| {
                            item_editor::draw_definition_picker_with_open_request(
                                ui,
                                manifest,
                                ("equipment-definition", id_scope, character_index, slot),
                                query,
                                PickerHeight {
                                    min: ITEM_PICKER_MIN_HEIGHT,
                                    max: ITEM_PICKER_MAX_HEIGHT,
                                },
                                (Some(&picker_anchor), swap_requested),
                                |query_value| {
                                    let candidates = if query_value.trim().is_empty() {
                                        manifest.browse(bucket, class_type, show_dummy_items)
                                    } else {
                                        manifest.search(
                                            query_value,
                                            bucket,
                                            class_type,
                                            show_dummy_items,
                                        )
                                    };
                                    let needle = query_value.to_lowercase();
                                    let definitions = equipment_definition_choices(candidates);
                                    let existing_inventory = existing_inventory
                                        .iter()
                                        .filter(|choice| {
                                            existing_inventory_choice_matches(
                                                manifest,
                                                choice,
                                                query_value,
                                            )
                                        })
                                        .cloned()
                                        .collect();
                                    let show_empty_weapon = WEAPON_SLOTS.contains(&slot)
                                        && (query_value.trim().is_empty()
                                            || "empty weapon".contains(&needle));
                                    DefinitionPickerChoices {
                                        definitions,
                                        existing_inventory,
                                        clear: show_empty_weapon.then(|| ClearDefinitionChoice {
                                            label: "Empty weapon".to_owned(),
                                            tooltip: "Sets this equipment slot to empty."
                                                .to_owned(),
                                            selected: is_empty,
                                        }),
                                        random_item_builder_hash: current_hash.filter(|_| {
                                            WEAPON_SLOTS.contains(&slot)
                                                || ARMOR_SLOTS.contains(&slot)
                                        }),
                                        empty_message: "No compatible installed items found"
                                            .to_owned(),
                                    }
                                },
                            )
                        })
                        .inner
                    };
                    match picker_action {
                        Some(ItemEditorAction::ClearDefinition) => {
                            self.empty_weapon(character_index, slot);
                            self.searches.insert(key.clone(), String::new());
                        }
                        Some(ItemEditorAction::SetDefinition { hash }) => {
                            if let Some(item) = self.manifest.item_handle_for_bucket(hash, bucket) {
                                if slot == "subclass" {
                                    self.select_subclass_item(character_index, &item);
                                } else {
                                    self.select_item(character_index, slot, &item);
                                }
                                self.searches.insert(key.clone(), String::new());
                            }
                        }
                        Some(ItemEditorAction::EquipInventoryItem { item_index }) => {
                            self.equip_stored_item(
                                super::inventory::InventoryItemLocation {
                                    character_index,
                                    item_index,
                                },
                                slot,
                            );
                            self.searches.insert(key.clone(), String::new());
                        }
                        Some(ItemEditorAction::OpenInRandomItemBuilder { hash }) => {
                            randomize::request_item_builder(
                                ui.ctx(),
                                character_index,
                                hash,
                                authored_plugs.clone(),
                            );
                        }
                        _ => {}
                    }

                    if let Some(item) = &current {
                        self.draw_equipment_plugs(
                            ui,
                            EquipmentPlugEditor {
                                id_scope,
                                character_index,
                                slot,
                                item,
                                authored_plugs: authored_plugs.as_ref(),
                                guided_editable,
                            },
                        );
                    }
                });
        });
    }

    fn draw_equipment_plugs(&mut self, ui: &mut egui::Ui, editor: EquipmentPlugEditor<'_>) {
        let EquipmentPlugEditor {
            id_scope,
            character_index,
            slot,
            item,
            authored_plugs,
            guided_editable,
        } = editor;
        let (current_plugs, native_defaults) = displayed_plugs(authored_plugs, &item.default_plugs);
        if item.sockets.is_empty() && current_plugs.is_empty() {
            return;
        }
        let title = if native_defaults {
            format!("Plugs ({}, native defaults)", current_plugs.len())
        } else {
            format!("Plugs ({})", current_plugs.len())
        };
        egui::CollapsingHeader::new(title)
            .id_salt(("equipment-plugs", id_scope, character_index, slot))
            .show(ui, |ui| {
                let socket_count = item.sockets.len().max(current_plugs.len());
                // A plug's array index is part of the Sunrise save schema.
                // Keep sockets in that exact order even when a label is unknown.
                for socket_index in 0..socket_count {
                    let current_hash = current_plugs
                        .get(socket_index)
                        .and_then(parse_unsigned_value);
                    let native_default = native_plug_default(&item.default_plugs, socket_index);
                    let current_label = current_hash.map_or_else(
                        || "None".to_owned(),
                        |hash| self.manifest.plug_label(hash, self.show_plug_hashes),
                    );
                    let plug_search_key =
                        format!("plug-search:{id_scope}:{character_index}:{slot}:{socket_index}");
                    let mut plug_query = self
                        .plug_searches
                        .get(&plug_search_key)
                        .cloned()
                        .unwrap_or_default();
                    let snapshot = item_editor::plug_picker_snapshot(
                        &self.manifest,
                        item,
                        socket_index,
                        current_hash,
                        current_label,
                        native_default,
                        self.plug_selection_mode,
                    );
                    let searchable = snapshot.choices.len() > 12;
                    let action = ui
                        .add_enabled_ui(guided_editable, |ui| {
                            item_editor::draw_plug_picker(
                                ui,
                                &self.manifest,
                                (
                                    "equipment-plug",
                                    id_scope,
                                    character_index,
                                    slot,
                                    socket_index,
                                ),
                                &mut plug_query,
                                &snapshot,
                                PickerHeight {
                                    min: PLUG_PICKER_MIN_HEIGHT,
                                    max: PLUG_PICKER_MAX_HEIGHT,
                                },
                            )
                        })
                        .inner;
                    if let Some(ItemEditorAction::SetPlug { socket_index, hash }) = action {
                        self.select_plug(
                            character_index,
                            slot,
                            socket_index,
                            &snapshot.socket_label,
                            &item.default_plugs,
                            hash,
                        );
                    }
                    if searchable {
                        self.plug_searches.insert(plug_search_key, plug_query);
                    } else {
                        self.plug_searches.remove(&plug_search_key);
                    }
                }
            });
    }
}
