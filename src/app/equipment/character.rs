use crate::app::account_workspace as account;

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CharacterEditorValues {
    race: u64,
    gender: u64,
    class_type: u64,
    movement: u64,
    grenade: u64,
    super_ability: u64,
    melee: u64,
    class_ability: u64,
}

impl CharacterEditorValues {
    fn metadata_updates(
        self,
        include_abilities: bool,
    ) -> Vec<sundial_account::CharacterMetadataUpdate> {
        let mut updates = vec![
            sundial_account::CharacterMetadataUpdate::SetAppearanceAndClass {
                race: u8::try_from(self.race).expect("character race selectors contain u8 values"),
                gender: u8::try_from(self.gender)
                    .expect("character gender selectors contain u8 values"),
                class_type: u8::try_from(self.class_type)
                    .expect("character class selectors contain u8 values"),
            },
        ];
        if include_abilities {
            updates.push(sundial_account::CharacterMetadataUpdate::SetAbilities(
                sundial_account::CharacterAbilities {
                    movement: u8::try_from(self.movement)
                        .expect("movement selectors contain u8 values"),
                    grenade: u8::try_from(self.grenade)
                        .expect("grenade selectors contain u8 values"),
                    super_ability: u8::try_from(self.super_ability)
                        .expect("super selectors contain u8 values"),
                    melee: u8::try_from(self.melee).expect("melee selectors contain u8 values"),
                    class_ability: u8::try_from(self.class_ability)
                        .expect("class ability selectors contain u8 values"),
                },
            ));
        }
        updates
    }
}

struct CharacterFieldUiContext<'a> {
    index: usize,
    settings_schema: Option<u64>,
    original_class_type: u64,
    abilities_editable: bool,
    all_subclasses: &'a [Arc<ItemDef>],
    allow_cross_class_subclasses: bool,
}

struct CharacterFieldUiState {
    values: CharacterEditorValues,
    current_subclass_hash: Option<u64>,
    abilities: crate::catalog::AbilityOptions,
    attunement_index: usize,
    subclasses: Vec<Arc<ItemDef>>,
    selected_subclass: Option<Arc<ItemDef>>,
}

impl CharacterFieldUiState {
    fn select_subclass(&mut self, subclass: Arc<ItemDef>, settings_schema: Option<u64>) {
        self.current_subclass_hash = Some(subclass.hash);
        self.abilities = subclass.abilities.clone();
        let (movement, grenade, super_ability, melee, class_ability) =
            default_ability_values(self.values.class_type, &self.abilities, settings_schema);
        self.values.movement = movement;
        self.values.grenade = grenade;
        self.values.super_ability = super_ability;
        self.values.melee = melee;
        self.values.class_ability = class_ability;
        self.attunement_index = selected_attunement_index(
            &self.abilities,
            self.values.super_ability,
            self.values.melee,
        );
        self.selected_subclass = Some(subclass);
    }
}

fn draw_character_field_groups(
    ui: &mut egui::Ui,
    context: CharacterFieldUiContext<'_>,
    state: &mut CharacterFieldUiState,
) {
    let (group_columns, group_widths) = character_field_group_layout(ui.available_width());
    let subclass_selector_width = (group_widths[1] - 98.0).clamp(140.0, 260.0);
    let ability_selector_width = (group_widths[2] - 138.0).clamp(140.0, 260.0);
    egui::Grid::new(("character_field_groups", context.index))
        .num_columns(group_columns)
        .spacing([18.0, 12.0])
        .show(ui, |ui| {
            draw_character_identity_group(ui, group_widths[0], &context, state);
            if group_columns == 1 {
                ui.end_row();
            }

            draw_character_subclass_group(
                ui,
                group_widths[1],
                subclass_selector_width,
                &context,
                state,
            );
            if group_columns == 1 {
                ui.end_row();
            }

            draw_character_ability_group(
                ui,
                group_widths[2],
                ability_selector_width,
                &context,
                state,
            );
            ui.end_row();
        });
}

