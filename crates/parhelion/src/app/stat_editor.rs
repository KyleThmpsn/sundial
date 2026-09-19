//! Focused stat editor controls; recipe mutation occurs on user actions.
use super::*;
pub(crate) mod table;

pub(crate) fn left_cell(
    ui: &mut egui::Ui,
    width: f32,
    widget: impl egui::Widget,
) -> egui::Response {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center)
            .with_main_align(egui::Align::Min)
            .with_main_justify(true),
        |ui| ui.add(widget),
    )
    .inner
}

pub(super) fn draw_investment_stat_action(
    ui: &mut egui::Ui,
    action_width: f32,
    definition_index: u16,
    is_added: bool,
    is_removed: bool,
) -> Option<StatRowAction> {
    let (label, tooltip, action) = if is_added {
        (
            "×",
            "Remove this added investment stat",
            StatRowAction::RemoveAdded(definition_index),
        )
    } else if is_removed {
        (
            "↺",
            "Restore this gameplay-donor investment stat row",
            StatRowAction::RestoreDonor(definition_index),
        )
    } else {
        (
            "×",
            "Remove this investment stat row from the authored weapon",
            StatRowAction::RemoveDonor(definition_index),
        )
    };
    table::action(ui, action_width, label, tooltip)
        .clicked()
        .then_some(action)
}

pub(super) fn update_investment_stat_value(
    values: &mut Vec<WeaponStatOverride>,
    definition_index: u16,
    donor_value: i32,
    effective_value: i32,
    is_added: bool,
) {
    if !is_added && effective_value == donor_value {
        values.retain(|value| value.definition_index != definition_index);
    } else {
        match values
            .iter_mut()
            .find(|value| value.definition_index == definition_index)
        {
            Some(value) => value.value = effective_value,
            None => values.push(WeaponStatOverride {
                definition_index,
                value: effective_value,
            }),
        }
    }
}

