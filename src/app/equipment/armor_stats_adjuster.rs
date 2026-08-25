//! Whole-loadout armor stat targeting.
//!
//! This module owns the dedicated window, cached package-backed socket model,
//! best-effort optimizer, preview, atomic apply path, and focused solver tests.
//! The single-item randomizer keeps its independent allocator in
//! `armor_stat_allocation.rs`.

use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

use eframe::egui;
use serde_json::Value;

use crate::{
    app::{
        ARMOR_SLOTS, ConfirmationDialog, PlugSelectionMode, SLOTS, SundialApp, inventory, settings,
    },
    catalog::{Catalog, ItemDef, ItemRarity},
    hash::{parse_hash_hex, parse_unsigned_value},
};

use super::{
    armor_stat_allocation, displayed_plugs, equip_inventory_item, set_equipment_item_plug,
};

const WINDOW_SIZE: egui::Vec2 = egui::vec2(980.0, 720.0);
const WINDOW_MIN_SIZE: egui::Vec2 = egui::vec2(620.0, 460.0);
const MAX_SOLVER_STATES: usize = 1_000;
const MAX_PIECE_STATES: usize = 128;
const MAX_SLOT_PLANS: usize = 512;
const NO_DEFINITION_HASH: u64 = 0x811C_9DC5;
const PREVIEW_DEBOUNCE_SECONDS: f64 = 0.15;

#[derive(Default)]
pub(in crate::app) struct State {
    open: bool,
    character_index: usize,
    targets: [u16; 6],
    source_key: Option<SourceKey>,
    input: Option<LoadoutInput>,
    preview: Option<Solution>,
    preview_task: Option<PreviewTask>,
    preview_due_at: Option<f64>,
    feedback: Option<Feedback>,
    preserve_feedback_once: bool,
    window_generation: u64,
}

impl State {
    pub(in crate::app) fn open(&mut self, character_index: usize) {
        if self.character_index != character_index {
            self.character_index = character_index;
            self.targets = [0; 6];
            self.source_key = None;
            self.input = None;
            self.preview = None;
            self.preview_task = None;
            self.preview_due_at = None;
            self.feedback = None;
            self.preserve_feedback_once = false;
        }
        self.window_generation = self.window_generation.wrapping_add(1);
        self.open = true;
    }
}

