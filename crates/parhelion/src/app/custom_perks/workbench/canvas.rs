//! One program canvas for a stock effect and an authored program.
//!
//! Both read the same way: a header with the name, then trigger, action, end condition
//! and reactivation rows. An editable block draws the program controls
//! from `program.rs`. A locked block draws the summary line of the native node, its support
//! badge and its mapped facts, and never hides a node Parhelion cannot read.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    action::{ActionSummary, ConditionRole, GroupSummary, SummaryLine},
    dependencies::Behavior,
    nodes::Support,
    program::Program,
};

/// Width of the row label column.
const LABEL_WIDTH: f32 = 112.0;

/// Draws parameter controls for the asset a stock effect block references.
pub(in crate::app::custom_perks) type Placer<'a> = dyn FnMut(&mut egui::Ui, u32) + 'a;

/// The editing backend for a program. Without it the program is drawn locked.
pub(in crate::app::custom_perks) struct Editing<'a> {
    pub workbench: &'a mut Workbench,
    pub catalog: Option<&'a InvestmentCatalog>,
}

/// Where the blocks come from.
pub(in crate::app::custom_perks) enum Backend<'a> {
    /// An authored program. Blocks are editable when a workbench is supplied.
    Program {
        program: &'a mut Program,
        /// The installed perk this program was recovered from, when it is not an authored
        /// one. A stock effect reads in these rows before anyone converts it.
        stock: Option<&'a str>,
        /// What this effect does, in the game's own words, when there is such a sentence.
        description: Option<&'a str>,
        /// Names for the assets the rows reference, so a locked card names them as a live
        /// one does and its component buttons exist at all.
        labels: &'a BTreeMap<u32, String>,
        editing: Option<Editing<'a>>,
    },
    /// A decoded stock action, read through its summary.
    Stock {
        summary: &'a ActionSummary,
        action_tag: u32,
        /// Whether the action fits the program model.
        editable: bool,
    },
    /// The cached digest of a stock action whose payload has not been loaded yet.
    Digest {
        behavior: &'a Behavior,
        /// Source description is appropriate only while the stock behavior is unchanged.
        description: Option<&'a str>,
        /// Names for the assets the reading references.
        labels: &'a BTreeMap<u32, String>,
        /// An activation the reader chose for this effect, which replaces the stock
        /// trigger in the reading. The rest of the stock behavior is unchanged.
        activation: Option<&'a str>,
    },
}

/// Everything one canvas needs for a frame.
pub(in crate::app::custom_perks) struct Canvas<'a, 'b> {
    /// Name of a stock effect. A program shows its own name instead.
    pub name: &'a str,
    pub backend: Backend<'a>,
    /// Controls drawn at the right of the header, such as the effect menu.
    pub header: Option<&'b mut dyn FnMut(&mut egui::Ui)>,
    /// Parameter controls drawn on the stock effect block that references the given asset.
    pub place: Option<&'b mut Placer<'b>>,
    /// Controls drawn under the rows, inside the same card.
    pub footer: Option<&'b mut dyn FnMut(&mut egui::Ui)>,
    /// A command drawn inside the trigger row of a stock reading, so a stock effect places
    /// it where an authored program places its own trigger control.
    pub trigger_command: Option<&'b mut dyn FnMut(&mut egui::Ui)>,
}

/// What the user asked for on the canvas this frame.
#[derive(Default)]
pub(in crate::app::custom_perks) struct Output {
    /// The program action whose asset properties should open.
    pub edit_action: Option<usize>,
}

pub(in crate::app::custom_perks) fn draw(ui: &mut egui::Ui, canvas: Canvas<'_, '_>) -> Output {
    draw_card(ui, canvas, None)
}

pub(super) fn draw_effect(ui: &mut egui::Ui, canvas: Canvas<'_, '_>, card: cards::Card) -> Output {
    draw_card(ui, canvas, Some(card))
}

