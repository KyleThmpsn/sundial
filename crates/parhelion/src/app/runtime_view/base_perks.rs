use super::*;

impl PackageAuthoringApp {
    pub(in crate::app) fn draw_base_sandbox_perks(
        &mut self,
        ui: &mut egui::Ui,
        gameplay_donor: Option<&WeaponDonor>,
    ) {
        draw_donor_section_label(
            ui,
            "Base Sandbox Perks",
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
                if ui.button("Edit Perk Rows").clicked() {
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
            if ui.button("Restore Gameplay Values").clicked() {
                restore = true;
            }
            if ui
                .add_enabled(perk_count < 64, egui::Button::new("+ Add Base Perk"))
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
}
