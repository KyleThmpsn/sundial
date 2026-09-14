use super::*;
use sundial::package_authoring::sandbox_perk::activation::{PerkActivation, supports_activation};

impl Workbench {
    pub(super) fn stock_effect_name(
        &self,
        choices: &[WeaponSandboxPerkChoice],
        index: u16,
    ) -> String {
        self.ingredients
            .as_ref()
            .and_then(|(_, _, data)| data.references.shared_label(usize::from(index)))
            .unwrap_or_else(|| effect_name(choices, index))
    }

    pub(super) fn draw_basics(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        recipe: &mut PerkRecipe,
    ) {
        let open = self.page == Page::Basics;
        ui.horizontal_wrapped(|ui| {
            crate::app::style::compact_controls(ui);
            if let Some(catalog) = catalog {
                let choices = self.templates.get_or_insert_with(|| {
                    catalog.perk_template_choices_from(
                        crate::package_profile::is_stock_item_definition,
                    )
                });
                let hash = recipe
                    .classification
                    .as_ref()
                    .unwrap_or(&recipe.template_plug)
                    .parse_u32()
                    .unwrap_or_default();
                let type_name = catalog
                    .item_type_name(hash)
                    .unwrap_or_else(|| "Unknown Type".into());
                ui.label("Type")
                    .on_hover_text("The perk's type label. Set its socket on the Weapon tab.");
                // One choice per native type, retaining the current source when unchanged.
                let mut types = BTreeMap::new();
                for choice in choices.iter() {
                    if !choice.representative_type_name.trim().is_empty() {
                        types
                            .entry(choice.representative_type_name.as_str())
                            .or_insert(choice.representative_hash);
                    }
                }
                egui::ComboBox::from_id_salt("perk-type")
                    .selected_text(&type_name)
                    .show_ui(ui, |ui| {
                        crate::app::style::workbench_style(ui);
                        for (name, source) in types {
                            if ui.selectable_label(name == type_name, name).clicked()
                                && name != type_name
                            {
                                recipe.classification = Some(source.into());
                            }
                        }
                    });
            }
            let response = egui::CollapsingHeader::new("Description")
                .id_salt("perk-description")
                .open(Some(open))
                .show(ui, |_| {});
            if response.header_response.clicked() {
                self.page = if open { Page::Effects } else { Page::Basics };
            }
            if let Some(catalog) = catalog {
                let choices = self
                    .templates
                    .as_ref()
                    .expect("Presentation choices loaded above");
                let hash = recipe.template_plug.parse_u32().unwrap_or_default();
                if let Some(picked) = pickers::popup(
                    ui,
                    "perk-icon",
                    "Change Icon…",
                    &mut self.icon_query,
                    |ui, query, reset, height| {
                        crate::app::style::workbench_style(ui);
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
                            "perk-icon-results",
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
                ) {
                    // Changing the icon must not silently change the selected type.
                    if recipe.classification.is_none() {
                        recipe.classification = Some(recipe.template_plug.clone());
                    }
                    recipe.template_plug = picked.into();
                }
            }
        });
        if self.page == Page::Basics {
            if let Some(summary) = self.description_from_effects(recipe, catalog) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Use Effect Summary")
                        .on_hover_text(
                            "Replace the description with a summary of the current effects.",
                        )
                        .clicked()
                    {
                        recipe.description = summary;
                    }
                });
            }
            ui.add(
                egui::TextEdit::multiline(&mut recipe.description)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("Describe what this perk does. Leave blank for no description."),
            );
        }
    }

    fn description_from_effects(
        &self,
        recipe: &PerkRecipe,
        catalog: Option<&InvestmentCatalog>,
    ) -> Option<String> {
        if recipe.effects.is_empty() {
            return None;
        }
        recipe
            .effects
            .iter()
            .map(|effect| {
                if let Some(program) = &effect.program {
                    if program.native.is_some()
                        || program.actions.is_empty()
                        || program.actions.iter().any(|action| {
                            action.asset().is_some_and(|asset| {
                                asset.graph == 0 || !self.asset_labels.contains_key(&asset.graph)
                            })
                        })
                    {
                        return None;
                    }
                    Some(guidance::summary_with_assets(
                        program,
                        Some(&self.keys.catalog),
                        Some(&self.asset_labels),
                    ))
                } else if effect.runtime_values.is_empty()
                    && effect.action_float_values.is_empty()
                    && effect.projectiles.is_empty()
                    && effect.activation.is_none()
                {
                    catalog
                        .and_then(|catalog| {
                            catalog.perk_component_description(effect.source_perk_index)
                        })
                        .filter(|text| !text.trim().is_empty())
                        .map(str::to_owned)
                } else {
                    None
                }
            })
            .collect::<Option<Vec<_>>>()
            .map(|lines| lines.join("\n\n"))
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
        if recipe.effects.is_empty() {
            self.draw_starting_points(ui, choices);
        }
        if self.perk_names.len() != choices.len() {
            self.perk_names = choices
                .iter()
                .map(|choice| (choice.perk_index, choice.representative_name.clone()))
                .collect();
            self.asset_label_source = None;
        }
        self.refresh_item_names(catalog);
        self.refresh_asset_labels();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Effects");
            sundial::investment::draw_authoring_info_icon(
                ui,
                "Each effect is one program: what starts it, what it does and when it ends. Stock effects come from installed perks. Custom effects are compiled from scratch.",
            );
            if experimental && ui.button("Add Effect").clicked() {
                if let Some(index) = self.free_metadata_index(recipe, choices) {
                    recipe.effects.push(program::new_effect(index));
                }
            }
            self.draw_existing_behavior_picker(ui, catalog, choices, recipe);
            crate::app::style::more_menu(ui, |ui| {
                if experimental && ui.button("Add Complete Program").clicked() {
                    if let Some(index) = self.free_metadata_index(recipe, choices) {
                        let mut effect = program::new_effect(index);
                        effect.program.as_mut().expect("new program").native = Some(
                            sundial::package_authoring::sandbox_perk::program::NativeProgram::empty(),
                        );
                        recipe.effects.push(effect);
                    }
                    ui.close_menu();
                }
                if ui.button("Engine Catalog…").clicked() {
                    self.engine.open = true;
                    ui.close_menu();
                }
            });
        });
        if let Some(warning) =
            sundial::package_authoring::sandbox_perk::sunrise_perk_projection_warning(
                recipe.effects.len(),
            )
        {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        let mut events = EffectEvents::default();
        for (position, effect) in recipe.effects.iter_mut().enumerate() {
            let index = effect.source_perk_index;
            if effect.program.is_some() {
                self.draw_program_effect(ui, catalog, effect, experimental, &mut events);
            } else {
                let name = format!(
                    "{}. {}",
                    position + 1,
                    self.stock_effect_name(choices, index)
                );
                let description = catalog
                    .and_then(|catalog| catalog.perk_component_description(index))
                    .filter(|description| !description.is_empty())
                    .filter(|_| {
                        effect.runtime_values.is_empty()
                            && effect.action_float_values.is_empty()
                            && effect.projectiles.is_empty()
                            && effect.activation.is_none()
                    });
                self.draw_stock_effect(ui, &name, description, effect, experimental, &mut events);
            }
        }
        if let Some(index) = events.remove {
            recipe
                .effects
                .retain(|effect| effect.source_perk_index != index);
        }
        if let Some((effect, action, asset)) = events.edit_asset {
            self.open_asset_editor(packages, &ctx, recipe, effect, action, asset);
        }
        if let Some(effect) = events.edit.and_then(|index| {
            recipe
                .effects
                .iter()
                .find(|effect| effect.source_perk_index == index)
                .cloned()
        }) {
            self.open_behavior_editor(packages, &ctx, recipe, choices, effect);
        }
        self.draw_stats(ui, catalog, recipe);
    }

    /// An authored program on the shared canvas. The canvas draws it locked when program
    /// editing is off.
    fn draw_program_effect(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        effect: &mut WeaponSandboxPerkRuntimeRecipe,
        experimental: bool,
        events: &mut EffectEvents,
    ) {
        let index = effect.source_perk_index;
        let Some(program) = &mut effect.program else {
            return;
        };
        let mut remove = false;
        let mut header = |ui: &mut egui::Ui| {
            if experimental {
                crate::app::style::more_menu(ui, |ui| {
                    crate::app::style::workbench_style(ui);
                    if ui.button("Remove Effect").clicked() {
                        remove = true;
                        ui.close_menu();
                    }
                });
            }
        };
        let output = ui
            .push_id(index, |ui| {
                canvas::draw(
                    ui,
                    canvas::Canvas {
                        name: "",
                        backend: canvas::Backend::Program {
                            program,
                            editing: experimental.then_some(canvas::Editing {
                                workbench: self,
                                catalog,
                            }),
                        },
                        header: Some(&mut header),
                        place: None,
                        footer: None,
                    },
                )
            })
            .inner;
        if remove {
            events.remove = Some(index);
        }
        if let Some(action) = output.edit_action
            && let Some(asset) = program.asset(action)
        {
            events.edit_asset = Some((index, action, asset.clone()));
        }
    }

    /// A stock effect card: the cached digest of its action when the dependency index has
    /// one, otherwise just the name, with the edit and activation controls around it.
    fn draw_stock_effect(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        description: Option<&str>,
        effect: &mut WeaponSandboxPerkRuntimeRecipe,
        experimental: bool,
        events: &mut EffectEvents,
    ) {
        let index = effect.source_perk_index;
        let issue = self.discovery.perk_issue(index).map(str::to_owned);
        let mut remove = false;
        let mut edit = false;
        let mut header = |ui: &mut egui::Ui| {
            crate::app::style::more_menu(ui, |ui| {
                crate::app::style::workbench_style(ui);
                if ui.button("Remove Effect").clicked() {
                    remove = true;
                    ui.close_menu();
                }
                if experimental && ui.button("Inspect Pattern Dependencies…").clicked() {
                    crate::app::runtime_dependencies::request(ui.ctx(), Some(usize::from(index)));
                    ui.close_menu();
                }
            });
            if ui
                .add_enabled(issue.is_none(), egui::Button::new("Edit Behavior…"))
                .on_hover_text(if editor::has_guided_profile(index) {
                    "Change this effect's values, projectiles and activation."
                } else {
                    "Change this effect's values and activation."
                })
                .clicked()
            {
                edit = true;
            }
        };
        let mut footer = |ui: &mut egui::Ui| {
            if let Some(issue) = &issue {
                ui.colored_label(ui.visuals().warn_fg_color, issue);
            }
            let count = effect.runtime_values.len()
                + effect.action_float_values.len()
                + effect.projectiles.len();
            if count > 0 {
                ui.small(format!("{count} Edits"))
                    .on_hover_text("Parameter, action and projectile edits this effect carries.");
            }
            if effect.activation.is_some() && ui.button("Reset Activation").clicked() {
                effect.activation = None;
            }
            if supports_activation(index) {
                draw_activation_choice(ui, &mut effect.activation, experimental);
            }
        };
        // The cached digest keeps the card informative before the editor loads the action.
        let digest = self.discovery.behavior(index);
        let reading = digest.is_none() && self.discovery.busy();
        ui.push_id(index, |ui| match digest {
            Some(behavior) => {
                canvas::draw(
                    ui,
                    canvas::Canvas {
                        name,
                        backend: canvas::Backend::Digest {
                            behavior,
                            description,
                        },
                        header: Some(&mut header),
                        place: None,
                        footer: Some(&mut footer),
                    },
                );
            }
            None => {
                crate::app::style::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            header(ui);
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(name).strong()).wrap(),
                                    );
                                },
                            );
                        });
                    });
                    if reading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.small("Reading…");
                        });
                    }
                    footer(ui);
                });
            }
        });
        if remove {
            events.remove = Some(index);
        }
        if edit {
            events.edit = Some(index);
        }
    }

    /// Opens the property editor on one asset of an authored program action.
    fn open_asset_editor(
        &mut self,
        packages: &Path,
        ctx: &egui::Context,
        recipe: &PerkRecipe,
        effect: u16,
        action: usize,
        asset: sundial::package_authoring::sandbox_perk::program::Asset,
    ) {
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
            ctx,
        ));
        self.editing_effect = Some(effect);
        self.editing_program_action = Some(action);
    }

    /// Opens the behavior editor on a stock effect, carrying its current overrides.
    fn open_behavior_editor(
        &mut self,
        packages: &Path,
        ctx: &egui::Context,
        recipe: &PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
        effect: WeaponSandboxPerkRuntimeRecipe,
    ) {
        let key = PerkEditorKey {
            socket_index: 0,
            choice_index: 0,
            source_plug_hash: recipe.template_plug.parse_u32().unwrap_or_default(),
            source_perk_index: effect.source_perk_index,
        };
        let mut editor = PerkEditor::open(
            packages.to_owned(),
            key,
            self.stock_effect_name(choices, effect.source_perk_index),
            effect.runtime_values,
            effect.action_float_values,
            effect.projectiles,
            ctx,
        );
        editor.activation = effect.activation;
        editor.projectile_labels = choices
            .iter()
            .map(|choice| (choice.perk_index, choice.representative_name.clone()))
            .collect();
        editor.item_names = self.item_names.clone();
        self.editing_effect = Some(effect.source_perk_index);
        self.editing_program_action = None;
        self.editor = Some(editor);
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
        self.refresh_asset_labels();
        let names = choices
            .iter()
            .map(|choice| {
                (
                    choice.perk_index,
                    self.stock_effect_name(choices, choice.perk_index),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let picked = pickers::browser_with_toolbar(
            ui,
            "existing-behavior",
            "Use Existing Perk…",
            "Browse Perk Effects",
            &mut self.effect_query,
            |ui, query, reset, _height| {
                let mut filter_changed = reset;
                let mut available = Vec::new();
                ui.horizontal(|ui| {
                    let filter_width = (ui.available_width() * 0.12).clamp(80.0, 132.0);
                    let search_width =
                        (ui.available_width() - filter_width * 3.0 - 285.0).max(100.0);
                    filter_changed |= pickers::search(ui, query, reset, search_width);
                    filter_changed |= guidance::filters(
                        ui,
                        &mut self.effect_purpose,
                        &mut self.effect_editing,
                        filter_width,
                    );
                    let before = self.ingredient_source;
                    egui::ComboBox::from_id_salt("ingredient-source")
                        .width(filter_width)
                        .truncate()
                        .selected_text(before.map_or("All Sources", |source| source.label()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.ingredient_source, None, "All Sources");
                            for source in sundial::investment::IngredientSource::ALL {
                                ui.selectable_value(
                                    &mut self.ingredient_source,
                                    Some(source),
                                    source.label(),
                                );
                            }
                        });
                    filter_changed |= before != self.ingredient_source;
                    let (show_all, visibility_changed) = pickers::show_all(ui);
                    filter_changed |= visibility_changed;
                    let query = query.trim().to_lowercase();
                    available = choices
                        .iter()
                        .filter(|choice| {
                            let description = catalog
                                .and_then(|catalog| {
                                    catalog.perk_component_description(choice.perk_index)
                                })
                                .unwrap_or_default();
                            let behavior = self.discovery.behavior(choice.perk_index);
                            let summary =
                                behavior.map(guidance::behavior_search).unwrap_or_default();
                            let sources = self
                                .ingredients
                                .as_ref()
                                .and_then(|(_, _, data)| data.sources.get(&choice.perk_index));
                            let source_text = sources
                                .map(|sources| {
                                    sources
                                        .iter()
                                        .map(|source| source.label())
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                })
                                .unwrap_or_default();
                            let context = self
                                .ingredients
                                .as_ref()
                                .and_then(|(_, _, data)| data.context.get(&choice.perk_index))
                                .map(|context| {
                                    context.iter().cloned().collect::<Vec<_>>().join(" ")
                                })
                                .unwrap_or_default();
                            (show_all
                                || (behavior.is_some_and(|behavior| {
                                    behavior.editable && !behavior.effect_kinds.is_empty()
                                }) && !choice.representative_name.starts_with("Ability ")
                                    && !choice.representative_name.starts_with("Effect ")))
                                && self.ingredient_source.is_none_or(|source| {
                                    sources.is_some_and(|sources| sources.contains(&source))
                                })
                                && self.effect_purpose.allows(behavior)
                                && self.effect_editing.allows(behavior)
                                && pickers::matches(
                                    &query,
                                    &format!(
                                        "{} {} {} {description} {summary} {source_text} {context}",
                                        names[&choice.perk_index].clone(),
                                        choice.perk_index,
                                        self.ingredients
                                            .as_ref()
                                            .map(|(_, _, data)| data
                                                .references
                                                .details(usize::from(choice.perk_index)))
                                            .unwrap_or_default()
                                    ),
                                )
                        })
                        .collect::<Vec<_>>();
                    ui.label(if available.len() == 1 {
                        "1 Result".to_owned()
                    } else {
                        format!("{} Results", available.len())
                    });
                    if self.discovery.busy() {
                        ui.spinner();
                    }
                });
                ui.separator();
                let height = (ui.available_height() - 4.0).max(110.0);
                available
                    .sort_by_cached_key(|choice| names[&choice.perk_index].clone().to_lowercase());
                let keys = available
                    .iter()
                    .map(|choice| u64::from(choice.perk_index))
                    .collect::<Vec<_>>();
                pickers::BrowserList { keys: &keys, height, reset: reset || filter_changed,
                    row_height: sundial::investment::authoring_choice_row_height(ui) }.draw_body(ui,
                    |ui, index, selected| {
                        let choice = available[index];
                        let name = names[&choice.perk_index].clone();
                        let technical = self.discovery.data.as_ref()
                            .and_then(|data| data.perks.perks.get(usize::from(choice.perk_index)))
                            .map(reading::identity).unwrap_or_else(|| format!("Effect {}", choice.perk_index));
                        if let Some(catalog) = catalog {
                            catalog.draw_authoring_choice_row(ui, Some(choice.representative_hash), &name, Some(&technical), selected)
                        } else { sundial::investment::draw_asset_choice_row(ui, &name, &technical, selected) }
                    },
                    |ui, index| {
                        let choice = available[index];
                        ui.heading(names[&choice.perk_index].clone());
                        let issue = self.discovery.perk_issue(choice.perk_index);
                        let included = recipe.effects.iter().any(|effect| effect.program.is_none() && effect.source_perk_index == choice.perk_index);
                        if ui.add_enabled(issue.is_none() && !included, crate::app::style::primary(ui, if included { "Already Added" } else { "Add Behavior" })).clicked() { return Some(choice.perk_index); }
                        if let Some(issue) = issue { ui.colored_label(ui.visuals().error_fg_color, issue); }
                        if let Some(description) = catalog.and_then(|catalog| catalog.perk_component_description(choice.perk_index)).filter(|description| !description.is_empty()) {
                            ui.separator();
                            ui.label(description);
                        }
                        if let Some(behavior) = self.discovery.behavior(choice.perk_index) {
                            ui.separator();
                            reading::overview(ui, behavior, &self.asset_labels);
                            egui::CollapsingHeader::new("Technical Details").default_open(true).show(ui, |ui| {
                                reading::draw(ui, behavior, self.discovery.data.as_ref(), choice.perk_index, &self.asset_labels);
                            });
                        }
                        if let Some((_, _, ingredients)) = &self.ingredients {
                            ui.horizontal_wrapped(|ui| {
                                if let Some(sources) = ingredients.sources.get(&choice.perk_index) {
                                    ui.label(sources.iter().map(|source| source.label()).collect::<Vec<_>>().join(" and "));
                                }
                                if ingredients.context.get(&choice.perk_index).is_some_and(|context| !context.is_empty()) {
                                    sundial::investment::draw_authoring_info_icon(ui,
                                        "This ability component may require the selected subclass or ability state.");
                                }
                            });
                        }
                        None
                    })
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
        let editor_top = ui.cursor().top();
        ui.horizontal(|ui| {
            ui.weak("Effect");
            ui.weak("/");
            ui.add(egui::Label::new(egui::RichText::new(&editor.plug_label).strong()).truncate());
        });
        let errors = editor.validation_errors();
        let valid = editor.graph.is_some() && !editor.is_loading() && errors.is_empty();
        let mut apply = false;
        let mut back = false;
        ui.horizontal_wrapped(|ui| {
            let apply_button = crate::app::style::primary(ui, "Apply and Back");
            if ui.add_enabled(valid, apply_button).clicked() {
                apply = true;
            }
            if ui
                .add_enabled(!editor.is_loading(), egui::Button::new("Reset Effect"))
                .on_hover_text("Clear every edit to this effect and go back to its stock values.")
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
        let remaining_height = (height - (ui.cursor().top() - editor_top) - 4.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt((
                "standalone-effect-editor",
                &recipe.id,
                self.editing_effect,
                self.editing_program_action,
            ))
            .max_height(remaining_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                editor.draw_parameters(ui, ctx, experimental);
                ui.add_space(8.0);
            });
        if let Some(program) = editor.take_conversion() {
            // The program replaces every stock override. The build rejects the two together.
            if let Some(effect) = recipe
                .effects
                .iter_mut()
                .find(|effect| Some(effect.source_perk_index) == self.editing_effect)
            {
                effect.program = Some(program);
                effect.runtime_values.clear();
                effect.action_float_values.clear();
                effect.projectiles.clear();
                effect.activation = None;
            }
            back = true;
        }
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
                        .and_then(|program| program.asset_mut(action))
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

/// What the user asked for across the effect cards this frame. At most one card acts per
/// frame, so the last click wins in the rare case of two.
#[derive(Default)]
struct EffectEvents {
    /// Remove the effect with this finished perk index.
    remove: Option<u16>,
    /// Open the behavior editor on this stock effect.
    edit: Option<u16>,
    /// Open the asset editor on one action of an authored program.
    edit_asset: Option<(
        u16,
        usize,
        sundial::package_authoring::sandbox_perk::program::Asset,
    )>,
}

/// The activation selector of a stock effect, or its locked reading when program editing
/// is off.
fn draw_activation_choice(
    ui: &mut egui::Ui,
    activation: &mut Option<PerkActivation>,
    experimental: bool,
) {
    if experimental {
        ui.horizontal(|ui| {
            ui.label("Activation");
            egui::ComboBox::from_id_salt("standalone-activation")
                .selected_text(activation.map_or("Original Activation", PerkActivation::label))
                .show_ui(ui, |ui| {
                    ui.selectable_value(activation, None, "Original Activation");
                    for choice in PerkActivation::ALL {
                        ui.selectable_value(activation, Some(choice), choice.label());
                    }
                });
        });
    } else {
        ui.label(format!(
            "Activation: {}",
            activation.map_or("Original", PerkActivation::label)
        ))
        .on_hover_text("Turn on Experimental Features in Preferences to change the activation.");
    }
}

/// Every node kind the client registers, whether or not a stock perk uses it, with how far
/// Parhelion can use it today. Authorable kinds are the ones Add Action offers.
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
