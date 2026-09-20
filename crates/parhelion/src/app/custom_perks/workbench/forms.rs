use super::*;
use sundial::package_authoring::sandbox_perk::activation::PerkActivation;

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
                controls::sized(ui, controls::NARROW_COLUMN, |ui| {
                    egui::ComboBox::from_id_salt("perk-type")
                        .width(controls::NARROW_COLUMN)
                        .truncate()
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
                        })
                        .response
                        .on_hover_text(&type_name);
                    pickers::name_combo(ui, "perk-type", "Perk Type");
                });
            }
            // A disclosure drawn as one control. It used to be a label padded with four
            // spaces and an arrow painted into a copied response rect, which left the caret
            // adrift from its own button and gave the control no pressed state.
            let caret = if open {
                egui_phosphor::regular::CARET_DOWN
            } else {
                egui_phosphor::regular::CARET_RIGHT
            };
            let description = ui
                .add(egui::Button::new(format!("Description {caret}")).selected(open))
                .on_hover_text("Show this perk's name, icon and description.");
            if description.clicked() {
                self.page = if open { Page::Effects } else { Page::Basics };
            }
            if let Some(picked) = pickers::browser_with_toolbar(
                ui,
                ("perk-icons", &recipe.id),
                "Change Icon…",
                "Choose Perk Icon",
                &mut self.icon_query,
                |ui, query, opened, height| {
                    self.icons.draw(
                        ui,
                        query,
                        opened,
                        height,
                        icons::Browser {
                            packages: self.discovery.packages(),
                            catalog,
                            current: recipe.icon.as_ref(),
                        },
                    )
                },
            ) {
                match picked {
                    icons::Selection::Local(_) => unreachable!("Perk picker embeds local icons"),
                    icons::Selection::Icon(icon) => recipe.icon = Some(icon),
                    icons::Selection::Perk(hash) => {
                        if recipe.classification.is_none() {
                            recipe.classification = Some(recipe.template_plug.clone());
                        }
                        recipe.template_plug = hash.into();
                        recipe.icon = None;
                    }
                }
            }
        });
        if self.page == Page::Basics {
            ui.add(
                egui::TextEdit::multiline(&mut recipe.description)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text("Describe what this perk does. Leave blank for no description."),
            );
            let summary = self.description_from_effects(recipe, catalog);
            // One row high, so the right-aligned command sits under the text box instead of
            // centering itself in whatever height the scroll area has left.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui
                        .add_enabled(summary.is_some(), egui::Button::new("Use Effect Summary").small())
                        .on_hover_text("Replace the description with a summary of the current effects.")
                        .on_disabled_hover_text("A complete description cannot yet be generated for these effects. You can write one above.")
                        .clicked()
                        && let Some(summary) = summary
                    {
                        recipe.description = summary;
                    }
                },
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
            // The Basics header sizes its controls the same way, so moving between the two
            // pages does not change the height of the row under the heading.
            crate::app::style::compact_controls(ui);
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
                        if let Some(program) = effect.program.as_mut() {
                            program.native = Some(
                                sundial::package_authoring::sandbox_perk::program::NativeProgram::empty(),
                            );
                        }
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
        ui.add_space(4.0);
        let mut events = EffectEvents::default();
        for (position, effect) in recipe.effects.iter_mut().enumerate() {
            // Cards carry their own outline. A gap between them keeps two effects from
            // reading as one.
            if position > 0 {
                ui.add_space(4.0);
            }
            let index = effect.source_perk_index;
            if effect.program.is_some() {
                self.draw_program_effect(ui, catalog, effect, experimental, &mut events);
            } else {
                let title = self.stock_effect_name(choices, index);
                let name = format!("{}. {title}", position + 1);
                let description = catalog
                    .and_then(|catalog| catalog.perk_component_description(index))
                    .filter(|description| !description.is_empty())
                    .filter(|_| {
                        effect.runtime_values.is_empty()
                            && effect.action_float_values.is_empty()
                            && effect.projectiles.is_empty()
                            && effect.activation.is_none()
                    });
                self.draw_stock_effect(
                    ui,
                    &name,
                    &title,
                    description,
                    effect,
                    experimental,
                    &mut events,
                );
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
                            stock: None,
                            description: None,
                            labels: &BTreeMap::new(),
                            editing: experimental.then_some(canvas::Editing {
                                workbench: self,
                                catalog,
                            }),
                        },
                        header: Some(&mut header),
                        place: None,
                        footer: None,
                        trigger_command: None,
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
    #[allow(clippy::too_many_arguments)]
    fn draw_stock_effect(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        // The effect's own name, without the position this card shows it at. An adopted
        // program keeps this, since the position is a property of the list, not the effect.
        title: &str,
        description: Option<&str>,
        effect: &mut WeaponSandboxPerkRuntimeRecipe,
        experimental: bool,
        events: &mut EffectEvents,
    ) {
        let index = effect.source_perk_index;
        let issue = self.discovery.perk_issue(index).map(str::to_owned);
        let activation_summary = effect.activation.map(|activation| {
            let trigger = match activation {
                PerkActivation::WeaponKill => "A kill with this weapon",
                PerkActivation::PrecisionWeaponKill => "A precision kill with this weapon",
                PerkActivation::MeleeKill => "A melee kill",
                PerkActivation::GrenadeKill => "A grenade kill",
                PerkActivation::AnyKill => "Any credited kill",
            };
            format!("{trigger} activates this effect and its actions.")
        });
        let description = activation_summary.as_deref().or(description);
        let mut remove = false;
        let mut edit = false;
        let mut edit_trigger = false;
        let mut adopted = None;
        let digest = self.discovery.behavior(index);
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
        // Editing one of these rows is what turns a stock effect into an authored program,
        // and a program replaces every stock override rather than sitting beside it. So the
        // rows stay live only while there is nothing to lose.
        let overridden = !effect.runtime_values.is_empty()
            || !effect.action_float_values.is_empty()
            || !effect.projectiles.is_empty()
            || effect.activation.is_some();
        let live = experimental && !overridden && issue.is_none();
        // A locked card says why it is locked. Reading it as merely greyed out was the whole
        // complaint, and the reason is never that the effect cannot be edited.
        let locked_note = (overridden && issue.is_none()).then(|| {
            let mut carried = Vec::new();
            if !effect.projectiles.is_empty() {
                carried.push("a projectile swap".to_owned());
            }
            let values = effect.runtime_values.len() + effect.action_float_values.len();
            if values == 1 {
                carried.push("1 value edit".to_owned());
            } else if values > 1 {
                carried.push(format!("{values} value edits"));
            }
            if effect.activation.is_some() {
                carried.push("a changed trigger".to_owned());
            }
            format!(
                "These rows are locked because this effect carries {}. Edit Behavior turns it into an editable program without losing them.",
                carried.join(" and ")
            )
        });
        let mut footer = |ui: &mut egui::Ui| {
            if let Some(issue) = &issue {
                ui.colored_label(ui.visuals().warn_fg_color, issue);
            }
            if let Some(note) = &locked_note {
                ui.small(note);
            }
            // A live card edits its trigger in place, so the command belongs to a locked one.
            if !live {
                edit_trigger = ui
                    .add_enabled(
                        experimental && issue.is_none(),
                        egui::Button::new("Edit Trigger…"),
                    )
                    .on_hover_text(
                        "Open this effect's editable trigger, including its original filters.",
                    )
                    .clicked();
            }
        };
        // The cached digest keeps the card informative before the editor loads the action.
        let reading = digest.is_none() && self.discovery.busy();
        // Recovering the program is how the digest decides a perk is editable, so the result
        // is already in hand. Showing it means a stock effect reads in the same rows, with the
        // same real parameters, as one the reader has converted, with no click to find out.
        let mut recovered = digest.and_then(|behavior| behavior.program.clone());
        if let Some(program) = recovered.as_mut() {
            program.name = title.to_owned();
        }
        let asset_labels = recovered
            .as_ref()
            .and_then(|program| program.native.as_ref())
            .map(|native| self.program_asset_labels(native, title))
            .unwrap_or_default();
        let had_digest = digest.is_some();
        let activation = effect.activation.map(PerkActivation::label);
        // The recovered program is owned, so the editing branch can take the workbench
        // mutably. The digest branch borrows it only to read, so the two are drawn apart
        // rather than from one closure that would need both at once.
        if let Some(program) = recovered.as_mut() {
            let before = program.clone();
            ui.push_id(index, |ui| {
                canvas::draw(
                    ui,
                    canvas::Canvas {
                        name,
                        backend: canvas::Backend::Program {
                            program,
                            stock: Some(name),
                            description,
                            labels: &asset_labels,
                            editing: live.then_some(canvas::Editing {
                                workbench: self,
                                catalog: None,
                            }),
                        },
                        header: Some(&mut header),
                        place: None,
                        footer: Some(&mut footer),
                        trigger_command: None,
                    },
                );
            });
            // Only a live card may adopt. Without this the read-only gate above decided
            // what was drawn and nothing else, so a locked card could still convert itself.
            if live && *program != before {
                adopted = Some(program.clone());
            }
        } else if let Some(behavior) = self.discovery.behavior(index) {
            ui.push_id(index, |ui| {
                canvas::draw(
                    ui,
                    canvas::Canvas {
                        name,
                        backend: canvas::Backend::Digest {
                            behavior,
                            description,
                            labels: &self.asset_labels,
                            activation,
                        },
                        header: Some(&mut header),
                        place: None,
                        footer: Some(&mut footer),
                        trigger_command: None,
                    },
                );
            });
        } else {
            let _ = had_digest;
            ui.push_id(index, |ui| {
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
            });
        }
        // Editing a recovered row is the conversion. The program owns the behavior from here,
        // which is why it is only offered while the effect carries no stock override.
        if let Some(program) = adopted {
            effect.program = Some(program);
            effect.runtime_values.clear();
            effect.action_float_values.clear();
            effect.projectiles.clear();
            effect.activation = None;
        }
        if remove {
            events.remove = Some(index);
        }
        if edit || edit_trigger {
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
                // Four filters plus search and visibility do not fit one line in a narrow
                // window. Wrapping keeps every control usable.
                ui.horizontal_wrapped(|ui| {
                    // Every filter on this line is held to one width, the same one the
                    // behavior picker uses, so the two toolbars read alike. The search box
                    // takes what the named controls leave rather than a hand-summed total
                    // that goes stale the moment one of them changes.
                    const SHOW_ALL_WIDTH: f32 = 90.0;
                    let filter_width = guidance::FILTER_WIDTH;
                    let search_width = (ui.available_width()
                        - filter_width * 4.0
                        - SHOW_ALL_WIDTH
                        - ui.spacing().item_spacing.x * 5.0)
                        .max(160.0);
                    filter_changed |= pickers::search(ui, query, reset, search_width);
                    filter_changed |= guidance::filters(
                        ui,
                        &mut self.effect_purpose,
                        &mut self.effect_editing,
                        filter_width,
                    );
                    let before = self.ingredient_source;
                    let source_label = before.map_or("All Sources", |source| source.label());
                    controls::sized(ui, filter_width, |ui| {
                        egui::ComboBox::from_id_salt("ingredient-source")
                            .width(filter_width)
                            .truncate()
                            .selected_text(source_label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.ingredient_source,
                                    None,
                                    "All Sources",
                                );
                                for source in sundial::investment::IngredientSource::ALL {
                                    ui.selectable_value(
                                        &mut self.ingredient_source,
                                        Some(source),
                                        source.label(),
                                    );
                                }
                            })
                            .response
                            .on_hover_text(format!("Ingredient Source: {source_label}"));
                        pickers::name_combo(ui, "ingredient-source", "Ingredient Source");
                    });
                    filter_changed |= before != self.ingredient_source;
                    let order_before = self.effect_order;
                    controls::sized(ui, filter_width, |ui| {
                        egui::ComboBox::from_id_salt("effect-order")
                            .width(filter_width)
                            .truncate()
                            .selected_text(format!("Sort: {}", self.effect_order.label()))
                            .show_ui(ui, |ui| {
                                for choice in guidance::EffectOrder::ALL {
                                    ui.selectable_value(
                                        &mut self.effect_order,
                                        choice,
                                        choice.label(),
                                    )
                                    .on_hover_text(choice.hint());
                                }
                            })
                            .response
                            .on_hover_text("Order the results. Sorting never hides an effect.");
                        pickers::name_combo(ui, "effect-order", "Sort Order");
                    });
                    filter_changed |= order_before != self.effect_order;
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
                let normalized_query = query.trim().to_lowercase();
                let order = self.effect_order;
                available.sort_by_cached_key(|choice| {
                    let name = names[&choice.perk_index].to_lowercase();
                    // A perk-name match comes before incidental native field/context
                    // matches, such as Fourth Float plus Extend Timers.
                    let direct = pickers::matches(&normalized_query, &name);
                    guidance::effect_sort_key(
                        order,
                        &choice.representative_type_name,
                        &name,
                        direct,
                    )
                });
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
                            egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
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
        // Exactly one transition leaves the editor per frame. An explicit click wins over a
        // conversion that became ready in the same frame: Back drops the conversion with the
        // drafts, and Apply keeps the stock overrides the user confirmed. Only an unclicked
        // frame lets the automatic conversion replace the effect. Letting both run left a
        // program beside stock overrides, which the build rejects, or rewrote an effect the
        // user had just discarded.
        let conversion = editor.take_conversion();
        if apply || back {
            drop(conversion);
        } else if let Some(program) = conversion {
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
