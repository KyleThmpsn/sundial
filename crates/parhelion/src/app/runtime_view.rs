//! Advanced native runtime and sandbox editing.
use super::*;

impl PackageAuthoringApp {
    pub(super) fn draw_base_sandbox_perks(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Base sandbox perks",
            Some(
                "Ordered finished sandbox-perk indices emitted by the base item before equipped socket plugs. Elemental damage markers live here too. This does not edit the perks supplied by socket columns.",
            ),
        );
        let inherited = gameplay_donor
            .map(|donor| donor.base_sandbox_perks.as_slice())
            .unwrap_or_default();
        let effective = self
            .recipe
            .overrides
            .base_sandbox_perks
            .as_deref()
            .unwrap_or(inherited);
        if let Some(warning) =
            sundial::package_authoring::sandbox_perk::sunrise_perk_projection_warning(
                effective.len(),
            )
        {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        ui.weak("The replicated weapon bank holds 16 entries total: base perks first, then equipped plugs in socket order. Socket alternatives are not all active at once.");
        if self.recipe.overrides.base_sandbox_perks.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(if inherited.is_empty() {
                    "Inheriting an empty base-perk array".to_owned()
                } else {
                    format!("Inheriting {} base-perk row(s)", inherited.len())
                });
                if ui.button("Edit perk rows").clicked() {
                    self.recipe.overrides.base_sandbox_perks = Some(inherited.to_vec());
                }
            });
            for &perk in inherited {
                ui.monospace(sandbox_perk_choice_label(perk, &self.sandbox_perk_choices));
            }
            return;
        }

        let choices = &self.sandbox_perk_choices;
        let perk_count = self
            .recipe
            .overrides
            .base_sandbox_perks
            .as_ref()
            .map_or(0, Vec::len);
        let mut restore = false;
        let mut add = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Restore gameplay values").clicked() {
                restore = true;
            }
            if ui
                .add_enabled(perk_count < 64, egui::Button::new("+ Add base perk"))
                .clicked()
            {
                add = true;
            }
        });
        if restore {
            self.recipe.overrides.base_sandbox_perks = None;
            return;
        }
        let perks = self
            .recipe
            .overrides
            .base_sandbox_perks
            .as_mut()
            .expect("base sandbox perks remain customized");
        if add
            && let Some(choice) = choices
                .iter()
                .find(|choice| !perks.contains(&choice.perk_index))
        {
            perks.push(choice.perk_index);
        }
        let mut remove = None;
        for (index, perk) in perks.iter_mut().enumerate() {
            let choice_width = (ui.available_width() - 210.0).clamp(180.0, 330.0);
            ui.horizontal_wrapped(|ui| {
                ui.monospace(format!("{}.", index + 1));
                ui.add(egui::DragValue::new(perk).range(0..=u16::MAX - 1).speed(1))
                    .on_hover_text("Finished sandbox-perk table index");
                egui::ComboBox::from_id_salt(("base-sandbox-perk", index))
                    .selected_text(sandbox_perk_choice_label(*perk, choices))
                    .width(choice_width)
                    .show_ui(ui, |ui| {
                        for choice in choices {
                            ui.selectable_value(
                                perk,
                                choice.perk_index,
                                sandbox_perk_choice_label(choice.perk_index, choices),
                            );
                        }
                    });
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            perks.remove(index);
        }
        let unique = perks.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != perks.len() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Base sandbox-perk rows cannot contain duplicate indices.",
            );
        }
        for &perk in perks.iter() {
            if !choices.iter().any(|choice| choice.perk_index == perk) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Sandbox-perk index {perk} is not active and referenced in this install."
                    ),
                );
            }
        }
    }

    pub(super) fn draw_item_traits(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Item traits (classification)",
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
                if ui.button("Edit trait rows").clicked() {
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
            if ui.button("Restore gameplay values").clicked() {
                restore = true;
            }
            if ui
                .add_enabled(trait_count < 256, egui::Button::new("+ Add trait"))
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

    pub(super) fn draw_native_inventory_fields(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Native inventory fields",
            Some(
                "Low-level inline item scalars. Max stack size is a signed 32-bit content field; stock instanced weapons normally use 1. Inventory bucket and equipment slot are authored together by Combat profile.",
            ),
        );
        let inherited = gameplay_donor
            .and_then(|donor| donor.max_stack_size)
            .unwrap_or(1);
        ui.horizontal(|ui| {
            ui.label("Max stack size");
            match self.recipe.overrides.max_stack_size.as_mut() {
                Some(value) => {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(1..=i32::MAX as u32)
                            .speed(1),
                    );
                    if ui.button("Restore gameplay value").clicked() {
                        self.recipe.overrides.max_stack_size = None;
                    }
                }
                None => {
                    ui.monospace(inherited.to_string());
                    if ui.button("Edit value").clicked() {
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
            ui.label("Power-cap version rows");
            if let Some(groups) = self.recipe.overrides.power_cap_groups.as_mut() {
                if ui.button("Restore gameplay rows").clicked() {
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
                        egui::Button::new("Customize each row"),
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
                "Complete ordered version-group array from the native quality block. The ordinary Power cap picker writes one group to every row; this advanced editor can preserve distinct groups per row.",
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
            ui.label("Socket entry list");
            match self.recipe.overrides.socket_entry_list_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value).range(0..=socket_entry_list_max));
                    ui.weak(format!("{socket_entry_list_count} installed rows"));
                    if ui.button("Restore gameplay value").clicked() {
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
                            egui::Button::new("Edit row index"),
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
                "Raw row index selected by the item's talent-grid holder. Stock weapons normally use the empty row; non-empty rows are primarily subclass data and can radically change client behavior.",
            );
        });
        let inherited_plug_category = gameplay_donor.and_then(|donor| donor.plug_category_hash);
        ui.horizontal(|ui| {
            ui.label("Plug category");
            match self.recipe.overrides.plug_category_hash.as_mut() {
                Some(hash) => {
                    let mut value = hash.parse_u32().unwrap_or_default();
                    if ui.add(egui::DragValue::new(&mut value)).changed() {
                        *hash = HexHash::new(value);
                    }
                    ui.monospace(format!("0x{value:08X}"));
                    if ui.button("Restore gameplay value").clicked() {
                        self.recipe.overrides.plug_category_hash = None;
                    }
                }
                None => {
                    ui.monospace(inherited_plug_category.map_or_else(
                        || "none / sentinel".to_owned(),
                        |value| format!("0x{value:08X}"),
                    ));
                    if ui.button("Edit hash").clicked() {
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
            ui.label("Roll set");
            match self.recipe.overrides.roll_set_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value));
                    ui.monospace(format!("0x{value:04X}"));
                    if ui.button("Restore gameplay value").clicked() {
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
                            egui::Button::new("Edit row index"),
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
            ui.label("Linked item row");
            match self.recipe.overrides.linked_plug_index.as_mut() {
                Some(value) => {
                    ui.add(egui::DragValue::new(value));
                    ui.monospace(format!("0x{value:04X}"));
                    if ui.button("Restore gameplay value").clicked() {
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
                            egui::Button::new("Edit row index"),
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
            ui.label("Instanced item");
            ui.monospace("true");
            draw_authoring_info_icon(
                ui,
                "A weapon requires per-instance state for sockets, power, and equipped selections. Parhelion therefore preserves the donor's instanced-item invariant instead of exposing a malformed stackable-weapon state.",
            );
        });
    }

    pub(super) fn draw_raw_payload_patches(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Raw payload patches",
            Some(
                "Escape hatch for native fields without a decoded name. Final byte replacements are stored in recipes as hexadecimal bytes and applied to the selected finished row or payload. Required identity, index, and graph invariants are still validated.",
            ),
        );
        let mut remove = None;
        let patch_count = self.recipe.overrides.raw_payload_patches.len();
        for (index, patch) in self
            .recipe
            .overrides
            .raw_payload_patches
            .iter_mut()
            .enumerate()
        {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("Patch {}", index + 1));
                egui::ComboBox::from_id_salt(("raw-payload-target", index))
                    .selected_text(patch.target.label())
                    .width(ui.available_width().clamp(180.0, 230.0))
                    .show_ui(ui, |ui| {
                        for target in RecipeRawPayloadTarget::ALL {
                            ui.selectable_value(&mut patch.target, target, target.label());
                            if matches!(
                                target,
                                RecipeRawPayloadTarget::ItemTraitRows
                                    | RecipeRawPayloadTarget::ItemStringDefinition
                                    | RecipeRawPayloadTarget::ItemStringIndexRow
                                    | RecipeRawPayloadTarget::DensePresentationRow
                                    | RecipeRawPayloadTarget::SandboxPatternIndexRow
                                    | RecipeRawPayloadTarget::CollectibleDisplayRow
                            ) {
                                ui.separator();
                            }
                        }
                    });
                ui.label("Offset");
                ui.add(egui::DragValue::new(&mut patch.offset).speed(1));
                ui.monospace(format!("0x{:X}", patch.offset));
                if ui.button("Remove").clicked() {
                    remove = Some(index);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Bytes");
                ui.add(
                    egui::TextEdit::singleline(&mut patch.bytes)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("00 FF 2A …"),
                );
            });
            if !valid_hex_patch_text(&patch.bytes) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Patch {} needs a non-empty even-length hexadecimal byte string.",
                        index + 1
                    ),
                );
            }
            if index + 1 < patch_count {
                ui.separator();
            }
        }
        if let Some(index) = remove {
            self.recipe.overrides.raw_payload_patches.remove(index);
        }
        if ui.button("+ Add raw patch").clicked() {
            self.recipe
                .overrides
                .raw_payload_patches
                .push(WeaponRawPayloadPatchRecipe::default());
        }
    }

    pub(super) fn draw_runtime_component_donors(&mut self, ui: &mut egui::Ui) {
        let graph = self.runtime_graph.as_ref().and_then(|(key, graph)| {
            (Some(key) == self.runtime_graph_target.as_ref()).then(|| Arc::clone(graph))
        });
        if let Some(donor_width) = runtime_workspace_donor_width(ui.available_width()) {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(donor_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(donor_width);
                        self.draw_runtime_component_donor_column(ui, graph.as_deref());
                    },
                );
                ui.separator();
                let value_width = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(value_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(value_width);
                        self.draw_runtime_value_column(ui, graph.as_deref());
                    },
                );
            });
        } else {
            self.draw_runtime_component_donor_column(ui, graph.as_deref());
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(5.0);
            self.draw_runtime_value_column(ui, graph.as_deref());
        }
    }

    pub(super) fn draw_runtime_component_donor_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        self.draw_runtime_component_donor_pickers(ui, graph);
        if self.show_experimental_options {
            ui.add_space(8.0);
            self.draw_runtime_resource_patches(ui);
        }
    }

    fn draw_runtime_component_donor_pickers(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        draw_donor_section_label(
            ui,
            "Runtime Component Donors",
            Some(
                "A selection replaces the complete shared owner partition for that resource, including every alias and any other component binding owned by the same partition. Unselected owners remain byte-identical to the baseline. Cross-family components can depend on different runtime data, so test new combinations in-game even when the native graph validates.",
            ),
        );
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Mixing component donors is highly experimental and has a high risk of crashes. Use with caution.",
        );
        if self.runtime_graph_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Reading the selected pattern's complete runtime graph…");
            });
        }
        if let Some((key, error)) = self.runtime_graph_error.as_ref()
            && Some(key) == self.runtime_graph_target.as_ref()
        {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Runtime graph could not be decoded: {error}"),
            );
            if ui.small_button("Retry runtime scan").clicked() {
                self.runtime_graph_error = None;
            }
        }
        let mut active = BTreeMap::<u32, String>::new();
        if let Some(graph) = graph {
            for binding in &graph.bindings {
                active
                    .entry(binding.binding_hash)
                    .or_insert_with(|| binding.binding_label.clone());
            }
        } else {
            // An experimental choice can prevent full field decoding. Keep repair and
            // compatibility-review controls available instead of trapping that saved choice.
            ui.weak("Runtime data is unavailable. Saved donors can still be reviewed or reset.");
            for control in PRIMARY_RUNTIME_COMPONENTS {
                active.insert(control.binding_hash, control.label.to_owned());
            }
            for component in &self.recipe.runtime_component_donors {
                if let Ok(hash) = component.binding_hash.parse_u32() {
                    active
                        .entry(hash)
                        .or_insert_with(|| format!("Binding 0x{hash:08X}"));
                }
            }
        }
        for control in PRIMARY_RUNTIME_COMPONENTS {
            if active.contains_key(&control.binding_hash) {
                self.draw_runtime_component_donor_picker(
                    ui,
                    control.binding_hash,
                    control.label,
                    control.tooltip,
                );
                ui.add_space(4.0);
            }
        }

        let additional_count = active
            .iter()
            .filter(|(hash, _)| {
                !PRIMARY_RUNTIME_COMPONENTS
                    .iter()
                    .any(|known| known.binding_hash == **hash)
            })
            .count();
        if ui
            .button(format!("Advanced Runtime Bindings… ({additional_count})"))
            .clicked()
        {
            self.runtime_bindings_open = true;
        }

        let stale = self
            .recipe
            .runtime_component_donors
            .iter()
            .filter_map(|component| component.binding_hash.parse_u32().ok())
            .filter(|binding_hash| !active.contains_key(binding_hash))
            .collect::<Vec<_>>();
        for binding_hash in stale {
            ui.horizontal(|ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Saved component binding 0x{binding_hash:08X} is not present in this pattern"),
                );
                if ui.small_button("Remove").clicked() {
                    self.recipe.set_runtime_component_donor(binding_hash, None);
                }
            });
        }
    }

    pub(super) fn draw_runtime_bindings_window(&mut self, ctx: &egui::Context) {
        if !self.runtime_bindings_open
            || !self.show_experimental_options
            || self.build_receiver.is_some()
            || self.install_receiver.is_some()
        {
            return;
        }
        let graph = self.runtime_graph.as_ref().and_then(|(key, graph)| {
            (Some(key) == self.runtime_graph_target.as_ref()).then(|| Arc::clone(graph))
        });
        let mut open = true;
        egui::Window::new("Advanced Runtime Bindings")
            .id(egui::Id::new("parhelion-runtime-bindings-window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(660.0)
            .default_height(600.0)
            .resizable(true)
            .show(ctx, |ui| {
            workbench_style(ui);
            ui.label(format!("{} · Component sources", self.recipe.name));
            let additional = if let Some(graph) = graph.as_deref() {
                graph.bindings.iter()
                    .filter(|binding| !PRIMARY_RUNTIME_COMPONENTS.iter().any(|known| known.binding_hash == binding.binding_hash))
                    .map(|binding| (binding.binding_hash, binding.binding_label.clone()))
                    .collect::<BTreeMap<_, _>>()
            } else {
                ui.weak("Runtime data is unavailable. Saved additional donors remain available for repair.");
                self.recipe.runtime_component_donors.iter()
                    .filter_map(|component| component.binding_hash.parse_u32().ok())
                    .filter(|hash| !PRIMARY_RUNTIME_COMPONENTS.iter().any(|known| known.binding_hash == *hash))
                    .map(|hash| (hash, format!("Binding 0x{hash:08X}")))
                    .collect::<BTreeMap<_, _>>()
            };
            ui.horizontal_wrapped(|ui| {
                ui.label("Filter");
                named_control(ui.add(
                    egui::TextEdit::singleline(&mut self.runtime_binding_filter)
                        .desired_width(260.0)
                        .hint_text("Name or 0x hash"),
                ), "Filter Runtime Bindings");
            });
            let query = self.runtime_binding_filter.trim().to_ascii_lowercase();
            let filtered = additional
                .into_iter()
                .filter(|(binding_hash, discovered_label)| {
                    let known = runtime_component_control(*binding_hash);
                    let label = known.map_or(discovered_label.as_str(), |control| control.label);
                    query.is_empty()
                        || label.to_ascii_lowercase().contains(&query)
                        || format!("0x{binding_hash:08x}").contains(&query)
                })
                .collect::<Vec<_>>();
            ui.weak(format!(
                "{} matching binding{}",
                filtered.len(),
                if filtered.len() == 1 { "" } else { "s" }
            ));
            egui::ScrollArea::vertical()
                .id_salt(("additional-runtime-binding-results", self.recipe_panel_scope()))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (binding_hash, discovered_label) in filtered {
                        let known = runtime_component_control(binding_hash);
                        let label =
                            known.map_or(discovered_label.as_str(), |control| control.label);
                        let tooltip = known.map_or(
                            "A binding discovered directly from the selected runtime entity. The compiler requires the selected donor to expose the same binding shape.",
                            |control| control.tooltip,
                        );
                        self.draw_runtime_component_donor_picker(
                            ui,
                            binding_hash,
                            label,
                            tooltip,
                        );
                        ui.add_space(4.0);
                    }
                });
        });

        self.runtime_bindings_open = open;
    }

    pub(super) fn draw_runtime_value_column(
        &mut self,
        ui: &mut egui::Ui,
        graph: Option<&WeaponRuntimeGraph>,
    ) {
        if let Some(graph) = graph {
            self.draw_runtime_values(ui, graph);
        } else {
            draw_donor_section_label(
                ui,
                "Runtime Values",
                Some(
                    "Raw fields decoded from the selected runtime and component donors. Saved field edits are shown here, but binary patches, automatic ammo and HUD edits, and raw entity patches are applied only during compilation. Field names and types do not establish final in-game behavior or units.",
                ),
            );
            ui.weak("Runtime values appear after the selected runtime row has been decoded.");
        }
    }

    pub(super) fn draw_runtime_values(&mut self, ui: &mut egui::Ui, graph: &WeaponRuntimeGraph) {
        let resolved_count = graph
            .fields()
            .filter(|field| {
                field.source != WeaponRuntimeFieldSource::OpaqueNativeType
                    && runtime_field_is_editable(field)
            })
            .count();
        let technical_count = graph
            .fields()
            .filter(|field| {
                field.source == WeaponRuntimeFieldSource::OpaqueNativeType
                    && runtime_field_is_editable(field)
            })
            .count();
        draw_donor_section_label(
            ui,
            "Runtime Values",
            Some(
                "Raw fields decoded from the selected runtime and component donors. Saved field edits are shown here, but binary patches, automatic ammo and HUD edits, and raw entity patches are applied only during compilation. Field names and types do not establish final in-game behavior or units.",
            ),
        );
        ui.weak("Source: Selected runtime and component donors, with saved field edits.");
        ui.weak("Binary, ammo, HUD and raw entity patches are applied at build time, not shown here. These are raw package fields, not final in-game stats.");
        ui.horizontal(|ui| {
            ui.label("Filter");
            named_control(
                ui.add(
                    egui::TextEdit::singleline(&mut self.runtime_value_query)
                        .desired_width(ui.available_width())
                        .hint_text("Field, component, schema, type, or 0x hash"),
                ),
                "Filter Runtime Values",
            );
        });
        ui.horizontal_wrapped(|ui| {
            if self.show_experimental_options {
                ui.checkbox(
                    &mut self.show_technical_runtime_values,
                    format!("Show all native values ({technical_count} additional)"),
                )
                .on_hover_text(
                    "Includes unreflected byte ranges from the selected weapon's concrete runtime resources. Some ranges may contain pointers, descriptors, or coupled state.",
                );
            }
            ui.add(egui::Label::new(egui::RichText::new(format!(
                "{resolved_count} resolved · {} customized",
                self.recipe.overrides.runtime_values.len()
            )).weak()).wrap_mode(egui::TextWrapMode::Extend));
        });
        if self.show_experimental_options && self.show_technical_runtime_values {
            ui.weak(
                "Technical ranges are exact package bytes, not guessed gameplay properties. Invalid pointer, descriptor, or coupled values can make the client reject or crash on the weapon.",
            );
        }

        let list_height = (ui.ctx().screen_rect().height() * 0.52).clamp(300.0, 540.0);
        egui::ScrollArea::vertical()
            .id_salt(("parhelion-runtime-values", self.recipe_panel_scope()))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .max_height(list_height)
            // This sits inside a split layout and an outer page scroll area. A maximum
            // alone lets egui collapse it to its 64px minimum during height measurement.
            .min_scrolled_height(list_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
        let live_locators = graph
            .fields()
            .map(|field| &field.locator)
            .collect::<BTreeSet<_>>();
        let stale = self
            .recipe
            .overrides
            .runtime_values
            .iter()
            .enumerate()
            .filter(|(_, value)| !live_locators.contains(&value.locator))
            .map(|(index, value)| (index, value.locator.clone()))
            .collect::<Vec<_>>();
        let mut remove_stale = None;
        for (index, locator) in stale {
            ui.horizontal(|ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Customized binding 0x{:08X}, schema 0x{:08X}, root offset 0x{:X} is not present in the effective graph",
                        locator.binding_hash, locator.root_schema, locator.value_offset
                    ),
                );
                if ui.small_button("Remove stale value").clicked() {
                    remove_stale = Some(index);
                }
            });
        }
        if let Some(index) = remove_stale {
            let removed = self.recipe.overrides.runtime_values.remove(index);
            self.runtime_value_text
                .retain(|(locator, _), _| locator != &removed.locator);
        }

        let query = self.runtime_value_query.trim().to_ascii_lowercase();
        let mut visible_count = 0usize;
        for resource in &graph.resources {
            let resource_matches = query.is_empty()
                || resource.binding_label.to_ascii_lowercase().contains(&query)
                || format!("0x{:08x}", resource.binding_hash).contains(&query)
                || format!("0x{:08x}", resource.owner_tag).contains(&query)
                || format!("0x{:08x}", resource.concrete_class).contains(&query)
                || resource.definition.as_ref().is_some_and(|definition| {
                    format!("0x{:08x}", definition.schema).contains(&query)
                });
            let roots = std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(|root| {
                    let fields = root
                        .fields
                        .iter()
                        .filter(|field| {
                            self.runtime_field_is_visible(field, &query, resource_matches)
                        })
                        .collect::<Vec<_>>();
                    (root, fields)
                })
                .filter(|(_, fields)| !fields.is_empty())
                .collect::<Vec<_>>();
            let field_count = roots.iter().map(|(_, fields)| fields.len()).sum::<usize>();
            if field_count == 0 {
                continue;
            }
            visible_count += field_count;
            let resource_suffix = if resource.resource_count > 1 {
                format!(
                    " · resource {}/{}",
                    resource.resource_index + 1,
                    resource.resource_count
                )
            } else {
                String::new()
            };
            egui::CollapsingHeader::new(format!(
                "{}{} · class 0x{:08X} · {} value{}",
                resource.binding_label,
                resource_suffix,
                resource.concrete_class,
                field_count,
                if field_count == 1 { "" } else { "s" }
            ))
            .id_salt((
                "runtime-resource",
                resource.binding_hash,
                resource.resource_index,
                resource.concrete_class,
            ))
            .default_open(!query.is_empty())
            .show(ui, |ui| {
                ui.weak(format!(
                    "Owner 0x{:08X}{}",
                    resource.owner_tag,
                    if resource.alias_bindings.is_empty() {
                        String::new()
                    } else {
                        format!(" · {} alias bindings", resource.alias_bindings.len())
                    },
                ));
                for (root, fields) in roots {
                    egui::CollapsingHeader::new(format!(
                        "{} · schema 0x{:08X} · owner offset 0x{:X} · 0x{:X} bytes · {}",
                        root.kind.label(),
                        root.schema,
                        root.owner_offset,
                        root.byte_size,
                        if root.generated_schema {
                            "generated"
                        } else {
                            "native"
                        }
                    ))
                    .id_salt((
                        "runtime-component-root",
                        resource.binding_hash,
                        resource.resource_index,
                        root.kind,
                        root.schema,
                    ))
                    .default_open(!query.is_empty())
                    .show(ui, |ui| {
                        for field in fields {
                            self.draw_runtime_value_field(ui, field);
                        }
                    });
                }
            });
        }
        for owner in &graph.owners {
            let binding_label = graph
                .bindings
                .iter()
                .find(|binding| binding.binding_hash == owner.anchor_binding_hash)
                .map_or_else(
                    || format!("Binding 0x{:08X}", owner.anchor_binding_hash),
                    |binding| binding.binding_label.clone(),
                );
            let owner_matches = query.is_empty()
                || binding_label.to_ascii_lowercase().contains(&query)
                || format!("0x{:08x}", owner.owner_tag).contains(&query)
                || format!("0x{:08x}", owner.anchor_binding_hash).contains(&query);
            let owner_visible = owner.roots.iter().any(|root| {
                root.fields
                    .iter()
                    .any(|field| self.runtime_field_is_visible(field, &query, owner_matches))
            });
            if !owner_visible {
                continue;
            }
            let owner_field_count = owner
                .roots
                .iter()
                .flat_map(|root| &root.fields)
                .filter(|field| self.runtime_field_is_visible(field, &query, owner_matches))
                .count();
            visible_count += owner_field_count;
            egui::CollapsingHeader::new(format!(
                "Shared owner state · {} · 0x{:08X} · {} value{}",
                binding_label,
                owner.owner_tag,
                owner_field_count,
                if owner_field_count == 1 { "" } else { "s" }
            ))
            .id_salt((
                "runtime-owner",
                owner.owner_tag,
                owner.anchor_binding_hash,
                owner.anchor_resource_index,
            ))
            .default_open(!query.is_empty())
            .show(ui, |ui| {
                for root in &owner.roots {
                    let root_fields = root
                        .fields
                        .iter()
                        .filter(|field| self.runtime_field_is_visible(field, &query, owner_matches))
                        .collect::<Vec<_>>();
                    if root_fields.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!(
                        "{} · schema 0x{:08X} · {}",
                        root.kind.label(),
                        root.schema,
                        if root.generated_schema {
                            "generated"
                        } else {
                            "native"
                        }
                    ))
                    .id_salt(("runtime-root", owner.owner_tag, root.kind, root.schema))
                    .default_open(!query.is_empty())
                    .show(ui, |ui| {
                        for field in root_fields {
                            self.draw_runtime_value_field(ui, field);
                        }
                    });
                }
            });
        }
        if visible_count == 0 {
            ui.weak("No runtime values match the current filter.");
        }
            });
    }

    pub(super) fn runtime_field_is_visible(
        &self,
        field: &WeaponRuntimeField,
        query: &str,
        owner_matches: bool,
    ) -> bool {
        let customized = self
            .recipe
            .overrides
            .runtime_values
            .iter()
            .any(|value| value.locator == field.locator);
        if !runtime_field_is_in_editor_scope(
            field.source,
            runtime_field_is_editable(field),
            customized,
            self.show_experimental_options,
            self.show_technical_runtime_values,
        ) {
            return false;
        }
        if query.is_empty() || owner_matches {
            return true;
        }
        field.name.to_ascii_lowercase().contains(query)
            || field.path_label.to_ascii_lowercase().contains(query)
            || runtime_value_kind_label(&field.kind)
                .to_ascii_lowercase()
                .contains(query)
            || format!("0x{:08x}", field.locator.root_schema).contains(query)
            || format!("0x{:08x}", field.locator.type_handle).contains(query)
            || format!("0x{:x}", field.owner_offset).contains(query)
            || format!("0x{:x}", field.locator.value_offset).contains(query)
            || field.locator.path.iter().any(|element| {
                format!("0x{:08x}", element.name_hash).contains(query)
                    || format!("0x{:08x}", element.type_handle).contains(query)
            })
    }

    pub(super) fn draw_runtime_value_field(
        &mut self,
        ui: &mut egui::Ui,
        field: &WeaponRuntimeField,
    ) {
        draw_runtime_value_override_field(
            ui,
            field,
            &mut self.recipe.overrides.runtime_values,
            &mut self.runtime_value_text,
        );
    }

    pub(super) fn draw_runtime_resource_patches(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Binary Runtime Patches")
            .id_salt(("runtime-value-bytes", self.recipe_panel_scope()))
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.weak(
                        "Advanced same-size edits inside a selected concrete runtime resource.",
                    );
                    draw_authoring_info_icon(
                        ui,
                        "Offsets are relative to the resource record selected by the binding. Parhelion clones the owning component tag, rebases its self references, and keeps the edit private to this authored weapon. Use raw runtime-entity patches only for entity-map or descriptor bytes.",
                    );
                });
                let mut remove = None;
                let patch_count = self.recipe.overrides.runtime_resource_patches.len();
                for (index, patch) in self
                    .recipe
                    .overrides
                    .runtime_resource_patches
                    .iter_mut()
                    .enumerate()
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("Patch {}", index + 1));
                        let parsed_binding = patch.binding_hash.parse_u32().ok();
                        egui::ComboBox::from_id_salt(("runtime-value-binding", index))
                            .selected_text(parsed_binding.and_then(runtime_component_control).map_or(
                                "Choose known binding…",
                                |control| control.label,
                            ))
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for control in PRIMARY_RUNTIME_COMPONENTS
                                    .into_iter()
                                    .chain(ADDITIONAL_RUNTIME_COMPONENTS)
                                {
                                    if ui
                                        .selectable_label(
                                            parsed_binding == Some(control.binding_hash),
                                            format!(
                                                "{} · 0x{:08X}",
                                                control.label, control.binding_hash
                                            ),
                                        )
                                        .clicked()
                                    {
                                        patch.binding_hash = HexHash::new(control.binding_hash);
                                    }
                                }
                            });
                        let mut binding_text = patch.binding_hash.to_string();
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut binding_text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(112.0),
                            )
                            .on_hover_text(
                                "Any active native binding hash is accepted; named weapon bindings are available in the menu.",
                            )
                            .changed()
                        {
                            patch.binding_hash.set_text(binding_text);
                        }
                        ui.label("Resource Index");
                        ui.add(egui::DragValue::new(&mut patch.resource_index))
                            .on_hover_text("Zero-based resource index within the selected binding.");
                        ui.label("Offset");
                        ui.add(egui::DragValue::new(&mut patch.offset).speed(1));
                        ui.monospace(format!("0x{:X}", patch.offset));
                        if ui.button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Bytes");
                        ui.add(
                            egui::TextEdit::singleline(&mut patch.bytes)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .hint_text("00 FF 2A …"),
                        );
                    });
                    if !patch.graph_values.is_empty() {
                        ui.label(format!("Private Graph: {} edits (weapon-wide)", patch.graph_values.len()))
                            .on_hover_text("Bytes identify the stock graph. The build links a private edited copy here; this is not gated by the custom perk. Edit graph values in the recipe JSON.");
                    }
                    if patch.binding_hash.parse_u32().ok().is_none_or(|hash| {
                        matches!(hash, 0 | u32::MAX)
                    }) {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!(
                                "Runtime value patch {} needs a non-reserved canonical binding hash.",
                                index + 1
                            ),
                        );
                    }
                    if index + 1 < patch_count {
                        ui.separator();
                    }
                    if !valid_hex_patch_text(&patch.bytes) {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!(
                                "Runtime value patch {} needs a non-empty even-length hexadecimal byte string.",
                                index + 1
                            ),
                        );
                    }
                }
                if let Some(index) = remove {
                    self.recipe
                        .overrides
                        .runtime_resource_patches
                        .remove(index);
                }
                if ui.button("+ Add Binary Patch").clicked() {
                    self.recipe
                        .overrides
                        .runtime_resource_patches
                        .push(WeaponRuntimeResourcePatchRecipe::default());
                }
            });
    }

    pub(super) fn runtime_component_baseline_hash(&self) -> Option<u32> {
        let current_key = self.runtime_graph_key();
        if let Some(hash) = self
            .runtime_graph
            .as_ref()
            .filter(|(key, _)| Some(key) == current_key.as_ref())
            .map(|(_, graph)| graph.item_hash)
            .filter(|hash| *hash != 0)
        {
            return Some(hash);
        }
        let Some(index) = self.recipe.overrides.weapon_pattern_index else {
            return self
                .recipe
                .donor
                .item_hash
                .parse_u32()
                .ok()
                .filter(|hash| *hash != 0);
        };
        // An explicit runtime row can be unrelated to the gameplay donor. Without a
        // current graph, only a representative verified against that row is a baseline.
        self.recipe
            .overrides
            .weapon_pattern_donor_hash
            .as_ref()
            .and_then(|hash| hash.parse_u32().ok())
            .filter(|hash| {
                self.donor_summaries
                    .iter()
                    .any(|donor| donor.hash == *hash && donor.weapon_pattern_index == Some(index))
            })
            .or_else(|| {
                self.donor_summaries
                    .iter()
                    .find(|donor| donor.weapon_pattern_index == Some(index))
                    .map(|donor| donor.hash)
            })
            .filter(|hash| *hash != 0)
    }

    pub(super) fn draw_runtime_component_donor_picker(
        &mut self,
        ui: &mut egui::Ui,
        binding_hash: u32,
        label: &str,
        tooltip: &str,
    ) {
        draw_donor_section_label(
            ui,
            label,
            Some(&format!("{tooltip} Native binding: 0x{binding_hash:08X}.")),
        );
        let pattern_hash = self.runtime_component_baseline_hash();
        let current_reference = self.recipe.runtime_component_donor(binding_hash).cloned();
        let current_hash = current_reference
            .as_ref()
            .and_then(|donor| donor.item_hash.parse_u32().ok());
        let selected_text = current_reference.as_ref().map_or_else(
            || {
                pattern_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || "Follow runtime baseline".to_owned(),
                        |donor| format!("Follow {} · 0x{:08X}", donor.name, donor.hash),
                    )
            },
            |reference| {
                current_hash
                    .and_then(|hash| self.donor_summaries.iter().find(|donor| donor.hash == hash))
                    .map_or_else(
                        || {
                            format!(
                                "{} · {}",
                                reference
                                    .expected_name
                                    .as_deref()
                                    .unwrap_or("Unknown component donor"),
                                reference.item_hash
                            )
                        },
                        |donor| {
                            format!(
                                "{} · {} · 0x{:08X}",
                                donor.name, donor.type_name, donor.hash
                            )
                        },
                    )
            },
        );
        self.draw_checked_runtime_donor_header(
            ui,
            binding_hash,
            &selected_text,
            current_hash,
            pattern_hash,
        );
    }
}