#[derive(Clone, Debug)]
struct Feedback {
    text: String,
    detail: Option<String>,
    is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceKey {
    character_index: usize,
    plug_mode: PlugSelectionMode,
    armor_json: String,
}

#[derive(Clone)]
struct LoadoutInput {
    pieces: Vec<ArmorPiece>,
    candidates: Vec<Vec<ArmorCandidate>>,
    current_totals: [u16; 6],
}

#[derive(Clone)]
struct ArmorCandidate {
    piece: ArmorPiece,
    origin: ArmorOrigin,
    fixed_totals: [i32; 6],
    sockets: Vec<MutableSocket>,
    exotic: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ArmorOrigin {
    Equipped,
    Inventory {
        instance_soid: u64,
        definition_hash: u32,
    },
}

#[derive(Clone)]
struct ArmorPiece {
    slot: &'static str,
    label: &'static str,
    name: String,
    item: Option<Arc<ItemDef>>,
    current_plugs: Vec<Option<u64>>,
    current_totals: [u16; 6],
    locked: bool,
    masterworked: bool,
    issue: Option<String>,
}

#[derive(Clone)]
struct MutableSocket {
    socket_index: usize,
    current: Option<u64>,
    choices: Vec<SocketChoice>,
    kind: SocketKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SocketKind {
    Stat,
    Masterwork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SocketChoice {
    hash: Option<u64>,
    values: [i32; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocketAssignment {
    piece_index: usize,
    socket_index: usize,
    previous: Option<u64>,
    selected: Option<u64>,
    kind: SocketKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PieceSelection {
    candidate_index: usize,
    projected_totals: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArmorSwap {
    piece_index: usize,
    instance_soid: u64,
    definition_hash: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Solution {
    projected_totals: [u16; 6],
    selections: Vec<PieceSelection>,
    swaps: Vec<ArmorSwap>,
    assignments: Vec<SocketAssignment>,
    shortfalls: [u16; 6],
    exact: bool,
}

#[derive(Clone)]
struct SearchState {
    totals: [i32; 6],
    plans: Vec<PiecePlan>,
    swaps: usize,
    plug_changes: usize,
    masterworks: usize,
    exotics: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PiecePlan {
    candidate_index: usize,
    totals: [i32; 6],
    selections: Vec<Option<u64>>,
    plug_changes: usize,
    masterworks: usize,
}

#[derive(Clone)]
struct PieceSearchState {
    totals: [i32; 6],
    selections: Vec<Option<u64>>,
    plug_changes: usize,
    masterworks: usize,
}

struct PreviewTask {
    source_key: SourceKey,
    targets: [u16; 6],
    receiver: Receiver<Solution>,
}

pub(in crate::app) fn draw_entry_button(ui: &mut egui::Ui, editable: bool) -> egui::Response {
    ui.add_enabled(editable, egui::Button::new("Armor stats…"))
        .on_hover_text("Adjust stat plugs across all equipped armor")
}

pub(super) fn equipped_totals(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
) -> [u16; 6] {
    let equipment = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object);
    let mut totals = [0_u16; 6];

    for slot in ARMOR_SLOTS {
        let (label, bucket_hash) = SLOTS
            .iter()
            .find_map(|(known, label, bucket)| (*known == *slot).then_some((*label, *bucket)))
            .unwrap_or((slot, 0));
        let piece = read_piece(
            catalog,
            slot,
            label,
            bucket_hash,
            equipment.and_then(|equipment| equipment.get(*slot)),
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

    refresh_input(
        &mut state,
        &app.document,
        &app.manifest,
        character_index,
        app.plug_selection_mode,
    );
    refresh_preview(&mut state, context);

    let mut open = state.open;
    let mut targets_changed = false;
    let mut clear_requested = false;
    let mut apply_requested = false;
    let mut requested_mode = app.plug_selection_mode;
    let window_size = super::dialog_size_constraints(context, WINDOW_SIZE, WINDOW_MIN_SIZE);

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
                    draw_controls(
                        ui,
                        &state,
                        &mut requested_mode,
                        &mut clear_requested,
                        &mut apply_requested,
                    );
                    if app.show_safety_warnings {
                        super::super::draw_plug_selection_warning(ui, requested_mode);
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
        if requested_mode == PlugSelectionMode::AnyPlug && !app.really_unsafe_warning_acknowledged {
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
                    ui.add(egui::Image::new((icon.id(), egui::vec2(14.0, 14.0))));
                }
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_stat_readout(ui, 92.0, "Projected", projected, short);
            draw_stat_readout(ui, 76.0, "Current", current, false);
        });
    });
}

fn draw_stat_readout(ui: &mut egui::Ui, width: f32, label: &str, value: u16, short: bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.label(
                egui::RichText::new(format!("{label} {value}"))
                    .size(11.0)
                    .color(if short {
                        ui.visuals().warn_fg_color
                    } else {
                        ui.visuals().weak_text_color()
                    }),
            );
        },
    );
}

fn draw_controls(
    ui: &mut egui::Ui,
    state: &State,
    requested_mode: &mut PlugSelectionMode,
    clear_requested: &mut bool,
    apply_requested: &mut bool,
) {
    let has_targets = state.targets.iter().any(|target| *target > 0);
    let can_apply = state
        .preview
        .as_ref()
        .is_some_and(|solution| !solution.assignments.is_empty() || !solution.swaps.is_empty());
    let narrow = ui.available_width() < 760.0;
    ui.horizontal_wrapped(|ui| {
        ui.label("Plug safety:");
        egui::ComboBox::from_id_salt("armor-stats-adjuster-safety")
            .selected_text(requested_mode.label())
            .show_ui(ui, |ui| {
                for mode in [
                    PlugSelectionMode::Supported,
                    PlugSelectionMode::MatchingSocketType,
                    PlugSelectionMode::GearType,
                    PlugSelectionMode::AnyPlug,
                ] {
                    ui.selectable_value(requested_mode, mode, mode.label());
                }
            });
        ui.label(egui::RichText::new("Locked armor is preserved").weak());
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

fn preview_column_widths(available: f32, gap: f32) -> [f32; 4] {
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

fn is_armor_stat_mod_socket(catalog: &Catalog, item: &ItemDef, socket_index: usize) -> bool {
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

fn is_armor_stat_mod_plug(catalog: &Catalog, hash: u64) -> bool {
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

fn refresh_input(
    state: &mut State,
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    plug_mode: PlugSelectionMode,
) {
    let key = source_key(document, character_index, plug_mode);
    if state.source_key.as_ref() == Some(&key) {
        return;
    }
    state.source_key = Some(key);
    state.input = Some(build_input(document, catalog, character_index, plug_mode));
    state.preview = None;
    state.preview_task = None;
    if state.preserve_feedback_once {
        state.preserve_feedback_once = false;
    } else {
        state.feedback = None;
    }
}

fn refresh_preview(state: &mut State, context: &egui::Context) {
    if let Some(task) = state.preview_task.take() {
        if state.source_key.as_ref() != Some(&task.source_key) || state.targets != task.targets {
            // The worker can finish in the background; its stale result is intentionally dropped.
        } else {
            match task.receiver.try_recv() {
                Ok(solution) => {
                    state.preview = Some(solution);
                    state.preview_due_at = None;
                }
                Err(TryRecvError::Empty) => {
                    state.preview_task = Some(task);
                    context.request_repaint_after(Duration::from_millis(16));
                }
                Err(TryRecvError::Disconnected) => {
                    state.preview_due_at = None;
                }
            }
        }
    }

    if state.preview.is_some()
        || state.preview_task.is_some()
        || !state.targets.iter().any(|target| *target > 0)
    {
        return;
    }

    let now = context.input(|input| input.time);
    if let Some(due_at) = state.preview_due_at {
        if now < due_at {
            context.request_repaint_after(Duration::from_secs_f64(due_at - now));
            return;
        }
        state.preview_due_at = None;
    }

    let (Some(input), Some(source_key)) = (state.input.clone(), state.source_key.clone()) else {
        return;
    };
    let targets = state.targets;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(solve(&input, targets));
    });
    state.preview_task = Some(PreviewTask {
        source_key,
        targets,
        receiver,
    });
    context.request_repaint_after(Duration::from_millis(16));
}

fn source_key(document: &Value, character_index: usize, plug_mode: PlugSelectionMode) -> SourceKey {
    let character = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(Value::as_object);
    let equipment_json = character
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object)
        .map(|equipment| {
            ARMOR_SLOTS
                .iter()
                .map(|slot| {
                    serde_json::to_string(equipment.get(*slot).unwrap_or(&Value::Null))
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .unwrap_or_default();
    let inventory_json = character
        .and_then(|character| character.get("inventory"))
        .map(|inventory| serde_json::to_string(inventory).unwrap_or_default())
        .unwrap_or_default();
    SourceKey {
        character_index,
        plug_mode,
        armor_json: format!("{equipment_json}|{inventory_json}"),
    }
}

fn build_input(
    document: &Value,
    catalog: &Catalog,
    character_index: usize,
    plug_mode: PlugSelectionMode,
) -> LoadoutInput {
    let equipment = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("equipment"))
        .and_then(Value::as_object);
    let class_type = document
        .pointer("/state/characters")
        .and_then(Value::as_array)
        .and_then(|characters| characters.get(character_index))
        .and_then(|character| character.get("class"))
        .and_then(Value::as_u64)
        .unwrap_or(99);
    let stored = inventory::character_inventory(document, character_index)
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut pieces = Vec::with_capacity(ARMOR_SLOTS.len());
    let mut candidates = Vec::with_capacity(ARMOR_SLOTS.len());

    for slot in ARMOR_SLOTS.iter().copied() {
        let (label, bucket_hash) = SLOTS
            .iter()
            .find_map(|(known, label, bucket)| (*known == slot).then_some((*label, *bucket)))
            .unwrap_or((slot, 0));
        let value = equipment.and_then(|equipment| equipment.get(slot));
        let mut equipped_piece = read_piece(catalog, slot, label, bucket_hash, value);
        if let Some(item) = equipped_piece.item.as_ref() {
            equipped_piece.current_totals = cap_u16_totals(armor_stat_allocation::selected_totals(
                catalog,
                item,
                &equipped_piece.current_plugs,
            ));
        }
        let preserve_slot = equipped_piece.locked;
        let mut slot_candidates = vec![candidate_from_piece(
            catalog,
            equipped_piece.clone(),
            ArmorOrigin::Equipped,
            plug_mode,
        )];
        if !preserve_slot && class_type <= 2 {
            for snapshot in &stored {
                if snapshot.quantity != 1
                    || snapshot.flags.unwrap_or_default() & inventory::INVENTORY_FLAG_LOCKED != 0
                {
                    continue;
                }
                let Some(item) = catalog
                    .item_handle_for_bucket(u64::from(snapshot.definition_hash), bucket_hash)
                else {
                    continue;
                };
                if item.class_type != 3 && item.class_type != class_type {
                    continue;
                }
                let piece = read_inventory_piece(catalog, slot, label, item, snapshot);
                slot_candidates.push(candidate_from_piece(
                    catalog,
                    piece,
                    ArmorOrigin::Inventory {
                        instance_soid: snapshot.instance_soid,
                        definition_hash: snapshot.definition_hash,
                    },
                    plug_mode,
                ));
            }
        }
        pieces.push(equipped_piece);
        candidates.push(slot_candidates);
    }

    let mut current_totals = [0_u16; 6];
    for piece in &pieces {
        for (total, value) in current_totals.iter_mut().zip(piece.current_totals) {
            *total = total.saturating_add(value);
        }
    }
    LoadoutInput {
        pieces,
        candidates,
        current_totals: cap_u16_totals(current_totals),
    }
}

fn candidate_from_piece(
    catalog: &Catalog,
    piece: ArmorPiece,
    origin: ArmorOrigin,
    plug_mode: PlugSelectionMode,
) -> ArmorCandidate {
    let Some(item) = piece.item.as_ref() else {
        return ArmorCandidate {
            piece,
            origin,
            fixed_totals: [0; 6],
            sockets: Vec::new(),
            exotic: false,
        };
    };
    let mut fixed_totals = catalog.armor_stat_values(item.hash);
    let mut sockets = Vec::new();
    let mut intrinsic_counted = false;
    for socket_index in 0..piece.current_plugs.len() {
        let current = piece.current_plugs[socket_index];
        let kind = if socket_is_preserved_masterwork(item, socket_index) {
            SocketKind::Masterwork
        } else {
            SocketKind::Stat
        };
        let choices = if piece.issue.is_none() && !piece.locked {
            match kind {
                SocketKind::Stat
                    if socket_is_adjustable_stat_socket(catalog, item, socket_index) =>
                {
                    socket_choices(catalog, item, socket_index, current, plug_mode)
                }
                SocketKind::Masterwork if !piece.masterworked => {
                    masterwork_choices(catalog, item, socket_index, current)
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let mutable = choices.len() > 1
            && choices.iter().any(|choice| {
                choice.hash != current && choice.values.iter().any(|value| *value > 0)
            });
        if mutable {
            sockets.push(MutableSocket {
                socket_index,
                current,
                choices,
                kind,
            });
        } else if let Some(hash) = current {
            if armor_stat_allocation::is_intrinsic_plug(catalog, hash) {
                if intrinsic_counted {
                    continue;
                }
                intrinsic_counted = true;
            }
            for (total, value) in
                fixed_totals
                    .iter_mut()
                    .zip(armor_stat_allocation::socket_stat_values(
                        catalog,
                        item,
                        socket_index,
                        hash,
                    ))
            {
                *total = total.saturating_add(value);
            }
        }
    }
    let exotic = catalog.item_rarity(item.hash) == ItemRarity::Exotic;
    ArmorCandidate {
        piece,
        origin,
        fixed_totals,
        sockets,
        exotic,
    }
}

fn masterwork_choices(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
) -> Vec<SocketChoice> {
    let Some(socket) = item.sockets.get(socket_index) else {
        return Vec::new();
    };
    let current_choice = SocketChoice {
        hash: current,
        values: current.map_or([0; 6], |hash| {
            armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash)
        }),
    };
    let best = catalog
        .socket_options(socket)
        .iter()
        .copied()
        .filter(|hash| valid_masterwork_plug(catalog, *hash))
        .map(|hash| SocketChoice {
            hash: Some(hash),
            values: armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash),
        })
        .filter(|choice| choice.values.iter().any(|value| *value > 0))
        .max_by_key(|choice| (choice.values.iter().sum::<i32>(), Reverse(choice.hash)));
    let mut choices = vec![current_choice];
    if let Some(best) = best
        && best.hash != current
    {
        choices.push(best);
    }
    choices
}

fn valid_masterwork_plug(catalog: &Catalog, hash: u64) -> bool {
    catalog.display_name(hash).is_some_and(|name| {
        let name = name.trim();
        name.contains("Masterwork")
            || name.ends_with(" Energy 10")
            || name.starts_with("Tier 10 Armor")
    })
}

fn read_inventory_piece(
    catalog: &Catalog,
    slot: &'static str,
    label: &'static str,
    item: Arc<ItemDef>,
    snapshot: &inventory::InventoryItemSnapshot,
) -> ArmorPiece {
    let mut current_plugs = match &snapshot.plugs {
        inventory::ItemPlugs::NativeDefaults => item
            .default_plugs
            .iter()
            .map(|plug| plug.as_deref().and_then(parse_hash_hex))
            .collect::<Vec<_>>(),
        inventory::ItemPlugs::Authored(plugs) => plugs
            .iter()
            .map(|plug| plug.map(u64::from))
            .collect::<Vec<_>>(),
    };
    let socket_count = item
        .sockets
        .len()
        .max(item.default_plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    current_plugs.resize(socket_count, None);
    let flags = snapshot.flags.unwrap_or_default();
    let masterworked = piece_is_masterworked(catalog, &item, &current_plugs);
    let current_totals = cap_u16_totals(armor_stat_allocation::selected_totals(
        catalog,
        &item,
        &current_plugs,
    ));
    ArmorPiece {
        slot,
        label,
        name: item.name.clone(),
        item: Some(item),
        current_plugs,
        current_totals,
        locked: flags & inventory::INVENTORY_FLAG_LOCKED != 0,
        masterworked,
        issue: None,
    }
}

fn socket_is_adjustable_stat_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
) -> bool {
    armor_stat_allocation::is_allocation_socket(catalog, item, socket_index)
        || is_armor_stat_mod_socket(catalog, item, socket_index)
}

fn socket_is_preserved_masterwork(item: &ItemDef, socket_index: usize) -> bool {
    item.sockets.get(socket_index).is_some_and(|socket| {
        matches!(socket.socket_type, 29..=43 | 520 | 678 | 679) || {
            let label = socket.label.to_ascii_lowercase();
            label.contains("masterwork")
                || label.contains("armor tier")
                || label.contains("armor energy")
        }
    })
}

fn piece_is_masterworked(catalog: &Catalog, item: &ItemDef, plugs: &[Option<u64>]) -> bool {
    plugs
        .iter()
        .copied()
        .enumerate()
        .any(|(socket_index, hash)| {
            let Some(hash) = hash else {
                return false;
            };
            if !socket_is_preserved_masterwork(item, socket_index) {
                return false;
            }
            catalog.display_name(hash).is_some_and(|name| {
                let name = name.trim();
                name.contains("Masterwork")
                    || name.ends_with(" Energy 10")
                    || name.starts_with("Tier 10 Armor")
            })
        })
}

fn read_piece(
    catalog: &Catalog,
    slot: &'static str,
    label: &'static str,
    bucket_hash: u64,
    value: Option<&Value>,
) -> ArmorPiece {
    let Some(object) = value.and_then(Value::as_object) else {
        return unavailable_piece(slot, label, "No equipped armor");
    };
    let Some(definition_hash) = object.get("definition_hash").and_then(parse_unsigned_value) else {
        return unavailable_piece(slot, label, "Invalid definition");
    };
    let Some(item) = catalog.item_handle_for_bucket(definition_hash, bucket_hash) else {
        return unavailable_piece(slot, label, "Definition unavailable");
    };

    let mut issue = None;
    let (raw_plugs, _) = displayed_plugs(object.get("plugs"), &item.default_plugs);
    if !matches!(object.get("plugs"), Some(Value::Null | Value::Array(_))) {
        issue = Some("Plugs unavailable".to_owned());
    }
    let mut current_plugs = raw_plugs
        .iter()
        .map(|value| {
            if value.is_null() {
                None
            } else {
                parse_unsigned_value(value).or_else(|| value.as_str().and_then(parse_hash_hex))
            }
        })
        .collect::<Vec<_>>();
    if raw_plugs
        .iter()
        .zip(&current_plugs)
        .any(|(value, hash)| !value.is_null() && hash.is_none())
    {
        issue = Some("Invalid authored plug".to_owned());
    }
    let socket_count = item
        .sockets
        .len()
        .max(item.default_plugs.len())
        .min(inventory::MAX_ITEM_PLUGS);
    current_plugs.resize(socket_count, None);

    let flags = match object.get("flags") {
        None => None,
        Some(value) => match parse_unsigned_value(value)
            .and_then(|flags| u8::try_from(flags).ok())
            .filter(|flags| *flags <= inventory::INVENTORY_FLAG_MASK)
        {
            Some(flags) => Some(flags),
            None => {
                issue = Some("Invalid item flags".to_owned());
                None
            }
        },
    };

    let masterworked = piece_is_masterworked(catalog, &item, &current_plugs);

    ArmorPiece {
        slot,
        label,
        name: item.name.clone(),
        item: Some(item),
        current_plugs,
        current_totals: [0; 6],
        locked: flags.unwrap_or_default() & inventory::INVENTORY_FLAG_LOCKED != 0,
        masterworked,
        issue,
    }
}

fn unavailable_piece(slot: &'static str, label: &'static str, issue: &str) -> ArmorPiece {
    ArmorPiece {
        slot,
        label,
        name: "Unavailable".to_owned(),
        item: None,
        current_plugs: Vec::new(),
        current_totals: [0; 6],
        locked: false,
        masterworked: false,
        issue: Some(issue.to_owned()),
    }
}

fn socket_choices(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
    mode: PlugSelectionMode,
) -> Vec<SocketChoice> {
    let Some(socket) = item.sockets.get(socket_index) else {
        return Vec::new();
    };
    let allocation_socket =
        armor_stat_allocation::is_allocation_socket(catalog, item, socket_index);
    let armor_mod_socket = is_armor_stat_mod_socket(catalog, item, socket_index);
    if !allocation_socket && !armor_mod_socket {
        return Vec::new();
    }
    let mut hashes = match mode {
        PlugSelectionMode::Supported => catalog.socket_options(socket).to_vec(),
        PlugSelectionMode::MatchingSocketType => {
            catalog.socket_type_options(socket.socket_type).to_vec()
        }
        PlugSelectionMode::GearType => catalog.gear_type_options(item, socket_index),
        PlugSelectionMode::AnyPlug => catalog.all_plug_options().to_vec(),
    };
    if let Some(current) = current {
        hashes.push(current);
    }
    hashes.sort_unstable();
    hashes.dedup();

    let mut by_values = HashMap::<[i32; 6], Option<u64>>::new();
    if current.is_none() || armor_mod_socket {
        by_values.insert([0; 6], None);
    }
    for hash in hashes {
        if hash == NO_DEFINITION_HASH || u32::try_from(hash).is_err() {
            continue;
        }
        if allocation_socket && !armor_stat_allocation::is_allocation_plug(catalog, hash) {
            continue;
        }
        if armor_mod_socket && !is_armor_stat_mod_plug(catalog, hash) {
            continue;
        }
        let values = armor_stat_allocation::socket_stat_values(catalog, item, socket_index, hash);
        if values.iter().all(|value| *value == 0) && Some(hash) != current {
            continue;
        }
        by_values
            .entry(values)
            .and_modify(|stored| {
                if Some(hash) == current
                    || stored.is_some_and(|old| Some(old) != current && hash < old)
                {
                    *stored = Some(hash);
                }
            })
            .or_insert(Some(hash));
    }

    let mut choices = by_values
        .into_iter()
        .map(|(values, hash)| SocketChoice { hash, values })
        .collect::<Vec<_>>();
    choices.sort_by_key(|choice| (choice.hash != current, choice.values, choice.hash));
    choices
}

fn solve(input: &LoadoutInput, targets: [u16; 6]) -> Solution {
    let mut states = vec![SearchState {
        totals: [0; 6],
        plans: Vec::with_capacity(input.candidates.len()),
        swaps: 0,
        plug_changes: 0,
        masterworks: 0,
        exotics: 0,
    }];
    for candidates in &input.candidates {
        let plans = slot_plans(candidates, targets);
        let mut next = HashMap::<[u16; 6], SearchState>::new();
        for state in &states {
            for plan in &plans {
                let candidate = &candidates[plan.candidate_index];
                let exotics = state.exotics + usize::from(candidate.exotic);
                if exotics > 1 {
                    continue;
                }
                let mut candidate = state.clone();
                for (total, value) in candidate.totals.iter_mut().zip(plan.totals) {
                    *total = total.saturating_add(value);
                }
                candidate.plans.push(plan.clone());
                candidate.swaps += usize::from(plan.candidate_index != 0);
                candidate.plug_changes += plan.plug_changes;
                candidate.masterworks += plan.masterworks;
                candidate.exotics = exotics;
                let key = solver_key(candidate.totals, targets);
                match next.entry(key) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(candidate);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if partial_cmp(&candidate, entry.get(), targets) == Ordering::Less {
                            entry.insert(candidate);
                        }
                    }
                }
            }
        }
        if next.len() > MAX_SOLVER_STATES {
            next = prune_search_map(next, targets);
        }
        states = next.into_values().collect();
    }

    let best = states
        .into_iter()
        .min_by(|left, right| final_cmp(left, right, targets))
        .unwrap_or(SearchState {
            totals: [0; 6],
            plans: Vec::new(),
            swaps: 0,
            plug_changes: 0,
            masterworks: 0,
            exotics: 0,
        });
    solution_from_search(input, targets, best)
}

fn slot_plans(candidates: &[ArmorCandidate], targets: [u16; 6]) -> Vec<PiecePlan> {
    let mut by_result = HashMap::<([u16; 6], bool), PiecePlan>::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        for plan in piece_plans(candidate, candidate_index, targets) {
            let key = (solver_key(plan.totals, targets), candidate.exotic);
            match by_result.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(plan);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if piece_plan_cmp(&plan, entry.get(), targets) == Ordering::Less {
                        entry.insert(plan);
                    }
                }
            }
        }
    }
    let mut plans = by_result.into_values().collect::<Vec<_>>();
    plans.sort_by(|left, right| piece_plan_cmp(left, right, targets));
    plans.truncate(MAX_SLOT_PLANS);
    plans
}

fn piece_plans(
    candidate: &ArmorCandidate,
    candidate_index: usize,
    targets: [u16; 6],
) -> Vec<PiecePlan> {
    let mut states = vec![PieceSearchState {
        totals: candidate.fixed_totals,
        selections: Vec::with_capacity(candidate.sockets.len()),
        plug_changes: 0,
        masterworks: 0,
    }];
    for socket in &candidate.sockets {
        let choices = choices_for_targets(socket, targets);
        let mut next = HashMap::<[u16; 6], PieceSearchState>::new();
        for state in &states {
            for choice in &choices {
                let mut next_state = state.clone();
                for (total, value) in next_state.totals.iter_mut().zip(choice.values) {
                    *total = total.saturating_add(value);
                }
                next_state.selections.push(choice.hash);
                if choice.hash != socket.current {
                    match socket.kind {
                        SocketKind::Stat => next_state.plug_changes += 1,
                        SocketKind::Masterwork => next_state.masterworks += 1,
                    }
                }
                let key = solver_key(next_state.totals, targets);
                match next.entry(key) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(next_state);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if piece_state_cmp(&next_state, entry.get(), targets) == Ordering::Less {
                            entry.insert(next_state);
                        }
                    }
                }
            }
        }
        let mut next = next.into_values().collect::<Vec<_>>();
        next.sort_by(|left, right| piece_state_cmp(left, right, targets));
        next.truncate(MAX_PIECE_STATES);
        states = next;
    }
    states
        .into_iter()
        .map(|state| PiecePlan {
            candidate_index,
            totals: state.totals,
            selections: state.selections,
            plug_changes: state.plug_changes,
            masterworks: state.masterworks,
        })
        .collect()
}

fn piece_state_cmp(
    left: &PieceSearchState,
    right: &PieceSearchState,
    targets: [u16; 6],
) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| left.selections.cmp(&right.selections))
}

fn piece_plan_cmp(left: &PiecePlan, right: &PiecePlan, targets: [u16; 6]) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| (left.candidate_index != 0).cmp(&(right.candidate_index != 0)))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| left.candidate_index.cmp(&right.candidate_index))
        .then_with(|| left.selections.cmp(&right.selections))
}

fn choices_for_targets(socket: &MutableSocket, targets: [u16; 6]) -> Vec<SocketChoice> {
    let mut groups = HashMap::<[i32; 6], Vec<SocketChoice>>::new();
    for choice in &socket.choices {
        let key = std::array::from_fn(|index| {
            if targets[index] == 0 {
                0
            } else {
                choice.values[index]
            }
        });
        groups.entry(key).or_default().push(*choice);
    }

    let ignored_total = |choice: &SocketChoice| {
        choice
            .values
            .into_iter()
            .zip(targets)
            .filter(|(_, target)| *target == 0)
            .map(|(value, _)| value)
            .sum::<i32>()
    };
    let mut choices = Vec::new();
    for group in groups.into_values() {
        let mut keep = Vec::new();
        let mut retain = |candidate: Option<&SocketChoice>| {
            if let Some(candidate) = candidate
                && !keep.contains(candidate)
            {
                keep.push(*candidate);
            }
        };
        retain(group.iter().find(|choice| choice.hash == socket.current));
        retain(
            group
                .iter()
                .min_by_key(|choice| (ignored_total(choice), choice.hash)),
        );
        retain(
            group
                .iter()
                .max_by_key(|choice| (ignored_total(choice), Reverse(choice.hash))),
        );
        choices.extend(keep);
    }
    choices.sort_by_key(|choice| (choice.hash != socket.current, choice.values, choice.hash));
    choices
}

fn prune_search_map(
    states: HashMap<[u16; 6], SearchState>,
    targets: [u16; 6],
) -> HashMap<[u16; 6], SearchState> {
    let mut states = states.into_values().collect::<Vec<_>>();
    states.sort_by(|left, right| partial_cmp(left, right, targets));
    states.truncate(MAX_SOLVER_STATES);
    states
        .into_iter()
        .map(|state| (solver_key(state.totals, targets), state))
        .collect()
}

fn solution_from_search(input: &LoadoutInput, targets: [u16; 6], search: SearchState) -> Solution {
    let mut selections = Vec::with_capacity(search.plans.len());
    let mut swaps = Vec::new();
    let mut assignments = Vec::new();
    for (piece_index, plan) in search.plans.iter().enumerate() {
        let candidate = &input.candidates[piece_index][plan.candidate_index];
        selections.push(PieceSelection {
            candidate_index: plan.candidate_index,
            projected_totals: capped_totals(plan.totals),
        });
        if let ArmorOrigin::Inventory {
            instance_soid,
            definition_hash,
        } = candidate.origin
        {
            swaps.push(ArmorSwap {
                piece_index,
                instance_soid,
                definition_hash,
            });
        }
        assignments.extend(candidate.sockets.iter().zip(&plan.selections).filter_map(
            |(socket, selected)| {
                (*selected != socket.current).then_some(SocketAssignment {
                    piece_index,
                    socket_index: socket.socket_index,
                    previous: socket.current,
                    selected: *selected,
                    kind: socket.kind,
                })
            },
        ));
    }
    let projected_totals = capped_totals(search.totals);
    let shortfalls = std::array::from_fn(|index| {
        if targets[index] == 0 {
            0
        } else {
            targets[index].saturating_sub(projected_totals[index])
        }
    });
    Solution {
        projected_totals,
        selections,
        swaps,
        assignments,
        shortfalls,
        exact: shortfalls.iter().all(|shortfall| *shortfall == 0),
    }
}

fn partial_cmp(left: &SearchState, right: &SearchState, targets: [u16; 6]) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| left.swaps.cmp(&right.swaps))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| {
            target_excess(left.totals, targets).cmp(&target_excess(right.totals, targets))
        })
        .then_with(|| total_value(right.totals, targets).cmp(&total_value(left.totals, targets)))
        .then_with(|| plan_tie_key(left).cmp(&plan_tie_key(right)))
}

