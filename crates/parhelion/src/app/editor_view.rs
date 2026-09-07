//! Everyday weapon editor and identity/appearance views.
use super::*;

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
            .default_open(false)
            .show(ui, |ui| {
            ui.label("Source text");
            ui.add(egui::TextEdit::singleline(&mut self.recipe.source).desired_width(f32::INFINITY));
            let mut custom_type = self.recipe.type_name.is_some();
            if ui
                .checkbox(&mut custom_type, "Custom item-type label")
                .on_hover_text(
                    "Writes the independent item-type localization reference at item-string offset 0x90. Disable this to preserve the gameplay donor's label.",
                )
                .changed()
            {
                self.recipe.type_name = custom_type.then(|| {
                    donor
                        .map(|donor| donor.summary.type_name.clone())
                        .unwrap_or_default()
                });
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
            let mut custom_collection_name = self.recipe.collection_name.is_some();
            if ui
                .checkbox(&mut custom_collection_name, "Separate Collections name")
                .changed()
            {
                self.recipe.collection_name = custom_collection_name.then(|| self.recipe.name.clone());
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
                    "Separate Collections description",
                )
                .changed()
            {
                self.recipe.collection_description =
                    custom_collection_description.then(|| self.recipe.flavor.clone());
            }
            if let Some(value) = &mut self.recipe.collection_description {
                ui.add(
                    egui::TextEdit::multiline(value)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2)
                        .hint_text("Description shown only in Collections"),
                );
            }
            let mut inventory_hint = self.recipe.inventory_hint.is_some();
            if ui
                .checkbox(&mut inventory_hint, "Inventory acquisition hint")
                .on_hover_text(
                    "Optional inventory tooltip acquisition text, separate from Collections Source. This is display text only: authored weapons can be reacquired from Collections.",
                )
                .changed()
            {
                self.recipe.inventory_hint = inventory_hint.then(|| {
                    "Curated roll: This weapon can be reacquired from Collections.".to_owned()
                });
            }
            if let Some(value) = &mut self.recipe.inventory_hint {
                ui.add(
                    egui::TextEdit::singleline(value)
                        .desired_width(f32::INFINITY)
                        .hint_text("Inventory tooltip acquisition line"),
                );
            }
            let mut collection_requirement = self.recipe.collection_requirement.is_some();
            if ui
                .checkbox(&mut collection_requirement, "Collections requirement line")
                .on_hover_text(
                    "Writes the collectible-display requirement/warning text. Reacquisition remains enabled; leave this disabled for the normal blank line.",
                )
                .changed()
            {
                self.recipe.collection_requirement =
                    collection_requirement.then(|| "Collection requirement".to_owned());
            }
            if let Some(value) = &mut self.recipe.collection_requirement {
                ui.add(
                    egui::TextEdit::singleline(value)
                        .desired_width(f32::INFINITY)
                        .hint_text("Collections requirement or warning"),
                );
            }
            ui.separator();
            self.draw_locale_text_overrides(ui);
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
                "Investment fields remain rooted in the gameplay donor. The runtime baseline supplies the complete native weapon entity. Ammo classification, inventory slot, geometry, icon definition, and render gear are stored separately, but native runtime components can still be coupled. Test component combinations in-game; package validation cannot establish their runtime compatibility.",
            );
        });
        self.draw_runtime_source_summary(ui);
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
        self.hud_icon_editor
            .draw(ui, &mut self.recipe.overrides.hud_icon);
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
        ui.weak("Stable identifiers for this weapon and its game data. Rename on the Weapon tab; duplicate to create a separate weapon.");
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
            icon_definition_hash.unwrap_or_else(|| "Build to allocate".to_owned());
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

    pub(super) fn draw_locale_text_overrides(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Locale payload overrides");
            draw_authoring_info_icon(
                ui,
                "Sparse replacements for the 13 native localization payloads. Indices deliberately follow package order because this client data does not prove human language names for every slot.",
            );
            let used = self
                .recipe
                .locale_overrides
                .iter()
                .map(|locale| locale.locale_index)
                .collect::<BTreeSet<_>>();
            let next = (0_u8..13).find(|index| !used.contains(index));
            if ui
                .add_enabled(next.is_some(), egui::Button::new("+ Add locale"))
                .on_disabled_hover_text("All 13 locale payloads already have an override.")
                .clicked()
                && let Some(locale_index) = next
            {
                self.recipe.locale_overrides.push(WeaponLocaleTextRecipe {
                    locale_index,
                    ..Default::default()
                });
            }
        });

        let primary = [
            ("Weapon Name", Some(self.recipe.name.clone()), false),
            ("Flavor Text", Some(self.recipe.flavor.clone()), true),
            ("Source text", Some(self.recipe.source.clone()), true),
            ("Item-type label", self.recipe.type_name.clone(), false),
            (
                "Collections name",
                self.recipe.collection_name.clone(),
                false,
            ),
            (
                "Collections description",
                self.recipe.collection_description.clone(),
                true,
            ),
            ("Inventory hint", self.recipe.inventory_hint.clone(), false),
            (
                "Collections requirement",
                self.recipe.collection_requirement.clone(),
                false,
            ),
        ];
        let mut remove = None;
        for (index, locale) in self.recipe.locale_overrides.iter_mut().enumerate() {
            let response =
                egui::CollapsingHeader::new(format!("Locale payload {}", locale.locale_index))
                    .id_salt(("locale-text-override", index))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Payload index");
                            ui.add(egui::DragValue::new(&mut locale.locale_index).range(0..=12));
                            if ui.button("Remove locale").clicked() {
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
                        for ((label, fallback, multiline), value) in primary.iter().zip(fields) {
                            if let Some(fallback) = fallback {
                                draw_optional_locale_text_field(
                                    ui, label, value, fallback, *multiline,
                                );
                            }
                        }
                    });
            response.header_response.on_hover_text(format!(
                "Overrides only localization payload {}; all unselected fields use the primary recipe text.",
                locale.locale_index
            ));
        }
        if let Some(index) = remove {
            self.recipe.locale_overrides.remove(index);
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
                "Each locale payload index can be overridden only once.",
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
                    "Values are saved to the weapon's stat data. The game may scale them for display; RPM is a common example. Add stat lets you use stat definitions from installed weapons, but a stat may not appear in game unless the selected stat group and weapon support it.",
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
            egui::CollapsingHeader::new("Stat options · scaling and internal values")
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(&mut self.show_internal_stats, "Show internal stats")
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
                if ui.button("Custom Perks…")
                    .on_hover_text("Authoring is coming soon. Reuse existing custom perks from saved recipes.")
                    .clicked() {
                    self.private_perk_socket = Some(self.recipe.overrides.socket_plug_variants.first()
                        .map_or(0, |variant| usize::from(variant.socket_index)));
                }
                ui.menu_button("Socket Options", |ui| {
                    draw_plug_safety_selector(ui, "parhelion-socket-column-plug-safety", &mut self.plug_selection_mode);
                    if show_experimental_options {
                        ui.checkbox(&mut self.show_technical_socket_rows, "Show native rows")
                            .on_hover_text("Show the complete native socket-row fields beneath each plug column.");
                    }
                    ui.separator();
                    if ui.add_enabled(has_authored_columns, egui::Button::new("Restore all base sockets"))
                        .on_hover_text("Remove every explicit socket and custom perk override, then return to the base weapon's collection roll")
                        .clicked() {
                        self.recipe.overrides.socket_columns.clear();
                        self.recipe.overrides.socket_plug_variants.clear();
                        self.perk_editor = None;
                        self.private_perk_socket = None;
                        self.plug_queries.clear();
                        ui.close_menu();
                    }
                });
            });
            ui.label("First choice starts equipped. Extra choices are alternatives.")
                .on_hover_text("Use separate sockets for perks that should work together. Click a socket role to change its native type. Existing inventory copies retain their saved choices.");
            if self.show_plug_safety_warnings {
                draw_plug_safety_warning(ui, self.plug_selection_mode);
            }
            let Self {
                catalog,
                recipe,
                plug_queries,
                socket_choice_pages,
                plug_selection_mode,
                show_plug_safety_warnings,
                show_technical_socket_rows,
                private_perk_socket,
                log,
                ..
            } = self;
            if let Some(catalog) = catalog.as_ref() {
                draw_socket_pickers(
                    ui,
                    SocketPickerContext {
                        catalog,
                        recipe,
                        queries: plug_queries,
                        pages: socket_choice_pages,
                        plug_selection_mode,
                        show_plug_safety_warnings: *show_plug_safety_warnings,
                        show_experimental_options,
                        show_technical_rows: show_technical_socket_rows,
                        private_perk_socket,
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
}
