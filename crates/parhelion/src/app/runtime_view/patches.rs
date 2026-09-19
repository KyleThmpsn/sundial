use super::*;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_raw_payload_patches(&mut self, ui: &mut egui::Ui) {
        draw_donor_section_label(
            ui,
            "Raw Payload Patches",
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
                if draw_patch_offset(ui, &mut patch.offset) {
                    remove = Some(index);
                }
            });
            draw_patch_bytes(ui, &mut patch.bytes);
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
        if ui.button("+ Add Raw Patch").clicked() {
            self.recipe
                .overrides
                .raw_payload_patches
                .push(WeaponRawPayloadPatchRecipe::default());
        }
    }

    pub(in crate::app) fn draw_runtime_resource_patches(&mut self, ui: &mut egui::Ui) {
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
                                "Choose Known Binding…",
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
                                "Any active native binding hash is accepted. Named weapon bindings are available in the menu.",
                            )
                            .changed()
                        {
                            patch.binding_hash.set_text(binding_text);
                        }
                        ui.label("Resource Index");
                        ui.add(egui::DragValue::new(&mut patch.resource_index))
                            .on_hover_text("Zero-based resource index within the selected binding.");
                        if draw_patch_offset(ui, &mut patch.offset) {
                            remove = Some(index);
                        }
                    });
                    draw_patch_bytes(ui, &mut patch.bytes);
                    if !patch.graph_values.is_empty() {
                        ui.label(format!("Private Graph: {} edits (weapon-wide)", patch.graph_values.len()))
                            .on_hover_text("Bytes identify the stock graph. The build links a private edited copy here. This is not gated by the custom perk. Edit graph values in the recipe JSON.");
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
}

fn draw_patch_offset(ui: &mut egui::Ui, offset: &mut u32) -> bool {
    ui.label("Offset");
    ui.add(egui::DragValue::new(offset).speed(1));
    ui.monospace(format!("0x{offset:X}"));
    ui.button("Remove").clicked()
}

fn draw_patch_bytes(ui: &mut egui::Ui, bytes: &mut String) {
    ui.horizontal(|ui| {
        ui.label("Bytes");
        ui.add(
            egui::TextEdit::singleline(bytes)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .hint_text("00 FF 2A …"),
        );
    });
}