fn final_cmp(left: &SearchState, right: &SearchState, targets: [u16; 6]) -> Ordering {
    let left_score = score(left.totals, targets);
    let right_score = score(right.totals, targets);
    left_score
        .cmp(&right_score)
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| left.swaps.cmp(&right.swaps))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| {
            target_excess(left.totals, targets).cmp(&target_excess(right.totals, targets))
        })
        .then_with(|| total_value(right.totals, targets).cmp(&total_value(left.totals, targets)))
        .then_with(|| plan_tie_key(left).cmp(&plan_tie_key(right)))
}

fn plan_tie_key(state: &SearchState) -> Vec<(usize, Vec<Option<u64>>)> {
    state
        .plans
        .iter()
        .map(|plan| (plan.candidate_index, plan.selections.clone()))
        .collect()
}

fn score(totals: [i32; 6], targets: [u16; 6]) -> (u32, u16) {
    let totals = capped_totals(totals);
    let mut shortfall = 0_u32;
    let mut largest = 0_u16;
    for index in 0..6 {
        if targets[index] == 0 {
            continue;
        }
        let missing = targets[index].saturating_sub(totals[index]);
        shortfall = shortfall.saturating_add(u32::from(missing));
        largest = largest.max(missing);
    }
    (shortfall, largest)
}

