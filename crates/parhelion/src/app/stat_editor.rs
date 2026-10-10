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

/// The columns of a stat row, laid out as Destiny's item tooltip lays them: the name against the
/// right of its column, the bar, the value, and room for one action. Every row takes the same
/// widths, so the bars line up. A weapon's rows add the stat's in-game reading after the bar, and
/// its definition index before the name while internal stats show.
pub(crate) struct StatColumns {
    id: f32,
    name: f32,
    bar: f32,
    reading: f32,
}

/// Where one stat row's parts go. A column the rows do not have is empty.
pub(crate) struct StatCells {
    pub(crate) id: egui::Rect,
    pub(crate) name: egui::Rect,
    pub(crate) bar: egui::Rect,
    pub(crate) reading: egui::Rect,
    pub(crate) value: egui::Rect,
    pub(crate) action: egui::Rect,
}

impl StatColumns {
    const VALUE: f32 = 44.0;
    const ACTION: f32 = 44.0;

    /// Columns for rows named `names` in the width left on `ui`, with an `id` and a `reading`
    /// column of those widths, or none where a width is zero. The name column takes the widest
    /// name, up to two fifths of the width, and the bar the rest.
    pub(crate) fn new<'a>(
        ui: &egui::Ui,
        names: impl Iterator<Item = &'a str>,
        (id, reading): (f32, f32),
    ) -> Self {
        let widest = names
            .map(|name| text_width(ui, name))
            .fold(0.0, f32::max)
            .ceil();
        let width = ui.available_width();
        let mut columns = Self {
            id,
            name: widest.min(width * 0.4),
            bar: 0.0,
            reading,
        };
        // The bar takes what the other columns leave, less the gap before it.
        columns.bar = (width - columns.width(ui) - ui.spacing().item_spacing.x).max(40.0);
        columns
    }

    /// The widths of the columns a row has, in order.
    fn widths(&self) -> impl Iterator<Item = f32> {
        [
            self.id,
            self.name,
            self.bar,
            self.reading,
            Self::VALUE,
            Self::ACTION,
        ]
        .into_iter()
        .filter(|width| *width > 0.0)
    }

    /// A row's width, with the gaps between its columns.
    fn width(&self, ui: &egui::Ui) -> f32 {
        let gap = ui.spacing().item_spacing.x;
        self.widths().map(|width| width + gap).sum::<f32>() - gap
    }

    /// Allocates one row of `height` and splits it into its cells.
    fn cells(&self, ui: &mut egui::Ui, height: f32) -> StatCells {
        let gap = ui.spacing().item_spacing.x;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(self.width(ui), height), egui::Sense::hover());
        let mut left = rect.left();
        let mut cell = |width: f32| {
            let cell =
                egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(width, height));
            if width > 0.0 {
                left += width + gap;
            }
            cell
        };
        StatCells {
            id: cell(self.id),
            name: cell(self.name),
            bar: cell(self.bar),
            reading: cell(self.reading),
            value: cell(Self::VALUE),
            action: cell(Self::ACTION),
        }
    }

    /// Allocates one row and draws its name, bright or grey. Returns where its other parts go.
    pub(crate) fn row(&self, ui: &mut egui::Ui, name: &str, bright: bool) -> StatCells {
        let cells = self.cells(ui, ui.spacing().interact_size.y);
        let color = if bright {
            ui.visuals().strong_text_color()
        } else {
            style::secondary(ui.visuals())
        };
        let galley = egui::WidgetText::from(egui::RichText::new(name).color(color)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            self.name,
            egui::TextStyle::Body,
        );
        let at = cells.name.right_center() - egui::vec2(galley.size().x, galley.size().y / 2.0);
        ui.painter().galley(at, galley, color);
        cells
    }

    /// A small caption over the value column, saying what its numbers are.
    pub(crate) fn caption(&self, ui: &mut egui::Ui, value: &str) {
        let cells = self.cells(ui, 14.0);
        let color = style::secondary(ui.visuals());
        let galley =
            ui.painter()
                .layout_no_wrap(value.to_owned(), egui::FontId::proportional(11.0), color);
        let at = egui::pos2(
            cells.value.center().x - galley.size().x / 2.0,
            cells.value.bottom() - galley.size().y,
        );
        ui.painter().galley(at, galley, color);
    }
}