fn draw_card(ui: &mut egui::Ui, canvas: Canvas<'_, '_>, card: Option<cards::Card>) -> Output {
    let Canvas {
        name,
        backend,
        header,
        place,
        footer,
        trigger_command,
    } = canvas;
    let mut output = Output::default();
    crate::app::style::card(ui, |ui| {
        match backend {
            Backend::Program {
                program,
                stock,
                description,
                labels,
                editing,
            } => {
                let editable = editing.is_some();
                let expanded = draw_header(ui, header, card, |ui| {
                    // A stock card draws a clone whose name this frame restamped, so a box
                    // over it read every keystroke as an edit and converted the effect. The
                    // name becomes editable once the effect owns its program.
                    if editable && stock.is_none() {
                        let width = (ui.available_width() - 120.0).max(120.0);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut program.name)
                                .id_salt("effect-name")
                                .desired_width(width)
                                .hint_text("Effect Name"),
                        );
                        crate::app::style::named_control(response, "Effect Name");
                    } else {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(stock.unwrap_or(&program.name)).strong(),
                            )
                            .wrap(),
                        );
                    }
                });
                if !expanded {
                    return;
                }
                if !editable && stock.is_none() {
                    ui.small("Turn on Experimental Features in Preferences to edit this effect.");
                }
                if program.native.is_none() && (!editable || program.actions.len() > 1) {
                    let summary = super::guidance::summary_with_assets(
                        program,
                        editing
                            .as_ref()
                            .map(|editing| &editing.workbench.keys.catalog),
                        editing
                            .as_ref()
                            .map(|editing| &editing.workbench.asset_labels),
                    );
                    // Truncated labels already expose their full text on hover.
                    ui.add(egui::Label::new(egui::RichText::new(&summary).weak()).truncate());
                }
                if let Some(description) = description {
                    ui.add(egui::Label::new(description).wrap());
                }
                ui.add_space(4.0);
                output.edit_action = draw_program_rows(ui, program, labels, editing);
            }
            Backend::Stock {
                summary,
                action_tag,
                editable,
            } => {
                let expanded = draw_header(ui, header, card, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap());
                    stock_badge(ui, summary.support, editable);
                });
                if !expanded {
                    return;
                }
                ui.add(egui::Label::new(&summary.headline).wrap());
                draw_stock_rows(ui, summary, place);
                for note in &summary.notes {
                    ui.small(note);
                }
                ui.horizontal(|ui| {
                    if ui.small_button("Copy Summary").clicked() {
                        ui.ctx().copy_text(summary.render());
                    }
                    if action_tag != 0 {
                        ui.weak(format!("Action 0x{action_tag:08X}"));
                    }
                });
            }
            Backend::Digest {
                behavior,
                description,
                labels,
                activation,
            } => {
                let expanded = draw_header(ui, header, card, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap());
                    stock_badge(ui, behavior.support, behavior.editable);
                });
                if !expanded {
                    return;
                }
                ui.add(egui::Label::new(description.unwrap_or(&behavior.headline)).wrap());
                // A stock effect reads in the rows an authored one is edited in. It used to
                // show one line naming its effect kinds, so what a stock perk actually does
                // stayed hidden until the reader converted it.
                super::reading::rows(ui, behavior, labels, activation, trigger_command);
            }
        }
        if let Some(footer) = footer {
            footer(ui);
        }
    });
    output
}

pub(super) fn draw_header(
    ui: &mut egui::Ui,
    controls: Option<&mut (dyn FnMut(&mut egui::Ui) + '_)>,
    card: Option<cards::Card>,
    title: impl FnOnce(&mut egui::Ui),
) -> bool {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(controls) = controls {
                controls(ui);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if let Some(card) = card {
                    card.controls(ui);
                }
                title(ui);
            });
        });
    });
    card.is_none_or(|card| card.expanded(ui.ctx()))
}

fn stock_badge(ui: &mut egui::Ui, support: Support, editable: bool) {
    if !editable {
        support_badge(ui, support);
    }
}

pub(in crate::app::custom_perks) use sundial::ui::catalog::support_badge;

