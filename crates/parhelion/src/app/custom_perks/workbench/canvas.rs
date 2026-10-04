//! One program canvas for a stock effect and an authored program.
//!
//! Both read the same way: a header with the name, then trigger, action, end condition
//! and reactivation rows. An editable block draws the program controls from `program.rs`. A
//! locked block reads the same native records, and never hides a node Parhelion cannot read.
use super::*;
use sundial::package_authoring::sandbox_perk::{
    dependencies::Behavior, nodes::Support, program::Program,
};

/// Width of the row label column.
const LABEL_WIDTH: f32 = 112.0;

/// The editing backend for a program. Without it the program is drawn locked.
pub(in crate::app::custom_perks) struct Editing<'a> {
    pub workbench: &'a mut Workbench,
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

pub(super) fn draw_effect(ui: &mut egui::Ui, canvas: Canvas<'_, '_>, card: cards::Card) -> Output {
    let Canvas {
        name,
        backend,
        header,
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
                let folded = !card.expanded(ui.ctx());
                let brief = folded
                    .then(|| program::brief(program))
                    .flatten()
                    .map(|(trigger, actions, more)| line(&trigger, &actions, more));
                let expanded = draw_header(
                    ui,
                    header,
                    card,
                    |ui| {
                        // Every card leads with its place in the list, since problems name an
                        // effect by that number. A stock card's name already carries it. That
                        // card draws a clone whose name this frame restamped, so a box over it
                        // read every keystroke as an edit and converted the effect. The name
                        // becomes editable once the effect owns its program, and a folded card
                        // reads as its name.
                        let number = format!("{}.", card.number());
                        if editable && stock.is_none() && !folded {
                            ui.strong(&number);
                            let width = (ui.available_width() - 120.0).max(120.0);
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut program.name)
                                    .id_salt("effect-name")
                                    .desired_width(width)
                                    .hint_text("Effect Name"),
                            );
                            crate::app::style::named_control(response, "Effect Name");
                        } else {
                            // The number stays its own label here too, so the name reads as
                            // itself wherever the card is found by it.
                            if stock.is_none() {
                                ui.strong(&number);
                            }
                            let title = stock.map_or_else(|| program.name.clone(), str::to_owned);
                            ui.add(egui::Label::new(egui::RichText::new(title).strong()).wrap());
                        }
                    },
                    || brief,
                );
                if !expanded {
                    return;
                }
                if program.native.is_none() && !editable {
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
                let structure = card.structure(ui.ctx());
                output.edit_action = draw_program_rows(ui, program, labels, editing, structure);
            }
            Backend::Digest {
                behavior,
                description,
                labels,
                activation,
            } => {
                let expanded = draw_header(
                    ui,
                    header,
                    card,
                    |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(name).strong()).wrap());
                        stock_badge(ui, behavior.support, behavior.editable);
                    },
                    || {
                        // A chosen activation replaces the stock trigger in the reading.
                        Some(match behavior.program.as_ref().and_then(program::brief) {
                            Some((trigger, actions, more)) => {
                                line(activation.unwrap_or(&trigger), &actions, more)
                            }
                            None => behavior.headline.clone(),
                        })
                    },
                );
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

/// The card's header. A folded card also reads what it does, trigger then actions, so a long
/// perk scans without opening each effect.
pub(super) fn draw_header(
    ui: &mut egui::Ui,
    controls: Option<&mut (dyn FnMut(&mut egui::Ui) + '_)>,
    card: cards::Card,
    title: impl FnOnce(&mut egui::Ui),
    summary: impl FnOnce() -> Option<String>,
) -> bool {
    let folded = !card.expanded(ui.ctx());
    // A folded reading that the title line cannot hold whole moves under the title, where it
    // has the card's width. Cut short on the title line it kept the trigger, which every
    // card shares, and lost the actions that tell the cards apart.
    let mut deferred = None;
    let mut title_left = None;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(controls) = controls {
                controls(ui);
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                card.controls(ui);
                title_left = Some(ui.cursor().left());
                title(ui);
                if folded && let Some(summary) = summary() {
                    let galley = egui::WidgetText::from(summary.as_str()).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        f32::INFINITY,
                        egui::TextStyle::Body,
                    );
                    if galley.size().x + 8.0 <= ui.available_width() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            let color = crate::app::style::secondary(ui.visuals());
                            ui.add(egui::Label::new(egui::RichText::new(summary).color(color)));
                        });
                    } else {
                        deferred = Some(summary);
                    }
                }
            });
        });
    });
    if let Some(summary) = deferred {
        ui.horizontal(|ui| {
            if let Some(left) = title_left {
                ui.add_space((left - ui.cursor().left()).max(0.0));
            }
            // The reading wraps rather than cutting off, so a folded card says all of what it
            // does at every width. It is still a few lines against the open card's dozens.
            let color = crate::app::style::secondary(ui.visuals());
            ui.add(egui::Label::new(egui::RichText::new(summary).color(color)).wrap());
        });
    }
    card.expanded(ui.ctx())
}

