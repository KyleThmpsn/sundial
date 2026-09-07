//! Armor-stat target, status, and preview rendering.

use crate::app::account_workspace as account;

use super::*;

pub(in crate::app) fn draw_entry_button(ui: &mut egui::Ui, editable: bool) -> egui::Response {
    ui.add_enabled(editable, egui::Button::new("Armor Stats…"))
        .on_hover_text("Adjust stat plugs across all equipped armor")
}

pub(in crate::app::equipment) fn equipped_totals(
    document: &account::WorkspaceDocument,
    catalog: &Catalog,
    character_index: usize,
) -> [u16; 6] {
    let equipment = account::equipped_item_snapshots(document, character_index).unwrap_or_default();
    let mut totals = [0_u16; 6];

    for slot in ARMOR_SLOTS {
        let (label, bucket_hash) = SLOTS
            .iter()
            .find_map(|(known, label, bucket)| (*known == *slot).then_some((*label, *bucket)))
            .unwrap_or((slot, 0));
        let piece = read_snapshot_piece(
            catalog,
            slot,
            label,
            bucket_hash,
            equipment.iter().find(|item| item.slot == *slot),
        );
        let Some(item) = piece.item.as_ref() else {
            continue;
        };
        let piece_totals =
            armor_stat_allocation::selected_totals(catalog, item, &piece.current_plugs);
        for (total, value) in totals.iter_mut().zip(piece_totals) {
            *total = total.saturating_add(value);
        }
    }

    cap_u16_totals(totals)
}

pub(in crate::app) fn draw_window(
    app: &mut SundialApp,
    context: &egui::Context,
    character_index: usize,
) {
    let mut state = std::mem::take(&mut app.armor_stats_adjuster);
    if state.character_index != character_index && state.open {
        state.open(character_index);
    }
    if !state.open {
        app.armor_stats_adjuster = state;
        return;
    }

    let allow_inventory_swaps = state.allow_inventory_swaps.unwrap_or(true);
    refresh_input(
        &mut state,
        &app.document,
        &app.manifest,
        character_index,
        app.plug_selection_mode,
        allow_inventory_swaps,
    );
    refresh_preview(&mut state, context);

    let mut open = state.open;
    let mut targets_changed = false;
    let mut clear_requested = false;
    let mut apply_requested = false;
    let mut requested_mode = app.plug_selection_mode;
    let window_size =
        crate::app::equipment::dialog_size_constraints(context, WINDOW_SIZE, WINDOW_MIN_SIZE);

    egui::Window::new("Armor Stats Adjustments")
        .id(egui::Id::new((
            "armor-stats-adjuster",
            character_index,
            state.window_generation,
            window_size.compact,
        )))
        .collapsible(false)
        .resizable(true)
        .default_size(window_size.default)
        .min_size(window_size.min)
        .max_size(window_size.max)
        .open(&mut open)
        .show(context, |ui| {
            egui::ScrollArea::vertical()
                .id_salt((
                    "armor-stats-adjuster-scroll",
                    character_index,
                    state.window_generation,
                    window_size.compact,
                ))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_header(ui, &state);
                    ui.add_space(6.0);
                    targets_changed |= draw_targets(ui, &app.manifest, &mut state);
                    ui.add_space(8.0);
                    let swap_setting_changed = draw_controls(
                        ui,
                        &mut state,
                        &mut requested_mode,
                        &mut clear_requested,
                        &mut apply_requested,
                    );
                    if swap_setting_changed {
                        state.source_key = None;
                        state.input = None;
                        state.preview = None;
                        state.preview_task = None;
                        state.preview_due_at =
                            Some(context.input(|input| input.time) + PREVIEW_DEBOUNCE_SECONDS);
                    }
                    if app.preferences.show_safety_warnings {
                        crate::app::draw_plug_selection_warning(ui, requested_mode);
                    }
                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(5.0);
                    draw_preview(ui, &app.manifest, &state);
                });
        });
    state.open = open;

    if clear_requested {
        state.targets = [0; 6];
        state.preview = None;
        state.preview_task = None;
        state.preview_due_at = None;
        state.feedback = None;
        state.preserve_feedback_once = false;
    } else if targets_changed {
        state.preview = None;
        state.preview_task = None;
        state.preview_due_at = Some(context.input(|input| input.time) + PREVIEW_DEBOUNCE_SECONDS);
        context.request_repaint_after(Duration::from_secs_f64(PREVIEW_DEBOUNCE_SECONDS));
        state.feedback = None;
        state.preserve_feedback_once = false;
    }

    if requested_mode != app.plug_selection_mode {
        if requested_mode == PlugSelectionMode::AnyPlug
            && !app.preferences.really_unsafe_warning_acknowledged
        {
            app.remember_plug_selection_mode_after_confirmation = false;
            app.confirmation = Some(ConfirmationDialog::ReallyUnsafe);
        } else {
            app.plug_selection_mode = requested_mode;
            state.source_key = None;
            state.input = None;
            state.preview = None;
            state.preview_task = None;
            state.preview_due_at =
                Some(context.input(|input| input.time) + PREVIEW_DEBOUNCE_SECONDS);
            context.request_repaint_after(Duration::from_secs_f64(PREVIEW_DEBOUNCE_SECONDS));
            state.feedback = None;
            state.preserve_feedback_once = false;
        }
    }

    if apply_requested {
        apply_preview(app, &mut state, character_index);
    }

    app.armor_stats_adjuster = state;
}

