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
                "Perks the base item applies before socket plugs, including damage type markers. Socket perks are separate.",
            ),
        );
        let inherited = gameplay_donor
            .map(|donor| donor.base_sandbox_perks.as_slice())
            .unwrap_or_default();
        if self.recipe.overrides.base_sandbox_perks.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(if inherited.is_empty() {
                    "Inheriting no base perks".to_owned()
                } else {
                    format!(
                        "Inheriting {} base-perk {}",
                        inherited.len(),
                        if inherited.len() == 1 { "row" } else { "rows" }
                    )
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
        let can_add = self
            .recipe
            .overrides
            .base_sandbox_perks
            .as_ref()
            .is_some_and(|perks| {
                perks.len() < 64
                    && choices
                        .iter()
                        .any(|choice| !perks.contains(&choice.perk_index))
            });
        let mut restore = false;
        let mut add = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Restore Gameplay Values").clicked() {
                restore = true;
            }
            if ui
                .add_enabled(can_add, egui::Button::new("+ Add Base Perk"))
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
                    .on_hover_text("Sandbox perk index");
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
                "Each base perk can appear only once.",
            );
        }
        for &perk in perks.iter() {
            if !choices.iter().any(|choice| choice.perk_index == perk) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Sandbox perk {perk} is not used in this install."),
                );
            }
        }
    }
}
