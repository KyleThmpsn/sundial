//! Boxed action scalars: bounded structural discovery, never guessed gameplay semantics.
use super::*;
use crate::WeaponSandboxPerkActionFloatRecipe;
use sundial::package_authoring::sandbox_perk::sandbox_perk_action_boxed_value_offset;

pub(super) fn action_offset(
    payload: &[u8],
    value: &WeaponSandboxPerkActionFloatRecipe,
) -> Result<usize, String> {
    sandbox_perk_action_boxed_value_offset(
        payload,
        value
            .node_type_handle
            .parse_u32()
            .map_err(|e| e.to_string())?,
        value.node_occurrence,
        value.value_pointer_offset,
        value
            .value_type_handle
            .parse_u32()
            .map_err(|e| e.to_string())?,
        4,
    )
}

pub(super) fn source_bits(
    payload: &[u8],
    value: &WeaponSandboxPerkActionFloatRecipe,
) -> Result<u32, String> {
    let offset = action_offset(payload, value)?;
    Ok(u32::from_le_bytes(
        payload
            .get(offset..offset + 4)
            .ok_or("Action scalar is truncated")?
            .try_into()
            .unwrap(),
    ))
}

pub(super) fn discover(payload: &[u8]) -> Vec<WeaponSandboxPerkActionFloatRecipe> {
    let mut values = Vec::new();
    // This native boxed-float layout is structurally verified by the compiler tests.
    // The values' gameplay meaning is not inferred from their numeric contents.
    for occurrence in 0..64 {
        let mut value = WeaponSandboxPerkActionFloatRecipe {
            node_type_handle: HexHash::new(0x8080_2F16),
            node_occurrence: occurrence,
            value_pointer_offset: 0x140,
            value_type_handle: HexHash::new(0x8080_2F1A),
            expected_bits: 0,
            value_bits: 0,
        };
        let Ok(bits) = source_bits(payload, &value) else {
            break;
        };
        if !f32::from_bits(bits).is_finite() {
            continue;
        }
        value.expected_bits = bits;
        value.value_bits = bits;
        values.push(value);
    }
    values
}

fn same_locator(
    a: &WeaponSandboxPerkActionFloatRecipe,
    b: &WeaponSandboxPerkActionFloatRecipe,
) -> bool {
    a.node_type_handle == b.node_type_handle
        && a.node_occurrence == b.node_occurrence
        && a.value_pointer_offset == b.value_pointer_offset
        && a.value_type_handle == b.value_type_handle
}

impl PerkEditor {
    pub(super) fn draw_action_values(
        &mut self,
        ui: &mut egui::Ui,
        loaded: &PrivatePerkRuntimeGraph,
        experimental: bool,
    ) {
        let mut choices = if experimental {
            discover(&loaded.action_payload)
        } else {
            Vec::new()
        };
        for saved in &self.action_draft {
            if !choices.iter().any(|value| same_locator(value, saved)) {
                choices.push(saved.clone());
            }
        }
        if choices.is_empty() {
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong("Action Scalars");
            sundial::investment::draw_authoring_info_icon(ui,
                "Action Scalars\nValues stored in the perk action. Hover over a scalar for its native locator. Reset restores the original package value.");
        });
        for (index, source) in choices.into_iter().enumerate() {
            ui.push_id(("action-scalar", index), |ui| {
                let existing = self.action_draft.iter().position(|value| same_locator(value, &source));
                let mut value = existing.map_or(source.clone(), |index| self.action_draft[index].clone());
                let original = source_bits(&loaded.action_payload, &source);
                let valid_source = original.as_ref().is_ok_and(|bits| *bits == value.expected_bits);
                ui.horizontal_wrapped(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(144.0, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(144.0);
                            ui.label(format!("Action Scalar {}", index + 1)).on_hover_text(format!("Node {} · occurrence {} · member +0x{:X} · boxed type {}", source.node_type_handle, source.node_occurrence, source.value_pointer_offset, source.value_type_handle));
                        },
                    );
                    let mut number = f32::from_bits(value.value_bits);
                    if ui.add_enabled(experimental && valid_source, egui::DragValue::new(&mut number).speed(0.01)).changed() {
                        value.value_bits = number.to_bits();
                        if let Some(index) = existing { self.action_draft.remove(index); }
                        if value.value_bits != value.expected_bits { self.action_draft.push(value.clone()); }
                    }
                    if let Ok(bits) = original { ui.weak(format!("Original: {}", f32::from_bits(bits))); }
                    if ui.add_enabled(existing.is_some(), egui::Button::new("Reset").small()).clicked() {
                        self.action_draft.retain(|value| !same_locator(value, &source));
                    }
                });
                if !valid_source { ui.colored_label(ui.visuals().error_fg_color, "Saved action locator or expected value no longer matches. Reset this edit."); }
                if !number_is_finite(value.value_bits) { ui.colored_label(ui.visuals().error_fg_color, "Enter a finite action value."); }
            });
        }
        if !experimental {
            ui.label("Enable experimental controls in Preferences to change action scalars. Saved edits are preserved and can be reset here.");
        }
    }
}

fn number_is_finite(bits: u32) -> bool {
    f32::from_bits(bits).is_finite()
}