fn draw_header(ui: &mut egui::Ui, state: &State) {
    const INTRO: &str = "Set overall goals for equipped armor. The closest safe configuration is previewed before it is applied.";
    ui.horizontal(|ui| {
        let available = ui.available_width();
        if available < 700.0 {
            ui.allocate_ui_with_layout(
                egui::vec2(available, ui.spacing().interact_size.y),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if let Some((text, detail, color)) = status_text(ui, state) {
                        ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                            .on_hover_text(detail);
                    }
                },
            );
            return;
        }
        let status_width = if state.feedback.is_some() {
            (available * 0.52).clamp(260.0, 480.0)
        } else {
            (available * 0.36).clamp(170.0, 290.0)
        };
        let intro_width = (available - status_width - ui.spacing().item_spacing.x).max(120.0);
        ui.add_sized(
            [intro_width, ui.spacing().interact_size.y],
            egui::Label::new(egui::RichText::new(INTRO).weak()).truncate(),
        )
        .on_hover_text(INTRO);
        ui.allocate_ui_with_layout(
            egui::vec2(status_width, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if let Some((text, detail, color)) = status_text(ui, state) {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).truncate())
                        .on_hover_text(detail);
                }
            },
        );
    });
}

fn status_text(ui: &egui::Ui, state: &State) -> Option<(String, String, egui::Color32)> {
    if let Some(feedback) = &state.feedback {
        return Some((
            feedback.text.clone(),
            feedback
                .detail
                .clone()
                .unwrap_or_else(|| feedback.text.clone()),
            if feedback.is_error {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().strong_text_color()
            },
        ));
    }
    if !state.targets.iter().any(|target| *target > 0) {
        return Some((
            "Set one or more goals".to_owned(),
            "Set one or more goals".to_owned(),
            ui.visuals().weak_text_color(),
        ));
    }
    if state.preview_task.is_some() || state.preview_due_at.is_some() {
        return Some((
            "Updating preview…".to_owned(),
            "Finding the closest valid armor configuration.".to_owned(),
            ui.visuals().weak_text_color(),
        ));
    }
    let solution = state.preview.as_ref()?;
    if solution.exact {
        Some((
            "Targets met".to_owned(),
            "Every selected armor stat goal can be met.".to_owned(),
            ui.visuals().strong_text_color(),
        ))
    } else {
        let missed = solution
            .shortfalls
            .iter()
            .filter(|shortfall| **shortfall > 0)
            .count();
        let detail = format_shortfalls(solution.shortfalls);
        Some((
            format!("Closest match · {missed} goals short"),
            detail,
            ui.visuals().warn_fg_color,
        ))
    }
}

