//! Everyday weapon editor and identity/appearance views.
use super::*;

mod text_fields;
use text_fields::{OptionalText, TextSection};

impl PackageAuthoringApp {
    pub(super) fn draw_core_recipe_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        if let Some(donor_width) = workbench_left_column_width(ui.available_width()) {
            let definition_width = ui.available_width() - donor_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(donor_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(donor_width);
                        self.draw_donor_section(ui);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(definition_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(definition_width);
                        let donor = self.current_donor();
                        self.draw_definition_panel(ui, donor.as_ref());
                    },
                );
            });
        } else {
            self.draw_donor_section(ui);
            ui.separator();
            let donor = self.current_donor();
            self.draw_definition_panel(ui, donor.as_ref());
        }
        let donor = self.current_donor();
        let donor = donor.as_ref();
        if donor.is_none() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "The selected gameplay donor is not available in the currently loaded Sundial catalog.",
            );
        }

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        let available_width = ui.available_width();
        if let Some(stats_width) = workbench_left_column_width(available_width) {
            let spacing = ui.spacing().item_spacing.x;
            let sockets_width = available_width - stats_width - spacing;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(stats_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(stats_width);
                        self.draw_investment_stats_panel(ui, donor);
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(sockets_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(sockets_width);
                        self.draw_socket_columns_panel(ui, donor);
                    },
                );
            });
        } else {
            self.draw_socket_columns_panel(ui, donor);
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            self.draw_investment_stats_panel(ui, donor);
        }
    }

    pub(super) fn draw_weapon_name(&mut self, ui: &mut egui::Ui) {
        let mut edited_name = self
            .invalid_weapon_name
            .as_ref()
            .map_or_else(|| self.recipe.name.clone(), |(text, _)| text.clone());
        let response = ui
            .horizontal(|ui| {
                let label = ui.add_sized(
                    [90.0, ui.spacing().interact_size.y],
                    egui::Label::new("Weapon Name"),
                );
                ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut edited_name),
                )
                .labelled_by(label.id)
            })
            .inner;
        if response.changed() {
            self.edit_weapon_name(edited_name);
        }
        if let Some((_, error)) = &self.invalid_weapon_name {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    pub(super) fn edit_weapon_name(&mut self, name: String) {
        self.invalid_weapon_name = self
            .recipe
            .rename_authored_item(&name)
            .err()
            .map(|error| (name, format!("Finish editing the weapon name: {error}")));
        self.invalidate_results();
        self.synchronize_recipe_dirty();
    }

    pub(super) fn draw_recipe_namespace(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Namespace");
            if !self.recipe.identity_is_name_derived() {
                draw_authoring_info_icon(ui,
                    "This recipe has custom identity hashes. Renaming generates new hashes from the new parhelion.* namespace.");
            }
        });
        let mut namespace = self.recipe.namespace.clone();
        ui.add_sized(
            [ui.available_width(), ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut namespace).interactive(false),
        ).on_hover_text("Derived from the weapon name. Renaming updates all generated identity hashes together.");
    }

    pub(super) fn draw_definition_panel(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        let panel_scope = self.recipe_panel_scope();
        self.draw_weapon_name(ui);
        ui.horizontal_top(|ui| {
            let label = ui.add_sized(
                [90.0, ui.spacing().interact_size.y],
                egui::Label::new("Flavor Text"),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.recipe.flavor)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2),
            )
            .labelled_by(label.id);
        });

        egui::CollapsingHeader::new("Text Presentation")
            .id_salt(("parhelion-text-presentation", panel_scope.as_str()))
            .default_open(self.recipe.overrides.lore.is_some())
            .show(ui, |ui| {
            ui.weak("Turning off optional text also removes its translations.");
            let mut custom_type = self.recipe.type_name.is_some();
            if ui
                .checkbox(&mut custom_type, "Custom item-type label")
                .on_hover_text(
                    "Writes the independent item-type localization reference at item-string offset 0x90. Disable this to preserve the gameplay donor's label.",
                )
                .changed()
            {
                let value = custom_type.then(|| {
                    donor
                        .map(|donor| donor.summary.type_name.clone())
                        .unwrap_or_default()
                });
                text_fields::set(&mut self.recipe, OptionalText::TypeName, value);
            }
            if let Some(type_name) = &mut self.recipe.type_name {
                ui.add(
                    egui::TextEdit::singleline(type_name)
                        .desired_width(f32::INFINITY)
                        .hint_text("Item type shown by the game"),
                );
            } else if let Some(donor) = donor {
                ui.label(format!("Inherited: {}", donor.summary.type_name));
            }

            ui.separator();
            let mut inventory_hint = self.recipe.inventory_hint.is_some();
            if ui
                .checkbox(&mut inventory_hint, "Inventory acquisition hint")
                .on_hover_text(
                    "Optional inventory tooltip acquisition text, separate from Collections Source. This is display text only: authored weapons can be reacquired from Collections.",
                )
                .changed()
            {
                let value = inventory_hint.then(|| {
                    "Curated roll: This weapon can be reacquired from Collections.".to_owned()
                });
                text_fields::set(&mut self.recipe, OptionalText::InventoryHint, value);
            }
            if let Some(value) = &mut self.recipe.inventory_hint {
                ui.add(
                    egui::TextEdit::singleline(value)
                        .desired_width(f32::INFINITY)
                        .hint_text("Inventory tooltip acquisition line"),
                );
            }
            ui.separator();
            self.presentation_editor.draw_lore(ui, &mut self.recipe.overrides, &self.packages, self.recipe.donor.item_hash.parse_u32().ok());
            ui.separator();
            self.draw_locale_text_overrides(ui, TextSection::Weapon);
            });

        ui.add_space(4.0);
        let mut inventory_slot_changed = false;
        let column_count = core_profile_column_count(ui.available_width());
        for fields in [0, 1, 2, 3, 4].chunks(column_count) {
            ui.columns(column_count, |columns| {
                for (&field, column) in fields.iter().zip(columns) {
                    match field {
                        0 | 1 => {
                            let changed = draw_combat_profile_control(
                                column,
                                &mut self.recipe.overrides,
                                donor,
                                field == 0,
                            );
                            inventory_slot_changed |= field == 0 && changed;
                        }
                        2 => draw_ammo_type_control(column, &mut self.recipe.overrides, donor),
                        3 => draw_rarity_control(column, &mut self.recipe.overrides, donor),
                        _ => draw_power_cap_control(
                            column,
                            &mut self.recipe.overrides,
                            donor,
                            self.catalog.as_ref(),
                        ),
                    }
                }
            });
        }
        draw_combat_profile_diagnostics(ui, &self.recipe.overrides, donor);
        if inventory_slot_changed && let Some(gameplay_donor) = donor {
            reconcile_presentation_donor(
                &mut self.recipe,
                &gameplay_donor.summary,
                &self.donor_summaries,
            );
        }
    }

    pub(super) fn draw_gameplay_workspace(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("Gameplay Properties");
            draw_authoring_info_icon(
                ui,
                "Customize behavior from the selected base weapon and runtime source. Components from different weapons may not work together. Test the complete combination in game.",
            );
        });
        self.draw_runtime_source_summary(ui);
        if self.show_experimental_options && ui.button("Perks & Patterns…").clicked() {
            runtime_dependencies::request(ui.ctx(), None);
        }
        ui.add_space(5.0);
        if !self.show_experimental_options
            && !AdvancedGameplayPage::STABLE.contains(&self.advanced_gameplay_page)
        {
            self.advanced_gameplay_page = AdvancedGameplayPage::Runtime;
            self.scroll_recipe_to_top = true;
        }
        let visible_pages: &[AdvancedGameplayPage] = if self.show_experimental_options {
            &AdvancedGameplayPage::ALL
        } else {
            &AdvancedGameplayPage::STABLE
        };
        ui.horizontal_wrapped(|ui| {
            for &page in visible_pages {
                if ui
                    .selectable_value(&mut self.advanced_gameplay_page, page, page.label())
                    .changed()
                {
                    self.scroll_recipe_to_top = true;
                }
            }
        });
        ui.separator();
        ui.add_space(5.0);
        match self.advanced_gameplay_page {
            AdvancedGameplayPage::Runtime => {
                self.draw_weapon_pattern_picker(ui, donor);
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(5.0);
                if self.show_experimental_options {
                    self.draw_runtime_component_donors(ui);
                } else {
                    ui.label("Component mixing, runtime values, and base-item perk or trait rows are experimental. Enable advanced technical controls in Preferences to edit them.");
                    ui.label("Saved overrides remain active when these controls are hidden.");
                    if ui.button("Open Preferences…").clicked() {
                        self.preferences_page = preferences_view::PreferencesPage::EditorLibrary;
                        self.preferences_open = true;
                    }
                }
            }
            AdvancedGameplayPage::PerksTraits => {
                self.draw_base_sandbox_perks(ui, donor);
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(5.0);
                self.draw_item_traits(ui, donor);
            }
            AdvancedGameplayPage::Inventory => self.draw_native_inventory_fields(ui, donor),
            AdvancedGameplayPage::Raw => self.draw_raw_payload_patches(ui),
        }
    }

    pub(super) fn draw_collections_workspace(&mut self, ui: &mut egui::Ui) {
        let mut badges = self
            .recipe_entries
            .iter()
            .filter_map(|entry| entry.badge.clone())
            .collect::<Vec<_>>();
        badges.sort_by(|a, b| a.name.cmp(&b.name));
        badges.dedup();
        ui.scope(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Collections");
                self.draw_collection_capacity(ui);
            });
            ui.add_space(8.0);
            if ui.available_width() >= 700.0 {
                ui.columns(2, |columns| {
                    egui::Frame::group(columns[0].style())
                        .inner_margin(12)
                        .show(&mut columns[0], |ui| {
                            ui.set_width(ui.available_width());
                            self.draw_collection_destination(ui);
                        });
                    columns[0].add_space(8.0);
                    egui::Frame::group(columns[0].style())
                        .inner_margin(12)
                        .show(&mut columns[0], |ui| {
                            ui.set_width(ui.available_width());
                            self.draw_collection_text(ui);
                        });
                    egui::Frame::group(columns[1].style())
                        .inner_margin(12)
                        .show(&mut columns[1], |ui| {
                            ui.set_width(ui.available_width());
                            self.presentation_editor.draw_badge(
                                ui,
                                &mut self.recipe.overrides,
                                &badges,
                            );
                        });
                });
            } else {
                egui::Frame::group(ui.style())
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.draw_collection_destination(ui);
                    });
                ui.add_space(8.0);
                egui::Frame::group(ui.style())
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.presentation_editor.draw_badge(
                            ui,
                            &mut self.recipe.overrides,
                            &badges,
                        );
                    });
                ui.add_space(8.0);
                egui::Frame::group(ui.style())
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.draw_collection_text(ui);
                    });
            }
        });
    }

    fn draw_collection_text(&mut self, ui: &mut egui::Ui) {
        ui.strong("Collections Text");
        let source_label = ui.label("Source");
        ui.add_sized(
            [ui.available_width(), ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut self.recipe.source).desired_width(f32::INFINITY),
        )
        .labelled_by(source_label.id);
        ui.add_space(5.0);
        let custom_text = self.recipe.collection_name.is_some()
            || self.recipe.collection_description.is_some()
            || self.recipe.collection_requirement.is_some();
        egui::CollapsingHeader::new("Custom Text")
            .id_salt(("collection-custom-text", self.recipe_panel_scope()))
            .default_open(custom_text)
            .show(ui, |ui| {
        let mut custom_collection_name = self.recipe.collection_name.is_some();
        if ui
            .checkbox(&mut custom_collection_name, "Separate Collections Name")
            .changed()
        {
            let value = custom_collection_name.then(|| self.recipe.name.clone());
            text_fields::set(&mut self.recipe, OptionalText::CollectionName, value);
        }
        if let Some(value) = &mut self.recipe.collection_name {
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(f32::INFINITY)
                    .hint_text("Name shown only in Collections"),
            );
        }
        let mut custom_collection_description = self.recipe.collection_description.is_some();
        if ui
            .checkbox(
                &mut custom_collection_description,
                "Separate Collections Description",
            )
            .changed()
        {
            let value = custom_collection_description.then(|| self.recipe.flavor.clone());
            text_fields::set(&mut self.recipe, OptionalText::CollectionDescription, value);
        }
        if let Some(value) = &mut self.recipe.collection_description {
            ui.add(
                egui::TextEdit::multiline(value)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text("Description shown only in Collections"),
            );
        }
        let mut collection_requirement = self.recipe.collection_requirement.is_some();
        if ui
                .checkbox(&mut collection_requirement, "Collections Requirement Line")
                .on_hover_text(
                    "Writes the collectible-display requirement/warning text. Reacquisition remains enabled. Leave this disabled for the normal blank line.",
                )
                .changed()
            {
                let value = collection_requirement.then(|| "Collection requirement".to_owned());
                text_fields::set(&mut self.recipe, OptionalText::CollectionRequirement, value);
            }
        if let Some(value) = &mut self.recipe.collection_requirement {
            ui.add(
                egui::TextEdit::singleline(value)
                    .desired_width(f32::INFINITY)
                    .hint_text("Collections requirement or warning"),
            );
        }
        })
        .header_response
        .on_hover_text("Turning off optional text also removes its translations.");
        ui.add_space(5.0);
        self.draw_locale_text_overrides(ui, TextSection::Collections);
    }

    pub(super) fn draw_appearance_workspace(&mut self, ui: &mut egui::Ui) {
        ui.heading("Icon & Colors");
        ui.label("These follow the appearance on the Weapon tab unless you choose another source.");
        ui.add_space(8.0);
        if ui.available_width() >= 880.0 {
            ui.columns(2, |columns| {
                self.draw_icon_donor_picker(&mut columns[0]);
                self.draw_render_gear_donor_picker(&mut columns[1]);
            });
        } else {
            self.draw_icon_donor_picker(ui);
            ui.add_space(8.0);
            self.draw_render_gear_donor_picker(ui);
        }
        ui.add_space(12.0);
        ui.separator();
        self.presentation_editor
            .draw_corner(ui, &mut self.recipe.overrides);
        ui.add_space(12.0);
        ui.separator();
        let appearance = self
            .recipe
            .presentation_donor
            .as_ref()
            .unwrap_or(&self.recipe.donor);
        let item_hash = appearance.item_hash.parse_u32().ok();
        let summary =
            item_hash.and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash));
        let name = summary
            .map(|donor| donor.name.as_str())
            .or(appearance.expected_name.as_deref())
            .unwrap_or("Weapon Appearance");
        self.hud_icon_editor.draw(
            ui,
            &mut self.recipe.overrides.hud_icon,
            crate::hud_icon::ui::Appearance {
                packages: &self.packages,
                pattern_index: summary.and_then(|donor| donor.weapon_pattern_index),
                name,
            },
        );
        if self.show_experimental_options {
            let geometry_donor = self.current_geometry_donor();
            let render_gear_donor = self.current_render_gear_donor();
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(5.0);
            egui::CollapsingHeader::new("Technical Appearance Data")
                .id_salt(("appearance-data", self.recipe_panel_scope()))
                .show(ui, |ui| {
                    self.draw_translation_overrides(
                        ui,
                        geometry_donor.as_ref(),
                        render_gear_donor.as_ref(),
                    );
                });
        }
    }

    pub(super) fn draw_identity_workspace(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recipe Identity");
        ui.weak("Identifiers stay the same when you rename a weapon. Use Duplicate to create a separate weapon.");
        ui.add_space(6.0);
        self.draw_recipe_namespace(ui);
        ui.separator();
        ui.add_space(6.0);
        let derived_identity = WeaponCloneIdentity::from_namespace(&self.recipe.namespace).ok();
        let derived_hash = |select: fn(WeaponCloneIdentity) -> u32| {
            HexHash::new(derived_identity.map_or(0, select))
        };
        let type_hash = self
            .recipe
            .identity
            .type_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.type_hash));
        let pattern_global_id_hash = self
            .recipe
            .identity
            .pattern_global_id_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.pattern_global_id_hash));
        let collection_name_hash = self
            .recipe
            .identity
            .collection_name_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.collection_name_hash));
        let collection_description_hash = self
            .recipe
            .identity
            .collection_description_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.collection_description_hash));
        let inventory_hint_hash = self
            .recipe
            .identity
            .inventory_hint_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.inventory_hint_hash));
        let collection_requirement_hash = self
            .recipe
            .identity
            .collection_requirement_hash
            .clone()
            .unwrap_or_else(|| derived_hash(|identity| identity.collection_requirement_hash));
        let icon_definition_hash = self
            .recipe
            .identity
            .item_hash
            .parse_u32()
            .ok()
            .and_then(|item_hash| {
                self.latest_build
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|report| {
                        report
                            .weapons
                            .iter()
                            .find(|weapon| weapon.item_hash == item_hash)
                    })
            })
            .map(|weapon| format!("0x{:08X}", weapon.icon_definition_hash));
        let icon_definition_is_available = icon_definition_hash.is_some();
        let icon_definition_hash =
            icon_definition_hash.unwrap_or_else(|| "Assigned During Build".to_owned());
        let fields = [
            ("Item", self.recipe.identity.item_hash.to_string(), true),
            (
                "Collectible",
                self.recipe.identity.collectible_hash.to_string(),
                true,
            ),
            ("Unlock", self.recipe.identity.unlock_hash.to_string(), true),
            (
                "Pattern Global ID",
                pattern_global_id_hash.to_string(),
                true,
            ),
            (
                "Name String",
                self.recipe.identity.name_hash.to_string(),
                true,
            ),
            ("Type String", type_hash.to_string(), true),
            (
                "Flavor String",
                self.recipe.identity.flavor_hash.to_string(),
                true,
            ),
            (
                "Source String",
                self.recipe.identity.source_hash.to_string(),
                true,
            ),
            (
                "Collection Name String",
                collection_name_hash.to_string(),
                true,
            ),
            (
                "Collection Description String",
                collection_description_hash.to_string(),
                true,
            ),
            (
                "Inventory Hint String",
                inventory_hint_hash.to_string(),
                true,
            ),
            (
                "Collection Requirement String",
                collection_requirement_hash.to_string(),
                true,
            ),
            (
                "Icon Definition",
                icon_definition_hash,
                icon_definition_is_available,
            ),
        ];
        if ui.available_width() >= 760.0 {
            ui.columns(2, |columns| {
                draw_identity_group(
                    &mut columns[0],
                    "Game Records",
                    fields[..4].iter().chain(fields[12..].iter()),
                );
                draw_identity_group(&mut columns[1], "Text References", fields[4..12].iter());
            });
        } else {
            draw_identity_group(
                ui,
                "Game Records",
                fields[..4].iter().chain(fields[12..].iter()),
            );
            ui.add_space(8.0);
            draw_identity_group(ui, "Text References", fields[4..12].iter());
        }
    }

    fn draw_locale_text_overrides(&mut self, ui: &mut egui::Ui, section: TextSection) {
        let used = self
            .recipe
            .locale_overrides
            .iter()
            .map(|locale| locale.locale_index)
            .collect::<BTreeSet<_>>();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Translations");
            ui.add_enabled_ui(used.len() < text_fields::LANGUAGES.len(), |ui| {
                ui.menu_button("Add Language", |ui| {
                    for (index, language) in text_fields::LANGUAGES.iter().enumerate() {
                        let locale_index = index as u8;
                        if !used.contains(&locale_index) && ui.button(*language).clicked() {
                            self.recipe.locale_overrides.push(WeaponLocaleTextRecipe {
                                locale_index,
                                ..Default::default()
                            });
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        let primary = [
            ("Weapon Name", Some(self.recipe.name.clone()), false),
            ("Flavor Text", Some(self.recipe.flavor.clone()), true),
            ("Collections Source", Some(self.recipe.source.clone()), true),
            ("Item-Type Label", self.recipe.type_name.clone(), false),
            (
                "Collections Name",
                self.recipe.collection_name.clone(),
                false,
            ),
            (
                "Collections Description",
                self.recipe.collection_description.clone(),
                true,
            ),
            ("Inventory Hint", self.recipe.inventory_hint.clone(), false),
            (
                "Collections Requirement",
                self.recipe.collection_requirement.clone(),
                false,
            ),
        ];
        let mut remove = None;
        for (index, locale) in self.recipe.locale_overrides.iter_mut().enumerate() {
            egui::CollapsingHeader::new(text_fields::language(locale.locale_index))
                .id_salt(("locale-text-override", section, index))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("translation-language", section, index))
                            .selected_text(text_fields::language(locale.locale_index))
                            .show_ui(ui, |ui| {
                                for (candidate, name) in text_fields::LANGUAGES.iter().enumerate() {
                                    let candidate = candidate as u8;
                                    if candidate == locale.locale_index
                                        || !used.contains(&candidate)
                                    {
                                        ui.selectable_value(
                                            &mut locale.locale_index,
                                            candidate,
                                            *name,
                                        );
                                    }
                                }
                            });
                        if ui.button(section.clear_label()).clicked() {
                            remove = Some(index);
                        }
                    });
                    let fields = [
                        &mut locale.name,
                        &mut locale.flavor,
                        &mut locale.source,
                        &mut locale.type_name,
                        &mut locale.collection_name,
                        &mut locale.collection_description,
                        &mut locale.inventory_hint,
                        &mut locale.collection_requirement,
                    ];
                    for (field, ((label, fallback, multiline), value)) in
                        primary.iter().zip(fields).enumerate()
                    {
                        if section.includes(field)
                            && let Some(fallback) = fallback
                        {
                            draw_optional_locale_text_field(ui, label, value, fallback, *multiline);
                        }
                    }
                });
        }
        if let Some(index) = remove {
            text_fields::clear_locale(&mut self.recipe, index, section);
        }
        let unique = self
            .recipe
            .locale_overrides
            .iter()
            .map(|locale| locale.locale_index)
            .collect::<BTreeSet<_>>();
        if unique.len() != self.recipe.locale_overrides.len() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Each language can appear only once.",
            );
        }
    }

    pub(super) fn draw_investment_stats_panel(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        if let Some(donor) = donor {
            // Rendering preserves explicit saved overrides; normalize only on an edit.
            let donor_exact = self.recipe.overrides.investment_stats.is_empty()
                && self.recipe.overrides.removed_investment_stats.is_empty();
            ui.horizontal_wrapped(|ui| {
                ui.heading("Weapon Stats");
                draw_authoring_info_icon(
                    ui,
                    "Raw Value is saved to the weapon. Preview shows the scaled value, such as RPM. Added stats only appear in game when the weapon and its stat group support them.",
                );
                if ui
                    .add_enabled(!donor_exact, egui::Button::new("Reset Stats"))
                    .on_hover_text("Reset every investment value to the gameplay donor")
                    .clicked()
                {
                    self.recipe.overrides.investment_stats.clear();
                    self.recipe.overrides.removed_investment_stats.clear();
                }
            });
            ui.add_space(4.0);
            egui::CollapsingHeader::new("Stat Options · Scaling and Internal Values")
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(&mut self.show_internal_stats, "Show Internal Stats")
                        .on_hover_text("Show package-level Attack, Power and unnamed rows. Hidden rows are preserved.");
                    self.draw_stat_group_picker(ui, Some(donor));
                });
            draw_investment_stats(
                ui,
                &mut self.recipe.overrides.investment_stats,
                &mut self.recipe.overrides.removed_investment_stats,
                donor,
                self.show_internal_stats,
            );
        } else {
            ui.heading("Weapon Stats");
            ui.label("Load the donor catalog to edit definition-aware stats.");
        }
    }

    pub(super) fn draw_socket_columns_panel(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        if let Some(donor) = donor {
            let show_experimental_options = self.show_experimental_options;
            let has_authored_columns = !self.recipe.overrides.socket_columns.is_empty()
                || !self.recipe.overrides.socket_plug_variants.is_empty();
            ui.horizontal_wrapped(|ui| {
                ui.heading("Perks & Sockets");
                if ui
                    .button("Use Custom Perk…")
                    .on_hover_text("Add a saved custom perk to this weapon.")
                    .clicked()
                {
                    self.perk_workbench.open = true;
                }
                self.draw_socket_options(ui, has_authored_columns);
            });
            ui.label("First choice starts equipped. Right-click an extra choice to make it default.")
                .on_hover_text("Use separate sockets for perks that should work together. Click a socket role to change its native type. Existing inventory copies retain their saved choices.");
            if self.show_plug_safety_warnings {
                draw_plug_safety_warning(ui, self.plug_selection_mode);
            }
            let Self {
                catalog,
                recipe,
                plug_queries,
                socket_choice_pages,
                recipe_library,
                plug_selection_mode,
                show_plug_safety_warnings,
                show_technical_socket_rows,
                perk_request,
                log,
                ..
            } = self;
            if let Some(catalog) = catalog.as_ref() {
                draw_socket_pickers(
                    ui,
                    SocketPickerContext {
                        catalog,
                        recipe_library: recipe_library.as_ref(),
                        recipe,
                        queries: plug_queries,
                        pages: socket_choice_pages,
                        plug_selection_mode,
                        show_plug_safety_warnings: *show_plug_safety_warnings,
                        show_experimental_options,
                        show_technical_rows: show_technical_socket_rows,
                        perk_request,
                        donor,
                        log,
                    },
                );
            }
        } else {
            ui.heading("Perks & Sockets");
            ui.label("Load the donor catalog to use Sundial's compatible-plug picker.");
        }
    }

    pub(super) fn draw_socket_options(&mut self, ui: &mut egui::Ui, has_authored_columns: bool) {
        draw_plug_safety_selector(
            ui,
            "parhelion-socket-column-plug-safety",
            &mut self.plug_selection_mode,
        );
        ui.menu_button("Socket Options", |ui| {
                    if self.show_experimental_options {
                        ui.checkbox(&mut self.show_technical_socket_rows, "Show Socket Details")
                            .on_hover_text("Show additional socket settings beneath each plug column.");
                        ui.separator();
                    }
                    if ui.add_enabled(has_authored_columns, egui::Button::new("Restore All Base Sockets"))
                        .on_hover_text("Remove every explicit socket and custom perk override, then return to the base weapon's collection roll")
                        .clicked() {
                        self.recipe.overrides.socket_columns.clear();
                        self.recipe.overrides.socket_plug_variants.clear();
                        self.perk_request = None;
                        self.plug_queries.clear();
                        ui.close_menu();
                    }
                });
    }
}