/// One canvas row: a fixed label column and the blocks beside it. The label sits on the
/// first line of its content so a row with one control reads as one line.
pub(super) fn row<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.add_space(3.0);
    if ui.available_width() < LABEL_WIDTH + 300.0 {
        return ui
            .vertical(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.strong(label).on_hover_text(hint);
                    },
                );
                content(ui)
            })
            .inner;
    }
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, ui.spacing().interact_size.y),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.add(
                    egui::Label::new(egui::RichText::new(label).strong())
                        .halign(egui::Align::Max)
                        .wrap(),
                )
                .on_hover_text(hint);
            },
        );
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            content(ui)
        })
        .inner
    })
    .inner
}

/// One effect block. It sits inside the card's outline, so it is set off by its own fill
/// rather than a second outline of equal weight.
pub(super) fn block<R>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.push_id(salt, |ui| {
        crate::app::style::block(ui.style())
            .fill(ui.visuals().window_fill())
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                content(ui)
            })
            .inner
    })
}

/// The same action commands and alignment serve guided and recovered effects.
pub(super) fn action_header(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    index: usize,
    count: usize,
    properties: Option<&mut properties::Panel>,
) -> program::ActionEvent {
    let mut event = program::ActionEvent::default();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::app::style::more_menu(ui, |ui| {
                for (label, target) in [
                    ("Move Up", index.checked_sub(1)),
                    ("Move Down", (index + 1 < count).then_some(index + 1)),
                ] {
                    if ui
                        .add_enabled(target.is_some(), egui::Button::new(label))
                        .clicked()
                    {
                        event.swap_with = target;
                        ui.close_menu();
                    }
                }
                if ui.button("Remove Action").clicked() {
                    event.remove = true;
                    ui.close_menu();
                }
            });
            if let Some(properties) = properties {
                properties.button(ui);
            }
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                |ui| {
                    ui.strong(format!("{}. {title}", index + 1))
                        .on_hover_text(hint);
                },
            );
        });
    });
    event
}

/// A condition row's content, aligned with the effect blocks but without a frame of its
/// own, so a single control does not sit inside two outlines.
pub(super) fn plain<R>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.push_id(salt, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        content(ui)
                    })
                    .inner
            },
        )
        .inner
    })
    .inner
}

pub(super) const ACTIVATION_HINT: &str = "Any of these conditions can trigger this effect.";
pub(super) const EFFECTS_HINT: &str =
    "Actions run when this effect is triggered, in the order shown.";
pub(super) const REMOVAL_HINT: &str =
    "Any one of these conditions ends this effect. Spawned objects keep their own lifetime.";
pub(super) const REARM_HINT: &str = "Any one of these conditions lets this effect trigger again.";