fn draw_targets(ui: &mut egui::Ui, catalog: &Catalog, state: &mut State) -> bool {
    let current = state
        .input
        .as_ref()
        .map_or([0; 6], |input| input.current_totals);
    let projected = state
        .preview
        .as_ref()
        .map_or(current, |solution| solution.projected_totals);
    let available = ui.available_width();
    let columns = if available >= 700.0 { 2 } else { 1 };
    let cell_width = if columns == 2 {
        (available - ui.spacing().item_spacing.x).max(0.0) / 2.0
    } else {
        available
    };
    let mut changed = false;

    egui::Grid::new(ui.id().with("armor-stat-goal-grid"))
        .num_columns(columns)
        .spacing(egui::vec2(ui.spacing().item_spacing.x, 7.0))
        .show(ui, |ui| {
            for index in 0..6 {
                ui.allocate_ui_with_layout(
                    egui::vec2(cell_width, 72.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(cell_width);
                        draw_stat_heading(
                            ui,
                            catalog,
                            index,
                            current[index],
                            projected[index],
                            state.targets[index] > projected[index],
                        );
                        ui.spacing_mut().slider_width = (cell_width - 48.0).max(120.0);
                        let response = ui.add(
                            egui::Slider::new(
                                &mut state.targets[index],
                                0..=armor_stat_allocation::TARGET_MAX,
                            )
                            .step_by(1.0)
                            .show_value(true),
                        );
                        changed |= response.changed();
                        response.on_hover_text(format!(
                            "Minimum overall {}. 0 ignores this stat; 100 is the useful cap.",
                            armor_stat_allocation::STAT_NAMES[index]
                        ));
                    },
                );
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    changed
}

fn draw_stat_heading(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    index: usize,
    current: u16,
    projected: u16,
    short: bool,
) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(112.0, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.strong(armor_stat_allocation::STAT_NAMES[index]);
                if let Some(icon) = catalog
                    .armor_stat_icon_texture(ui.ctx(), armor_stat_allocation::STAT_NAMES[index])
                {
                    ui.add(
                        egui::Image::new((icon.id(), egui::vec2(14.0, 14.0)))
                            .tint(ui.visuals().text_color()),
                    );
                }
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_stat_readout(ui, 110.0, "Projected", projected, short);
            draw_stat_readout(ui, 100.0, "Current", current, false);
        });
    });
}

fn draw_stat_readout(ui: &mut egui::Ui, width: f32, label: &str, value: u16, short: bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(format!("{label} {value}")).color(if short {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().text_color()
                }),
            );
        },
    );
}

fn draw_controls(
    ui: &mut egui::Ui,
    state: &mut State,
    requested_mode: &mut PlugSelectionMode,
    clear_requested: &mut bool,
    apply_requested: &mut bool,
) -> bool {
    let has_targets = state.targets.iter().any(|target| *target > 0);
    let can_apply = state
        .preview
        .as_ref()
        .is_some_and(|solution| !solution.assignments.is_empty() || !solution.swaps.is_empty());
    let narrow = ui.available_width() < 760.0;
    let mut allow_inventory_swaps = state.allow_inventory_swaps.unwrap_or(true);
    let mut swap_setting_changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("Plug Safety:");
        egui::ComboBox::from_id_salt("armor-stats-adjuster-safety")
            .selected_text(requested_mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    PlugSelectionMode::Supported,
                    PlugSelectionMode::SocketAndGearType,
                    PlugSelectionMode::MatchingSocketType,
                    PlugSelectionMode::GearType,
                    PlugSelectionMode::AnyPlug,
                ] {
                    ui.selectable_value(requested_mode, mode, mode.label());
                }
            });
        swap_setting_changed = ui
            .checkbox(
                &mut allow_inventory_swaps,
                "Use better armor from character inventory",
            )
            .on_hover_text(
                "When enabled, the preview may equip unlocked armor stored on this character. The currently equipped piece is moved back to inventory.",
            )
            .changed();
        ui.label(egui::RichText::new("Locked armor is always preserved").weak());
        if !narrow {
            draw_control_actions(ui, has_targets, can_apply, clear_requested, apply_requested);
        }
    });
    if narrow {
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            draw_control_actions(ui, has_targets, can_apply, clear_requested, apply_requested);
        });
    }
    state.allow_inventory_swaps = Some(allow_inventory_swaps);
    swap_setting_changed
}

