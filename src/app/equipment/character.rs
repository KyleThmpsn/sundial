use super::*;

impl SundialApp {
    pub(in crate::app) fn draw_character_fields(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editable: bool,
    ) {
        let settings_schema = game_settings::schema_version(&self.document);
        let metadata = self
            .account_workspace
            .character_metadata(&self.document, index)
            .ok();
        let fallback_character = self
            .characters()
            .and_then(|characters| characters.get(index))
            .cloned()
            .unwrap_or(Value::Null);
        let character = &fallback_character;
        if metadata.is_none() && character.is_null() {
            return;
        }
        let soid = self
            .account_workspace
            .character_soid(&self.document, index)
            .map_or_else(|| "Unknown".to_owned(), |soid| format!("0x{soid:016X}"));
        let mut race = metadata.map_or_else(
            || character.get("race").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.race),
        );
        let mut gender = metadata.map_or_else(
            || character.get("gender").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.gender),
        );
        let mut class_type = metadata.map_or_else(
            || character.get("class").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.class_type),
        );
        let mut movement = metadata.map_or_else(
            || {
                character
                    .get("movement_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(4)
            },
            |metadata| u64::from(metadata.abilities.movement),
        );
        let mut grenade = metadata.map_or_else(
            || {
                character
                    .get("grenade_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(7)
            },
            |metadata| u64::from(metadata.abilities.grenade),
        );
        let mut super_ability = metadata.map_or_else(
            || {
                character
                    .get("super_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
            },
            |metadata| u64::from(metadata.abilities.super_ability),
        );
        let mut melee = metadata.map_or_else(
            || {
                character
                    .get("melee_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(11)
            },
            |metadata| u64::from(metadata.abilities.melee),
        );
        let mut class_ability = metadata.map_or_else(
            || {
                character
                    .get("class_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(2)
            },
            |metadata| u64::from(metadata.abilities.class_ability),
        );
        let original_class_type = class_type;
        let mut current_subclass_hash = self
            .account_workspace
            .equipped_item_snapshots(&self.document, index)
            .ok()
            .and_then(|items| {
                items
                    .into_iter()
                    .find(|item| item.slot == "subclass")
                    .and_then(|item| item.definition_hash)
            });
        let mut abilities = current_subclass_hash
            .and_then(|hash| self.manifest.get_for_bucket(hash, 3_284_755_031))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let mut attunement_index = selected_attunement_index(&abilities, super_ability, melee);
        let all_subclasses: Vec<Arc<ItemDef>> = self
            .manifest
            .items
            .iter()
            .filter(|item| item.bucket_hash == 3_284_755_031)
            .cloned()
            .collect();
        let mut subclasses: Vec<Arc<ItemDef>> = all_subclasses
            .iter()
            .filter(|item| item.class_type == class_type)
            .cloned()
            .collect();
        let mut selected_subclass = None::<Arc<ItemDef>>;
        let stored_warning = self.document.uses_json_account().then(|| {
            self.source_warning
                .as_deref()
                .filter(|warning| {
                    warning.starts_with(&format!("Character {} ", index + 1))
                        && (warning.contains("ability") || warning.contains("super and melee"))
                })
                .map(str::to_owned)
        });
        let ability_warning = current_subclass_hash
            .and_then(|subclass_hash| {
                character_ability_issue_for_values(
                    subclass_hash,
                    Some(movement),
                    Some(grenade),
                    Some(super_ability),
                    Some(melee),
                    Some(class_ability),
                )
            })
            .or_else(|| {
                metadata
                    .is_none()
                    .then(|| character.as_object().and_then(character_ability_issue))
                    .flatten()
            })
            .or_else(|| stored_warning.flatten());

        ui.heading(format!("Character {}", index + 1));
        ui.label(egui::RichText::new(soid).monospace().weak());
        if let Some(warning) = ability_warning {
            ui.add_space(6.0);
            ui.colored_label(
                        ui.visuals().warn_fg_color,
                format!(
                    "Warning: {warning}. This can prevent Sunrise from loading the character. Choose supported abilities below and save before launching."
                ),
            );
        }
        ui.add_space(8.0);
        let (group_columns, group_widths) = character_field_group_layout(ui.available_width());
        let subclass_selector_width = (group_widths[1] - 98.0).clamp(140.0, 260.0);
        let ability_selector_width = (group_widths[2] - 138.0).clamp(140.0, 260.0);
        let mut previous_attunement = attunement_index;
        egui::Grid::new(("character_field_groups", index))
            .num_columns(group_columns)
            .spacing([18.0, 12.0])
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(group_widths[0]);
                    ui.strong("Identity");
                    ui.add_space(3.0);
                    egui::Grid::new(("character_identity_fields", index))
                        .num_columns(2)
                        .spacing([18.0, 8.0])
                        .show(ui, |ui| {
                ui.label("Class");
                combo_u64(
                    ui,
                    "class",
                    &mut class_type,
                    &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")],
                );
                if class_type != original_class_type {
                    subclasses = all_subclasses
                        .iter()
                        .filter(|item| item.class_type == class_type)
                        .cloned()
                        .collect();
                    if let Some(subclass) = subclasses
                        .iter()
                        .find(|item| item.name == default_subclass_name(class_type))
                        .cloned()
                        .or_else(|| subclasses.first().cloned())
                    {
                        current_subclass_hash = Some(subclass.hash);
                        abilities = subclass.abilities.clone();
                        (movement, grenade, super_ability, melee, class_ability) =
                            default_ability_values(class_type, &abilities, settings_schema);
                        attunement_index =
                            selected_attunement_index(&abilities, super_ability, melee);
                        selected_subclass = Some(subclass);
                    }
                }
                ui.end_row();
                ui.label("Race");
                combo_u64(
                    ui,
                    "race",
                    &mut race,
                    &[(0, "Human"), (1, "Awoken"), (2, "Exo")],
                );
                ui.end_row();
                ui.label("Gender");
                combo_u64(ui, "gender", &mut gender, &[(0, "Male"), (1, "Female")]);
                ui.end_row();
                        });
                });
                if group_columns == 1 {
                    ui.end_row();
                }

                ui.vertical(|ui| {
                    ui.set_width(group_widths[1]);
                    ui.strong("Subclass");
                    ui.add_space(3.0);
                    egui::Grid::new(("character_subclass_fields", index))
                        .num_columns(2)
                        .spacing([18.0, 5.0])
                        .show(ui, |ui| {
                ui.label("Subclass");
                let selected_name = current_subclass_hash
                    .and_then(|hash| subclasses.iter().find(|item| item.hash == hash))
                    .map_or("Unknown subclass", |item| item.name.as_str());
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected_name)
                    .width(subclass_selector_width)
                    .show_ui(ui, |ui| {
                        for subclass in &subclasses {
                            let selected = current_subclass_hash == Some(subclass.hash);
                            if ui.selectable_label(selected, &subclass.name).clicked() && !selected
                            {
                                current_subclass_hash = Some(subclass.hash);
                                abilities = subclass.abilities.clone();
                                (movement, grenade, super_ability, melee, class_ability) =
                                    default_ability_values(class_type, &abilities, settings_schema);
                                attunement_index =
                                    selected_attunement_index(&abilities, super_ability, melee);
                                selected_subclass = Some(subclass.clone());
                            }
                        }
                    });
                ui.end_row();

                ui.label("Attunement");
                previous_attunement = attunement_index;
                let selected_attunement = abilities
                    .attunements
                    .get(attunement_index)
                    .map_or("No attunement data", |attunement| attunement.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected_attunement)
                    .width(subclass_selector_width)
                    .show_ui(ui, |ui| {
                        for (choice_index, attunement) in abilities.attunements.iter().enumerate() {
                            ui.selectable_value(
                                &mut attunement_index,
                                choice_index,
                                &attunement.name,
                            );
                        }
                    });
                ui.end_row();

                if let Some(attunement) = abilities.attunements.get(attunement_index)
                    && !attunement.perks.is_empty()
                {
                    ui.label("");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for perk in &attunement.perks {
                            ui.label(&perk.name);
                        }
                    });
                    ui.end_row();
                }
                        });
                    if let Some(attunement) = abilities.attunements.get(attunement_index) {
                        let current_pair_is_valid = attunement.melee.entry == melee
                            && attunement
                                .super_abilities
                                .iter()
                                .any(|choice| choice.entry == super_ability);
                        if attunement_index != previous_attunement || !current_pair_is_valid {
                            melee = attunement.melee.entry;
                            super_ability = attunement
                                .super_abilities
                                .first()
                                .map_or(10, |choice| choice.entry);
                        }
                    }
                });
                if group_columns == 1 {
                    ui.end_row();
                }

                ui.vertical(|ui| {
                    ui.set_width(group_widths[2]);
                    ui.strong("Abilities");
                    ui.add_space(3.0);
                    egui::Grid::new(("character_ability_fields", index))
                        .num_columns(2)
                        .spacing([18.0, 8.0])
                        .show(ui, |ui| {
                for (label, id, value, choices) in [
                    (
                        "Movement ability",
                        "movement_ability",
                        &mut movement,
                        &abilities.movement,
                    ),
                    (
                        "Grenade ability",
                        "grenade_ability",
                        &mut grenade,
                        &abilities.grenade,
                    ),
                ] {
                    ui.label(label);
                    ability_combo(ui, id, value, choices, ability_selector_width);
                    ui.end_row();
                }
                if let Some(attunement) = abilities.attunements.get(attunement_index) {
                    ui.label("Super ability");
                    ui.label(
                        attunement
                            .super_abilities
                            .first()
                            .map_or("Unknown super", |choice| choice.name.as_str()),
                    );
                    ui.end_row();
                    ui.label("Melee ability");
                    ui.label(&attunement.melee.name);
                    ui.end_row();
                } else {
                    for (label, id, value, choices) in [
                        (
                            "Super ability",
                            "super_ability",
                            &mut super_ability,
                            &abilities.super_ability,
                        ),
                        (
                            "Melee ability",
                            "melee_ability",
                            &mut melee,
                            &abilities.melee,
                        ),
                    ] {
                        ui.label(label);
                        ability_combo(ui, id, value, choices, ability_selector_width);
                        ui.end_row();
                    }
                }
                ui.label("Class ability").on_hover_text(
                    "Dodge, Barricade, and Rift remain independent choices. Attunement perks may modify their behavior.",
                );
                ability_combo(
                    ui,
                    "class_ability",
                    &mut class_ability,
                    &abilities.class_ability,
                    ability_selector_width,
                );
                ui.end_row();
                        });
                });
                ui.end_row();
            });

        // A disabled egui scope still executes this function. Do not let its fallback display
        // values materialize missing fields in a read-only schema.
        if !editable {
            return;
        }

        let selecting_subclass = selected_subclass.is_some();
        let armor_template = (class_type != original_class_type)
            .then(|| self.class_armor_defaults.get(&class_type).cloned())
            .flatten();
        let mut candidate = self.document.clone();
        let mut metadata_updates = vec![
            sundial_account::CharacterMetadataUpdate::SetAppearanceAndClass {
                race: u8::try_from(race).expect("character race selectors contain u8 values"),
                gender: u8::try_from(gender).expect("character gender selectors contain u8 values"),
                class_type: u8::try_from(class_type)
                    .expect("character class selectors contain u8 values"),
            },
        ];
        if !selecting_subclass {
            metadata_updates.push(sundial_account::CharacterMetadataUpdate::SetAbilities(
                sundial_account::CharacterAbilities {
                    movement: u8::try_from(movement).expect("movement selectors contain u8 values"),
                    grenade: u8::try_from(grenade).expect("grenade selectors contain u8 values"),
                    super_ability: u8::try_from(super_ability)
                        .expect("super selectors contain u8 values"),
                    melee: u8::try_from(melee).expect("melee selectors contain u8 values"),
                    class_ability: u8::try_from(class_ability)
                        .expect("class ability selectors contain u8 values"),
                },
            ));
        }
        if let Err(error) =
            self.account_workspace
                .apply_character_updates(&mut candidate, index, metadata_updates)
        {
            self.set_status(error, true);
            return;
        }
        if let Some(source_character_index) = armor_template
            && let Err(error) = self.account_workspace.restore_class_armor(
                &mut candidate,
                source_character_index,
                index,
            )
        {
            self.set_status(error, true);
            return;
        }
        if let Some(subclass) = selected_subclass.as_ref()
            && let Err(error) = equip_subclass_with_default_abilities(
                self.account_workspace,
                &mut candidate,
                index,
                subclass,
            )
        {
            self.set_status(error, true);
            return;
        }
        if candidate != self.document {
            self.document = candidate;
            self.dirty = true;
            if let Some(subclass) = selected_subclass {
                self.set_status(format!("Equipped {}", subclass.name), false);
            }
        }
    }

    pub(in crate::app) fn draw_item_safety_controls(&mut self, ui: &mut egui::Ui) {
        self.draw_plug_safety_selector(ui, true);
    }

    pub(in crate::app) fn draw_plug_safety_controls(&mut self, ui: &mut egui::Ui) {
        self.draw_plug_safety_selector(ui, false);
    }

    fn draw_plug_safety_selector(&mut self, ui: &mut egui::Ui, show_dummy_items: bool) {
        let mut requested_plug_selection_mode = self.plug_selection_mode;
        ui.horizontal_wrapped(|ui| {
            ui.label("Plug safety:");
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::Supported,
                PlugSelectionMode::Supported.label(),
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::SocketAndGearType,
                PlugSelectionMode::SocketAndGearType.label(),
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::MatchingSocketType,
                PlugSelectionMode::MatchingSocketType.label(),
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::GearType,
                PlugSelectionMode::GearType.label(),
            );
            ui.radio_value(
                &mut requested_plug_selection_mode,
                PlugSelectionMode::AnyPlug,
                PlugSelectionMode::AnyPlug.label(),
            );
            if show_dummy_items {
                ui.separator();
                ui.checkbox(&mut self.show_dummy_items, "Show dummy items")
                    .on_hover_text(
                        "Includes display-only definitions that cannot normally be obtained in the game.",
                    );
            }
        });
        if requested_plug_selection_mode != self.plug_selection_mode {
            if requested_plug_selection_mode == PlugSelectionMode::AnyPlug
                && !self.really_unsafe_warning_acknowledged
            {
                self.remember_plug_selection_mode_after_confirmation = false;
                self.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
            } else {
                self.plug_selection_mode = requested_plug_selection_mode;
            }
        }
        if self.show_safety_warnings {
            super::draw_plug_selection_warning(ui, self.plug_selection_mode);
        }
    }
}