fn draw_program_rows(
    ui: &mut egui::Ui,
    program: &mut Program,
    labels: &BTreeMap<u32, String>,
    editing: Option<Editing<'_>>,
) -> Option<usize> {
    // Nested conditions need the same semantic editor whether the effect was authored
    // here or recovered from a package. Keep the original representation on a read-only
    // frame, and adopt the checked native draft only after an actual edit.
    if program.native.is_none() && editing.is_some() && program::uses_complete_editor(program) {
        match sundial::package_authoring::sandbox_perk::program::native_draft(program) {
            Ok(mut native) => {
                native.graph.compact();
                let before = native.clone();
                let mut displayed = Program {
                    name: program.name.clone(),
                    native: Some(native),
                    ..Program::default()
                };
                let output = draw_program_rows(ui, &mut displayed, labels, editing);
                if displayed.native.as_ref() != Some(&before) {
                    *program = displayed;
                } else if let Some(asset) = output.and_then(|index| displayed.asset(index)) {
                    if let Some(index) = program
                        .actions
                        .iter()
                        .position(|action| action.asset() == Some(asset))
                    {
                        return Some(index);
                    }
                    // A native-only component has no slot in the guided representation.
                    // Opening its editor adopts the same checked record used by the view.
                    *program = displayed;
                }
                return output;
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }
    if let Some(native) = &mut program.native {
        return match editing {
            Some(Editing { workbench, .. }) => {
                let labels = workbench.program_asset_labels(native, &program.name);
                program::draw_complete(
                    ui,
                    native,
                    true,
                    &labels,
                    &mut |ui, request| match request {
                        program::NativeRequest::Action(context) => workbench
                            .behaviors
                            .draw_action(
                                ui,
                                &workbench.discovery,
                                &workbench.perk_names,
                                &workbench.asset_labels,
                                context,
                                &workbench.keys.catalog,
                            )
                            .map(super::behaviors::Selection::Action),
                        program::NativeRequest::Condition(label) => workbench
                            .behaviors
                            .draw_condition_named(
                                ui,
                                &workbench.discovery,
                                &workbench.perk_names,
                                &workbench.asset_labels,
                                label,
                            )
                            .map(super::behaviors::Selection::Condition),
                        program::NativeRequest::Asset(asset, scope) => {
                            let label = match scope {
                                super::assets::AssetScope::Projectiles => "Projectile",
                                super::assets::AssetScope::Spawnable => "Object or Effect",
                                _ => "Attachment",
                            };
                            properties::field(ui, label, "The asset this action uses.", |ui| {
                                workbench.draw_asset_picker(ui, None, asset, scope);
                            });
                            if scope == super::assets::AssetScope::Projectiles {
                                workbench.properties.movement(ui, asset);
                            }
                            None
                        }
                    },
                )
            }
            None => program::draw_complete(ui, native, false, labels, &mut |_, _| None),
        };
    }
    let mut edit = None;
    match editing {
        Some(Editing { workbench, catalog }) => {
            row(ui, "Trigger", ACTIVATION_HINT, |ui| {
                plain(ui, "trigger", |ui| {
                    program::draw_trigger_block(ui, program, |ui, label, retained| {
                        workbench.behaviors.draw_trigger(
                            ui,
                            &workbench.discovery,
                            &workbench.perk_names,
                            &workbench.asset_labels,
                            label,
                            retained,
                        )
                    })
                });
            });
            row(ui, "Actions", EFFECTS_HINT, |ui| {
                if program.actions.is_empty()
                    && let Some(hint) = program.authoring_hint()
                {
                    ui.weak(hint);
                }
                let kill_trigger = program.has_kill_trigger();
                let count = program.actions.len();
                let mut remove = None;
                let mut swap = None;
                for (index, action) in program.actions.iter_mut().enumerate() {
                    let response = block(ui, index, |ui| {
                        let event = workbench.draw_action_block(
                            ui,
                            catalog,
                            kill_trigger,
                            action,
                            index,
                            count,
                        );
                        if event.edit {
                            edit = Some(index);
                        }
                        if event.remove {
                            remove = Some(index);
                        }
                        if let Some(target) = event.swap_with {
                            swap = Some((index, target));
                        }
                    });
                    if workbench.reveal_action == Some(index) {
                        response.response.scroll_to_me(Some(egui::Align::Center));
                        workbench.reveal_action = None;
                    }
                }
                if let Some(index) = remove {
                    program.actions.remove(index);
                    edit = None;
                }
                if let Some((from, to)) = swap {
                    program.actions.swap(from, to);
                    edit = None;
                }
                ui.horizontal(|ui| {
                    if let Some(action) = workbench.behaviors.draw_action(
                        ui,
                        &workbench.discovery,
                        &workbench.perk_names,
                        &workbench.asset_labels,
                        program,
                        &workbench.keys.catalog,
                    ) {
                        program.actions.push(action);
                    }
                });
            });
            workbench.draw_removal_block(ui, program);
            program::draw_rearm_block(ui, program, |ui, label| {
                workbench.behaviors.draw_condition_named(
                    ui,
                    &workbench.discovery,
                    &workbench.perk_names,
                    &workbench.asset_labels,
                    label,
                )
            });
        }
        None => {
            row(ui, "Trigger", ACTIVATION_HINT, |ui| {
                plain(ui, "trigger", |ui| {
                    ui.label(program::trigger_text(program));
                });
            });
            row(ui, "Actions", EFFECTS_HINT, |ui| {
                if program.actions.is_empty() {
                    ui.weak("No actions yet.");
                }
                for (index, action) in program.actions.iter().enumerate() {
                    block(ui, index, |ui| {
                        ui.add(
                            egui::Label::new(format!(
                                "{}. {}",
                                index + 1,
                                program::action_text(action, None)
                            ))
                            .wrap(),
                        );
                    });
                }
            });
            if let Some(text) = program::removal_text(program, None) {
                row(ui, "End Condition", REMOVAL_HINT, |ui| {
                    plain(ui, "removal", |ui| {
                        ui.label(text);
                    });
                });
            }
            if let Some(text) = program::rearm_text(program) {
                row(ui, program::rearm_label(program), REARM_HINT, |ui| {
                    plain(ui, "rearm", |ui| {
                        ui.label(text);
                    });
                });
            }
        }
    }
    edit
}

fn draw_stock_rows(ui: &mut egui::Ui, summary: &ActionSummary, mut place: Option<&mut Placer<'_>>) {
    for (index, group) in summary.groups.iter().enumerate() {
        ui.push_id(index, |ui| {
            if summary.groups.len() > 1 {
                ui.add_space(4.0);
                ui.strong(&group.label);
            }
            draw_stock_group(ui, group, place.as_deref_mut());
        });
    }
}

fn draw_stock_group(ui: &mut egui::Ui, group: &GroupSummary, mut place: Option<&mut Placer<'_>>) {
    let activation = group.conditions(ConditionRole::Activation);
    row(ui, "Trigger", ACTIVATION_HINT, |ui| {
        plain(ui, "trigger", |ui| {
            if activation.is_empty() {
                ui.label("Always").on_hover_text(
                    "No activation condition. The action starts as soon as the perk is applied.",
                );
            }
            for (index, line) in activation.iter().enumerate() {
                ui.push_id(index, |ui| step(ui, line));
            }
        });
    });
    row(ui, "Actions", EFFECTS_HINT, |ui| {
        if group.effects.is_empty() {
            ui.weak("No effects.");
        }
        for (index, line) in group.effects.iter().enumerate() {
            block(ui, index, |ui| {
                ui.horizontal_top(|ui| {
                    ui.strong(format!("{}.", index + 1));
                    ui.vertical(|ui| {
                        ui.set_min_width(ui.available_width());
                        step(ui, line);
                        if let (Some(place), Some(asset)) = (place.as_deref_mut(), line.asset) {
                            place(ui, asset);
                        }
                    });
                });
            });
        }
    });
    for (role, heading, hint) in [
        (ConditionRole::Removal, "End Condition", REMOVAL_HINT),
        (ConditionRole::Rearm, "Reactivation", REARM_HINT),
    ] {
        let lines = group.conditions(role);
        if lines.is_empty() {
            continue;
        }
        row(ui, heading, hint, |ui| {
            plain(ui, heading, |ui| {
                for (index, line) in lines.iter().enumerate() {
                    ui.push_id(index, |ui| step(ui, line));
                }
            });
        });
    }
}

/// One locked summary line: the text, the kind name on hover, a support badge for nodes
/// Parhelion cannot fully read, and the mapped facts behind a `Fields` disclosure.
pub(in crate::app::custom_perks) fn step(ui: &mut egui::Ui, line: &SummaryLine) {
    ui.horizontal_top(|ui| {
        ui.add_space(14.0 * line.depth as f32);
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(egui::Label::new(&line.text).wrap())
                    .on_hover_text(&line.kind_name);
                if line.support >= Support::Structural {
                    support_badge(ui, line.support);
                }
            });
            if let Some((condition, node)) = &line.native {
                egui::CollapsingHeader::new("Complete Native Record")
                    .id_salt("complete-native-record")
                    .show(ui, |ui| super::program::read_native(ui, *condition, node));
            }
            if !line.detail.is_empty() {
                egui::CollapsingHeader::new(egui::RichText::new("Fields").small())
                    .id_salt("behavior-line-fields")
                    .show(ui, |ui| {
                        for detail in &line.detail {
                            ui.small(detail);
                        }
                    });
            }
        });
    });
}
