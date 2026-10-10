//! Everyday weapon editor and identity/appearance views.
use super::*;

mod text_fields;
use text_fields::{OptionalText, TextSection};

impl PackageAuthoringApp {
    pub(super) fn draw_core_recipe_editor(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let donor = self.current_donor();
        let model = self.catalog.is_some() && donor.is_some();
        let column = egui::Layout::top_down(egui::Align::Min);
        if let Some(donor_width) = workbench_left_column_width(ui.available_width()) {
            let spacing = ui.spacing().item_spacing.x;
            let beside = ui.available_width() - donor_width - spacing;
            let preview_width = model
                .then(|| preview_column_width(beside, spacing))
                .flatten();
            let definition_width = beside - preview_width.map_or(0.0, |width| width + spacing);
            ui.horizontal_top(|ui| {
                let donors = ui
                    .allocate_ui_with_layout(egui::vec2(donor_width, 0.0), column, |ui| {
                        ui.set_width(donor_width);
                        self.draw_donor_section(ui);
                    })
                    .response
                    .rect;
                ui.allocate_ui_with_layout(egui::vec2(definition_width, 0.0), column, |ui| {
                    ui.set_width(definition_width);
                    self.draw_definition_panel(ui, donor.as_ref());
                    // Without the room for a column of its own, the preview goes under the text.
                    if model && preview_width.is_none() {
                        ui.add_space(8.0);
                        self.draw_weapon_preview(ui, None);
                    }
                });
                // The preview ends with the donor column, which keeps its height while Text
                // Presentation opens and closes.
                if let Some(width) = preview_width {
                    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), column, |ui| {
                        ui.set_width(width);
                        self.draw_weapon_preview(ui, Some(donors.height()));
                    });
                }
            });
        } else {
            self.draw_donor_section(ui);
            ui.separator();
            self.draw_definition_panel(ui, donor.as_ref());
            if model {
                ui.add_space(8.0);
                self.draw_weapon_preview(ui, None);
            }
        }
        let donor = donor.as_ref();
        if donor.is_none() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Gameplay donor not found in the catalog.",
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

    /// The recipe's weapon as the game draws it: the appearance donor's model in the recipe's
    /// colors with its ornament, turning on drag. Beside the weapon's text it ends where the
    /// donor column ends, `band`, within its bounds. Its corner opens the model viewer, which
    /// has the full tools.
    fn draw_weapon_preview(&self, ui: &mut egui::Ui, band: Option<f32>) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        let appearance = donor_view::preview::loadout(catalog, &self.recipe)
            .map(|loadout| catalog.preview_appearance(&loadout));
        let id = egui::Id::new("weapon-preview");
        let width = ui.available_width();
        let height = preview_height(width, band);
        // The appearance's icon stands in until its model is read.
        let icon = self
            .recipe
            .presentation_donor
            .as_ref()
            .unwrap_or(&self.recipe.donor)
            .item_hash
            .parse_u32()
            .ok()
            .and_then(|hash| catalog.item_icon(ui.ctx(), hash));
        sundial::ui::model_preview::still::placeholder(ui.ctx(), id, icon);
        let response = sundial::ui::model_preview::still::show(
            ui,
            id,
            &self.packages,
            appearance.clone(),
            &[],
            egui::vec2(width, height),
        );
        if let Some(appearance) = appearance {
            sundial::ui::model_preview::pop_out(
                ui,
                id,
                response.rect,
                &self.packages,
                (appearance, &self.recipe.name),
            );
        }
    }

    pub(super) fn draw_weapon_name(&mut self, ui: &mut egui::Ui, label_width: f32) {
        let mut edited_name = self
            .invalid_weapon_name
            .as_ref()
            .map_or_else(|| self.recipe.name.clone(), |(text, _)| text.clone());
        let response = ui
            .horizontal(|ui| {
                let label = ui.add_sized(
                    [label_width, ui.spacing().interact_size.y],
                    egui::Label::new(format!("{} Name", self.recipe.kind.label())),
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
                draw_authoring_info_icon(ui, "Custom identity hashes. Renaming replaces them.");
            }
        });
        let mut namespace = self.recipe.namespace.clone();
        ui.add_sized(
            [ui.available_width(), ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut namespace).interactive(false),
        )
        .on_hover_text("Set by the weapon name. Renaming changes every hash.");
    }

    pub(super) fn draw_definition_panel(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        let type_donor = self.recipe.type_donor_hash();
        let shown_type = self
            .donor_summaries
            .iter()
            .find(|summary| summary.hash == type_donor)
            .map(|summary| summary.type_name.clone())
            .or_else(|| donor.map(|donor| donor.summary.type_name.clone()));
        self.draw_item_text(ui, shown_type.as_deref());
        self.draw_weapon_profile(ui, donor);
    }

    /// Name, flavor text and the folded text presentation every kind shares.
    pub(super) fn draw_item_text(&mut self, ui: &mut egui::Ui, inherited_type: Option<&str>) {
        let panel_scope = self.recipe_panel_scope();
        let label_width = text_label_width(ui, self.recipe.kind);
        self.draw_weapon_name(ui, label_width);
        ui.horizontal_top(|ui| {
            let label = ui.add_sized(
                [label_width, ui.spacing().interact_size.y],
                egui::Label::new("Flavor Text"),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.recipe.flavor)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2),
            )
            .labelled_by(label.id);
        });

        let noun = self.recipe.kind.noun();
        egui::CollapsingHeader::new("Text Presentation")
            .id_salt(("parhelion-text-presentation", panel_scope.as_str()))
            .default_open(false)
            .show(ui, |ui| {
                let mut custom_type = self.recipe.type_name.is_some();
                if ui
                    .checkbox(&mut custom_type, "Custom Item-Type Label")
                    .on_hover_text("Off uses the default item type.")
                    .changed()
                {
                    let value = custom_type.then(|| inherited_type.unwrap_or_default().to_owned());
                    text_fields::set(&mut self.recipe, OptionalText::TypeName, value);
                }
                if let Some(type_name) = &mut self.recipe.type_name {
                    ui.add(
                        egui::TextEdit::singleline(type_name)
                            .desired_width(f32::INFINITY)
                            .hint_text("Item type shown by the game"),
                    );
                } else if let Some(inherited_type) = inherited_type {
                    ui.label(format!("Inherited: {inherited_type}"));
                }

                ui.separator();
                let mut inventory_hint = self.recipe.inventory_hint.is_some();
                if ui
                    .checkbox(&mut inventory_hint, "Inventory Acquisition Hint")
                    .on_hover_text("Inventory tooltip text only. Separate from Collections Source.")
                    .changed()
                {
                    let value = inventory_hint.then(|| {
                        format!("Curated roll: This {noun} can be reacquired from Collections.")
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
                // A shader and an emblem have no lore tab.
                if self.recipe.kind.has_lore_tab() {
                    self.presentation_editor.draw_lore(
                        ui,
                        &mut self.recipe.overrides,
                        &self.packages,
                        (
                            self.recipe.donor.item_hash.parse_u32().ok(),
                            self.recipe.kind,
                        ),
                    );
                    ui.separator();
                }
                self.draw_locale_text_overrides(ui, TextSection::Weapon);
            });
    }

    /// Slot, damage, ammo, rarity and power cap for a weapon.
    fn draw_weapon_profile(&mut self, ui: &mut egui::Ui, donor: Option<&WeaponDonor>) {
        ui.add_space(4.0);
        // A behavior copied from Hard Light or Borealis owns the damage type.
        let damage_locked = self
            .recipe
            .overrides
            .additional_behaviors
            .iter()
            .filter_map(|chosen| crate::weapon::behavior::behavior(&chosen.behavior))
            .any(crate::weapon::behavior::source_switches_element);
        let variable_available =
            donor.is_some_and(|gameplay| variable_damage_supported(&gameplay.summary));
        let mut inventory_slot_changed = false;
        let mut damage_changed = false;
        style::tiles(ui, |ui, width| {
            for field in 0..6 {
                style::tile_column(ui, (width, ("weapon-field", field)), |column| match field {
                    0 | 1 => {
                        let changed = draw_combat_profile_control(
                            column,
                            &mut self.recipe.overrides,
                            donor,
                            field == 0,
                            variable_available,
                            damage_locked,
                        );
                        inventory_slot_changed |= field == 0 && changed;
                        damage_changed |= field == 1 && changed;
                    }
                    2 => draw_ammo_type_control(column, &mut self.recipe.overrides, donor),
                    3 => draw_rarity_control(column, &mut self.recipe.overrides, donor),
                    4 => draw_power_cap_control(
                        column,
                        &mut self.recipe.overrides,
                        donor,
                        self.catalog.as_ref(),
                    ),
                    _ => self.draw_behavior_cell(column, donor),
                });
            }
        });
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
        ui.heading("Gameplay")
            .on_hover_text("How the weapon fires and behaves, and the parts it takes from others");
        ui.add_space(6.0);
        // Parts beside the Barrel's settings when both fit, the projectile's cards under them.
        let column = egui::Layout::top_down(egui::Align::Min);
        if let Some(parts_width) = workbench_left_column_width(ui.available_width()) {
            let barrel_width = ui.available_width() - parts_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(parts_width, 0.0), column, |ui| {
                    ui.set_width(parts_width);
                    self.draw_gameplay_parts(ui, donor);
                });
                ui.allocate_ui_with_layout(egui::vec2(barrel_width, 0.0), column, |ui| {
                    ui.set_width(barrel_width);
                    self.draw_barrel_controls(ui);
                });
            });
        } else {
            self.draw_gameplay_parts(ui, donor);
            self.draw_barrel_controls(ui);
        }
        self.draw_fired_projectile(ui);
        if self.show_experimental_options {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);
            self.draw_gameplay_technical(ui, donor);
        }
    }

    pub(super) fn draw_collections_workspace(&mut self, ui: &mut egui::Ui) {
        let mut badges = self
            .recipe_entries
            .iter()
            .filter_map(|entry| Some((entry.badge.clone()?, entry.path.clone())))
            .collect::<Vec<_>>();
        badges.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        badges.dedup_by(|a, b| a.0 == b.0);
        let class_armor = self.recipe.kind == ItemKind::Armor;
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
                                class_armor,
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
                            class_armor,
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
                let mut custom_collection_description =
                    self.recipe.collection_description.is_some();
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
                    .on_hover_text("Display text only. Reacquisition stays enabled.")
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
        #[cfg(feature = "d2-model-importer")]
        let imported = self.recipe.overrides.imported_graph.is_some();
        #[cfg(not(feature = "d2-model-importer"))]
        let imported = false;
        // A weapon's model shows in Placement below. Other items, and imported models, which
        // Placement does not draw, keep the preview beside the heading.
        let previewed = self.recipe.kind.is_weapon() && !imported;
        ui.horizontal_wrapped(|ui| {
            ui.heading("Appearance");
            if !previewed {
                self.draw_appearance_preview(ui);
            }
        });
        // Where the model comes from, on one line: an ornament, or an imported model. Without
        // either there is nothing to choose, and the row stays out.
        #[cfg(feature = "d2-model-importer")]
        let importing = self.importer.enabled;
        #[cfg(not(feature = "d2-model-importer"))]
        let importing = false;
        ui.add_space(6.0);
        if importing || self.appearance_ornaments_offered() {
            ui.horizontal(|ui| {
                let width = Self::appearance_label_width(ui);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.weak("Model").on_hover_text(
                            "Where the model comes from: an ornament, or an imported model",
                        );
                    },
                );
                self.draw_appearance_ornaments(ui);
                #[cfg(feature = "d2-model-importer")]
                self.draw_imported_model_picker(ui);
            });
        }
        // An import an importer fix has since made unsafe or wrong to install.
        #[cfg(feature = "d2-model-importer")]
        if self
            .recipe
            .overrides
            .imported_graph
            .as_ref()
            .is_some_and(parhelion_import::GraphReference::needs_reimport)
        {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(Self::appearance_label_width(ui));
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Made by an older importer. Import it again.",
                );
            });
        }
        // How the model is held, fired and reloaded, beside where it comes from.
        if self.recipe.kind.is_weapon() {
            self.draw_animation_part(ui);
        }
        ui.add_space(10.0);
        // The four smaller parts side by side when they fit, two by two, or stacked.
        let width = ui.available_width();
        let per_row = if width >= 1240.0 {
            4
        } else if width >= 640.0 {
            2
        } else {
            1
        };
        for row in [0, 1, 2, 3].chunks(per_row) {
            if per_row == 1 {
                for &tile in row {
                    self.draw_appearance_tile(ui, tile);
                    ui.add_space(10.0);
                }
                continue;
            }
            ui.columns(per_row, |columns| {
                for (column, &tile) in columns.iter_mut().zip(row) {
                    self.draw_appearance_tile(column, tile);
                }
            });
            ui.add_space(10.0);
        }
        if self.recipe.kind == ItemKind::Weapon {
            // Offered only while the D2 importer is turned on.
            #[cfg(feature = "d2-model-importer")]
            if self.importer.enabled {
                self.draw_shader_glow(ui);
            }
            self.draw_type_source(ui);
            ui.add_space(8.0);
        }
        ui.separator();
        ui.add_space(8.0);
        if self.recipe.kind.is_weapon() {
            self.draw_appearance_placement(ui);
        }
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

    /// Whether equipped shaders with an animated glow also light the weapon's glowing parts.
    /// New weapons start with it on. An imported model keeps its own materials, so it is off there.
    #[cfg(feature = "d2-model-importer")]
    fn draw_shader_glow(&mut self, ui: &mut egui::Ui) {
        let imported = self.recipe.overrides.imported_graph.is_some();
        ui.add_enabled(
            !imported,
            egui::Checkbox::new(
                &mut self.recipe.overrides.shader_glow,
                "Shaders Light Glowing Parts",
            ),
        )
        .on_hover_text(
            "Shaders with an animated glow also light this weapon's glowing parts, such as sights \
             and vents. Other parts stay as they are. Test in game",
        )
        .on_disabled_hover_text("Imported models keep their own materials.");
    }

    /// Whether a weapon wearing another type's appearance shows that type, and files under it in
    /// Collections, or keeps the base weapon's. Shown only when the two types differ.
    fn draw_type_source(&mut self, ui: &mut egui::Ui) {
        let summary = |hash: Option<u32>| {
            hash.and_then(|hash| self.donor_summaries.iter().find(|d| d.hash == hash))
                .map(|summary| summary.type_name.clone())
                .filter(|name| !name.trim().is_empty())
        };
        let base = summary(self.recipe.donor.item_hash.parse_u32().ok());
        let look = summary(
            self.recipe
                .presentation_donor
                .as_ref()
                .and_then(|donor| donor.item_hash.parse_u32().ok()),
        );
        let (Some(base), Some(look)) = (base, look) else {
            return;
        };
        if base == look {
            return;
        }
        ui.checkbox(
            &mut self.recipe.overrides.base_type,
            format!("Keep {base} Type"),
        )
        .on_hover_text(format!(
            "Shows {base} as the weapon type and files it under {base} in Collections. Off, it \
             takes {look} from the appearance"
        ));
    }

    /// One of the Appearance tab's four smaller parts, by position.
    fn draw_appearance_tile(&mut self, ui: &mut egui::Ui, tile: usize) {
        match tile {
            0 => self.draw_icon_donor_picker(ui),
            1 => self.draw_render_gear_donor_picker(ui),
            2 => self
                .presentation_editor
                .draw_corner(ui, &mut self.recipe.overrides),
            _ => self.draw_appearance_hud_icon(ui),
        }
    }

    fn draw_appearance_hud_icon(&mut self, ui: &mut egui::Ui) {
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
    }

    pub(super) fn draw_identity_workspace(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recipe Identity");
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
        let build = self
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
            });
        let game_fields = game_identity_fields(&self.recipe, &pattern_global_id_hash);
        let text_fields = [
            IdentityField::new("Name String", self.recipe.identity.name_hash.to_string()),
            IdentityField::new("Type String", type_hash.to_string()),
            IdentityField::new(
                "Flavor String",
                self.recipe.identity.flavor_hash.to_string(),
            ),
            IdentityField::new(
                "Source String",
                self.recipe.identity.source_hash.to_string(),
            ),
            IdentityField::new("Collection Name String", collection_name_hash.to_string()),
            IdentityField::new(
                "Collection Description String",
                collection_description_hash.to_string(),
            ),
            IdentityField::new("Inventory Hint String", inventory_hint_hash.to_string()),
            IdentityField::new(
                "Collection Requirement String",
                collection_requirement_hash.to_string(),
            ),
        ];
        let package_fields = package_identity_fields(self.recipe.kind, build);
        let mut presentation_fields = Vec::new();
        if self.recipe.overrides.lore.is_some() {
            presentation_fields.extend([
                IdentityField::new(
                    "Lore Entry",
                    format!(
                        "0x{:08X}",
                        crate::presentation::text_hash(&self.recipe.namespace, "lore-entry")
                    ),
                ),
                IdentityField::new(
                    "Lore Text",
                    format!(
                        "0x{:08X}",
                        crate::presentation::text_hash(&self.recipe.namespace, "lore")
                    ),
                ),
            ]);
        }
        if let Some(badge) = &self.recipe.overrides.badge {
            let badge_hash = |field: &str| crate::presentation::text_hash(&badge.name, field);
            presentation_fields.extend([
                IdentityField::new("Badge Icon", format!("0x{:08X}", badge_hash("badge-icon"))),
                IdentityField::new(
                    "Badge Objective",
                    format!("0x{:08X}", badge_hash("badge-objective")),
                ),
                IdentityField::new(
                    "Badge Name String",
                    format!("0x{:08X}", badge_hash("badge-name")),
                ),
                IdentityField::new(
                    "Badge Description String",
                    format!("0x{:08X}", badge_hash("badge-description")),
                ),
            ]);
            for (index, label) in [
                "Badge Node 1",
                "Badge Node 2",
                "Badge Node 3",
                "Badge Node 4",
            ]
            .into_iter()
            .enumerate()
            {
                presentation_fields.push(IdentityField::new(
                    label,
                    format!(
                        "0x{:08X}",
                        crate::presentation::badge_node_hash(&badge.name, index)
                    ),
                ));
            }
            for (index, label) in [
                "Badge Record 1",
                "Badge Record 2",
                "Badge Record 3",
                "Badge Record 4",
            ]
            .into_iter()
            .enumerate()
            {
                presentation_fields.push(IdentityField::new(
                    label,
                    format!("0x{:08X}", badge_hash(&format!("badge-record-{index}"))),
                ));
            }
        }
        if let Some(destination) = self.recipe.overrides.collection_destination
            && destination.stock_exemplar().is_none()
        {
            presentation_fields.extend([
                IdentityField::new(
                    "Collection Page Node",
                    format!("0x{:08X}", destination.hash("node")),
                ),
                IdentityField::new(
                    "Collection Page Objective",
                    format!("0x{:08X}", destination.hash("objective")),
                ),
            ]);
        }
        if ui.available_width() >= 760.0 {
            ui.columns(2, |columns| {
                draw_identity_group(&mut columns[0], "Game Records", game_fields.iter());
                columns[0].add_space(8.0);
                draw_identity_group(&mut columns[0], "Build Locations", package_fields.iter());
                if !presentation_fields.is_empty() {
                    columns[0].add_space(8.0);
                    draw_identity_group(
                        &mut columns[0],
                        "Presentation Records",
                        presentation_fields.iter(),
                    );
                }
                draw_identity_group(&mut columns[1], "Text References", text_fields.iter());
            });
        } else {
            draw_identity_group(ui, "Game Records", game_fields.iter());
            ui.add_space(8.0);
            draw_identity_group(ui, "Build Locations", package_fields.iter());
            if !presentation_fields.is_empty() {
                ui.add_space(8.0);
                draw_identity_group(ui, "Presentation Records", presentation_fields.iter());
            }
            ui.add_space(8.0);
            draw_identity_group(ui, "Text References", text_fields.iter());
        }
        if !self.recipe.overrides.socket_plug_variants.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.heading("Custom Perk Identities");
            if let Some(build) = build {
                for plug in &build.custom_plugs {
                    let title = format!(
                        "Socket {} · Choice {} · {}",
                        plug.socket_index + 1,
                        plug.choice_index + 1,
                        plug.name.as_deref().unwrap_or("Custom Perk")
                    );
                    let mut fields = vec![
                        IdentityField::new("Item", format!("0x{:08X}", plug.item_hash)),
                        IdentityField::new("Item Table Index", plug.item_index.to_string()),
                        IdentityField::new(
                            "Definition Tag",
                            format!("0x{:08X}", plug.definition_hash),
                        ),
                        IdentityField::new("String Tag", format!("0x{:08X}", plug.string_hash)),
                    ];
                    if let Some(hash) = plug.icon_definition_hash {
                        fields.push(IdentityField::new(
                            "Icon Definition Tag",
                            format!("0x{hash:08X}"),
                        ));
                    }
                    if let Some(hash) = plug.name_hash {
                        fields.push(IdentityField::new("Name String", format!("0x{hash:08X}")));
                    }
                    if let Some(hash) = plug.description_hash {
                        fields.push(IdentityField::new(
                            "Description String",
                            format!("0x{hash:08X}"),
                        ));
                    }
                    for perk in &plug.perks {
                        fields.extend([
                            IdentityField::new(
                                format!("Effect {} Definition", perk.source_perk_index),
                                format!("0x{:08X}", perk.perk_hash),
                            ),
                            IdentityField::new(
                                format!("Effect {} Runtime", perk.source_perk_index),
                                format!("0x{:08X}", perk.runtime_key),
                            ),
                        ]);
                    }
                    ui.add_space(6.0);
                    draw_identity_group(ui, &title, fields.iter());
                }
            } else {
                let pending = [IdentityField::assigned_during_build(
                    "Custom Perk Identities",
                )];
                draw_identity_group(ui, "Build Required", pending.iter());
            }
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
                            ui.close();
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
            let reset = ui
                .horizontal_wrapped(|ui| {
                    style::heading(ui, "Weapon Stats", !donor_exact).on_hover_text(
                        "Each row reads its in-game value, such as RPM, then the raw value saved \
                         to the weapon. Added stats need stat group support",
                    );
                    !donor_exact && style::reset_icon(ui, "Reset Weapon Stats")
                })
                .inner;
            if reset {
                self.recipe.overrides.investment_stats.clear();
                self.recipe.overrides.removed_investment_stats.clear();
            }
            ui.add_space(4.0);
            egui::CollapsingHeader::new("Stat Options")
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(&mut self.show_internal_stats, "Show Internal Stats")
                        .on_hover_text(
                            "Shows Attack, Power and unnamed stats. Hidden stats are kept.",
                        );
                    self.draw_stat_group_picker(ui, Some(donor));
                });
            let warnings = self
                .rate_conversion_warning(donor)
                .map(|warning| (super::stat_editor::ROUNDS_PER_MINUTE_HASH, warning))
                .into_iter()
                .collect::<Vec<_>>();
            // In the order the game lists the stats of the group the weapon displays with.
            let order = self
                .recipe
                .overrides
                .stat_group_index
                .or(donor.summary.stat_group_index)
                .zip(self.catalog.as_ref())
                .map_or_else(Vec::new, |(group, catalog)| {
                    catalog.stat_display_order(group)
                });
            draw_investment_stats(
                ui,
                &mut self.recipe.overrides.investment_stats,
                &mut self.recipe.overrides.removed_investment_stats,
                donor,
                (self.show_internal_stats, &order),
                &warnings,
            );
        } else {
            ui.heading("Weapon Stats");
            ui.label("Load the catalog to edit stats.");
        }
    }

    /// Why Rounds Per Minute fires at another weapon type's rates, when the build is known to
    /// make it. The stat translator keeps one table per weapon type, and a swapped runtime's
    /// pattern row names its own type. An appearance whose rig moves keeps the base's table.
    fn rate_conversion_warning(&self, donor: &WeaponDonor) -> Option<String> {
        let base = &donor.summary;
        let index = self.recipe.overrides.weapon_pattern_index?;
        let runtime = self
            .donor_summaries
            .iter()
            .find(|other| other.weapon_pattern_index == Some(index))?;
        (runtime.weapon_translation_group != base.weapon_translation_group
            && runtime.type_name != base.type_name)
            .then(|| {
                format!(
                    "Fires at {} rates from the {} runtime. Preview may differ.",
                    runtime.type_name, runtime.name
                )
            })
    }

    pub(super) fn draw_socket_columns_panel(
        &mut self,
        ui: &mut egui::Ui,
        donor: Option<&WeaponDonor>,
    ) {
        if let Some(donor) = donor {
            // A chosen behavior claims its sockets here rather than at build time, so the list
            // below is the weapon that gets built.
            super::socket_editor::sync_behavior_socket_pins(
                &mut self.recipe,
                &mut self.behavior_pins,
                donor,
            );
            let show_experimental_options = self.show_experimental_options;
            let has_authored_columns = !self.recipe.overrides.socket_columns.is_empty()
                || !self.recipe.overrides.socket_plug_variants.is_empty();
            ui.horizontal_wrapped(|ui| {
                style::heading(ui, "Perks & Sockets", has_authored_columns);
                draw_authoring_info_icon(
                    ui,
                    "The first choice starts equipped. Right-click a choice to make it the \
                     default. Click a role to change it. Perks that work together need separate \
                     sockets. Saved copies keep their choices.",
                );
                self.draw_socket_options(ui, has_authored_columns);
            });
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
            ui.label("Load the catalog to edit sockets.");
        }
    }

    /// What follows the Perks & Sockets heading: the plugs the pickers offer while that is not the
    /// usual scope, Custom Perks, and the section's overflow menu with socket details and Restore
    /// All Base Sockets. Each picker's Plugs Offered dropdown changes the scope, which reads in the
    /// words of the base item, as Dawn's do.
    pub(super) fn draw_socket_options(&mut self, ui: &mut egui::Ui, has_authored_columns: bool) {
        if self.plug_selection_mode != PlugSelectionMode::default() {
            let item = self.recipe.donor.item_hash.parse_u32().ok();
            let scope = match (self.catalog.as_ref(), item) {
                (Some(catalog), Some(item)) => {
                    catalog.plug_scope_label(self.plug_selection_mode, item)
                }
                _ => self.plug_selection_mode.label().to_owned(),
            };
            let badge = style::badge(ui, &scope).on_hover_text("Plugs Offered");
            style::named_control(badge, format!("Plugs Offered: {scope}"));
        }
        if ui
            .button("Custom Perks…")
            .on_hover_text("Open the Custom Perk Workbench.")
            .clicked()
        {
            self.perk_workbench.open = true;
        }
        let mut restore = false;
        style::more_menu(ui, "Perks & Sockets", |ui| {
            workbench_style(ui);
            // Socket details are a weapon option. A gear socket keeps its base's plug sets.
            if self.show_experimental_options && self.recipe.kind.is_weapon() {
                ui.checkbox(&mut self.show_technical_socket_rows, "Show Socket Details")
                    .on_hover_text("Shows extra settings under each socket.");
            }
            if ui
                .add_enabled(
                    has_authored_columns,
                    egui::Button::new("Restore All Base Sockets"),
                )
                .on_hover_text("Remove every socket change and custom perk.")
                .clicked()
            {
                restore = true;
                ui.close();
            }
        });
        if restore {
            if self.recipe.overrides.socket_plug_variants.is_empty() {
                self.restore_base_sockets();
            } else {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(restore_base_sockets_id(), true));
            }
        }
        self.draw_restore_base_sockets_confirmation(ui.ctx());
    }

    fn restore_base_sockets(&mut self) {
        self.recipe.overrides.socket_columns.clear();
        self.recipe.overrides.socket_plug_variants.clear();
        self.perk_request = None;
        self.plug_queries.clear();
    }

    fn draw_restore_base_sockets_confirmation(&mut self, ctx: &egui::Context) {
        let id = restore_base_sockets_id();
        if !ctx.data(|data| data.get_temp::<bool>(id)).unwrap_or(false) {
            return;
        }
        let count = self.recipe.overrides.socket_plug_variants.len();
        let mut restore = false;
        let mut cancel = false;
        let response = egui::Modal::new(id.with("modal")).show(ctx, |ui| {
            ui.set_width(380.0);
            workbench_style(ui);
            ui.heading("Restore All Base Sockets?");
            ui.label(format!(
                "Removes {count} custom perk {}.",
                if count == 1 { "choice" } else { "choices" }
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                restore = ui.button("Restore").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        cancel |= response.should_close();
        if restore {
            self.restore_base_sockets();
        }
        if restore || cancel {
            ctx.data_mut(|data| data.remove::<bool>(id));
        }
    }
}

/// Holds the pending Restore All Base Sockets confirmation.
fn restore_base_sockets_id() -> egui::Id {
    egui::Id::new("parhelion-restore-base-sockets")
}

/// Wide enough for the longest name label, "Ghost Shell Name", and never narrower than the 90
/// points the weapon rows have always used.
fn text_label_width(ui: &egui::Ui, kind: crate::ItemKind) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(format!("{} Name", kind.label()), font, egui::Color32::WHITE)
            .size()
            .x
    });
    (width + 6.0).max(90.0)
}

