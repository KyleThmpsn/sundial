use super::*;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_item_traits(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Item Traits (Classification)",
            Some(
                "Complete ordered native trait-definition indices. These classify the base item for client systems independently from socket perks. Removing weapon-family traits can make a definition intentionally unconventional or unusable.",
            ),
        );
        let inherited = gameplay_donor
            .map(|donor| donor.trait_indices.as_slice())
            .unwrap_or_default();
        if self.recipe.overrides.trait_indices.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Inheriting {} trait row(s)", inherited.len()));
                if ui.button("Edit Trait Rows").clicked() {
                    self.recipe.overrides.trait_indices = Some(inherited.to_vec());
                }
            });
            for &trait_index in inherited {
                ui.monospace(trait_choice_label(trait_index, &self.trait_choices));
            }
            return;
        }

        let choices = &self.trait_choices;
        let trait_count = self
            .recipe
            .overrides
            .trait_indices
            .as_ref()
            .map_or(0, Vec::len);
        let mut restore = false;
        let mut add = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Restore Gameplay Values").clicked() {
                restore = true;
            }
            if ui
                .add_enabled(trait_count < 256, egui::Button::new("+ Add Trait"))
                .clicked()
            {
                add = true;
            }
        });
        if restore {
            self.recipe.overrides.trait_indices = None;
            return;
        }
        let traits = self
            .recipe
            .overrides
            .trait_indices
            .as_mut()
            .expect("item traits remain customized");
        if add
            && let Some(choice) = choices
                .iter()
                .find(|choice| !traits.contains(&choice.trait_index))
        {
            traits.push(choice.trait_index);
        }
        let mut remove = None;
        for (index, trait_index) in traits.iter_mut().enumerate() {
            let choice_width = (ui.available_width() - 210.0).clamp(180.0, 330.0);
            ui.horizontal_wrapped(|ui| {
                ui.monospace(format!("{}.", index + 1));
                ui.add(
                    egui::DragValue::new(trait_index)
                        .range(0..=u16::MAX - 1)
                        .speed(1),
                )
                .on_hover_text("Native item-trait definition index");
                egui::ComboBox::from_id_salt(("item-trait", index))
                    .selected_text(trait_choice_label(*trait_index, choices))
                    .width(choice_width)
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            ui.selectable_value(
                                trait_index,
                                choice.trait_index,
                                trait_choice_label(choice.trait_index, choices),
                            )
                            .on_hover_text(&choice.description);
                        }
                    });
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            traits.remove(index);
        }
        if traits.iter().copied().collect::<BTreeSet<_>>().len() != traits.len() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Item-trait rows cannot contain duplicate indices.",
            );
        }
        for &trait_index in traits.iter() {
            if !choices
                .iter()
                .any(|choice| choice.trait_index == trait_index)
            {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Item-trait index {trait_index} is not present in this install."),
                );
            }
        }
    }

    pub(in crate::app) fn draw_native_inventory_fields(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Native Inventory Fields",
            Some(
                "Low-level inline item scalars. Max stack size is a signed 32-bit content field. Stock instanced weapons normally use 1. Inventory bucket and equipment slot are authored together by Combat Profile.",
            ),
        );
        let inherited = gameplay_donor
            .and_then(|donor| donor.max_stack_size)
            .unwrap_or(1);
        ui.horizontal(|ui| {
            ui.label("Max Stack Size");
            match self.recipe.overrides.max_stack_size.as_mut() {
                Some(value) => {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(1..=i32::MAX as u32)
                            .speed(1),
                    );
                    if ui.button("Restore Gameplay Value").clicked() {
                        self.recipe.overrides.max_stack_size = None;
                    }
                }
                None => {
                    ui.monospace(inherited.to_string());
                    if ui.button("Edit Value").clicked() {
                        self.recipe.overrides.max_stack_size = Some(inherited);
                    }
                }
            }
        });
        let inherited_power_cap_groups = gameplay_donor
            .map(|donor| donor.power_cap_groups.as_slice())
            .unwrap_or_default();
        let mut restore_power_cap_rows = false;
        ui.horizontal(|ui| {
            ui.label("Power-Cap Version Rows");
            if let Some(groups) = self.recipe.overrides.power_cap_groups.as_mut() {
                if ui.button("Restore Gameplay Rows").clicked() {
                    restore_power_cap_rows = true;
                } else {
                    ui.weak(format!("{} native rows", groups.len()));
                }
            } else {
                ui.monospace(if inherited_power_cap_groups.is_empty() {
                    "no quality rows".to_owned()
                } else {
                    effective_power_cap_rows(&self.recipe.overrides, inherited_power_cap_groups)
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                if ui
                    .add_enabled(
                        !inherited_power_cap_groups.is_empty(),
                        egui::Button::new("Customize Each Row"),
                    )
                    .on_disabled_hover_text("This donor has no native quality/version rows.")
                    .clicked()
                {
                    let groups = effective_power_cap_rows(&self.recipe.overrides, inherited_power_cap_groups);
                    self.recipe.overrides.power_cap_group = None;
                    self.recipe.overrides.power_cap_groups = Some(groups);
                }
            }
            draw_authoring_info_icon(
                ui,
                "Complete ordered version-group array from the native quality block. The ordinary Power cap picker writes one group to every row. This advanced editor can preserve distinct groups per row.",
            );
        });
        if restore_power_cap_rows {
            self.recipe.overrides.power_cap_groups = None;
        }
        if let Some(groups) = self.recipe.overrides.power_cap_groups.as_mut() {
            let choices = self
                .catalog
                .as_ref()
                .map_or_else(Vec::new, InvestmentCatalog::power_cap_choices);
            egui::Grid::new("power_cap_version_rows")
                .num_columns(3)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (index, group) in groups.iter_mut().enumerate() {
                        ui.label(format!("Version row {}", index + 1));
                        ui.add(egui::DragValue::new(group).range(0..=u16::MAX));
                        egui::ComboBox::from_id_salt(("power-cap-version-row", index))
                            .selected_text(
                                choices
                                    .iter()
                                    .find(|choice| choice.authoring_version_group == *group)
                                    .map_or_else(
                                        || format!("Custom group {group}"),
                                        |choice| choice.picker_label.to_owned(),
                                    ),
                            )
                            .show_ui(ui, |ui| {
                                for choice in &choices {
                                    ui.selectable_value(
                                        group,
                                        choice.authoring_version_group,
                                        &choice.picker_label,
                                    );
                                }
                            });
                        ui.end_row();
                    }
                });
        }
        let inherited_socket_entry_list =
            gameplay_donor.and_then(|donor| donor.socket_entry_list_index);
        let socket_entry_list_count = self
            .catalog
            .as_ref()
            .map_or(0, InvestmentCatalog::socket_entry_list_count);
        let socket_entry_list_max = socket_entry_list_count
            .saturating_sub(1)
            .min(usize::from(u16::MAX - 1)) as u16;
        ui.horizontal(|ui| {
            ui.label("Socket Entry List");
            match self.recipe.overrides.socket_entry_list_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value).range(0..=socket_entry_list_max));
                    ui.weak(format!("{socket_entry_list_count} installed rows"));
                    if ui.button("Restore Gameplay Value").clicked() {
                        self.recipe.overrides.socket_entry_list_index = None;
                    }
                }
                None => {
                    ui.monospace(inherited_socket_entry_list.map_or_else(
                        || "no holder".to_owned(),
                        |value| value.to_string(),
                    ));
                    if ui
                        .add_enabled(
                            inherited_socket_entry_list.is_some(),
                            egui::Button::new("Edit Row Index"),
                        )
                        .on_disabled_hover_text(
                            "This donor has no relocatable socket-entry-list holder.",
                        )
                        .clicked()
                    {
                        self.recipe.overrides.socket_entry_list_index = inherited_socket_entry_list;
                    }
                }
            }
            draw_authoring_info_icon(
                ui,
                "Raw row index selected by the item's talent-grid holder. Stock weapons normally use the empty row. Non-empty rows are primarily subclass data and can radically change client behavior.",
            );
        });
        let inherited_plug_category = gameplay_donor.and_then(|donor| donor.plug_category_hash);
        ui.horizontal(|ui| {
            ui.label("Plug Category");
            match self.recipe.overrides.plug_category_hash.as_mut() {
                Some(hash) => {
                    let mut value = hash.parse_u32().unwrap_or_default();
                    if ui.add(egui::DragValue::new(&mut value)).changed() {
                        *hash = HexHash::new(value);
                    }
                    ui.monospace(format!("0x{value:08X}"));
                    if ui.button("Restore Gameplay Value").clicked() {
                        self.recipe.overrides.plug_category_hash = None;
                    }
                }
                None => {
                    ui.monospace(inherited_plug_category.map_or_else(
                        || "none / sentinel".to_owned(),
                        |value| format!("0x{value:08X}"),
                    ));
                    if ui.button("Edit Hash").clicked() {
                        self.recipe.overrides.plug_category_hash =
                            Some(HexHash::new(inherited_plug_category.unwrap_or_default()));
                    }
                }
            }
            draw_authoring_info_icon(
                ui,
                "Raw category hash used by native plug metadata. Zero and 0xFFFFFFFF are native empty sentinels. Weapons rarely need this field, but it is part of the cloned gameplay definition.",
            );
        });
        let inherited_roll_set = gameplay_donor.and_then(|donor| donor.roll_set_index);
        ui.horizontal(|ui| {
            ui.label("Roll Set");
            match self.recipe.overrides.roll_set_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value));
                    ui.monospace(format!("0x{value:04X}"));
                    if ui.button("Restore Gameplay Value").clicked() {
                        self.recipe.overrides.roll_set_index = None;
                    }
                }
                None => {
                    ui.monospace(
                        inherited_roll_set
                            .map_or_else(|| "no plug block".to_owned(), |value| value.to_string()),
                    );
                    if ui
                        .add_enabled(
                            inherited_roll_set.is_some(),
                            egui::Button::new("Edit Row Index"),
                        )
                        .on_disabled_hover_text(
                            "This donor has no native plug block containing a roll-set field.",
                        )
                        .clicked()
                    {
                        self.recipe.overrides.roll_set_index = inherited_roll_set;
                    }
                }
            }
            draw_authoring_info_icon(
                ui,
                "Native randomized-roll-set table index embedded in the optional plug block.",
            );
        });
        let inherited_linked_plug = gameplay_donor.and_then(|donor| donor.linked_plug_index);
        let inherited_linked_hash = gameplay_donor.and_then(|donor| donor.linked_plug_hash);
        ui.horizontal(|ui| {
            ui.label("Linked Item Row");
            match self.recipe.overrides.linked_plug_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value));
                    ui.monospace(format!("0x{value:04X}"));
                    if ui.button("Restore Gameplay Value").clicked() {
                        self.recipe.overrides.linked_plug_index = None;
                    }
                }
                None => {
                    ui.monospace(inherited_linked_plug.map_or_else(
                        || "no linked-plug block".to_owned(),
                        |value| value.to_string(),
                    ));
                    if let Some(hash) = inherited_linked_hash {
                        ui.weak(format!("→ 0x{hash:08X}"));
                    }
                    if ui
                        .add_enabled(
                            inherited_linked_plug.is_some(),
                            egui::Button::new("Edit Row Index"),
                        )
                        .on_disabled_hover_text(
                            "This donor has no active native linked-plug record.",
                        )
                        .clicked()
                    {
                        self.recipe.overrides.linked_plug_index = inherited_linked_plug;
                    }
                }
            }
            draw_authoring_info_icon(
                ui,
                "Native item-table index held by the optional linked-plug record. The displayed hash is the installed item currently reached by that row.",
            );
        });
        ui.horizontal(|ui| {
            ui.label("Instanced Item");
            ui.monospace("true");
            draw_authoring_info_icon(
                ui,
                "A weapon requires per-instance state for sockets, power, and equipped selections. Parhelion therefore preserves the donor's instanced-item invariant instead of exposing a malformed stackable-weapon state.",
            );
        });
    }
}
