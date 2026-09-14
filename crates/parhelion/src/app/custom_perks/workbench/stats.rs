//! Always-equipped gameplay bonuses, alongside the effect builder.
use super::*;

impl Workbench {
    pub(super) fn draw_stats(
        &mut self,
        ui: &mut egui::Ui,
        catalog: Option<&InvestmentCatalog>,
        recipe: &mut PerkRecipe,
    ) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.strong("Stat Bonuses");
            sundial::investment::draw_authoring_info_icon(
                ui,
                "Applied while the perk is equipped. Effect triggers do not control them.",
            );
        });
        let source_stats = catalog
            .map(|catalog| {
                catalog
                    .item_stat_contributions(recipe.template_plug.parse_u32().unwrap_or_default())
            })
            .unwrap_or_default();
        let mut stats = catalog
            .map(InvestmentCatalog::perk_stat_choices)
            .unwrap_or_default();
        stats.sort_by_cached_key(|stat| stat.name.to_lowercase());
        let mut table = crate::app::stat_editor::table::Table::new(ui, false, false);
        table.name = table.name.min(200.0);
        let mut remove = None;
        if !recipe.stats.is_empty() {
            table.show(
                ui,
                "perk-stat-bonuses",
                "Bonus",
                "Added to the weapon while this perk is equipped. Negative values reduce the stat.",
                |ui| {
                    for stat in &mut recipe.stats {
                        let name = stats
                            .iter()
                            .find(|choice| choice.definition_index == stat.definition_index)
                            .map_or_else(
                                || format!("Stat {}", stat.definition_index),
                                |choice| choice.name.clone(),
                            );
                        crate::app::stat_editor::left_cell(
                            ui,
                            table.name,
                            egui::Label::new(&name).truncate().halign(egui::Align::LEFT),
                        )
                        .on_hover_text(&name);
                        // A perk stores a delta, not a weapon's absolute value. Do not apply
                        // the weapon display curve or its range to a negative bonus.
                        let response = table.value(ui, &mut stat.value, None, true);
                        if let Some(original) = source_stats
                            .iter()
                            .find(|source| source.definition_index == stat.definition_index)
                        {
                            response.context_menu(|ui| {
                                if ui
                                    .add_enabled(
                                        stat.value != original.value,
                                        egui::Button::new(format!("Reset to {}", original.value)),
                                    )
                                    .clicked()
                                {
                                    stat.value = original.value;
                                    ui.close_menu();
                                }
                            });
                        }
                        if crate::app::stat_editor::table::action(
                            ui,
                            table.action,
                            "×",
                            &format!("Remove {name} Bonus"),
                        )
                        .clicked()
                        {
                            remove = Some(stat.definition_index);
                        }
                        ui.end_row();
                    }
                },
            );
        }
        if let Some(index) = remove {
            recipe.stats.retain(|stat| stat.definition_index != index);
        }
        ui.add_enabled_ui(recipe.stats.len() < 16, |ui| {
            if let Some(index) = pickers::popup(
                ui,
                "add-perk-stat",
                "+ Add Stat",
                &mut self.stat_query,
                |ui, query, reset, height| {
                    let choices = stats
                        .iter()
                        .filter(|choice| {
                            pickers::matches(query, &choice.name)
                                && !recipe
                                    .stats
                                    .iter()
                                    .any(|stat| stat.definition_index == choice.definition_index)
                        })
                        .collect::<Vec<_>>();
                    pickers::results(
                        ui,
                        "perk-stat-results",
                        choices.len(),
                        height,
                        reset,
                        crate::app::style::list_row_height(ui),
                        |ui, index| {
                            crate::app::style::list_row(ui, false, &choices[index].name)
                                .clicked()
                                .then_some(choices[index].definition_index)
                        },
                    )
                },
            ) {
                recipe.stats.push(WeaponStatOverride {
                    definition_index: index,
                    value: 0,
                });
            }
        });
    }
}