fn target_excess(totals: [i32; 6], targets: [u16; 6]) -> u32 {
    capped_totals(totals)
        .into_iter()
        .zip(targets)
        .filter(|(_, target)| *target > 0)
        .map(|(total, target)| u32::from(total.saturating_sub(target)))
        .sum()
}

fn total_value(totals: [i32; 6], targets: [u16; 6]) -> u32 {
    capped_totals(totals)
        .into_iter()
        .zip(targets)
        .filter(|(_, target)| *target == 0)
        .map(|(total, _)| u32::from(total))
        .sum()
}

fn waste_above_cap(totals: [i32; 6]) -> u32 {
    totals
        .into_iter()
        .map(|value| value.saturating_sub(i32::from(armor_stat_allocation::TARGET_MAX)))
        .filter_map(|value| u32::try_from(value).ok())
        .sum()
}

fn capped_totals(totals: [i32; 6]) -> [u16; 6] {
    totals.map(|value| {
        u16::try_from(value.clamp(0, i32::from(armor_stat_allocation::TARGET_MAX)))
            .unwrap_or(armor_stat_allocation::TARGET_MAX)
    })
}

fn cap_u16_totals(totals: [u16; 6]) -> [u16; 6] {
    totals.map(|value| value.min(armor_stat_allocation::TARGET_MAX))
}

