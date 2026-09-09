use eframe::egui;

use crate::{
    app::PlugSelectionMode,
    catalog::{Catalog, ItemDef},
};

use super::{
    model::{
        AllocationFeedback, STAT_NAMES, State, TARGET_MAX, mode_allows_cross_group_allocations,
    },
    solver::{allocation_socket_groups, format_shortfalls, selected_totals, solve},
};

pub(in crate::app::equipment) const INLINE_CONTENT_WIDTH: f32 = 582.0;
const STAT_CELL_WIDTH: f32 = 150.0;

/// Draws the compact target editor and applies a solved allocation directly to
/// `plugs` when requested. Returns true only when the authored plug list changed.
pub(in crate::app::equipment) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &mut [Option<u64>],
    mode: PlugSelectionMode,
    state: &mut State,
) -> bool {
    let socket_groups = allocation_socket_groups(catalog, item, plugs.len());
    if socket_groups.iter().all(Option::is_none) {
        return false;
    }

    let current = selected_totals(catalog, item, plugs);
    let mut solve_requested = false;
    let mut clear_requested = false;
    let mut changed = false;

    ui.horizontal_top(|ui| {
        ui.add_space((ui.available_width() - INLINE_CONTENT_WIDTH).max(0.0));
        ui.spacing_mut().item_spacing.x = 5.0;
        let cross_group = mode_allows_cross_group_allocations(mode);
        let any_allocation_socket = socket_groups.iter().any(Option::is_some);
        let targets_changed = draw_stat_cells(
            ui,
            catalog,
            &mut state.targets,
            current,
            if cross_group {
                [any_allocation_socket; 6]
            } else {
                [
                    socket_groups[0].is_some(),
                    socket_groups[0].is_some(),
                    socket_groups[0].is_some(),
                    socket_groups[1].is_some(),
                    socket_groups[1].is_some(),
                    socket_groups[1].is_some(),
                ]
            },
        );
        if targets_changed {
            state.feedback = None;
        }

        ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
            let has_targets = state.targets.iter().any(|target| *target > 0);
            if ui
                .add_enabled(has_targets, egui::Button::new("Adjust stats"))
                .on_disabled_hover_text("Set at least one target first")
                .clicked()
            {
                solve_requested = true;
            }
            if ui
                .add_enabled(has_targets, egui::Button::new("Clear"))
                .clicked()
            {
                clear_requested = true;
            }
        });
    });

    if clear_requested {
        state.targets = [0; 6];
        state.feedback = None;
    } else if solve_requested {
        match solve(catalog, item, plugs, mode, state.targets) {
            Ok(solution) => {
                changed = solution.changed > 0;
                plugs.copy_from_slice(&solution.plugs);
                state.feedback = Some(AllocationFeedback {
                    text: if solution.changed == 0 {
                        "Targets already met".to_owned()
                    } else {
                        "Stats adjusted · Targets met".to_owned()
                    },
                    is_error: false,
                });
            }
            Err(failure) => {
                let shortfalls = format_shortfalls(state.targets, failure.best_totals);
                changed = failure.changed > 0;
                plugs.copy_from_slice(&failure.plugs);
                state.feedback = Some(AllocationFeedback {
                    text: if shortfalls.is_empty() {
                        failure
                            .reason
                            .unwrap_or_else(|| "No closer stat combination is available".to_owned())
                    } else {
                        format!("Closest available · {shortfalls} short")
                    },
                    is_error: true,
                });
            }
        }
    }

    changed
}

pub(in crate::app::equipment) fn is_available(
    catalog: &Catalog,
    item: &ItemDef,
    plug_count: usize,
) -> bool {
    allocation_socket_groups(catalog, item, plug_count)
        .iter()
        .any(Option::is_some)
}

fn draw_stat_cells(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    targets: &mut [u16; 6],
    current: [u16; 6],
    available: [bool; 6],
) -> bool {
    let mut changed = false;
    egui::Grid::new(ui.id().with("armor-stat-target-grid"))
        .num_columns(3)
        .spacing(egui::vec2(4.0, 3.0))
        .show(ui, |ui| {
            for row in 0..2 {
                for index in (row * 3)..(row * 3 + 3) {
                    changed |= draw_stat_cell(ui, catalog, targets, current, available, index);
                }
                ui.end_row();
            }
        });
    changed
}

fn draw_stat_cell(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    targets: &mut [u16; 6],
    current: [u16; 6],
    available: [bool; 6],
    index: usize,
) -> bool {
    ui.allocate_ui_with_layout(
        egui::vec2(STAT_CELL_WIDTH, 21.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(STAT_CELL_WIDTH);
            ui.spacing_mut().item_spacing.x = 3.0;
            let target_ui = ui.add_enabled_ui(available[index], |ui| {
                ui.add_sized(
                    [34.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut targets[index])
                        .range(0..=TARGET_MAX)
                        .speed(1.0),
                )
            });
            let response = target_ui.inner;
            response.widget_info(|| {
                let mut info =
                    egui::WidgetInfo::drag_value(available[index], f64::from(targets[index]));
                info.label = Some(format!("{} target", STAT_NAMES[index]));
                info
            });
            let target_changed = response.changed();
            let tooltip = format!(
                "Minimum {} to reach with Adjust stats. Set to 0 to ignore this stat.",
                STAT_NAMES[index]
            );
            response
                .on_disabled_hover_text("This stat has no allocation socket on the selected item")
                .on_hover_text(&tooltip);
            target_ui
                .response
                .on_disabled_hover_text("This stat has no allocation socket on the selected item")
                .on_hover_text(tooltip);
            let unmet = targets[index] > 0 && current[index] < targets[index];
            let text = egui::RichText::new(current[index].to_string()).small();
            if unmet {
                ui.add_sized(
                    [22.0, ui.spacing().interact_size.y],
                    egui::Label::new(text.color(ui.visuals().error_fg_color)),
                )
                .on_hover_text("Current item stat");
            } else {
                ui.add_sized([22.0, ui.spacing().interact_size.y], egui::Label::new(text))
                    .on_hover_text("Current item stat");
            }
            ui.add_enabled_ui(available[index], |ui| {
                ui.horizontal(|ui| {
                    if let Some(icon) = catalog.armor_stat_icon_texture(ui.ctx(), STAT_NAMES[index])
                    {
                        ui.add(egui::Image::new((icon.id(), egui::vec2(14.0, 14.0))))
                            .on_hover_text(STAT_NAMES[index]);
                    }
                    ui.label(STAT_NAMES[index]);
                });
            });
            target_changed
        },
    )
    .inner
}