fn draw_control_actions(
    ui: &mut egui::Ui,
    has_targets: bool,
    can_apply: bool,
    clear_requested: &mut bool,
    apply_requested: &mut bool,
) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add_enabled(has_targets, egui::Button::new("Clear"))
            .clicked()
        {
            *clear_requested = true;
        }
        if ui
            .add_enabled(can_apply, egui::Button::new("Adjust armor"))
            .on_disabled_hover_text(if has_targets {
                "The preview does not require any armor changes"
            } else {
                "Set at least one goal first"
            })
            .clicked()
        {
            *apply_requested = true;
        }
    });
}

fn draw_preview(ui: &mut egui::Ui, catalog: &Catalog, state: &State) {
    ui.horizontal(|ui| {
        ui.strong("Armor preview");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(solution) = &state.preview {
                let pieces = changed_piece_count(solution);
                let swaps = solution.swaps.len();
                let masterworks = solution
                    .assignments
                    .iter()
                    .filter(|assignment| assignment.kind == SocketKind::Masterwork)
                    .count();
                let plugs = solution.assignments.len().saturating_sub(masterworks);
                let text = if pieces == 0 {
                    "No armor changes".to_owned()
                } else {
                    format!(
                        "{pieces} pieces · {swaps} swaps · {masterworks} masterworks · {plugs} plugs"
                    )
                };
                ui.label(egui::RichText::new(text).weak());
            }
        });
    });
    ui.add_space(3.0);

    let Some(input) = &state.input else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Equipped armor could not be read.",
        );
        return;
    };
    let assignments = state
        .preview
        .as_ref()
        .map_or(&[][..], |solution| solution.assignments.as_slice());

    let widths = preview_column_widths(ui.available_width(), ui.spacing().item_spacing.x);
    ui.horizontal_top(|ui| {
        preview_cell(ui, widths[0], |ui| {
            ui.strong("Slot");
        });
        preview_cell(ui, widths[1], |ui| {
            ui.strong("Armor");
        });
        preview_cell(ui, widths[2], |ui| {
            ui.strong("Stat plugs");
        });
        preview_cell(ui, widths[3], |ui| {
            ui.horizontal(|ui| {
                ui.strong("Stats · current");
                crate::app::glyphs::inline_right_arrow(ui, ui.visuals().strong_text_color());
                ui.strong("projected");
            });
        });
    });
    ui.separator();

    for (piece_index, piece) in input.pieces.iter().enumerate() {
        let selected = state
            .preview
            .as_ref()
            .and_then(|solution| selected_candidate(input, solution, piece_index));
        let selected_piece = selected.map_or(piece, |candidate| &candidate.piece);
        let swapped = selected.is_some_and(|candidate| candidate.origin != ArmorOrigin::Equipped);
        let masterwork_planned = assignments.iter().any(|assignment| {
            assignment.piece_index == piece_index && assignment.kind == SocketKind::Masterwork
        });
        ui.push_id(("armor-stat-preview-row", piece.slot), |ui| {
            ui.horizontal_top(|ui| {
                preview_cell(ui, widths[0], |ui| {
                    ui.label(piece.label);
                });
                preview_cell(ui, widths[1], |ui| {
                    ui.horizontal(|ui| {
                        if let Some(item) = &selected_piece.item
                            && let Some(icon) = catalog.icon_texture(ui.ctx(), item.hash)
                        {
                            ui.add(egui::Image::new((icon.id(), egui::vec2(28.0, 28.0))));
                        }
                        ui.vertical(|ui| {
                            if swapped {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&selected_piece.name).strong(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(format!(
                                    "Equip {} from inventory instead of {}",
                                    selected_piece.name, piece.name
                                ));
                            } else {
                                ui.add(egui::Label::new(&selected_piece.name).truncate())
                                    .on_hover_text(&selected_piece.name);
                            }
                            let state_text = if swapped && masterwork_planned {
                                "From inventory · Masterwork".to_owned()
                            } else if swapped {
                                "From inventory".to_owned()
                            } else if masterwork_planned {
                                "Masterwork".to_owned()
                            } else if let Some(issue) = &piece.issue {
                                issue.clone()
                            } else if piece.locked {
                                "Locked · unchanged".to_owned()
                            } else if piece.masterworked {
                                "Masterworked · bonus preserved".to_owned()
                            } else {
                                "Available".to_owned()
                            };
                            ui.add(
                                egui::Label::new(egui::RichText::new(&state_text).small().weak())
                                    .truncate(),
                            )
                            .on_hover_text(state_text);
                        });
                    });
                });

                preview_cell(ui, widths[2], |ui| {
                    let piece_assignments = assignments
                        .iter()
                        .filter(|assignment| assignment.piece_index == piece_index)
                        .collect::<Vec<_>>();
                    let armor_mod =
                        armor_stat_mod_plan(catalog, selected_piece, assignments, piece_index);
                    let armor_mod_socket = armor_mod.as_ref().map(|plan| plan.0);
                    let mut drew_line = false;
                    if let Some((_, text, detail, changed)) = armor_mod {
                        let text = if changed {
                            egui::RichText::new(text).strong()
                        } else {
                            egui::RichText::new(text)
                        };
                        ui.add(egui::Label::new(text).truncate())
                            .on_hover_text(detail);
                        drew_line = true;
                    }
                    let mut grouped_changes = Vec::<(String, Vec<String>)>::new();
                    for assignment in piece_assignments {
                        if armor_mod_socket == Some(assignment.socket_index)
                            || assignment.kind == SocketKind::Masterwork
                        {
                            continue;
                        }
                        let socket = selected_piece
                            .item
                            .as_ref()
                            .and_then(|item| item.sockets.get(assignment.socket_index));
                        let socket_label = socket.map_or_else(
                            || format!("Socket {}", assignment.socket_index + 1),
                            |socket| {
                                if socket.label.trim().is_empty() {
                                    format!("Socket {}", assignment.socket_index + 1)
                                } else {
                                    socket.label.clone()
                                }
                            },
                        );
                        let selected = plug_name(catalog, assignment.selected);
                        let change = format!("{socket_label}: {selected}");
                        let detail = format!(
                            "{socket_label}: {} to {selected}",
                            plug_name(catalog, assignment.previous)
                        );
                        if let Some((_, details)) = grouped_changes
                            .iter_mut()
                            .find(|(known, _)| *known == change)
                        {
                            details.push(detail);
                        } else {
                            grouped_changes.push((change, vec![detail]));
                        }
                    }
                    for (change, details) in grouped_changes {
                        let visible = if details.len() > 1 {
                            format!("{}× {change}", details.len())
                        } else {
                            change
                        };
                        ui.add(egui::Label::new(&visible).truncate())
                            .on_hover_text(details.join("\n"));
                        drew_line = true;
                    }
                    if !drew_line {
                        ui.label(egui::RichText::new("No changes").weak());
                    }
                });

                preview_cell(ui, widths[3], |ui| {
                    let projected_piece = state
                        .preview
                        .as_ref()
                        .and_then(|solution| solution.selections.get(piece_index))
                        .map_or(piece.current_totals, |selection| selection.projected_totals);
                    draw_piece_stat_breakdown(ui, catalog, piece.current_totals, projected_piece);
                });
            });
            ui.separator();
        });
    }

    ui.add_space(6.0);
    if let Some(solution) = &state.preview {
        ui.label(
            egui::RichText::new(format!(
                "Projected: {}",
                format_totals(solution.projected_totals)
            ))
            .weak(),
        );
    } else {
        ui.label(
            egui::RichText::new("Set a goal to preview the closest valid configuration.").weak(),
        );
    }
}