/// The width `text` takes at the body size.
fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

/// The base weapon's in-game value and this one's, for a stat the game shows as a bar.
fn bar_change(stat: &WeaponInvestmentStat, value: i32, removed: bool) -> Option<(i32, i32)> {
    (!removed && !stat.display_as_numeric && !stat.display_interpolation.is_empty()).then(|| {
        (
            stat.in_game_display_value(stat.value),
            stat.in_game_display_value(value),
        )
    })
}

/// One weapon stat as a Destiny stat row: its name, its bar and in-game reading, its raw value and
/// one action. `in_game` says whether the game's tooltip lists the stat. Returns the raw value an
/// edit set and the action clicked.
fn draw_weapon_stat_row(
    ui: &mut egui::Ui,
    columns: &StatColumns,
    (stat, added, removed, in_game): (&WeaponInvestmentStat, bool, bool, bool),
    (value, warning): (i32, Option<&str>),
) -> (Option<i32>, Option<StatRowAction>) {
    ui.push_id(("weapon-stat", stat.definition_index), |ui| {
        let modified = added || removed || value != stat.value;
        let cells = columns.row(ui, &stat.name, modified);
        draw_stat_identity(ui, &cells, stat, (added, removed, in_game, warning));
        draw_stat_reading(ui, &cells, stat, (value, removed, in_game));
        let edited = draw_stat_value(ui, cells.value, stat, (value, removed));
        let action = draw_stat_action(ui, &cells, stat, (added, removed, modified));
        (edited, action)
    })
    .inner
}

/// What the name says on hover, the definition index before it while internal stats show, and
/// the warning beside it for a stat whose in-game effect differs from its reading.
fn draw_stat_identity(
    ui: &mut egui::Ui,
    cells: &StatCells,
    stat: &WeaponInvestmentStat,
    (added, removed, in_game, warning): (bool, bool, bool, Option<&str>),
) {
    if cells.id.width() > 0.0 {
        let mut details = format!("Definition index {}", stat.definition_index);
        if let Some(hash) = stat.definition_hash {
            details.push_str(&format!("\nDefinition hash 0x{hash:08X}"));
        }
        let index = egui::RichText::new(stat.definition_index.to_string()).monospace();
        ui.put(cells.id, egui::Label::new(index))
            .on_hover_text(details);
    }
    let hint = if added {
        Some("Added. Not on the base weapon, so its effect depends on the weapon")
    } else if removed {
        Some("Removed from the weapon")
    } else if !in_game {
        Some("Hidden in game")
    } else {
        None
    };
    if let Some(hint) = hint {
        ui.interact(cells.name, ui.id().with("name"), egui::Sense::hover())
            .on_hover_text(hint);
    }
    if let Some(warning) = warning {
        // The name sits against the right of its cell, so the warning goes before it.
        let size = ui.spacing().interact_size.y;
        let right = cells.name.right() - text_width(ui, &stat.name) - ui.spacing().item_spacing.x;
        let rect = egui::Rect::from_min_size(
            egui::pos2((right - size).max(cells.name.left()), cells.name.top()),
            egui::vec2(size, cells.name.height()),
        );
        let mut icon = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        draw_authoring_warning_icon(&mut icon, warning);
    }
}