pub(super) fn draw_investment_stats(
    ui: &mut egui::Ui,
    values: &mut Vec<WeaponStatOverride>,
    removed_definitions: &mut Vec<u16>,
    donor: &WeaponDonor,
    show_internal_stats: bool,
) {
    ui.add_space(3.0);

    let native_indices = donor
        .investment_stats
        .iter()
        .map(|stat| stat.definition_index)
        .collect::<BTreeSet<_>>();
    let mut rows = donor
        .investment_stats
        .iter()
        .cloned()
        .map(|stat| {
            let removed = removed_definitions.contains(&stat.definition_index);
            (stat, false, removed)
        })
        .collect::<Vec<_>>();
    let mut added_rows = values
        .iter()
        .filter(|value| !native_indices.contains(&value.definition_index))
        .map(|value| {
            let mut stat = donor
                .addable_investment_stats
                .iter()
                .find(|stat| stat.definition_index == value.definition_index)
                .cloned()
                .unwrap_or_else(|| WeaponInvestmentStat {
                    definition_index: value.definition_index,
                    definition_hash: None,
                    name: "Unknown weapon stat".to_owned(),
                    value: value.value,
                    minimum_value: None,
                    maximum_value: None,
                    display_as_numeric: false,
                    is_linear: false,
                    display_interpolation: Vec::new(),
                });
            stat.value = value.value;
            (stat, true, false)
        })
        .collect::<Vec<_>>();
    added_rows.sort_by_key(|(stat, _, _)| stat.definition_index);
    rows.extend(added_rows);

    let table = table::Table::new(ui, show_internal_stats, true);
    let (id_width, stat_width, display_width, action_width) =
        (table.id, table.name, table.preview, table.action);
    let mut row_action = None;
    let mut edited = false;
    table.show(ui, "dynamic_investment_stats", "Raw Value", "Raw value stored in the weapon's investment block", |ui| {
            for (stat, is_added, is_removed) in rows.iter().filter(|(stat, _, _)| {
                show_internal_stats || !is_internal_weapon_stat(stat.definition_index)
            })
            {
                let mut id_details = format!("Definition index {}", stat.definition_index);
                if let Some(hash) = stat.definition_hash {
                    id_details.push_str(&format!("\nDefinition hash 0x{hash:08X}"));
                }
                if show_internal_stats {
                left_cell(ui,
                    id_width,
                    egui::Label::new(
                        egui::RichText::new(stat.definition_index.to_string()).monospace(),
                    )
                    .halign(egui::Align::LEFT),
                )
                .on_hover_text(id_details);
                }
                let name_response = left_cell(ui,
                    stat_width,
                    egui::Label::new(if *is_removed {
                        format!("{}  ·  Removed", stat.name)
                    } else if *is_added {
                        format!("{}  ·  Added", stat.name)
                    } else {
                        stat.name.clone()
                    })
                        .truncate()
                        .halign(egui::Align::LEFT),
                );
                if *is_added {
                    name_response.on_hover_text(
                        "This definition is not present in the gameplay donor. Parhelion will append a canonical investment row. Runtime behavior remains weapon-dependent.",
                    );
                }
                let mut effective_value = values
                    .iter()
                    .find(|candidate| candidate.definition_index == stat.definition_index)
                    .map_or(stat.value, |value| value.value);
                let value_range = stat.value_range();
                let response = table.value(ui, &mut effective_value, value_range, !*is_removed)
                    .on_hover_text(match (stat.minimum_value, stat.maximum_value) {
                        (Some(minimum), Some(maximum)) => format!("Direct package value. Native range {minimum}–{maximum}."),
                        (None, Some(maximum)) => format!("Direct package value. Native maximum {maximum}. No minimum is defined."),
                        (Some(minimum), None) => format!("Direct package value. Native minimum {minimum}. No maximum is defined."),
                        (None, None) => "Direct package value. Drag or type to edit.".to_owned(),
                    });
                if response.changed() {
                    edited = true;
                    update_investment_stat_value(
                        values,
                        stat.definition_index,
                        stat.value,
                        effective_value,
                        *is_added,
                    );
                }
                let display_label = if *is_removed {
                    "N/A".to_owned()
                } else {
                    stat.in_game_display_label(effective_value)
                };
                let display_tooltip = if *is_added && stat.display_interpolation.is_empty() {
                    "The active stat display scaling has no curve for this added definition. Shown as the stored package value"
                } else if stat.display_interpolation.is_empty() {
                    "No native display curve is defined. Shown as the stored package value"
                } else if stat.display_as_numeric {
                    "Previewed from the active decoded numeric display curve"
                } else {
                    "Previewed from the active decoded display curve. The game may render this as a bar"
                };
                left_cell(ui,
                    display_width,
                    egui::Label::new(egui::RichText::new(display_label).monospace())
                        .halign(egui::Align::LEFT),
                )
                .on_hover_text(display_tooltip);
                row_action = row_action.or_else(|| {
                    draw_investment_stat_action(
                        ui,
                        action_width,
                        stat.definition_index,
                        *is_added,
                        *is_removed,
                    )
                });
                ui.end_row();
            }
        });
    edited |= row_action.is_some();
    match row_action {
        Some(StatRowAction::RemoveAdded(definition_index)) => {
            values.retain(|value| value.definition_index != definition_index);
        }
        Some(StatRowAction::RemoveDonor(definition_index)) => {
            values.retain(|value| value.definition_index != definition_index);
            removed_definitions.push(definition_index);
        }
        Some(StatRowAction::RestoreDonor(definition_index)) => {
            removed_definitions.retain(|value| *value != definition_index);
        }
        None => {}
    }

    let existing_indices = donor
        .investment_stats
        .iter()
        .map(|stat| stat.definition_index)
        .chain(values.iter().map(|value| value.definition_index))
        .collect::<BTreeSet<_>>();
    let choices = donor
        .addable_investment_stats
        .iter()
        .filter(|stat| !existing_indices.contains(&stat.definition_index))
        .filter(|stat| show_internal_stats || !is_internal_weapon_stat(stat.definition_index))
        .collect::<Vec<_>>();
    let mut add_definition = None;
    ui.add_space(3.0);
    ui.add_enabled_ui(!choices.is_empty(), |ui| {
        ui.menu_button("+ Add Stat", |ui| {
            ui.set_min_width(220.0);
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for stat in &choices {
                        let label = format!("{}  {}", stat.definition_index, stat.name);
                        let mut response = ui.button(label);
                        if let Some(hash) = stat.definition_hash {
                            response = response.on_hover_text(format!(
                                "Definition index {}\nDefinition hash 0x{hash:08X}",
                                stat.definition_index
                            ));
                        }
                        if response.clicked() {
                            add_definition = Some((stat.definition_index, stat.value));
                            ui.close_menu();
                        }
                    }
                });
        });
    });
    if let Some((definition_index, value)) = add_definition {
        edited = true;
        values.push(WeaponStatOverride {
            definition_index,
            value,
        });
    }
    if edited {
        values.sort_by_key(|value| value.definition_index);
        removed_definitions.sort_unstable();
        removed_definitions.dedup();
    }
}
