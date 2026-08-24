use super::*;

impl SundialApp {
    pub(in crate::app) fn draw_character_fields(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editable: bool,
    ) {
        let settings_schema = game_settings::schema_version(&self.document);
        let Some(character) = self.characters().and_then(|chars| chars.get(index)) else {
            return;
        };
        let soid = character
            .get("soid")
            .and_then(parse_unsigned_value)
            .map_or_else(|| "Unknown".to_owned(), format_hash_hex);
        let mut race = character.get("race").and_then(Value::as_u64).unwrap_or(0);
        let mut gender = character.get("gender").and_then(Value::as_u64).unwrap_or(0);
        let mut class_type = character.get("class").and_then(Value::as_u64).unwrap_or(0);
        let mut movement = character
            .get("movement_ability")
            .and_then(Value::as_u64)
            .unwrap_or(4);
        let mut grenade = character
            .get("grenade_ability")
            .and_then(Value::as_u64)
            .unwrap_or(7);
        let mut super_ability = character
            .get("super_ability")
            .and_then(Value::as_u64)
            .unwrap_or(10);
        let mut melee = character
            .get("melee_ability")
            .and_then(Value::as_u64)
            .unwrap_or(11);
        let mut class_ability = character
            .get("class_ability")
            .and_then(Value::as_u64)
            .unwrap_or(2);
        let original_class_type = class_type;
        let mut current_subclass_hash = character
            .pointer("/equipment/subclass/definition_hash")
            .and_then(parse_unsigned_value);
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
        let stored_warning = self
            .source_warning
            .as_deref()
            .filter(|warning| {
                warning.starts_with(&format!("Character {} ", index + 1))
                    && (warning.contains("ability") || warning.contains("super and melee"))
            })
            .map(str::to_owned);
        let ability_warning = character
            .as_object()
            .and_then(character_ability_issue)
            .or(stored_warning);

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

        let mut changed = false;
        let selecting_subclass = selected_subclass.is_some();
        let armor_template = (class_type != original_class_type)
            .then(|| self.class_armor_defaults.get(&class_type).cloned())
            .flatten();
        {
            let Some(character) = self.characters_mut().and_then(|chars| chars.get_mut(index))
            else {
                return;
            };
            let Some(object) = character.as_object_mut() else {
                return;
            };
            for (key, new_value) in [("race", race), ("gender", gender), ("class", class_type)] {
                let old = object.get(key).and_then(Value::as_u64);
                if old != Some(new_value) {
                    object.insert(key.into(), Value::from(new_value));
                    changed = true;
                }
            }
            if !selecting_subclass {
                for (key, new_value) in [
                    ("movement_ability", movement),
                    ("grenade_ability", grenade),
                    ("super_ability", super_ability),
                    ("melee_ability", melee),
                    ("class_ability", class_ability),
                ] {
                    if object.get(key).and_then(Value::as_u64) != Some(new_value) {
                        object.insert(key.into(), Value::from(new_value));
                        changed = true;
                    }
                }
            }
            if let Some(template) = armor_template.as_ref() {
                changed |= restore_class_armor(object, template);
            }
        }
        self.dirty |= changed;
        if let Some(subclass) = selected_subclass {
            self.select_subclass_item(index, &subclass);
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
