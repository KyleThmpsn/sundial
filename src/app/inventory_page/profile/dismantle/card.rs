//! Item cards and definition pickers.
use super::*;

impl SundialApp {
    pub(super) fn draw_dismantle_reward_card(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DismantleRewardSnapshot,
        editable: bool,
        filtered: bool,
        combined_gear_class: bool,
    ) -> Option<DismantleRewardAction> {
        let resolved = self.resolve_inventory_definition(snapshot.definition_hash);
        let valid = resolved
            .as_ref()
            .is_some_and(|definition| definition.metadata.is_profile_items_candidate());
        let hash_hex_text = format_hash_hex(u64::from(snapshot.definition_hash));
        let key = format!("dismantle-rewards:edit:{}", snapshot.location.index);
        let mut definition_hash = snapshot.definition_hash;
        let mut quantity = snapshot.quantity;
        let mut rarities = snapshot.rarities.clone();
        let mut gear_class = snapshot.gear_class;
        let mut masterworked = snapshot.masterworked;
        let mut changed = false;
        let mut remove_requested = false;
        let mut swap_requested = false;
        let mut swap_response = None;

        ui.push_id(("dismantle-reward", snapshot.location.index), |ui| {
            item_editor::draw_item_card(ui, |ui| {
                let definition = DefinitionSummary::from_name_and_type(
                    &hash_hex_text,
                    resolved.as_ref().map(|definition| {
                        (definition.name.as_str(), definition.type_name.as_str())
                    }),
                );
                let inspection_context = DefinitionInspectionContext {
                    source: format!(
                        "Dismantle Reward Policy · Row {}",
                        snapshot.location.index + 1
                    ),
                    instance_id: None,
                    authored_level: None,
                    flags: None,
                    plug_count: None,
                    plugs: None,
                    quantity: Some(i64::from(snapshot.quantity)),
                };
                let header_response = item_editor::draw_catalog_item_header_with_trailing(
                    ui,
                    &self.manifest,
                    Some(u64::from(snapshot.definition_hash)),
                    Some(inspection_context.clone()),
                    ItemHeader {
                        label: None,
                        soid: None,
                        definition,
                        icon: None,
                        fill: item_editor::muted_item_header_fill(ui),
                        valid,
                        invalid_message: "not a profile-scoped material definition",
                    },
                    |_| {},
                );

                ui.add_enabled_ui(editable, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Quantity");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut quantity)
                                    .range(1..=i32::MAX)
                                    .speed(1),
                            )
                            .changed();

                        if filtered {
                            ui.label("Rarity");
                            egui::ComboBox::from_id_salt("rarity")
                                .selected_text(dismantle_rarity_summary(&rarities))
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(rarities.is_empty(), "Any").clicked()
                                        && !rarities.is_empty()
                                    {
                                        rarities.clear();
                                        changed = true;
                                    }
                                    ui.separator();
                                    for rarity in DismantleRarity::ALL {
                                        let selected = rarities.contains(&rarity);
                                        if ui
                                            .selectable_label(
                                                selected,
                                                dismantle_rarity_label(rarity),
                                            )
                                            .clicked()
                                        {
                                            if selected {
                                                rarities.retain(|value| *value != rarity);
                                            } else {
                                                rarities.push(rarity);
                                                rarities.sort_unstable();
                                            }
                                            changed = true;
                                        }
                                    }
                                });
                        }
                    });

                    if filtered {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Class");
                            egui::ComboBox::from_id_salt("class")
                                .selected_text(dismantle_class_label(gear_class))
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(&mut gear_class, None, "Any Gear")
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut gear_class,
                                            Some(DismantleGearClass::Weapon),
                                            "Weapon",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut gear_class,
                                            Some(DismantleGearClass::Armor),
                                            "Armor",
                                        )
                                        .changed();
                                    if combined_gear_class {
                                        changed |= ui
                                            .selectable_value(
                                                &mut gear_class,
                                                Some(DismantleGearClass::Both),
                                                "Weapon + Armor",
                                            )
                                            .changed();
                                    }
                                });

                            ui.label("Masterwork");
                            egui::ComboBox::from_id_salt("masterwork")
                                .selected_text(dismantle_masterwork_label(masterworked))
                                .show_ui(ui, |ui| {
                                    changed |= ui
                                        .selectable_value(&mut masterworked, None, "Any State")
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut masterworked,
                                            Some(true),
                                            "Masterworked",
                                        )
                                        .changed();
                                    changed |= ui
                                        .selectable_value(
                                            &mut masterworked,
                                            Some(false),
                                            "Not Masterworked",
                                        )
                                        .changed();
                                });
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if item_editor::draw_trash_button(ui, true, "Delete dismantle policy")
                                .on_hover_text("Delete this payout policy")
                                .clicked()
                            {
                                remove_requested = true;
                            }
                            let response = ui
                                .add(egui::Button::new("Swap").small())
                                .on_hover_text("Choose a different payout material");
                            swap_requested = response.clicked();
                            swap_response = Some(response);
                        });
                    });

                    let picker_anchor = header_response.clone()
                        | swap_response
                            .clone()
                            .unwrap_or_else(|| header_response.clone());
                    let manifest = &self.manifest;
                    let query = self.searches.entry(key.clone()).or_default();
                    if let Some(ItemEditorAction::SetDefinition { hash }) =
                        item_editor::draw_definition_picker_with_open_request(
                            ui,
                            manifest,
                            ("dismantle-reward-definition", snapshot.location.index),
                            query,
                            picker_height(),
                            (Some(&picker_anchor), swap_requested),
                            |query| DefinitionPickerChoices {
                                definitions: without_definition_groups(profile_definition_choices(
                                    manifest
                                        .profile_item_candidates(query)
                                        .filter(|definition| {
                                            u32::try_from(definition.hash).is_ok()
                                        }),
                                )),
                                existing_inventory: Vec::new(),
                                clear: None,
                                random_item_builder_hash: None,
                                empty_message: "No profile material definitions match".to_owned(),
                            },
                        )
                        && let Ok(hash) = u32::try_from(hash)
                    {
                        definition_hash = hash;
                        changed = true;
                    }
                });
            });
        });

        if remove_requested {
            Some(DismantleRewardAction::Remove)
        } else if changed {
            Some(DismantleRewardAction::SetPolicy {
                definition_hash,
                quantity,
                rarities,
                gear_class,
                masterworked,
            })
        } else {
            None
        }
    }
}