fn solver_key(totals: [i32; 6], targets: [u16; 6]) -> [u16; 6] {
    let totals = capped_totals(totals);
    std::array::from_fn(|index| {
        if targets[index] == 0 {
            0
        } else {
            totals[index]
        }
    })
}

fn apply_preview(app: &mut SundialApp, state: &mut State, character_index: usize) {
    let Some(input) = state.input.as_ref() else {
        state.feedback = Some(Feedback {
            text: "Equipped armor is unavailable".to_owned(),
            detail: None,
            is_error: true,
        });
        return;
    };
    let Some(solution) = state.preview.clone() else {
        return;
    };
    if solution.assignments.is_empty() && solution.swaps.is_empty() {
        return;
    }

    let mut updated = app.document.clone();
    let result = (|| {
        for swap in &solution.swaps {
            let current = input
                .pieces
                .get(swap.piece_index)
                .ok_or("The armor preview is stale")?;
            if current.locked {
                return Err(format!("{} is locked", current.label));
            }
            let candidate = selected_candidate(input, &solution, swap.piece_index)
                .ok_or("The selected inventory armor is unavailable")?;
            let item = candidate
                .piece
                .item
                .as_ref()
                .ok_or("An armor definition is unavailable")?;
            if u32::try_from(item.hash).ok() != Some(swap.definition_hash) {
                return Err(
                    "The selected inventory armor changed before it could be equipped".to_owned(),
                );
            }
            let location = inventory::character_inventory(&updated, character_index)
                .map_err(|error| error.to_string())?
                .and_then(|items| {
                    items
                        .into_iter()
                        .find(|snapshot| snapshot.instance_soid == swap.instance_soid)
                        .map(|snapshot| snapshot.location)
                })
                .ok_or("The selected inventory armor no longer exists")?;
            equip_inventory_item(&mut updated, location, current.slot, item)?;
        }
        for assignment in &solution.assignments {
            let candidate = selected_candidate(input, &solution, assignment.piece_index)
                .ok_or("The armor preview is stale")?;
            let piece = &candidate.piece;
            if piece.locked || piece.issue.is_some() {
                return Err(format!("{} is no longer editable", piece.label));
            }
            let item = piece
                .item
                .as_ref()
                .ok_or("An armor definition is unavailable")?;
            set_equipment_item_plug(
                &mut updated,
                character_index,
                piece.slot,
                assignment.socket_index,
                &item.default_plugs,
                assignment.selected,
            )?;
        }
        settings::validate_document(&updated)
            .map_err(|error| format!("Adjusted armor did not pass validation: {error}"))?;
        Ok::<(), String>(())
    })();

    match result {
        Ok(()) => {
            let plug_count = solution
                .assignments
                .iter()
                .filter(|assignment| assignment.kind == SocketKind::Stat)
                .count();
            let masterwork_count = solution
                .assignments
                .iter()
                .filter(|assignment| assignment.kind == SocketKind::Masterwork)
                .count();
            let swap_count = solution.swaps.len();
            let piece_count = changed_piece_count(&solution);
            let (summary, detail) = if solution.exact {
                (
                    "Targets met".to_owned(),
                    "Every selected goal was met".to_owned(),
                )
            } else {
                let missed = solution
                    .shortfalls
                    .iter()
                    .filter(|shortfall| **shortfall > 0)
                    .count();
                (
                    format!("Closest match · {missed} goals short"),
                    format!("Closest match · {}", format_shortfalls(solution.shortfalls)),
                )
            };
            app.document = updated;
            app.dirty = true;
            app.set_status(
                format!(
                    "Adjusted {piece_count} armor piece(s): {swap_count} swap(s), {masterwork_count} masterwork(s), and {plug_count} stat plug(s); click Save to write it"
                ),
                false,
            );
            state.feedback = Some(Feedback {
                text: format!("Armor adjusted · {summary}"),
                detail: Some(detail),
                is_error: false,
            });
            state.source_key = None;
            state.input = None;
            state.preview = None;
            state.preview_task = None;
            state.preview_due_at = None;
            state.preserve_feedback_once = true;
        }
        Err(error) => {
            app.set_status(format!("Armor stats not adjusted: {error}"), true);
            state.feedback = Some(Feedback {
                text: error,
                detail: None,
                is_error: true,
            });
        }
    }
}

