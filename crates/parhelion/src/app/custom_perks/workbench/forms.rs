use super::*;
use sundial::package_authoring::sandbox_perk::activation::{PerkActivation, supports_activation};

impl Workbench {
    pub(super) fn draw_basics(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        recipe: &mut PerkRecipe,
    ) {
        ui.strong("Description");
        ui.add(
            egui::TextEdit::multiline(&mut recipe.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Describe what the perk does."),
        );
        ui.add_space(12.0);
        ui.strong("Stat Bonuses")
            .on_hover_text("Applies while the perk is equipped.");
        ui.small("These bonuses apply while equipped. Effect triggers do not control them.");
        let source_stats = catalog
            .map(|catalog| {
                catalog
                    .item_stat_contributions(recipe.template_plug.parse_u32().unwrap_or_default())
            })
            .unwrap_or_default();
        let mut stats = catalog
            .map(InvestmentCatalog::perk_stat_choices)
            .unwrap_or_default();
        stats.sort_by_cached_key(|stat| stat.name.to_lowercase());
        let mut remove = None;
        for stat in &mut recipe.stats {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    stats
                        .iter()
                        .find(|choice| choice.definition_index == stat.definition_index)
                        .map_or_else(
                            || format!("Stat {}", stat.definition_index),
                            |choice| choice.name.clone(),
                        ),
                );
                ui.add(egui::DragValue::new(&mut stat.value).speed(1.0));
                let original = source_stats
                    .iter()
                    .find(|source| source.definition_index == stat.definition_index);
                if ui
                    .button("Reset")
                    .on_hover_text("Restore the source contribution, or remove this added stat.")
                    .clicked()
                {
                    if let Some(original) = original {
                        stat.value = original.value;
                    } else {
                        remove = Some(stat.definition_index);
                    }
                }
                if ui.button("Remove").clicked() {
                    remove = Some(stat.definition_index);
                }
            });
        }
        if let Some(index) = remove {
            recipe.stats.retain(|stat| stat.definition_index != index);
        }
        ui.add_enabled_ui(recipe.stats.len() < 16, |ui| {
            if let Some(index) = pickers::popup(
                ui,
                "add-perk-stat",
                "Add Stat Bonus…",
                &mut self.stat_query,
                |ui, query, reset, height| {
                    let choices = stats
                        .iter()
                        .filter(|choice| {
                            pickers::matches(query, &choice.name)
                                && !recipe
                                    .stats
                                    .iter()
                                    .any(|stat| stat.definition_index == choice.definition_index)
                        })
                        .collect::<Vec<_>>();
                    pickers::results(
                        ui,
                        "perk-stat-results",
                        choices.len(),
                        height,
                        reset,
                        crate::app::style::list_row_height(ui),
                        |ui, index| {
                            crate::app::style::list_row(ui, false, &choices[index].name)
                                .clicked()
                                .then_some(choices[index].definition_index)
                        },
                    )
                },
            ) {
                recipe.stats.push(WeaponStatOverride {
                    definition_index: index,
                    value: 0,
                });
            }
        });
        ui.add_space(12.0);
        ui.strong("Icon and Category");
        if let Some(catalog) = catalog {
            let choices = self.templates.get_or_insert_with(|| {
                catalog.perk_template_choices_from(crate::package_profile::is_stock_item_definition)
            });
            for (label, query, category) in [
                ("Icon", &mut self.icon_query, false),
                ("Category", &mut self.category_query, true),
            ] {
                let current = if category {
                    recipe
                        .classification
                        .as_ref()
                        .unwrap_or(&recipe.template_plug)
                } else {
                    &recipe.template_plug
                };
                let hash = current.parse_u32().unwrap_or_default();
                ui.label(label);
                let source_name = catalog.plug_label(hash, false);
                let selected_label = if category {
                    choices
                        .iter()
                        .find(|choice| choice.representative_hash == hash)
                        .map_or_else(
                            || source_name.clone(),
                            |choice| {
                                format!("{} · {}", choice.representative_type_name, source_name)
                            },
                        )
                } else {
                    source_name
                };
                let picked = pickers::popup(
                    ui,
                    ("perk-presentation", category),
                    &selected_label,
                    query,
                    |ui, query, reset, height| {
                        let choices = choices
                            .iter()
                            .filter(|choice| {
                                pickers::matches(
                                    query,
                                    &format!(
                                        "{} {}",
                                        choice.representative_name, choice.representative_type_name
                                    ),
                                )
                            })
                            .collect::<Vec<_>>();
                        pickers::results(
                            ui,
                            ("perk-presentation-results", category),
                            choices.len(),
                            height,
                            reset,
                            sundial::investment::authoring_choice_row_height(ui),
                            |ui, index| {
                                let choice = choices[index];
                                catalog
                                    .draw_authoring_choice_row(
                                        ui,
                                        Some(choice.representative_hash),
                                        &choice.representative_name,
                                        Some(&choice.representative_type_name),
                                        hash == choice.representative_hash,
                                    )
                                    .clicked()
                                    .then_some(choice.representative_hash)
                            },
                        )
                    },
                );
                if let Some(picked) = picked {
                    if category {
                        recipe.classification = Some(picked.into());
                    } else {
                        recipe.template_plug = picked.into();
                    }
                }
                if category {
                    ui.small("Copies the source perk's category and type label. The socket role is set on the Weapon tab.");
                    if recipe.classification.is_some()
                        && ui.button("Restore Original Classification").clicked()
                    {
                        recipe.classification = None;
                    }
                }
            }
        }
    }

    pub(super) fn draw_effects(
        &mut self,
        ui: &mut egui::Ui,
        packages: &Path,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
        experimental: bool,
    ) {
        let ctx = ui.ctx().clone();
        if experimental && recipe.effects.is_empty() && ui.button("Create Effect").clicked() {
            if let Some(index) = self.free_metadata_index(recipe, choices) {
                recipe.effects.push(program::new_effect(index));
            }
        }
        if experimental && recipe.effects.is_empty() {
            ui.label(
                "Create an effect to choose its trigger and add projectile or emitter actions.",
            );
        }
        if let Some(warning) =
            sundial::package_authoring::sandbox_perk::sunrise_perk_projection_warning(
                recipe.effects.len(),
            )
        {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        let mut remove = None;
        let mut edit = None;
        let mut edit_asset = None;
        for (position, effect) in recipe.effects.iter_mut().enumerate() {
            if let Some(program) = &mut effect.program {
                if !experimental {
                    ui.strong(&program.name);
                    ui.label(format!(
                        "{} · {} Actions",
                        program.trigger.label(),
                        program.actions.len()
                    ));
                    ui.small("Enable Experimental Features in Preferences to edit this effect.");
                    continue;
                }
                ui.push_id(effect.source_perk_index, |ui| {
                    ui.group(|ui| {
                        ui.set_min_width(ui.available_width());
                        if let Some(action) = self.draw_program(ui, catalog, program) {
                            edit_asset = Some((
                                effect.source_perk_index,
                                action,
                                program.actions[action].asset().clone(),
                            ));
                        }
                        ui.menu_button("Effect Options", |ui| {
                            if ui.button("Remove Effect").clicked() {
                                remove = Some(effect.source_perk_index);
                                ui.close_menu();
                            }
                        });
                    });
                });
                continue;
            }
            let issue = self.discovery.perk_issue(effect.source_perk_index);
            ui.push_id(effect.source_perk_index, |ui| {
                ui.group(|ui| {
                    ui.set_min_width(ui.available_width());
                    let name = effect_name(choices, effect.source_perk_index);
                    ui.horizontal(|ui| {
                        ui.strong(format!("{}. {name}", position + 1));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.menu_button("More", |ui| {
                                if ui.button("Remove Effect").clicked() {
                                    remove = Some(effect.source_perk_index);
                                    ui.close_menu();
                                }
                            });
                            if ui
                                .add_enabled(issue.is_none(), egui::Button::new("Edit Behavior…"))
                                .clicked()
                            {
                                edit = Some(effect.clone());
                            }
                        });
                    });
                    if let Some(issue) = issue {
                        ui.colored_label(ui.visuals().warn_fg_color, issue);
                    }
                    if experimental && ui.button("Inspect Pattern Dependencies…").clicked() {
                        crate::app::runtime_dependencies::request(
                            ui.ctx(),
                            Some(usize::from(effect.source_perk_index)),
                        );
                    }
                    if editor::has_guided_profile(effect.source_perk_index) {
                        ui.small("Mapped projectile parameters available.");
                    }
                    let count = effect.runtime_values.len()
                        + effect.action_float_values.len()
                        + effect.projectiles.len();
                    if count > 0 {
                        ui.small(format!("{count} Changes"));
                    }
                    if effect.activation.is_some() && ui.button("Reset Activation").clicked() {
                        effect.activation = None;
                    }
                    if supports_activation(effect.source_perk_index) {
                        if experimental {
                            ui.horizontal(|ui| {
                                ui.label("Activation");
                                egui::ComboBox::from_id_salt("standalone-activation")
                                    .selected_text(
                                        effect
                                            .activation
                                            .map_or("Original Activation", PerkActivation::label),
                                    )
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut effect.activation,
                                            None,
                                            "Original Activation",
                                        );
                                        for activation in PerkActivation::ALL {
                                            ui.selectable_value(
                                                &mut effect.activation,
                                                Some(activation),
                                                activation.label(),
                                            );
                                        }
                                    });
                            });
                        } else {
                            ui.label(format!(
                                "Activation: {}",
                                effect.activation.map_or("Original", PerkActivation::label)
                            ))
                            .on_hover_text(
                                "Enable Experimental Features in Preferences to change activation.",
                            );
                        }
                    }
                });
            });
        }
        if let Some(index) = remove {
            recipe
                .effects
                .retain(|effect| effect.source_perk_index != index);
        }
        if let Some((effect, action, asset)) = edit_asset {
            let key = PerkEditorKey {
                socket_index: 0,
                choice_index: 0,
                source_plug_hash: recipe.template_plug.parse_u32().unwrap_or_default(),
                source_perk_index: effect,
            };
            let name = if asset.path.is_empty() {
                format!("Asset 0x{:08X}", asset.graph)
            } else {
                sundial::package_authoring::tft::asset_label(&asset.path)
            };
            self.editor = Some(PerkEditor::open_entity(
                packages.to_owned(),
                key,
                name,
                asset.graph,
                asset.values,
                &ctx,
            ));
            self.editing_effect = Some(effect);
            self.editing_program_action = Some(action);
        }
        if let Some(effect) = edit {
            let key = PerkEditorKey {
                socket_index: 0,
                choice_index: 0,
                source_plug_hash: recipe.template_plug.parse_u32().unwrap_or_default(),
                source_perk_index: effect.source_perk_index,
            };
            let mut editor = PerkEditor::open(
                packages.to_owned(),
                key,
                effect_name(choices, effect.source_perk_index),
                effect.runtime_values,
                effect.action_float_values,
                effect.projectiles,
                &ctx,
            );
            editor.projectile_labels = choices
                .iter()
                .map(|choice| (choice.perk_index, choice.representative_name.clone()))
                .collect();
            self.editing_effect = Some(effect.source_perk_index);
            self.editing_program_action = None;
            self.editor = Some(editor);
        }
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if experimental && !recipe.effects.is_empty() && ui.button("Add Effect").clicked() {
                if let Some(index) = self.free_metadata_index(recipe, choices) {
                    recipe.effects.push(program::new_effect(index));
                }
            }
            self.draw_existing_behavior_picker(ui, catalog, choices, recipe);
        });
        ui.small("New action combinations need an in-game check.");
    }

    fn free_metadata_index(
        &self,
        recipe: &PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
    ) -> Option<u16> {
        // This index supplies a finished-row layout only. The compiler emits an entirely new action.
        [421, 422, 1178, 338, 1778, 405]
            .into_iter()
            .chain(choices.iter().map(|choice| choice.perk_index))
            .find(|index| {
                !recipe
                    .effects
                    .iter()
                    .any(|effect| effect.source_perk_index == *index)
                    && self.discovery.perk_issue(*index).is_none()
            })
    }

    fn draw_existing_behavior_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
    ) {
        let picked = pickers::popup(
            ui,
            "existing-behavior",
            "Add Existing Behavior…",
            &mut self.effect_query,
            |ui, query, reset, height| {
                let mut available = choices
                    .iter()
                    .filter(|choice| {
                        let description = catalog
                            .and_then(|catalog| {
                                catalog.perk_description(choice.representative_hash)
                            })
                            .unwrap_or_default();
                        pickers::matches(
                            query,
                            &format!(
                                "{} {} {description}",
                                choice.representative_name, choice.perk_index
                            ),
                        ) && !recipe.effects.iter().any(|effect| {
                            effect.program.is_none()
                                && effect.source_perk_index == choice.perk_index
                        })
                    })
                    .collect::<Vec<_>>();
                available.sort_by_cached_key(|choice| choice.representative_name.to_lowercase());
                pickers::results(
                    ui,
                    "existing-behavior-results",
                    available.len(),
                    height,
                    reset,
                    sundial::investment::authoring_choice_row_height(ui),
                    |ui, index| {
                        let choice = available[index];
                        let issue = self.discovery.perk_issue(choice.perk_index);
                        ui.add_enabled_ui(issue.is_none(), |ui| {
                            let response = if let Some(catalog) = catalog {
                                catalog.draw_authoring_choice_row(
                                    ui,
                                    Some(choice.representative_hash),
                                    &effect_name(choices, choice.perk_index),
                                    catalog.perk_description(choice.representative_hash),
                                    false,
                                )
                            } else {
                                crate::app::style::list_row(ui, false, &choice.representative_name)
                            };
                            response
                                .on_disabled_hover_text(issue.unwrap_or_default())
                                .clicked()
                                .then_some(choice.perk_index)
                        })
                        .inner
                    },
                )
            },
        );
        if let Some(index) = picked {
            if let Some(position) = recipe
                .effects
                .iter()
                .position(|effect| effect.source_perk_index == index)
            {
                if let Some(replacement) = self.free_metadata_index(recipe, choices) {
                    recipe.effects[position].source_perk_index = replacement;
                } else {
                    return;
                }
            }
            recipe.effects.push(PerkRecipe::effect(index));
        }
    }

    pub(super) fn draw_effect_editor(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        recipe: &mut PerkRecipe,
        experimental: bool,
        height: f32,
    ) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        ui.horizontal(|ui| {
            ui.weak("Effect Builder");
            ui.weak("/");
            ui.add(egui::Label::new(egui::RichText::new(&editor.plug_label).strong()).truncate())
                .on_hover_text(&editor.plug_label);
        });
        let errors = editor.validation_errors();
        let valid = editor.graph.is_some() && errors.is_empty();
        let mut apply = false;
        let mut back = false;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(valid, egui::Button::new("Apply and Back"))
                .clicked()
            {
                apply = true;
            }
            if ui
                .add_enabled(!editor.is_loading(), egui::Button::new("Reset Effect"))
                .clicked()
            {
                let reload = !editor.projectile_draft.is_empty();
                editor.reset_all();
                if reload {
                    editor.start_load(ctx);
                }
            }
            if ui
                .button(if editor.has_changes() {
                    "Discard and Back"
                } else {
                    "Back"
                })
                .clicked()
            {
                back = true;
            }
        });
        if editor.graph.is_some()
            && let Some(error) = errors.first()
        {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("standalone-effect-editor")
            .max_height((height - 110.0).max(80.0))
            .show(ui, |ui| editor.draw_parameters(ui, ctx, experimental));
        if apply {
            if let Some(effect) = recipe
                .effects
                .iter_mut()
                .find(|effect| Some(effect.source_perk_index) == self.editing_effect)
            {
                if let Some(action) = self.editing_program_action {
                    if let Some(asset) = effect
                        .program
                        .as_mut()
                        .and_then(|program| program.actions.get_mut(action))
                        .map(|action| action.asset_mut())
                    {
                        asset.values = editor.draft.clone();
                    }
                } else {
                    effect.runtime_values = editor.draft.clone();
                    effect.action_float_values = editor.action_draft.clone();
                    effect.projectiles = editor.projectile_draft.clone();
                }
            }
            back = true;
        }
        if back {
            if let Some(document) = self.documents.get_mut(self.selected) {
                document.pending_effect = None;
            }
            self.retire_editor();
            self.editing_effect = None;
            self.editing_program_action = None;
            self.page = Page::Effects;
            self.persist_drafts();
        }
    }
}

pub(super) fn effect_name(choices: &[WeaponSandboxPerkChoice], index: u16) -> String {
    choices
        .iter()
        .find(|choice| choice.perk_index == index)
        .map_or_else(
            || format!("Effect {index}"),
            |choice| {
                if choices.iter().any(|other| {
                    other.perk_index != index
                        && other.representative_name == choice.representative_name
                }) {
                    format!("{} · Effect {index}", choice.representative_name)
                } else {
                    choice.representative_name.clone()
                }
            },
        )
}