/// A folded card's reading: its trigger, then its actions in order, then how many more behavior
/// groups it holds. An action repeated in a run reads once with its count. Past three runs the
/// first two lead and the rest are counted, so the line fits beside the effect's name and the
/// behavior count is not the part a narrow card cuts off.
fn line(trigger: &str, actions: &[String], more: usize) -> String {
    let mut runs: Vec<(&str, usize)> = Vec::new();
    for action in actions {
        match runs.last_mut() {
            Some((last, count)) if *last == action.as_str() => *count += 1,
            _ => runs.push((action, 1)),
        }
    }
    let shown = if runs.len() > 3 { 2 } else { runs.len() };
    let mut parts = runs[..shown]
        .iter()
        .map(|(action, count)| {
            if *count == 1 {
                (*action).to_owned()
            } else {
                format!("{action} ×{count}")
            }
        })
        .collect::<Vec<_>>();
    if shown < runs.len() {
        parts.push(format!("{} More", runs.len() - shown));
    }
    let reading = if parts.is_empty() {
        trigger.to_owned()
    } else {
        format!("{trigger}: {}", parts.join(", "))
    };
    match more {
        0 => reading,
        1 => format!("{reading} · 1 More Behavior"),
        more => format!("{reading} · {more} More Behaviors"),
    }
}

fn stock_badge(ui: &mut egui::Ui, support: Support, editable: bool) {
    if !editable {
        support_badge(ui, support);
    }
}

use sundial::ui::catalog::support_badge;