fn draw_character_identity_group(
    ui: &mut egui::Ui,
    width: f32,
    context: &CharacterFieldUiContext<'_>,
    state: &mut CharacterFieldUiState,
) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.strong("Identity");
        ui.add_space(3.0);
        egui::Grid::new(("character_identity_fields", context.index))
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                ui.label("Class");
                combo_u64(
                    ui,
                    "class",
                    &mut state.values.class_type,
                    &[(0, "Titan"), (1, "Hunter"), (2, "Warlock")],
                );
                if state.values.class_type != context.original_class_type {
                    state.subclasses = context
                        .all_subclasses
                        .iter()
                        .filter(|item| {
                            item_class_is_compatible(
                                item,
                                state.values.class_type,
                                context.allow_cross_class_subclasses,
                            )
                        })
                        .cloned()
                        .collect();
                    if let Some(subclass) = state
                        .subclasses
                        .iter()
                        .find(|item| item.name == default_subclass_name(state.values.class_type))
                        .cloned()
                        .or_else(|| state.subclasses.first().cloned())
                    {
                        state.select_subclass(subclass, context.settings_schema);
                    }
                }
                ui.end_row();

                ui.label("Race");
                combo_u64(
                    ui,
                    "race",
                    &mut state.values.race,
                    &[(0, "Human"), (1, "Awoken"), (2, "Exo")],
                );
                ui.end_row();

                ui.label("Gender");
                combo_u64(
                    ui,
                    "gender",
                    &mut state.values.gender,
                    &[(0, "Male"), (1, "Female")],
                );
                ui.end_row();
            });
    });
}

fn draw_character_subclass_group(
    ui: &mut egui::Ui,
    width: f32,
    selector_width: f32,
    context: &CharacterFieldUiContext<'_>,
    state: &mut CharacterFieldUiState,
) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.strong("Subclass");
        ui.add_space(3.0);
        egui::Grid::new(("character_subclass_fields", context.index))
            .num_columns(2)
            .spacing([18.0, 5.0])
            .show(ui, |ui| {
                ui.label("Subclass");
                let selected_name = state
                    .current_subclass_hash
                    .and_then(|hash| state.subclasses.iter().find(|item| item.hash == hash))
                    .map_or_else(
                        || "Unknown subclass".to_owned(),
                        |item| subclass_display_name(item, context.allow_cross_class_subclasses),
                    );
                let mut requested_subclass = None;
                egui::ComboBox::from_id_salt("subclass")
                    .selected_text(selected_name)
                    .width(selector_width)
                    .show_ui(ui, |ui| {
                        for subclass in &state.subclasses {
                            let selected = state.current_subclass_hash == Some(subclass.hash);
                            let display_name = subclass_display_name(
                                subclass,
                                context.allow_cross_class_subclasses,
                            );
                            if ui.selectable_label(selected, display_name).clicked() && !selected {
                                requested_subclass = Some(subclass.clone());
                            }
                        }
                    });
                if let Some(subclass) = requested_subclass {
                    state.select_subclass(subclass, context.settings_schema);
                }
                ui.end_row();

                if !context.abilities_editable {
                    return;
                }
                ui.label("Attunement");
                let previous_attunement = state.attunement_index;
                let selected_attunement = state
                    .abilities
                    .attunements
                    .get(state.attunement_index)
                    .map_or("No attunement data", |attunement| attunement.name.as_str());
                egui::ComboBox::from_id_salt("attunement")
                    .selected_text(selected_attunement)
                    .width(selector_width)
                    .show_ui(ui, |ui| {
                        for (choice_index, attunement) in
                            state.abilities.attunements.iter().enumerate()
                        {
                            ui.selectable_value(
                                &mut state.attunement_index,
                                choice_index,
                                &attunement.name,
                            );
                        }
                    });
                ui.end_row();

                if let Some(attunement) = state.abilities.attunements.get(state.attunement_index)
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

                if let Some(attunement) = state.abilities.attunements.get(state.attunement_index) {
                    let current_pair_is_valid = attunement.melee.entry == state.values.melee
                        && attunement
                            .super_abilities
                            .iter()
                            .any(|choice| choice.entry == state.values.super_ability);
                    if state.attunement_index != previous_attunement || !current_pair_is_valid {
                        state.values.melee = attunement.melee.entry;
                        state.values.super_ability = attunement
                            .super_abilities
                            .first()
                            .map_or(10, |choice| choice.entry);
                    }
                }
            });
    });
}