/// The in-game reading, where Destiny's tooltip puts it: after the bar for a stat the game shows as
/// one, and where the bar would start for one it shows as a number, such as Rounds Per Minute. A
/// stat the game hides reads nothing past its raw value, which is all it has. A removed stat says
/// so where its bar was.
fn draw_stat_reading(
    ui: &mut egui::Ui,
    cells: &StatCells,
    stat: &WeaponInvestmentStat,
    (value, removed, in_game): (i32, bool, bool),
) {
    let (cell, text, hint) = if removed {
        (
            cells.bar,
            egui::RichText::new("Removed").color(style::secondary(ui.visuals())),
            "Removed from the weapon",
        )
    } else if let Some((base, now)) = bar_change(stat, value, removed) {
        style::stat_bar(ui, cells.bar, (base, now), 100);
        if base != now {
            ui.interact(cells.bar, ui.id().with("bar"), egui::Sense::hover())
                .on_hover_text(format!("Base {base}"));
        }
        (
            cells.reading,
            egui::RichText::new(now.to_string()),
            "In-game value",
        )
    } else if !in_game {
        return;
    } else if stat.display_interpolation.is_empty() {
        (
            cells.bar,
            egui::RichText::new(value.to_string()),
            "No display scaling. Shows the raw value",
        )
    } else {
        // A bare number, since the name already says what it counts, such as Rounds Per Minute.
        (
            cells.bar,
            egui::RichText::new(stat.in_game_display_value(value).to_string()),
            "In-game value",
        )
    };
    let mut reading = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cell)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    reading
        .add(style::cut_label(&reading, text))
        .on_hover_text(hint);
}

/// The raw value's field. Returns the value an edit set.
fn draw_stat_value(
    ui: &mut egui::Ui,
    cell: egui::Rect,
    stat: &WeaponInvestmentStat,
    (mut value, removed): (i32, bool),
) -> Option<i32> {
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(cell).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    if removed {
        field.disable();
    }
    let hint = match (stat.minimum_value, stat.maximum_value) {
        (Some(minimum), Some(maximum)) => format!("Raw value, {minimum} to {maximum}"),
        (None, Some(maximum)) => format!("Raw value, up to {maximum}"),
        (Some(minimum), None) => format!("Raw value, from {minimum}"),
        (None, None) => "Raw value".to_owned(),
    };
    let name = if value == stat.value || removed {
        stat.name.clone()
    } else {
        format!("{}, base {}", stat.name, stat.value)
    };
    let input = egui::DragValue::new(&mut value)
        .speed(1.0)
        .clamp_existing_to_range(false);
    let input = match stat.value_range() {
        Some((minimum, maximum)) => input.range(minimum..=maximum),
        None => input,
    };
    let response = field.add(input).on_hover_text(hint);
    style::named_control(response, name)
        .changed()
        .then_some(value)
}

/// The row's one action: back to the base weapon's value once it differs, which also restores a
/// removed stat, and otherwise Remove. A stat the recipe added keeps its Remove in view. A base
/// stat's shows while the pointer is on its row, since removing one is rare, so the column marks
/// the changed rows.
fn draw_stat_action(
    ui: &mut egui::Ui,
    cells: &StatCells,
    stat: &WeaponInvestmentStat,
    (added, removed, modified): (bool, bool, bool),
) -> Option<StatRowAction> {
    let index = stat.definition_index;
    let mut actions = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cells.action)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if !added && modified {
        let restored = style::restore(&mut actions, &stat.name, &stat.value.to_string());
        return restored.then_some(if removed {
            StatRowAction::RestoreDonor(index)
        } else {
            StatRowAction::RestoreValue(index)
        });
    }
    if !added && !ui.rect_contains_pointer(cells.id.union(cells.action)) {
        return None;
    }
    let hint = if added {
        "Remove this added stat"
    } else {
        "Remove this stat"
    };
    let remove = actions
        .add(egui::Button::new("×").frame(false).small())
        .on_hover_text(hint);
    style::focus_ring(&actions, &remove);
    style::named_control(remove, format!("Remove {}", stat.name))
        .clicked()
        .then_some(if added {
            StatRowAction::RemoveAdded(index)
        } else {
            StatRowAction::RemoveDonor(index)
        })
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

/// Rounds Per Minute's definition hash, which the build is known to convert at another weapon
/// type's rates in some cases.
pub(super) const ROUNDS_PER_MINUTE_HASH: u32 = 0xFF66_4809;

