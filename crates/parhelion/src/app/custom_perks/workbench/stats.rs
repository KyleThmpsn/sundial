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
        ui.horizontal_wrapped(|ui| {
            // Stat Bonuses and Effects are the two sections of this page, so they carry the
            // same heading level and the same control sizing.
            crate::app::style::compact_controls(ui);
            ui.heading("Stat Bonuses");
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
            table.show(ui, "perk-stat-bonuses", "Bonus", "", |ui| {
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
            });
        }
        if let Some(index) = remove {
            recipe.stats.retain(|stat| stat.definition_index != index);
        }
        ui.add_enabled_ui(recipe.stats.len() < 16, |ui| {
            if let Some(index) = pickers::browser_with_toolbar(
                ui,
                "add-perk-stat",
                "+ Add Stat",
                "Add Stat",
                &mut self.stat_query,
                |ui, query, opened, _| {
                    let changed = ui
                        .horizontal(|ui| {
                            let width = (ui.available_width() - pickers::CLEAR_WIDTH).max(160.0);
                            sundial::ui::catalog::search(ui, query, opened, width, "Search Stats")
                        })
                        .inner;
                    let words = query.trim().to_lowercase();
                    let choices = stats
                        .iter()
                        .filter(|choice| {
                            pickers::matches(&words, &choice.name)
                                && !recipe
                                    .stats
                                    .iter()
                                    .any(|stat| stat.definition_index == choice.definition_index)
                        })
                        .collect::<Vec<_>>();
                    let keys = choices
                        .iter()
                        .map(|choice| u64::from(choice.definition_index))
                        .collect::<Vec<_>>();
                    pickers::BrowserList {
                        keys: &keys,
                        // The result count takes a line above the list.
                        height: (ui.available_height() - 24.0).max(160.0),
                        reset: opened || changed,
                        row_height: crate::app::style::list_row_height(ui),
                        select: None,
                    }
                    .draw_activating(
                        ui,
                        |ui, index, selected| {
                            crate::app::style::list_row(ui, selected, &choices[index].name)
                        },
                        |ui, index, activated| {
                            let choice = choices[index];
                            ui.heading(&choice.name);
                            // A double-click adds the stat, as Add Stat does.
                            let add = ui.add(crate::app::style::primary(ui, "Add Stat")).clicked()
                                || activated;
                            if let Some(stock) = source_stats
                                .iter()
                                .find(|stat| stat.definition_index == choice.definition_index)
                            {
                                ui.label(
                                    egui::RichText::new(format!("Stock Bonus {:+}", stock.value))
                                        .color(crate::app::style::secondary(ui.visuals())),
                                );
                            }
                            add.then_some(choice.definition_index)
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
