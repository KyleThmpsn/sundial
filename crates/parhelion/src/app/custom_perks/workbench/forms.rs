use super::*;
use sundial::package_authoring::sandbox_perk::activation::PerkActivation;

mod existing;

/// What a stock effect card is drawn from: the effect's names, and the document and packages it
/// belongs to. The title is the effect's own name without the position the card shows it at,
/// which an adopted program keeps, since the position is a property of the list.
struct StockEffectCard<'a> {
    packages: &'a Path,
    document: &'a str,
    name: &'a str,
    title: &'a str,
    description: Option<&'a str>,
    experimental: bool,
}

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

    /// A stock effect's card heading: its perk's name. The effect number that tells catalog
    /// entries of one name apart belongs in pickers. On a card, the position and the actions
    /// already tell effects apart, so "Rampage · Effect 351" read as an engine detail.
    fn stock_effect_heading(&self, choices: &[WeaponSandboxPerkChoice], index: u16) -> String {
        self.ingredients
            .as_ref()
            .and_then(|(_, _, data)| data.references.shared_label(usize::from(index)))
            .map_or_else(
                || {
                    choices
                        .iter()
                        .find(|choice| choice.perk_index == index)
                        .map_or_else(
                            || format!("Effect {index}"),
                            |choice| choice.representative_name.clone(),
                        )
                },
                // "Shared Effect 85" names a shared effect by its number alone.
                |shared| {
                    shared
                        .trim_end_matches(|character: char| character.is_ascii_digit())
                        .trim_end()
                        .to_owned()
                },
            )
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
                let counts = self
                    .plug_set_counts
                    .get_or_insert_with(|| catalog.reusable_set_counts());
                let types = self
                    .type_plugs
                    .get_or_insert_with(|| type_plugs(choices, catalog, counts));
                let hash = recipe
                    .classification
                    .as_ref()
                    .unwrap_or(&recipe.template_plug)
                    .parse_u32()
                    .unwrap_or_default();
                let type_name = catalog
                    .item_type_name(hash)
                    .unwrap_or_else(|| "Unknown Type".into());
                ui.label("Type");
                controls::sized(ui, controls::NARROW_COLUMN, |ui| {
                    egui::ComboBox::from_id_salt("perk-type")
                        .width(controls::NARROW_COLUMN)
                        .truncate()
                        .selected_text(&type_name)
                        .show_ui(ui, |ui| {
                            crate::app::style::perk_workbench_style(ui);
                            // One choice per native type, retaining the current source when
                            // unchanged.
                            for (name, &source) in types.iter() {
                                if ui
                                    .selectable_label(*name == type_name, name.as_str())
                                    .clicked()
                                    && *name != type_name
                                {
                                    recipe.classification = Some(source.into());
                                }
                            }
                        })
                        .response
                        .on_hover_text(&type_name);
                    pickers::name_combo(ui, "perk-type", "Perk Type");
                });
                // Shown where the type's plug is offered by a shared plug set, and while on, so
                // it can be turned off after the type changes.
                if (recipe.projectile.is_none() && counts.get(&hash).is_some_and(|&sets| sets > 0))
                    || recipe.offer_everywhere
                {
                    let offer = ui
                        .checkbox(&mut recipe.offer_everywhere, "Offer Everywhere")
                        .on_hover_text(format!(
                            "Every socket that offers {}",
                            catalog.plug_label(hash, false)
                        ));
                    // Turning it on takes the type's own plug when more sockets offer that, as
                    // one saved with Anti-Barrier Rounds for Weapon Mod moves to Boss Spec.
                    if offer.changed()
                        && recipe.offer_everywhere
                        && let Some(&plug) = types.get(&type_name)
                        && counts.get(&plug) > counts.get(&hash)
                    {
                        recipe.classification = Some(plug.into());
                    }
                }
            }
            if let Some(projectile) = recipe.projectile {
                ui.label(format!("Projectile: {}", projectile.label()))
                    .on_hover_text(
                        "Attaching this perk gives the weapon its own copy of this projectile.",
                    );
            }
            // A disclosure drawn as one control, so the caret stays with its own button and the
            // control shows a pressed state.
            let caret = if open {
                egui_phosphor::regular::CARET_DOWN
            } else {
                egui_phosphor::regular::CARET_RIGHT
            };
            let description = ui.add(
                egui::Button::new(crate::app::style::text_with_icon(ui, "Description ", caret))
                    .selected(open),
            );
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
                    .hint_text("Description"),
            );
            let summary = self.description_from_effects(recipe, catalog);
            // One row high, so the right-aligned command sits under the text box instead of
            // centering itself in whatever height the scroll area has left.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui
                        .add_enabled(
                            summary.is_some(),
                            egui::Button::new("Use Effect Summary").small(),
                        )
                        .on_disabled_hover_text("No summary for these effects.")
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
                    if program.native.is_some() {
                        return describe_native(program, &self.asset_labels);
                    }
                    if program.actions.is_empty()
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

    fn refresh_effect_names(
        &mut self,
        ctx: &egui::Context,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
    ) {
        if self.perk_names.len() != choices.len() {
            self.perk_names = choices
                .iter()
                .map(|choice| (choice.perk_index, choice.representative_name.clone()))
                .collect();
            self.asset_name_revision += 1;
        }
        self.refresh_item_names(catalog);
        self.refresh_asset_labels(ctx);
    }

    fn draw_effects_header(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
    ) {
        ui.horizontal_wrapped(|ui| {
            // The Basics header sizes its controls the same way, so moving between the two
            // pages does not change the height of the row under the heading.
            crate::app::style::compact_controls(ui);
            ui.heading("Effects");
            if !recipe.effects.is_empty() {
                self.draw_add_effect_buttons(ui, catalog, choices, recipe, false);
            }
            crate::app::style::more_menu(ui, "Effects", |ui| {
                for (label, expanded) in [("Collapse All", false), ("Expand All", true)] {
                    if ui
                        .add_enabled(!recipe.effects.is_empty(), egui::Button::new(label))
                        .clicked()
                    {
                        for effect in &recipe.effects {
                            cards::Card::new(
                                &recipe.id,
                                effect.source_perk_index,
                                0,
                                recipe.effects.len(),
                            )
                            .set_expanded(ui.ctx(), expanded);
                        }
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Engine Catalog…").clicked() {
                    self.engine.open = true;
                    ui.close();
                }
            });
        });
        if self.duplicating.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Copying effect…");
            });
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
        self.refresh_effect_names(&ctx, catalog, choices);
        self.draw_effects_header(ui, catalog, choices, recipe);
        ui.add_space(4.0);
        if recipe.effects.is_empty() {
            crate::app::style::block(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.strong("No Effects Yet");
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    self.draw_add_effect_buttons(ui, catalog, choices, recipe, true);
                });
                self.draw_starter_perks(ui, choices, recipe);
            });
            ui.add_space(8.0);
        }
        self.retain_stock_programs(recipe);
        let mut events = EffectEvents::default();
        let count = recipe.effects.len();
        for (position, effect) in recipe.effects.iter_mut().enumerate() {
            // Cards carry their own outline. A gap between them keeps two effects from
            // reading as one.
            if position > 0 {
                ui.add_space(4.0);
            }
            let index = effect.source_perk_index;
            let card = cards::Card::new(&recipe.id, index, position, count);
            let reveal = self
                .reveal_problem
                .as_ref()
                .filter(|target| target.document == recipe.id && target.effect == index)
                .cloned();
            if let Some(target) = &reveal {
                card.set_expanded(&ctx, true);
                self.reveal_action = target.action;
            }
            let mut retry_reveal = false;
            let response = ui
                .push_id((&recipe.id, index), |ui| {
                    if let Some(target) = reveal.as_ref().and_then(|target| target.native.as_ref())
                    {
                        program::native::reveal(ui.ctx(), target.clone());
                    }
                    if let Some(target) = reveal
                        .as_ref()
                        .and_then(|target| target.native_field.as_ref())
                    {
                        program::native::problem::reveal(ui.ctx(), target.clone());
                    }
                    if effect.program.is_some() {
                        self.draw_program_effect(ui, effect, card, &mut events);
                    } else {
                        let title = self.stock_effect_heading(choices, index);
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
                            StockEffectCard {
                                packages,
                                document: &recipe.id,
                                name: &name,
                                title: &title,
                                description,
                                experimental,
                            },
                            effect,
                            card,
                            &mut events,
                        );
                    }
                    let shown = program::native::problem::finish(ui.ctx());
                    if !shown
                        && reveal
                            .as_ref()
                            .is_some_and(|target| target.native_field.is_some())
                        && !card.structure(ui.ctx())
                    {
                        // Some required fields have no normal card control. Retry once in
                        // the complete editor, keeping the same document and field target.
                        card.set_structure(ui.ctx(), true);
                        retry_reveal = true;
                        ui.ctx().request_repaint();
                    }
                })
                .response;
            // A card asks for one of its behavior groups to become an effect of its own, which
            // only the list can make, so the request is read back as soon as its card is drawn.
            let move_request = egui::Id::new(program::native::MOVE_GROUP);
            if let Some(group) = ui
                .ctx()
                .data_mut(|data| data.remove_temp::<usize>(move_request))
                && effect.program.is_some()
            {
                events.move_group = Some((index, group));
            }
            if !retry_reveal {
                self.finish_reveal(reveal, &response);
            }
            if let Some(movement) = card.drop_target(ui, response.rect) {
                events.movement = Some(movement);
            }
        }
        cards::scroll_during_drag(ui, &recipe.id);
        if let Some(movement) = events.movement {
            cards::apply_move(recipe, movement);
        }
        if let Some(index) = events.duplicate {
            self.duplicate_effect(packages, &ctx, recipe, choices, index);
        }
        if let Some(index) = events.remove {
            // Reusing a removed identity for a new effect should open its controls.
            cards::Card::new(&recipe.id, index, 0, count).set_expanded(&ctx, true);
            recipe
                .effects
                .retain(|effect| effect.source_perk_index != index);
        }
        if let Some((effect, group)) = events.move_group {
            self.move_group(&ctx, recipe, choices, effect, group);
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

    /// Moves an effect's behavior group into a new effect after it, opened, as Move to Its Own
    /// Effect asks.
    fn move_group(
        &mut self,
        ctx: &egui::Context,
        recipe: &mut PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
        effect: u16,
        group: usize,
    ) {
        let Some(index) = self.free_metadata_index(recipe, choices) else {
            self.error = Some("The perk has no room for another effect.".into());
            return;
        };
        match program::native::move_group(recipe, effect, group, index) {
            Ok(()) => {
                let count = recipe.effects.len();
                let position = recipe
                    .effects
                    .iter()
                    .position(|candidate| candidate.source_perk_index == index)
                    .unwrap_or_default();
                cards::Card::new(&recipe.id, index, position, count).set_expanded(ctx, true);
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// Add Effect starts a blank effect and Add from Perk copies one from a stock perk. The empty
    /// state leads with them, so `primary` fills the first.
    fn draw_add_effect_buttons(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
        primary: bool,
    ) {
        let add = if primary {
            crate::app::style::primary(ui, "Add Effect")
        } else {
            egui::Button::new("Add Effect")
        };
        if ui.add(add).clicked()
            && let Some(index) = self.free_metadata_index(recipe, choices)
        {
            recipe.effects.push(program::named_effect(index));
        }
        self.draw_existing_behavior_picker(ui, catalog, choices, recipe);
    }

    /// Well-known stock perks a first perk can start from, each added whole with one click.
    /// They are the perks Suggested lists first in Add from Perk, so a newcomer's first effect
    /// is one they know and can read, edit and build straight away.
    fn draw_starter_perks(
        &mut self,
        ui: &mut egui::Ui,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
    ) {
        const STARTERS: usize = 6;
        if self.discovery.data.is_none() {
            return;
        }
        let mut seen = BTreeSet::new();
        let starters = guidance::EFFECT_LEAD
            .iter()
            .filter_map(|lead| {
                choices
                    .iter()
                    .find(|choice| choice.representative_name.eq_ignore_ascii_case(lead))
            })
            .filter(|choice| seen.insert(choice.representative_hash))
            .filter(|choice| {
                choices
                    .iter()
                    .filter(|other| other.representative_hash == choice.representative_hash)
                    .all(|other| self.discovery.perk_issue(other.perk_index).is_none())
            })
            .take(STARTERS)
            .map(|choice| {
                (
                    choice.representative_name.clone(),
                    choice.representative_hash,
                )
            })
            .collect::<Vec<_>>();
        if starters.is_empty() {
            return;
        }
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Start From").color(crate::app::style::secondary(ui.visuals())),
            );
            for (name, hash) in &starters {
                if ui.button(name).clicked() {
                    chosen = Some(*hash);
                }
            }
        });
        if let Some(hash) = chosen {
            for choice in choices
                .iter()
                .filter(|choice| choice.representative_hash == hash)
            {
                let index = choice.perk_index;
                if !recipe
                    .effects
                    .iter()
                    .any(|effect| effect.source_perk_index == index)
                {
                    recipe.effects.push(PerkRecipe::effect(index));
                }
            }
        }
    }

    /// An authored program on the shared canvas.
    fn draw_program_effect(
        &mut self,
        ui: &mut egui::Ui,
        effect: &mut WeaponSandboxPerkRuntimeRecipe,
        card: cards::Card,
        events: &mut EffectEvents,
    ) {
        let index = effect.source_perk_index;
        let Some(program) = &mut effect.program else {
            return;
        };
        let mut remove = false;
        let mut add_group = false;
        let copying = self.duplicating.is_some();
        let mut header = |ui: &mut egui::Ui| {
            crate::app::style::more_menu(ui, "Effect", |ui| {
                crate::app::style::perk_workbench_style(ui);
                card.menu(ui, &mut events.movement);
                ui.separator();
                if ui
                    .add_enabled(!copying, egui::Button::new("Duplicate Effect"))
                    .clicked()
                {
                    events.duplicate = Some(index);
                    ui.close();
                }
                if behavior_group_command(ui) {
                    add_group = true;
                    ui.close();
                }
                card.structure_menu(ui);
                if ui.button("Remove Effect").clicked() {
                    remove = true;
                    ui.close();
                }
            });
        };
        let output = ui
            .push_id(index, |ui| {
                canvas::draw_effect(
                    ui,
                    canvas::Canvas {
                        name: "",
                        backend: canvas::Backend::Program {
                            program,
                            stock: None,
                            description: None,
                            labels: &BTreeMap::new(),
                            editing: Some(canvas::Editing { workbench: self }),
                        },
                        header: Some(&mut header),
                        footer: None,
                        trigger_command: None,
                    },
                    card,
                )
            })
            .inner;
        if remove {
            events.remove = Some(index);
        }
        if add_group && let Err(error) = program::native::add_behavior_group(program) {
            self.error = Some(error);
        }
        if let Some(action) = output.edit_action
            && let Some(asset) = program.asset(action)
        {
            events.edit_asset = Some((index, action, asset.clone()));
        }
    }

    /// A stock effect card. It reads in the rows an authored program is edited in, and the
    /// first edit makes the effect its own program. A card carrying overrides waits for the
    /// checked conversion to fold them in, and stays a reading when no exact program does.
    fn draw_stock_effect(
        &mut self,
        ui: &mut egui::Ui,
        stock: StockEffectCard<'_>,
        effect: &mut WeaponSandboxPerkRuntimeRecipe,
        card: cards::Card,
        events: &mut EffectEvents,
    ) {
        let StockEffectCard {
            packages,
            document,
            name,
            title,
            description,
            experimental,
        } = stock;
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
            format!("Triggered by {}.", trigger.to_lowercase())
        });
        let description = activation_summary.as_deref().or(description);
        let overridden = !effect.runtime_values.is_empty()
            || !effect.action_float_values.is_empty()
            || !effect.projectiles.is_empty()
            || effect.activation.is_some();
        // Recovering the program is how the digest decides a perk is editable, so an unchanged
        // effect already has it. Overrides need the conversion, which reads packages.
        let preparation = (overridden && issue.is_none())
            .then(|| self.prepare_stock_effect(packages, ui.ctx(), document, effect, title));
        let preparing = matches!(preparation, Some(stock::Preparation::Reading));
        let mut recovered = match preparation {
            Some(stock::Preparation::Ready(program)) => Some(*program),
            Some(_) => None,
            None => self
                .discovery
                .behavior(index)
                .and_then(|behavior| {
                    let mut program = behavior.program.clone()?;
                    program.name = title.to_owned();
                    Some(program)
                })
                // A declaration with no action of its own starts an empty program, so it edits
                // like any other effect. It stays the stock entry until something is added.
                .or_else(|| {
                    use sundial::package_authoring::sandbox_perk::program::Program;
                    (!overridden && self.discovery.declaration_only(index)).then(|| Program {
                        name: title.to_owned(),
                        ..Program::default()
                    })
                }),
        };
        let live = issue.is_none() && recovered.is_some();
        let mut remove = false;
        let mut edit = false;
        let mut add_group = false;
        let copying = self.duplicating.is_some();
        let mut header = |ui: &mut egui::Ui| {
            crate::app::style::more_menu(ui, "Effect", |ui| {
                crate::app::style::perk_workbench_style(ui);
                card.menu(ui, &mut events.movement);
                ui.separator();
                if ui
                    .add_enabled(
                        !copying && issue.is_none(),
                        egui::Button::new("Duplicate Effect"),
                    )
                    .clicked()
                {
                    events.duplicate = Some(index);
                    ui.close();
                }
                // A live card edits in place, so the editor is a detail behind the menu.
                if live && ui.button("Edit Behavior…").clicked() {
                    edit = true;
                    ui.close();
                }
                if live && behavior_group_command(ui) {
                    add_group = true;
                    ui.close();
                }
                if live {
                    card.structure_menu(ui);
                }
                if experimental && ui.button("Inspect Pattern Dependencies…").clicked() {
                    crate::app::runtime_dependencies::request(ui.ctx(), Some(usize::from(index)));
                    ui.close();
                }
                if ui.button("Remove Effect").clicked() {
                    remove = true;
                    ui.close();
                }
            });
            if !live
                && ui
                    .add_enabled(issue.is_none(), egui::Button::new("Edit Behavior…"))
                    .clicked()
            {
                edit = true;
            }
        };
        let mut footer = |ui: &mut egui::Ui| {
            if let Some(issue) = &issue {
                ui.colored_label(ui.visuals().warn_fg_color, issue);
            }
            if preparing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.small("Reading…");
                });
            }
        };
        let reading = self.discovery.behavior(index).is_none() && self.discovery.busy();
        let asset_labels = recovered
            .as_ref()
            .and_then(|program| program.native.as_ref())
            .map(|native| self.program_asset_labels(native, title))
            .unwrap_or_default();
        // Named as the trigger picker names kill triggers.
        let activation = effect
            .activation
            .map(|activation| format!("On {}", activation.label()));
        let mut adopted = None;
        if let Some(program) = recovered.as_mut() {
            let before = program.clone();
            let output = ui
                .push_id(index, |ui| {
                    canvas::draw_effect(
                        ui,
                        canvas::Canvas {
                            name,
                            backend: canvas::Backend::Program {
                                program,
                                stock: Some(name),
                                description,
                                labels: &asset_labels,
                                editing: live.then_some(canvas::Editing { workbench: self }),
                            },
                            header: Some(&mut header),
                            footer: Some(&mut footer),
                            trigger_command: None,
                        },
                        card,
                    )
                })
                .inner;
            if add_group && let Err(error) = program::native::add_behavior_group(program) {
                self.error = Some(error);
            }
            // Editing a row, or opening one of its assets, is the conversion.
            if live && (*program != before || output.edit_action.is_some()) {
                if let Some(action) = output.edit_action
                    && let Some(asset) = program.asset(action)
                {
                    events.edit_asset = Some((index, action, asset.clone()));
                }
                adopted = Some(program.clone());
            }
        } else if let Some(behavior) = self.discovery.behavior(index) {
            ui.push_id(index, |ui| {
                canvas::draw_effect(
                    ui,
                    canvas::Canvas {
                        name,
                        backend: canvas::Backend::Digest {
                            behavior,
                            description,
                            labels: &self.asset_labels,
                            activation: activation.as_deref(),
                        },
                        header: Some(&mut header),
                        footer: Some(&mut footer),
                        trigger_command: None,
                    },
                    card,
                );
            });
        } else {
            ui.push_id(index, |ui| {
                crate::app::style::card(ui, |ui| {
                    let expanded = canvas::draw_header(
                        ui,
                        Some(&mut header),
                        card,
                        |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap());
                        },
                        || None,
                    );
                    if !expanded {
                        return;
                    }
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
        // The program carries every override, checked exact, so the overrides go with it.
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

    pub(super) fn free_metadata_index(
        &self,
        recipe: &PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
    ) -> Option<u16> {
        crate::perk::EFFECT_CARRIERS
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

    /// Add from Perk. Each frame draws the toolbar, which collects the effects its filters
    /// leave, then lists their perks and adds the effects the detail pane picks.
    fn draw_existing_behavior_picker(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        choices: &[WeaponSandboxPerkChoice],
        recipe: &mut PerkRecipe,
    ) {
        self.refresh_asset_labels(ui.ctx());
        let picked = pickers::browser_with_toolbar(
            ui,
            "existing-behavior",
            "Add from Perk…",
            "Browse Perk Effects",
            &mut self.effect_query,
            |ui, query, reset, _height| {
                let ingredients = self.ingredients.as_ref().map(|(_, _, data)| data);
                let perks = existing::Perks {
                    choices,
                    names: EffectNames::current(&mut self.effect_names, choices, ingredients),
                    discovery: &self.discovery,
                    ingredients,
                    catalog,
                    asset_labels: &self.asset_labels,
                };
                let toolbar = existing::Toolbar {
                    purpose: &mut self.effect_purpose,
                    editing: &mut self.effect_editing,
                    source: &mut self.ingredient_source,
                    order: &mut self.effect_order,
                };
                let (changed, available) = existing::toolbar(
                    ui,
                    query,
                    reset,
                    toolbar,
                    perks.discovery.busy(),
                    |filter| perks.available(filter),
                );
                ui.separator();
                if existing::waiting(ui, perks.discovery) {
                    return None;
                }
                let listing =
                    perks.listing(available, &query.trim().to_lowercase(), self.effect_order);
                existing::list(ui, &perks, &listing, recipe, reset || changed)
            },
        );
        self.add_effects(recipe, choices, picked.into_iter().flatten());
    }

    /// Adds the picked stock effects. A stock effect already here stays once. An authored
    /// program holding the index moves to a free one, as a single add always did, and the rest
    /// are not added when none is free.
    fn add_effects(
        &self,
        recipe: &mut PerkRecipe,
        choices: &[WeaponSandboxPerkChoice],
        picked: impl IntoIterator<Item = u16>,
    ) {
        for index in picked {
            if existing::included(recipe, index) {
                continue;
            }
            if let Some(position) = recipe
                .effects
                .iter()
                .position(|effect| effect.source_perk_index == index)
            {
                let Some(replacement) = self.free_metadata_index(recipe, choices) else {
                    return;
                };
                recipe.effects[position].source_perk_index = replacement;
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
        if editor.graph.is_none() && !editor.is_loading() && editor.error.is_none() {
            editor.start_load(ctx);
        }
        let before = editor.snapshot();
        let mut restored_history = false;
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
        // History and reset first, then leaving: Back, and last the primary Apply and Back,
        // the order a dialog's Cancel and Done take.
        ui.horizontal_wrapped(|ui| {
            for (label, redo) in [("Undo", false), ("Redo", true)] {
                if ui
                    .add_enabled(editor.history_available(redo), egui::Button::new(label))
                    .clicked()
                {
                    editor.restore_history(redo);
                    restored_history = true;
                }
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
            ui.separator();
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
            let apply_button = crate::app::style::primary(ui, "Apply and Back");
            if ui.add_enabled(valid, apply_button).clicked() {
                apply = true;
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
        // Exactly one transition leaves the editor per frame. Back or Apply wins over a
        // conversion requested in the same frame: Back drops it with the drafts, and Apply
        // keeps the stock overrides the user confirmed. Otherwise Convert Anyway replaces the
        // effect. Letting both run left a program beside stock overrides, which
        // the build rejects, or rewrote an effect the user had just discarded.
        if !restored_history {
            editor.record_history(before, ctx);
        }
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
    duplicate: Option<u16>,
    movement: Option<cards::Move>,
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
    /// Move this effect's behavior group into an effect of its own.
    move_group: Option<(u16, usize)>,
}

/// Every stock effect's name as `Workbench::stock_effect_name` gives it, kept with the
/// ingredient catalog it was read from.
pub(super) struct EffectNames {
    ingredients: Option<Arc<sundial::investment::IngredientCatalog>>,
    names: BTreeMap<u16, String>,
}

impl EffectNames {
    fn read(
        choices: &[WeaponSandboxPerkChoice],
        ingredients: Option<&Arc<sundial::investment::IngredientCatalog>>,
    ) -> Self {
        let mut indices = BTreeMap::<&str, BTreeSet<u16>>::new();
        for choice in choices {
            indices
                .entry(choice.representative_name.as_str())
                .or_default()
                .insert(choice.perk_index);
        }
        let mut names = BTreeMap::new();
        for choice in choices {
            let index = choice.perk_index;
            names.entry(index).or_insert_with(|| {
                ingredients
                    .and_then(|data| data.references.shared_label(usize::from(index)))
                    .unwrap_or_else(|| {
                        if indices[choice.representative_name.as_str()].len() > 1 {
                            format!("{} · Effect {index}", choice.representative_name)
                        } else {
                            choice.representative_name.clone()
                        }
                    })
            });
        }
        Self {
            ingredients: ingredients.cloned(),
            names,
        }
    }

    /// The names `held` keeps for `choices`, read again unless they were read from this catalog.
    fn current<'a>(
        held: &'a mut Option<Self>,
        choices: &[WeaponSandboxPerkChoice],
        ingredients: Option<&Arc<sundial::investment::IngredientCatalog>>,
    ) -> &'a BTreeMap<u16, String> {
        if !held
            .as_ref()
            .is_some_and(|names| names.reads(choices, ingredients))
        {
            *held = Some(Self::read(choices, ingredients));
        }
        &held.as_ref().expect("effect names").names
    }

    /// Only choices owned by the held catalog are known to be unchanged.
    fn reads(
        &self,
        choices: &[WeaponSandboxPerkChoice],
        ingredients: Option<&Arc<sundial::investment::IngredientCatalog>>,
    ) -> bool {
        match (&self.ingredients, ingredients) {
            (Some(held), Some(data)) => {
                Arc::ptr_eq(held, data) && std::ptr::eq(choices, data.choices.as_slice())
            }
            _ => false,
        }
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

/// Each native perk type with the stock plug that stands for it: the plug the most shared plug
/// sets offer, one with effects before an empty one, then the first by name. A perk offered
/// everywhere joins the sets that offer its type's plug, so Weapon Mod takes Boss Spec, which
/// every weapon's mod socket offers, rather than Anti-Barrier Rounds, which four weapon types
/// offer.
fn type_plugs(
    choices: &[WeaponSandboxPerkChoice],
    catalog: &InvestmentCatalog,
    counts: &std::collections::HashMap<u32, usize>,
) -> BTreeMap<String, u32> {
    let mut types = BTreeMap::<String, (u32, (usize, bool))>::new();
    for choice in choices {
        if choice.representative_type_name.trim().is_empty() {
            continue;
        }
        let hash = choice.representative_hash;
        let rank = (
            counts.get(&hash).copied().unwrap_or_default(),
            !catalog.item_sandbox_perk_indices(hash).is_empty(),
        );
        match types.get_mut(&choice.representative_type_name) {
            Some(best) if best.1 >= rank => {}
            Some(best) => *best = (hash, rank),
            None => {
                types.insert(choice.representative_type_name.clone(), (hash, rank));
            }
        }
    }
    types
        .into_iter()
        .map(|(name, (hash, _))| (name, hash))
        .collect()
}

/// The effect menu's command for a separate trigger and its actions within the effect.
fn behavior_group_command(ui: &mut egui::Ui) -> bool {
    ui.button("Add Behavior Group")
        .on_hover_text(
            "Another behavior in this effect. Only the main behavior starts on an event.",
        )
        .clicked()
}

/// A native program in plain words, one sentence per behavior: its trigger, its actions with
/// the objects they name, and its timers. A starting point for the perk's description.
fn describe_native(
    program: &sundial::package_authoring::sandbox_perk::program::Program,
    labels: &BTreeMap<u32, String>,
) -> Option<String> {
    use sundial::package_authoring::sandbox_perk::{
        action::{self, DecodedCondition, FactValue},
        nodes,
    };
    let native = program.native.as_ref()?;
    let decoded = action::decode(&native.graph.emit().ok()?).ok()?;
    let seconds = |list: &[DecodedCondition]| -> Option<String> {
        match list {
            [timer] if timer.kind == 1 => timer
                .facts
                .iter()
                .find(|fact| fact.label == "Duration")
                .and_then(|fact| match fact.value {
                    FactValue::Seconds(seconds) | FactValue::Number(seconds) => Some(seconds),
                    _ => None,
                })
                .filter(|seconds| *seconds > 0.0)
                .map(|seconds| format!("{seconds} s")),
            _ => None,
        }
    };
    let sentences = decoded
        .groups
        .iter()
        .map(|group| {
            let trigger = if group.activation.is_empty() {
                nodes::condition_title(0).to_owned()
            } else {
                group
                    .activation
                    .iter()
                    .map(program::decoded_condition_title)
                    .collect::<Vec<_>>()
                    .join(" or ")
            };
            let actions = group
                .effects
                .iter()
                .rev()
                .map(|effect| {
                    let title = program::native_action_label(effect.kind, &effect.native);
                    match effect.referenced_tag.and_then(|tag| labels.get(&tag)) {
                        Some(name) => format!("{title} ({name})"),
                        None => title,
                    }
                })
                .collect::<Vec<_>>();
            let mut sentence = if actions.is_empty() {
                trigger
            } else {
                format!("{trigger}: {}", actions.join(", "))
            };
            if let Some(duration) = seconds(&group.removal) {
                sentence.push_str(&format!(" for {duration}"));
            }
            if let Some(cooldown) = seconds(&group.rearm) {
                sentence.push_str(&format!(", at most every {cooldown}"));
            }
            format!("{sentence}.")
        })
        .collect::<Vec<_>>();
    (!sentences.is_empty()).then(|| sentences.join(" "))
}