/// Draws the stat table. `order` lists the stats the game shows, by definition index, in the
/// order it shows them. `warnings` pairs a stat definition hash with why its in-game effect
/// differs from what the table suggests.
pub(super) fn draw_investment_stats(
    ui: &mut egui::Ui,
    values: &mut Vec<WeaponStatOverride>,
    removed_definitions: &mut Vec<u16>,
    donor: &WeaponDonor,
    (show_internal_stats, order): (bool, &[u16]),
    warnings: &[(u32, String)],
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
    // As the game lists them, then the stats it does not show, which keep their own order.
    rows.sort_by_key(|(stat, _, _)| {
        order
            .iter()
            .position(|index| *index == stat.definition_index)
            .unwrap_or(order.len())
    });

    let shown = rows
        .iter()
        .filter(|(stat, _, _)| {
            show_internal_stats || !is_internal_weapon_stat(stat.definition_index)
        })
        .map(|(stat, added, removed)| {
            let value = values
                .iter()
                .find(|candidate| candidate.definition_index == stat.definition_index)
                .map_or(stat.value, |value| value.value);
            (stat, *added, *removed, value)
        })
        .collect::<Vec<_>>();
    // Only a bar's reading follows it, so the readings take the room of the widest of those, and
    // at least three digits', and the bars keep their length while a value is dragged. The others
    // read where the bar would start.
    let reading = shown
        .iter()
        .filter_map(|(stat, _, removed, value)| bar_change(stat, *value, *removed))
        .map(|(_, now)| now.to_string())
        .chain(["000".to_owned()])
        .map(|text| text_width(ui, &text))
        .fold(0.0, f32::max)
        .ceil();
    let id = if show_internal_stats { 34.0 } else { 0.0 };
    let columns = StatColumns::new(
        ui,
        shown.iter().map(|(stat, _, _, _)| stat.name.as_str()),
        (id, reading),
    );
    columns.caption(ui, "Raw");
    let mut row_action = None;
    let mut edited = false;
    for &(stat, added, removed, value) in &shown {
        let warning = warnings
            .iter()
            .find(|(hash, _)| Some(*hash) == stat.definition_hash)
            .map(|(_, warning)| warning.as_str());
        // A weapon whose stat group is unknown counts every stat as shown.
        let in_game = order.is_empty() || order.contains(&stat.definition_index);
        let (changed, action) = draw_weapon_stat_row(
            ui,
            &columns,
            (stat, added, removed, in_game),
            (value, warning),
        );
        if let Some(value) = changed {
            edited = true;
            update_investment_stat_value(values, stat.definition_index, stat.value, value, added);
        }
        row_action = row_action.or(action);
    }
    edited |= row_action.is_some();
    match row_action {
        Some(
            StatRowAction::RemoveAdded(definition_index)
            | StatRowAction::RestoreValue(definition_index),
        ) => {
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
                            ui.close();
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

pub(super) fn merge_stat_profile_investment_rows(
    overrides: &mut WeaponRecipeOverrides,
    gameplay_stats: &[WeaponInvestmentStat],
    profile_stats: &[WeaponInvestmentStat],
) {
    let profile_definitions = profile_stats
        .iter()
        .map(|stat| stat.definition_index)
        .collect::<BTreeSet<_>>();
    overrides
        .removed_investment_stats
        .retain(|definition| !profile_definitions.contains(definition));

    for stat in profile_stats {
        let inherited = gameplay_stats
            .iter()
            .any(|gameplay| gameplay.definition_index == stat.definition_index);
        let already_authored = overrides
            .investment_stats
            .iter()
            .any(|value| value.definition_index == stat.definition_index);
        if !inherited && !already_authored {
            overrides.investment_stats.push(WeaponStatOverride {
                definition_index: stat.definition_index,
                value: stat.value,
            });
        }
    }
    overrides
        .investment_stats
        .sort_by_key(|value| value.definition_index);
}

#[derive(Clone, Copy)]
pub(super) enum StatRowAction {
    RemoveAdded(u16),
    RemoveDonor(u16),
    RestoreDonor(u16),
    /// Gives a stat the base weapon carries its base value again.
    RestoreValue(u16),
}

pub(super) const fn is_internal_weapon_stat(definition_index: u16) -> bool {
    matches!(definition_index, 0 | 1 | 13)
}