/// One canvas row: a fixed label column and the blocks beside it. The label sits on the
/// first line of its content so a row with one control reads as one line.
pub(super) fn row<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.add_space(3.0);
    // A note under a row keeps the row's content column and no label, so it lines up with the
    // control it is about. A hint that says nothing opens no tooltip.
    let hover = |response: egui::Response| {
        if hint.is_empty() {
            response
        } else {
            response.on_hover_text(hint)
        }
    };
    // Too narrow for a label column, so the label sits above its control. It leads from the
    // left there: right-aligning it put "Trigger" against the far edge with its combo half a
    // card away on the next line, which is a long way to travel to read one field.
    if ui.available_width() < LABEL_WIDTH + 300.0 {
        return ui
            .vertical(|ui| {
                if !label.is_empty() {
                    hover(ui.strong(label));
                }
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
                if !label.is_empty() {
                    hover(
                        ui.add(
                            egui::Label::new(egui::RichText::new(label).strong())
                                .halign(egui::Align::Max)
                                .wrap(),
                        ),
                    );
                }
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
            crate::app::style::more_menu(ui, "Action", |ui| {
                if let Some(properties) = properties {
                    properties.menu_item(ui);
                    ui.separator();
                }
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

/// The rows of a program card. `structure` shows the effect's native structure, which its
/// menu toggles.
fn draw_program_rows(
    ui: &mut egui::Ui,
    program: &mut Program,
    labels: &BTreeMap<u32, String>,
    editing: Option<Editing<'_>>,
    structure: bool,
) -> Option<usize> {
    // Every editable effect edits in one design, whether it was authored here, saved from a
    // guided card or recovered from a package. Keep the stored representation on a
    // read-only frame, and adopt the checked native draft only after an actual edit.
    if program.native.is_none() && editing.is_some() {
        match sundial::package_authoring::sandbox_perk::program::native_draft(program) {
            Ok(mut native) => {
                native.graph.compact();
                let before = native.clone();
                let mut displayed = program.with_native(native);
                let output = draw_program_rows(ui, &mut displayed, labels, editing, structure);
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
            // A saved effect that no longer converts stays readable, with the reason.
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                draw_program_reading(ui, program);
                return None;
            }
        }
    }
    let Some(native) = &mut program.native else {
        draw_program_reading(ui, program);
        return None;
    };
    program::native::tunings::publish(ui.ctx(), &program.ability_tunings);
    let output = match editing {
        Some(Editing { workbench, .. }) => {
            let labels = workbench.program_asset_labels(native, &program.name);
            program::draw_complete(ui, native, true, structure, &labels, &mut |ui, request| {
                match request {
                    program::NativeRequest::Action(context) => {
                        workbench.behaviors.draw_action_selection(
                            ui,
                            &workbench.discovery,
                            &workbench.perk_names,
                            &workbench.asset_labels,
                            context,
                            &workbench.keys.catalog,
                        )
                    }
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
                    program::NativeRequest::Trigger(label, retained) => {
                        workbench.behaviors.draw_trigger(
                            ui,
                            &workbench.discovery,
                            &workbench.perk_names,
                            &workbench.asset_labels,
                            label,
                            retained,
                        )
                    }
                    program::NativeRequest::AssetLength(asset, width) => {
                        workbench.properties.length_tile(ui, width, asset);
                        None
                    }
                    program::NativeRequest::Asset(asset, scope) => {
                        let label = match scope {
                            super::assets::AssetScope::Projectiles => "Projectile",
                            super::assets::AssetScope::Spawnable => "Object or Effect",
                            super::assets::AssetScope::DropEffect => "Drop Effect",
                            super::assets::AssetScope::Any => "Attachment",
                        };
                        // An asset's own components are what an author makes theirs, so
                        // their editor opens from beside the asset rather than from under
                        // Show Properties. A projectile edits its movement in place instead.
                        let editable = scope != super::assets::AssetScope::Projectiles
                            && !matches!(asset.graph, 0 | u32::MAX);
                        let edit_text = format!("Edit {label}…");
                        let mut edit = false;
                        // A tile, as every value under it is. The asset's name runs long, so
                        // the tile takes two widths, and Edit sits on the asset's line.
                        crate::app::style::tiles(ui, |ui, width| {
                            let gap = ui.spacing().item_spacing.x;
                            crate::app::style::tile(
                                ui,
                                (width * 2.0 + gap).min(ui.available_width()),
                                "action-asset",
                                label,
                                "",
                                false,
                                |ui| {
                                    ui.horizontal(|ui| {
                                        // The asset's button shortens its name to what the
                                        // line leaves, so Edit keeps its room at the end.
                                        let reserved = if editable {
                                            egui::WidgetText::from(edit_text.as_str())
                                                .into_galley(
                                                    ui,
                                                    Some(egui::TextWrapMode::Extend),
                                                    f32::INFINITY,
                                                    egui::TextStyle::Button,
                                                )
                                                .size()
                                                .x
                                                + ui.spacing().button_padding.x * 2.0
                                                + ui.spacing().item_spacing.x
                                        } else {
                                            0.0
                                        };
                                        ui.allocate_ui(
                                            egui::vec2(
                                                (ui.available_width() - reserved).max(0.0),
                                                ui.spacing().interact_size.y,
                                            ),
                                            |ui| {
                                                workbench.draw_asset_picker(ui, None, asset, scope);
                                            },
                                        );
                                        if editable {
                                            edit = ui
                                                .scope(|ui| {
                                                    crate::app::style::quiet(ui);
                                                    ui.button(&edit_text).clicked()
                                                })
                                                .inner;
                                        }
                                    });
                                },
                            );
                        });
                        if scope == super::assets::AssetScope::Projectiles {
                            workbench.properties.movement(ui, asset);
                            return None;
                        }
                        workbench.properties.values(ui, asset);
                        workbench.properties.hud_status(ui, asset);
                        edit.then_some(super::behaviors::Selection::Components)
                    }
                }
            })
        }
        None => program::draw_complete(ui, native, false, structure, labels, &mut |_, _| None),
    };
    settle_tunings(ui, program);
    output
}

/// Takes the tunings the key pickers defined this frame into the program, and drops the
/// tunings no action applies any more.
fn settle_tunings(ui: &mut egui::Ui, program: &mut Program) {
    for change in program::native::tunings::withdraw(ui.ctx()) {
        if let Err(error) = program.define_ability_tuning(change.replaced, change.tuning) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    program.prune_ability_tunings();
}

/// The locked reading of a stored guided program.
fn draw_program_reading(ui: &mut egui::Ui, program: &Program) {
    row(ui, "Trigger", ACTIVATION_HINT, |ui| {
        plain(ui, "trigger", |ui| {
            ui.label(program::trigger_text(program));
        });
    });
    row(ui, "Actions", EFFECTS_HINT, |ui| {
        if program.actions.is_empty() {
            ui.weak("No Actions");
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