/// The game identities a recipe carries. A subclass or mod has no Collections entry, so it has no
/// collectible or unlock.
fn game_identity_fields(
    recipe: &WeaponRecipe,
    pattern_global_id_hash: &HexHash,
) -> Vec<IdentityField> {
    let mut fields = vec![IdentityField::new(
        "Item",
        recipe.identity.item_hash.to_string(),
    )];
    if recipe.kind.has_collections() {
        fields.extend([
            IdentityField::new("Collectible", recipe.identity.collectible_hash.to_string()),
            IdentityField::new("Unlock", recipe.identity.unlock_hash.to_string()),
        ]);
    }
    fields.push(IdentityField::new(
        "Pattern Global ID",
        pattern_global_id_hash.to_string(),
    ));
    fields
}

/// The package rows the latest build assigned to the recipe's item, or placeholders before one.
fn package_identity_fields(
    kind: ItemKind,
    build: Option<&crate::workflow::WeaponBuildReport>,
) -> Vec<IdentityField> {
    let Some(build) = build else {
        let collection = [
            "Collectible Table Index",
            "Unlock Definition Index",
            "Unlock Flag",
        ];
        return [
            "Item Definition Tag",
            "Item String Tag",
            "Icon Definition Tag",
            "Item Table Index",
        ]
        .into_iter()
        .chain(collection.into_iter().filter(|_| kind.has_collections()))
        .map(IdentityField::assigned_during_build)
        .collect();
    };
    let mut fields = vec![
        IdentityField::new(
            "Item Definition Tag",
            format!("0x{:08X}", build.item_definition_hash),
        ),
        IdentityField::new(
            "Item String Tag",
            format!("0x{:08X}", build.item_string_hash),
        ),
        IdentityField::new(
            "Icon Definition Tag",
            format!("0x{:08X}", build.icon_definition_hash),
        ),
        IdentityField::new("Item Table Index", build.item_index.to_string()),
    ];
    if let Some(collection) = &build.collection {
        fields.extend([
            IdentityField::new(
                "Collectible Table Index",
                collection.collectible_index.to_string(),
            ),
            IdentityField::new(
                "Unlock Definition Index",
                collection.unlock_definition_index.to_string(),
            ),
            IdentityField::new(
                "Unlock Flag",
                format!(
                    "Bank {} · Slot {}",
                    collection.unlock_bank, collection.unlock_slot
                ),
            ),
        ]);
    }
    fields
}