fn draw_character_ability_group(
    ui: &mut egui::Ui,
    width: f32,
    selector_width: f32,
    context: &CharacterFieldUiContext<'_>,
    state: &mut CharacterFieldUiState,
) {
    if !context.abilities_editable {
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.strong("Abilities");
                crate::ui_help::info(ui, "Saved ability and attunement selections are not applied when the character loads with this settings format.");
            });
            ui.label("Choose abilities and attunement in game.");
        });
        return;
    }
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.strong("Abilities");
        ui.add_space(3.0);
        egui::Grid::new(("character_ability_fields", context.index))
            .num_columns(2)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                for (label, id, value, choices) in [
                    (
                        "Movement ability",
                        "movement_ability",
                        &mut state.values.movement,
                        &state.abilities.movement,
                    ),
                    (
                        "Grenade ability",
                        "grenade_ability",
                        &mut state.values.grenade,
                        &state.abilities.grenade,
                    ),
                ] {
                    ui.label(label);
                    ability_combo(ui, id, value, choices, selector_width);
                    ui.end_row();
                }
                if let Some(attunement) = state.abilities.attunements.get(state.attunement_index) {
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
                            &mut state.values.super_ability,
                            &state.abilities.super_ability,
                        ),
                        (
                            "Melee ability",
                            "melee_ability",
                            &mut state.values.melee,
                            &state.abilities.melee,
                        ),
                    ] {
                        ui.label(label);
                        ability_combo(ui, id, value, choices, selector_width);
                        ui.end_row();
                    }
                }
                ui.label("Class ability").on_hover_text(
                    "Dodge, Barricade, and Rift remain independent choices. Attunement perks may modify their behavior.",
                );
                ability_combo(
                    ui,
                    "class_ability",
                    &mut state.values.class_ability,
                    &state.abilities.class_ability,
                    selector_width,
                );
                ui.end_row();
            });
    });
}