pub(super) fn preview_column_widths(available: f32, gap: f32) -> [f32; 4] {
    let slot = if available >= 760.0 { 70.0 } else { 58.0 };
    let armor = (available * 0.22).clamp(125.0, 210.0);
    let stats = (available * 0.27).clamp(160.0, 300.0);
    let changes = (available - slot - armor - stats - 3.0 * gap).max(150.0);
    [slot, armor, changes, stats]
}

fn draw_piece_stat_breakdown(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    current: [u16; 6],
    projected: [u16; 6],
) {
    let gap = 8.0;
    let cell_width = ((ui.available_width() - gap) / 2.0).max(72.0);
    let compact = cell_width < 140.0;
    egui::Grid::new(ui.id().with("piece-stat-breakdown"))
        .num_columns(2)
        .spacing(egui::vec2(gap, 2.0))
        .show(ui, |ui| {
            for row in 0..3 {
                for index in [row, row + 3] {
                    ui.allocate_ui_with_layout(
                        egui::vec2(cell_width, ui.spacing().interact_size.y),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            if current[index] != projected[index] {
                                ui.strong(projected[index].to_string());
                                crate::app::glyphs::inline_right_arrow(
                                    ui,
                                    ui.visuals().strong_text_color(),
                                );
                            }
                            ui.strong(current[index].to_string());
                            if let Some(icon) = catalog.armor_stat_icon_texture(
                                ui.ctx(),
                                armor_stat_allocation::STAT_NAMES[index],
                            ) {
                                ui.add(egui::Image::new((icon.id(), egui::vec2(14.0, 14.0))));
                            }
                            let name = if compact {
                                &armor_stat_allocation::STAT_NAMES[index][..3]
                            } else {
                                armor_stat_allocation::STAT_NAMES[index]
                            };
                            ui.label(name);
                        },
                    )
                    .response
                    .on_hover_text(format!(
                        "{}: current {} · projected {}",
                        armor_stat_allocation::STAT_NAMES[index],
                        current[index],
                        projected[index]
                    ));
                }
                ui.end_row();
            }
        });
}