fn selected_candidate<'a>(
    input: &'a LoadoutInput,
    solution: &Solution,
    piece_index: usize,
) -> Option<&'a ArmorCandidate> {
    let candidate_index = solution
        .selections
        .get(piece_index)
        .map_or(0, |selection| selection.candidate_index);
    input.candidates.get(piece_index)?.get(candidate_index)
}

fn changed_piece_count(solution: &Solution) -> usize {
    let mut pieces = solution
        .assignments
        .iter()
        .map(|assignment| assignment.piece_index)
        .chain(solution.swaps.iter().map(|swap| swap.piece_index))
        .collect::<Vec<_>>();
    pieces.sort_unstable();
    pieces.dedup();
    pieces.len()
}

fn format_shortfalls(shortfalls: [u16; 6]) -> String {
    armor_stat_allocation::STAT_NAMES
        .into_iter()
        .zip(shortfalls)
        .filter_map(|(name, shortfall)| {
            (shortfall > 0).then(|| format!("{name} {shortfall} short"))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn format_totals(totals: [u16; 6]) -> String {
    armor_stat_allocation::STAT_NAMES
        .into_iter()
        .zip(totals)
        .map(|(name, total)| format!("{total} {name}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn plug_name(catalog: &Catalog, hash: Option<u64>) -> String {
    hash.and_then(|hash| catalog.display_name(hash).map(str::to_owned))
        .unwrap_or_else(|| "Empty".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_columns_fit_supported_window_widths() {
        for available in [520.0, 600.0, 760.0, 980.0] {
            let gap = 8.0;
            let widths = preview_column_widths(available, gap);
            let used = widths.into_iter().sum::<f32>() + 3.0 * gap;
            assert!(
                used <= available + f32::EPSILON,
                "preview used {used} px with {available} px available"
            );
        }
    }

    #[test]
    fn reopening_resets_window_geometry_without_losing_targets() {
        let mut state = State::default();
        state.targets[1] = 100;

        state.open(0);
        let first_generation = state.window_generation;
        state.open = false;
        state.open(0);

        assert_eq!(state.targets[1], 100);
        assert_eq!(state.window_generation, first_generation.wrapping_add(1));
    }

    fn choice(hash: u64, values: [i32; 6]) -> SocketChoice {
        SocketChoice {
            hash: Some(hash),
            values,
        }
    }

    fn input(sockets: Vec<MutableSocket>, fixed_totals: [i32; 6]) -> LoadoutInput {
        let candidates = sockets
            .into_iter()
            .enumerate()
            .map(|(index, socket)| {
                vec![ArmorCandidate {
                    piece: unavailable_piece("helmet", "Helmet", "test"),
                    origin: ArmorOrigin::Equipped,
                    fixed_totals: if index == 0 { fixed_totals } else { [0; 6] },
                    sockets: vec![socket],
                    exotic: false,
                }]
            })
            .collect::<Vec<_>>();
        LoadoutInput {
            pieces: (0..candidates.len())
                .map(|_| unavailable_piece("helmet", "Helmet", "test"))
                .collect(),
            candidates,
            current_totals: capped_totals(fixed_totals),
        }
    }

    fn socket(_index: usize, current: u64, choices: Vec<SocketChoice>) -> MutableSocket {
        MutableSocket {
            socket_index: 0,
            current: Some(current),
            choices,
            kind: SocketKind::Stat,
        }
    }

    #[test]
    fn exact_goal_can_be_shared_across_multiple_armor_pieces() {
        let input = input(
            vec![
                socket(
                    0,
                    10,
                    vec![choice(10, [0; 6]), choice(11, [0, 0, 10, 0, 0, 0])],
                ),
                socket(
                    1,
                    20,
                    vec![choice(20, [0; 6]), choice(21, [0, 0, 20, 0, 0, 0])],
                ),
            ],
            [0, 0, 70, 0, 0, 0],
        );

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(solution.exact);
        assert_eq!(solution.projected_totals[2], 100);
        assert_eq!(solution.assignments.len(), 2);
        assert_eq!(changed_piece_count(&solution), 2);
    }

    #[test]
    fn impossible_goal_returns_and_applies_the_closest_plan() {
        let input = input(
            vec![socket(
                0,
                10,
                vec![choice(10, [0; 6]), choice(11, [0, 0, 16, 0, 0, 0])],
            )],
            [0, 0, 80, 0, 0, 0],
        );

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(!solution.exact);
        assert_eq!(solution.projected_totals[2], 96);
        assert_eq!(solution.shortfalls[2], 4);
        assert_eq!(solution.assignments.len(), 1);
    }

    #[test]
    fn zero_targets_are_ignored_and_current_choices_win_ties() {
        let input = input(
            vec![socket(
                0,
                10,
                vec![
                    choice(10, [0, 0, 10, 0, 0, 0]),
                    choice(11, [50, 0, 0, 0, 0, 0]),
                ],
            )],
            [50, 50, 90, 50, 50, 50],
        );

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(solution.exact);
        assert!(solution.assignments.is_empty());
    }

    #[test]
    fn goals_are_minimums_and_already_met_stats_do_not_trigger_changes() {
        let input = input(
            vec![socket(
                0,
                10,
                vec![
                    choice(10, [0, 0, 18, 0, 0, 0]),
                    choice(11, [0, 0, 10, 0, 0, 0]),
                ],
            )],
            [0; 6],
        );

        let solution = solve(&input, [0, 0, 10, 0, 0, 0]);

        assert!(solution.exact);
        assert_eq!(solution.projected_totals[2], 18);
        assert!(solution.assignments.is_empty());
    }

    #[test]
    fn multi_goal_solver_minimizes_total_shortfall_before_the_largest_gap() {
        let input = input(
            vec![socket(
                0,
                10,
                vec![
                    choice(10, [0, 0, 0, 0, 0, 0]),
                    choice(11, [20, 0, 2, 0, 0, 0]),
                    choice(12, [10, 0, 10, 0, 0, 0]),
                ],
            )],
            [70, 0, 70, 0, 0, 0],
        );

        let solution = solve(&input, [90, 0, 90, 0, 0, 0]);

        assert_eq!(solution.projected_totals, [90, 0, 72, 0, 0, 0]);
        assert_eq!(solution.shortfalls, [0, 0, 18, 0, 0, 0]);
    }

    #[test]
    fn useful_stat_range_is_clamped_to_the_real_cap() {
        assert_eq!(
            capped_totals([-1, 0, 50, 100, 101, i32::MAX]),
            [0, 0, 50, 100, 100, 100]
        );
        assert_eq!(
            cap_u16_totals([0, 50, 99, 100, 101, u16::MAX]),
            [0, 50, 99, 100, 100, 100]
        );
    }

    #[test]
    fn solver_state_keys_cap_goals_and_compare_other_stats_separately() {
        assert_eq!(
            solver_key([10, 20, 130, 40, 150, 60], [0, 0, 100, 0, 100, 0]),
            [0, 0, 100, 0, 100, 0]
        );
    }

    #[test]
    fn solver_removes_points_above_the_cap_when_an_exact_option_exists() {
        let input = input(
            vec![socket(
                0,
                10,
                vec![
                    choice(10, [0, 0, 10, 0, 0, 0]),
                    choice(11, [0, 0, 5, 0, 0, 0]),
                ],
            )],
            [0, 0, 95, 0, 0, 0],
        );

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(solution.exact);
        assert_eq!(solution.projected_totals[2], 100);
        assert_eq!(solution.assignments.len(), 1);
        assert_eq!(solution.assignments[0].selected, Some(11));
    }

    #[test]
    fn candidate_reduction_keeps_differences_in_non_target_stats() {
        let socket = socket(
            0,
            10,
            vec![
                choice(10, [0, 0, 10, 0, 0, 0]),
                choice(11, [20, 0, 10, 0, 0, 0]),
            ],
        );

        assert_eq!(choices_for_targets(&socket, [0, 0, 100, 0, 0, 0]).len(), 2);
    }

    fn synthetic_candidate(
        fixed_totals: [i32; 6],
        origin: ArmorOrigin,
        sockets: Vec<MutableSocket>,
        exotic: bool,
    ) -> ArmorCandidate {
        ArmorCandidate {
            piece: unavailable_piece("helmet", "Helmet", "test"),
            origin,
            fixed_totals,
            sockets,
            exotic,
        }
    }

    #[test]
    fn inventory_armor_is_selected_when_equipped_armor_cannot_reach_the_goal() {
        let input = LoadoutInput {
            pieces: vec![unavailable_piece("helmet", "Helmet", "test")],
            candidates: vec![vec![
                synthetic_candidate([0, 0, 80, 0, 0, 0], ArmorOrigin::Equipped, vec![], false),
                synthetic_candidate(
                    [0, 0, 100, 0, 0, 0],
                    ArmorOrigin::Inventory {
                        instance_soid: 42,
                        definition_hash: 7,
                    },
                    vec![],
                    false,
                ),
            ]],
            current_totals: [0, 0, 80, 0, 0, 0],
        };

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(solution.exact);
        assert_eq!(solution.swaps.len(), 1);
        assert_eq!(solution.swaps[0].instance_soid, 42);
        assert_eq!(solution.selections[0].candidate_index, 1);
    }

    #[test]
    fn masterworking_current_armor_is_preferred_to_an_inventory_swap() {
        let masterwork = MutableSocket {
            socket_index: 5,
            current: Some(10),
            choices: vec![choice(10, [0; 6]), choice(11, [0, 0, 12, 0, 0, 0])],
            kind: SocketKind::Masterwork,
        };
        let input = LoadoutInput {
            pieces: vec![unavailable_piece("helmet", "Helmet", "test")],
            candidates: vec![vec![
                synthetic_candidate(
                    [0, 0, 88, 0, 0, 0],
                    ArmorOrigin::Equipped,
                    vec![masterwork],
                    false,
                ),
                synthetic_candidate(
                    [0, 0, 100, 0, 0, 0],
                    ArmorOrigin::Inventory {
                        instance_soid: 42,
                        definition_hash: 7,
                    },
                    vec![],
                    false,
                ),
            ]],
            current_totals: [0, 0, 88, 0, 0, 0],
        };

        let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

        assert!(solution.exact);
        assert!(solution.swaps.is_empty());
        assert_eq!(solution.assignments.len(), 1);
        assert_eq!(solution.assignments[0].kind, SocketKind::Masterwork);
    }

    #[test]
    fn optimizer_never_selects_two_exotic_armor_pieces() {
        let input = LoadoutInput {
            pieces: vec![
                unavailable_piece("helmet", "Helmet", "test"),
                unavailable_piece("gauntlets", "Gauntlets", "test"),
            ],
            candidates: vec![
                vec![
                    synthetic_candidate([0; 6], ArmorOrigin::Equipped, vec![], false),
                    synthetic_candidate(
                        [100, 0, 0, 0, 0, 0],
                        ArmorOrigin::Inventory {
                            instance_soid: 1,
                            definition_hash: 1,
                        },
                        vec![],
                        true,
                    ),
                ],
                vec![
                    synthetic_candidate([0; 6], ArmorOrigin::Equipped, vec![], false),
                    synthetic_candidate(
                        [0, 100, 0, 0, 0, 0],
                        ArmorOrigin::Inventory {
                            instance_soid: 2,
                            definition_hash: 2,
                        },
                        vec![],
                        true,
                    ),
                ],
            ],
            current_totals: [0; 6],
        };

        let solution = solve(&input, [100, 100, 0, 0, 0, 0]);

        assert!(!solution.exact);
        assert_eq!(solution.swaps.len(), 1);
        assert_eq!(
            solution
                .shortfalls
                .iter()
                .filter(|value| **value == 100)
                .count(),
            1
        );
    }
}