impl SundialApp {
    pub(in crate::app) fn draw_character_fields(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        editable: bool,
    ) {
        let settings_schema = game_settings::schema_version(&self.document);
        let metadata = account::character_metadata(&self.document, index).ok();
        let fallback_character = self
            .characters()
            .and_then(|characters| characters.get(index));
        let empty_character = Value::Null;
        let character = fallback_character.unwrap_or(&empty_character);
        if metadata.is_none() && character.is_null() {
            return;
        }
        let soid = account::character_soid(&self.document, index)
            .map_or_else(|| "Unknown".to_owned(), |soid| format!("0x{soid:016X}"));
        let race = metadata.map_or_else(
            || character.get("race").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.race),
        );
        let gender = metadata.map_or_else(
            || character.get("gender").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.gender),
        );
        let class_type = metadata.map_or_else(
            || character.get("class").and_then(Value::as_u64).unwrap_or(0),
            |metadata| u64::from(metadata.class_type),
        );
        let movement = metadata.map_or_else(
            || {
                character
                    .get("movement_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(4)
            },
            |metadata| u64::from(metadata.abilities.movement),
        );
        let grenade = metadata.map_or_else(
            || {
                character
                    .get("grenade_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(7)
            },
            |metadata| u64::from(metadata.abilities.grenade),
        );
        let super_ability = metadata.map_or_else(
            || {
                character
                    .get("super_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
            },
            |metadata| u64::from(metadata.abilities.super_ability),
        );
        let melee = metadata.map_or_else(
            || {
                character
                    .get("melee_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(11)
            },
            |metadata| u64::from(metadata.abilities.melee),
        );
        let class_ability = metadata.map_or_else(
            || {
                character
                    .get("class_ability")
                    .and_then(Value::as_u64)
                    .unwrap_or(2)
            },
            |metadata| u64::from(metadata.abilities.class_ability),
        );
        let original_values = CharacterEditorValues {
            race,
            gender,
            class_type,
            movement,
            grenade,
            super_ability,
            melee,
            class_ability,
        };
        let abilities_editable = !self.document.supports_v13_account();
        let materialize_display_values = self.document.uses_json_account()
            && [
                ("race", original_values.race),
                ("gender", original_values.gender),
                ("class", original_values.class_type),
                ("movement_ability", original_values.movement),
                ("grenade_ability", original_values.grenade),
                ("super_ability", original_values.super_ability),
                ("melee_ability", original_values.melee),
                ("class_ability", original_values.class_ability),
            ]
            .into_iter()
            .filter(|(field, _)| {
                abilities_editable || matches!(*field, "race" | "gender" | "class")
            })
            .any(|(field, value)| character.get(field).and_then(Value::as_u64) != Some(value));
        let original_class_type = class_type;
        let current_subclass_hash = account::equipped_item_snapshots(&self.document, index)
            .ok()
            .and_then(|items| {
                items
                    .into_iter()
                    .find(|item| item.slot == "subclass")
                    .and_then(|item| item.definition_hash)
            });
        let abilities = current_subclass_hash
            .and_then(|hash| self.manifest.get_for_bucket(hash, 3_284_755_031))
            .map(|item| item.abilities.clone())
            .unwrap_or_default();
        let attunement_index = selected_attunement_index(&abilities, super_ability, melee);
        let all_subclasses: Vec<Arc<ItemDef>> = self
            .manifest
            .items
            .iter()
            .filter(|item| item.bucket_hash == 3_284_755_031)
            .cloned()
            .collect();
        let allow_cross_class_subclasses = self.preferences.experimental_cross_class_subclasses;
        let subclasses: Vec<Arc<ItemDef>> = all_subclasses
            .iter()
            .filter(|item| item_class_is_compatible(item, class_type, allow_cross_class_subclasses))
            .cloned()
            .collect();
        let selected_subclass = None::<Arc<ItemDef>>;
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
        if abilities_editable && let Some(warning) = ability_warning {
            ui.add_space(6.0);
            ui.colored_label(
                        ui.visuals().warn_fg_color,
                format!(
                    "Warning: {warning}. This can prevent Sunrise from loading the character. Choose supported abilities below and save before launching."
                ),
            );
        }
        let mut field_state = CharacterFieldUiState {
            values: CharacterEditorValues {
                race,
                gender,
                class_type,
                movement,
                grenade,
                super_ability,
                melee,
                class_ability,
            },
            current_subclass_hash,
            abilities,
            attunement_index,
            subclasses,
            selected_subclass,
        };
        ui.add_space(8.0);
        draw_character_field_groups(
            ui,
            CharacterFieldUiContext {
                index,
                settings_schema,
                original_class_type,
                abilities_editable,
                all_subclasses: &all_subclasses,
                allow_cross_class_subclasses,
            },
            &mut field_state,
        );

        ui.add_enabled_ui(editable, |ui| self.draw_character_runtime(ui, index));

        // A disabled egui scope still executes this function. Do not let its fallback display
        // values materialize missing fields in a read-only schema.
        if !editable {
            return;
        }

        let edited_values = field_state.values;
        self.apply_character_editor_changes(
            index,
            original_values,
            edited_values,
            field_state.selected_subclass,
            allow_cross_class_subclasses,
            materialize_display_values,
        );
    }

    fn apply_character_editor_changes(
        &mut self,
        index: usize,
        original: CharacterEditorValues,
        edited: CharacterEditorValues,
        selected_subclass: Option<Arc<ItemDef>>,
        allow_cross_class_subclasses: bool,
        materialize_display_values: bool,
    ) {
        let selecting_subclass = selected_subclass.is_some();
        if edited == original && !selecting_subclass && !materialize_display_values {
            return;
        }
        let armor_template = (edited.class_type != original.class_type)
            .then(|| self.class_armor_defaults.get(&edited.class_type).copied())
            .flatten();
        let mut candidate = self.document.clone();
        let metadata_updates =
            edited.metadata_updates(!selecting_subclass && !candidate.supports_v13_account());
        if let Err(error) =
            account::apply_character_updates(&mut candidate, index, metadata_updates)
        {
            self.set_status(error, true);
            return;
        }
        if let Some(source_character_index) = armor_template
            && let Err(error) =
                account::restore_class_armor(&mut candidate, source_character_index, index)
        {
            self.set_status(error, true);
            return;
        }
        if let Some(subclass) = selected_subclass.as_ref()
            && let Err(error) = equip_subclass_with_default_abilities(
                &mut candidate,
                index,
                subclass,
                allow_cross_class_subclasses,
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
            ui.label("Plug Safety:");
            for candidate in PlugSelectionMode::ALL {
                ui.radio_value(
                    &mut requested_plug_selection_mode,
                    candidate,
                    candidate.label(),
                );
            }
            if show_dummy_items {
                ui.separator();
                ui.checkbox(&mut self.show_dummy_items, "Show Dummy Items")
                    .on_hover_text(
                        "Includes display-only definitions that cannot normally be obtained in the game.",
                    );
            }
        });
        if requested_plug_selection_mode != self.plug_selection_mode {
            if requested_plug_selection_mode == PlugSelectionMode::AnyPlug
                && !self.preferences.really_unsafe_warning_acknowledged
            {
                self.remember_plug_selection_mode_after_confirmation = false;
                self.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
            } else {
                self.plug_selection_mode = requested_plug_selection_mode;
            }
        }
        if self.preferences.show_safety_warnings {
            super::draw_plug_selection_warning(ui, self.plug_selection_mode);
        }
    }
}