fn preview_cell(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            contents(ui);
        },
    );
}

fn armor_stat_mod_plan(
    catalog: &Catalog,
    piece: &ArmorPiece,
    assignments: &[SocketAssignment],
    piece_index: usize,
) -> Option<(usize, String, String, bool)> {
    let item = piece.item.as_ref()?;
    let socket_index = (0..piece.current_plugs.len())
        .find(|index| is_armor_stat_mod_socket(catalog, item, *index))?;
    let previous = piece.current_plugs[socket_index];
    let selected = assignments
        .iter()
        .find(|assignment| {
            assignment.piece_index == piece_index && assignment.socket_index == socket_index
        })
        .map_or(previous, |assignment| assignment.selected);
    let previous_label = armor_stat_mod_label(catalog, item, socket_index, previous);
    let selected_label = armor_stat_mod_label(catalog, item, socket_index, selected);
    let changed = previous != selected;
    let empty = selected_label == "Empty";
    let text = if changed {
        format!("Armor mod: {previous_label} to {selected_label}")
    } else if !empty {
        format!("Armor mod: {selected_label} · kept")
    } else {
        "Armor mod: Empty".to_owned()
    };
    let detail = if changed {
        format!("Package-defined armor stat mod changes from {previous_label} to {selected_label}.")
    } else if !empty {
        format!("Package-defined armor stat mod remains {selected_label}.")
    } else {
        "No armor stat mod is equipped in this socket.".to_owned()
    };
    Some((socket_index, text, detail, changed))
}

pub(super) fn is_armor_stat_mod_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
) -> bool {
    let Some(socket) = item.sockets.get(socket_index) else {
        return false;
    };
    if socket
        .label
        .to_ascii_lowercase()
        .contains("general armor mod")
    {
        return true;
    }
    catalog
        .socket_options(socket)
        .iter()
        .copied()
        .any(|hash| is_armor_stat_mod_plug(catalog, hash))
}

pub(super) fn is_armor_stat_mod_plug(catalog: &Catalog, hash: u64) -> bool {
    catalog
        .display_name(hash)
        .is_some_and(|name| name.ends_with(" Mod"))
        && single_stat_value(catalog.armor_stat_values(hash)).is_some()
}

fn armor_stat_mod_label(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    hash: Option<u64>,
) -> String {
    let Some(hash) = hash else {
        return "Empty".to_owned();
    };
    let values = armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash);
    if let Some((index, value)) = single_stat_value(values) {
        return format!("{} {value:+}", armor_stat_allocation::STAT_NAMES[index]);
    }
    let name = plug_name(catalog, Some(hash));
    if name.to_ascii_lowercase().contains("empty") {
        "Empty".to_owned()
    } else {
        name
    }
}

fn single_stat_value(values: [i32; 6]) -> Option<(usize, i32)> {
    let mut non_zero = values
        .into_iter()
        .enumerate()
        .filter(|(_, value)| *value != 0);
    let value = non_zero.next()?;
    non_zero.next().is_none().then_some(value)
}
