//! A behavior card's condition lists, and what a group's triggers and endings say about it.
use super::*;

/// What a condition list is for on the card. The role names the row, chooses its hint and its
/// add command, and says which logic commands the list offers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    /// A behavior group's trigger list.
    Trigger,
    /// A behavior group's ending list.
    EndCondition,
    /// A behavior group's reactivation list.
    Reactivation,
    /// One requirement's alternatives inside All Requirements.
    Requirement,
    /// A counter's contributing conditions, added from their heading.
    Contribution,
    /// The condition a state check nests.
    Nested,
    /// The conditions a timer extension matches.
    Matching,
}

impl Role {
    fn title(self) -> &'static str {
        match self {
            Self::Trigger => "Trigger",
            Self::EndCondition => "End Condition",
            Self::Reactivation => "Reactivation",
            Self::Requirement => "Requirement",
            Self::Contribution => "Contributing Condition",
            Self::Nested => "Condition",
            Self::Matching => "Matching Conditions",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::Trigger => canvas::ACTIVATION_HINT,
            Self::EndCondition => canvas::REMOVAL_HINT,
            Self::Reactivation => canvas::REARM_HINT,
            _ => "",
        }
    }

    /// The command that adds a condition to an empty or non-trigger list.
    fn add_label(self) -> &'static str {
        match self {
            Self::EndCondition => "Add End Condition…",
            Self::Reactivation => "Add Reactivation Condition…",
            _ => "Add Condition…",
        }
    }

    /// Inside a requirement, And belongs to the requirements around it, so only Or is offered.
    fn offers_and(self) -> bool {
        self != Self::Requirement
    }

    /// A contributing list's add button sits beside its heading instead.
    fn adds_from_heading(self) -> bool {
        self == Self::Contribution
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn group_conditions(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    role: Role,
    entries: &[DecodedCondition],
    pick: &mut NativePicker<'_>,
    list: List,
    trigger: Option<bool>,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    canvas::row(ui, role.title(), role.hint(), |ui| {
        condition_list(ui, graph, role, entries, pick, list, trigger, pending)
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn condition_list(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    role: Role,
    entries: &[DecodedCondition],
    pick: &mut NativePicker<'_>,
    list: List,
    // For a group's trigger list, whether its actions hold state. Its pickers then offer the
    // trigger presets, which bring their own endings.
    trigger: Option<bool>,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    // A list passes when any one of its conditions does. And turns the list into one
    // requirement beside the new condition, so (A or B) and C never means starting over.
    let logic = !entries.is_empty() && list.class == action::CONDITION_ROW_CLASS;
    let and = logic && role.offers_and();
    let stacked = entries.len() > 1 && list.class == action::CONDITION_ROW_CLASS;
    let badge = crate::app::style::connector_width(ui, "Or");
    let gutter = badge + ui.spacing().item_spacing.x;
    for (number, condition) in entries.iter().enumerate() {
        let last = number + 1 == entries.len();
        if stacked && number > 0 {
            ui.add_space(4.0);
        }
        let path = list.node(number)?;
        ui.push_id((&list.owner, list.field, number), |ui| {
            // A contribution is one thing: its condition and its Counter Change share a block.
            let frame = if list.class == CONTRIBUTION_ROW_CLASS {
                crate::app::style::block(ui.style())
            } else {
                egui::Frame::new()
            };
            // A condition's less used fields open from its menu rather than a fold under every
            // condition on the card.
            let details_id = ui.make_persistent_id("node-details");
            if problem::contains(ui, &path) {
                ui.data_mut(|data| data.insert_temp(details_id, true));
            }
            let details_open = ui.data(|data| data.get_temp::<bool>(details_id).unwrap_or(false));
            frame
                .show(ui, |ui| {
                    // Contribution blocks share the column's width, as action blocks do, rather
                    // than each shrinking to its own condition.
                    if list.class == CONTRIBUTION_ROW_CLASS {
                        ui.set_min_width(ui.available_width());
                    }
                    structure::scoped(graph, &path, |graph, index| {
                        ui.horizontal_wrapped(|ui| {
                            if list.class == CONTRIBUTION_ROW_CLASS {
                                ui.strong(format!("Contribution {}", number + 1));
                            } else if number > 0 {
                                crate::app::style::connector(ui, "Or");
                            } else if stacked {
                                ui.allocate_exact_size(
                                    egui::vec2(badge, ui.spacing().interact_size.y),
                                    egui::Sense::hover(),
                                );
                            }
                            negation(ui, &mut graph.blocks[index]);
                            let title = uninverted_title(condition);
                            let replacement = match trigger {
                                Some(retained) => match pick_trigger(pick, ui, &title, retained) {
                                    // The first trigger is the group's, so a preset brings its
                                    // ending as choosing a trigger did on a guided card.
                                    Some(Picked::Preset(preset)) if number == 0 => {
                                        *pending = Some((list.clone(), Edit::Preset(preset)));
                                        None
                                    }
                                    Some(Picked::Preset(preset)) => preset_condition(preset)?,
                                    Some(Picked::Node(node)) => Some(node),
                                    None => None,
                                },
                                None => pick_condition(pick, ui, &title),
                            };
                            if let Some(node) = replacement {
                                *pending = Some((list.clone(), Edit::Replace(number, node)));
                            }
                            if trigger.is_some() && number == 0 {
                                in_hand(ui, condition, &list, pending);
                            }
                            // A timer's seconds are what it is, so they sit beside its title and
                            // read as one line: After a Delay, 6 s.
                            timer_seconds_inline(ui, graph, index, condition.kind)?;
                            crate::app::style::more_menu(ui, "Condition", |ui| {
                                let label = if details_open {
                                    "Hide Properties"
                                } else {
                                    "Show Properties"
                                };
                                if ui.button(label).clicked() {
                                    ui.data_mut(|data| data.insert_temp(details_id, !details_open));
                                    ui.close();
                                }
                                if ui.button("Remove Condition").clicked() {
                                    *pending = Some((list.clone(), Edit::Remove(number)));
                                    ui.close();
                                }
                            });
                            // The next condition joins from the end of the last one. Each command
                            // sits in its own scope, and a scope in a wrapping line never wraps,
                            // so after a long title And… squeezed to "…". Both move to the next
                            // line when the rest of this one cannot hold them.
                            if logic && last {
                                // In a wrapping line the available width is the whole line's.
                                if ui.available_size_before_wrap().x < logic_width(ui, and) {
                                    ui.end_row();
                                } else {
                                    ui.add_space(8.0);
                                }
                                logic_commands(ui, pick, &list, and, pending)?;
                            }
                            Ok::<_, String>(())
                        })
                        .inner?;
                        let indent = if stacked { gutter.round() as i8 } else { 0 };
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: indent,
                                ..egui::Margin::ZERO
                            })
                            .show(ui, |ui| {
                                // A timer's seconds sit on its title line.
                                if condition.kind != 1 {
                                    // Only the group's own trigger leads with its chance. An alternative's
                                    // shows once it is not certain, and otherwise waits in its properties.
                                    let first = trigger.is_some() && number == 0;
                                    if graph.blocks[index].class == trigger::CLASS {
                                        let chance = chance_leads(
                                            &graph.blocks[index],
                                            FieldView::Primary,
                                            true,
                                            first,
                                        );
                                        trigger::draw(ui, graph, index, chance)?;
                                    }
                                    controls(ui, graph, index, FieldView::Primary, true, first)?;
                                }
                                unset_note(ui, &graph.blocks[index]);
                                // Conditions and their alternatives remain normal controls at every depth.
                                if list.class == CONTRIBUTION_ROW_CLASS {
                                    // The header says what the row does, so the one number that matters is
                                    // never hidden behind it. The engine word rides along for people who know it.
                                    let title = format!(
                                        "Counter Change · {}",
                                        contribution_reading(graph, &list, number)?
                                    );
                                    egui::CollapsingHeader::new(title)
                                        .id_salt(("contribution", &list.owner, number))
                                        .show(ui, |ui| row_controls(ui, graph, &list, number))
                                        .body_returned
                                        .transpose()?;
                                }
                                match condition.kind {
                                    26 | 35 => {
                                        let children = List {
                                            owner: path.clone(),
                                            field: if condition.kind == 35 { 0x100 } else { 0x10 },
                                            class: if condition.kind == 35 {
                                                0
                                            } else {
                                                CONTRIBUTION_ROW_CLASS
                                            },
                                        };
                                        ui.indent("conditions", |ui| {
                                            if condition.kind == 26 {
                                                // The heading owns its add button, so adding a contribution
                                                // is not the last thing under an indented list.
                                                ui.horizontal(|ui| {
                                                    ui.strong("Contributing Conditions");
                                                    if let Some(node) = ui
                                                        .push_id(
                                                            (
                                                                &children.owner,
                                                                children.field,
                                                                "add",
                                                            ),
                                                            |ui| {
                                                                crate::app::style::quiet(ui);
                                                                pick_condition(
                                                                    pick,
                                                                    ui,
                                                                    "Add Condition…",
                                                                )
                                                            },
                                                        )
                                                        .inner
                                                    {
                                                        *pending = Some((
                                                            children.clone(),
                                                            Edit::Add(node),
                                                        ));
                                                    }
                                                });
                                                if condition.children.is_empty() {
                                                    let response = ui.colored_label(
                                                        ui.visuals().warn_fg_color,
                                                        "No contributing conditions.",
                                                    );
                                                    problem::node_response(ui, &path, &response);
                                                }
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("Required Condition")
                                                        .color(crate::app::style::secondary(
                                                            ui.visuals(),
                                                        )),
                                                );
                                            }
                                            condition_list(
                                                ui,
                                                graph,
                                                if condition.kind == 26 {
                                                    Role::Contribution
                                                } else {
                                                    Role::Nested
                                                },
                                                &condition.children,
                                                pick,
                                                children,
                                                None,
                                                pending,
                                            )
                                        })
                                        .inner?;
                                    }
                                    31 => requirements(ui, graph, &path, condition, pick, pending)?,
                                    _ => {}
                                }
                                if details_open {
                                    details(
                                        ui,
                                        graph,
                                        index,
                                        (list.class == CONTRIBUTION_ROW_CLASS)
                                            .then_some((&list, number)),
                                        trigger.is_some() && number == 0,
                                    )?;
                                }
                                Ok::<_, String>(())
                            })
                            .inner?;
                        Ok::<_, String>(())
                    })
                })
                .inner
        })
        .inner?;
    }
    // A contributing list's add button sits beside its heading instead, and a list with a
    // condition adds the next one from the end of its last line.
    if !logic && !role.adds_from_heading() && (list.class != 0 || entries.is_empty()) {
        // An empty trigger list is an effect that is always active, and its picker is where a
        // trigger is chosen, so it reads as the value it holds.
        if let (Some(retained), true) = (trigger, entries.is_empty()) {
            let picked = ui
                .push_id((&list.owner, list.field, "add-condition"), |ui| {
                    pick_trigger(pick, ui, nodes::condition_title(0), retained)
                })
                .inner;
            match picked {
                Some(Picked::Preset(preset)) => {
                    *pending = Some((list.clone(), Edit::Preset(preset)));
                }
                Some(Picked::Node(node)) => *pending = Some((list.clone(), Edit::Add(node))),
                None => {}
            }
            return Ok(());
        }
        let added = ui
            .push_id((&list.owner, list.field, "add-condition"), |ui| {
                crate::app::style::quiet(ui);
                pick_condition(pick, ui, role.add_label())
            })
            .inner;
        if let Some(node) = added {
            *pending = Some((list.clone(), Edit::Add(node)));
        }
    }
    Ok(())
}

/// A trigger picker's choice: one of the workbench's trigger presets, or a single condition.
enum Picked {
    Preset(Trigger),
    Node(NativeNode),
}

/// The picker for a group's triggers. It offers the trigger presets beside every condition.
fn pick_trigger(
    pick: &mut NativePicker<'_>,
    ui: &mut egui::Ui,
    label: &str,
    retained: bool,
) -> Option<Picked> {
    match pick(ui, NativeRequest::Trigger(label, retained))? {
        super::super::super::super::behaviors::Selection::Trigger(trigger) => {
            Some(Picked::Preset(trigger))
        }
        super::super::super::super::behaviors::Selection::Condition(node) => {
            Some(Picked::Node(node))
        }
        super::super::super::super::behaviors::Selection::Action(_)
        | super::super::super::super::behaviors::Selection::Actions(_)
        | super::super::super::super::behaviors::Selection::Components => None,
    }
}

/// The condition a trigger preset starts with, for a place that holds one condition.
fn preset_condition(trigger: Trigger) -> Result<Option<NativeNode>, String> {
    Ok(structure::preset(trigger)?
        .activation
        .first()
        .map(|node| NativeNode {
            kind: node.kind,
            bytes: node.native.clone(),
        }))
}

/// What a guided card said about a group's shape: an effect that never ends, or one that
/// spawns at an event its trigger never supplies.
pub(super) fn group_hint(group: &action::DecodedGroup) -> Option<&'static str> {
    let kill = group.activation.iter().any(places_event);
    let ends = !group.removal.is_empty();
    let retained = group
        .effects
        .iter()
        .any(|effect| effect.native.get(1).is_some_and(|byte| *byte != 0));
    let always = matches!(
        group.activation.as_slice(),
        [] | [DecodedCondition { kind: 0, .. }]
    );
    // A trigger that can fire again gets the warning drawn above this hint instead.
    if !always && !ends && retained && !validation::endless(group) {
        return Some(
            "A triggered effect with retained actions and no ending stays active until the perk is removed.",
        );
    }
    // A spawn's byte at +4 places it at the triggering event rather than the player.
    let event_spawn = group
        .effects
        .iter()
        .any(|effect| effect.kind == 3 && effect.native.get(4).is_some_and(|byte| *byte != 0));
    (!kill && event_spawn)
        .then_some("Spawning at the triggering event needs a kill or damage trigger.")
}

/// Required choices and empty selections stay visible beside their node. An empty mask
/// warns without blocking the perk, while known optional zeros need no choice.
pub(super) fn unset_note(ui: &mut egui::Ui, block: &native::Block) {
    let Ok(described) = fields::describe(block.class) else {
        return;
    };
    let unset = described.iter().find(|field| {
        block
            .bytes
            .get(field.offset..field.offset + field.width)
            .is_some_and(|bytes| {
                fields::unset(block.class, field, bytes)
                    || fields::empty_selection(block.class, field, bytes)
            })
    });
    if let Some(field) = unset {
        let label = super::super::plain_field_label(block.class, &field.label);
        let empty = field
            .bytes(block, 0)
            .is_some_and(|bytes| fields::empty_selection(block.class, field, bytes));
        ui.colored_label(
            ui.visuals().warn_fg_color,
            if empty {
                format!("The {label} selection is empty.")
            } else {
                format!("Choose the {label}.")
            },
        );
    }
}

/// Whether a trigger reports a place to spawn at: a kill or damage dealt, directly or inside a
/// counter, a "while" check or a requirement. Stock perks spawn at the event after all but the
/// last: String of Curses counts kills, Beacons of Empowerment and Striking Light check a state
/// around a kill, and Poison Arrows and Cellular Suppression spawn where damage lands.
pub(super) fn places_event(condition: &DecodedCondition) -> bool {
    matches!(condition.kind, 2 | 4)
        || condition.children.iter().any(places_event)
        || condition
            .subgroups
            .iter()
            .any(|subgroup| subgroup.conditions.iter().any(places_event))
}

/// Whether the group's only ending is the one its weapon trigger implies, the unequip after
/// an equip or the holster after a draw. It folds as a guided card's default ending did.
pub(super) fn implied_ending(group: &action::DecodedGroup) -> bool {
    matches!(
        (group.activation.as_slice(), group.removal.as_slice()),
        ([trigger], [ending])
            if matches!((trigger.kind, ending.kind), (14, 15) | (16, 17))
    ) && group.rearm.is_empty()
}

/// Whether a group's timing reads as one row: its ending and reactivation are each empty or a
/// single timer, or its ending is at once where an event fires it. The value says whether
/// nothing triggers it, when the one timer is a Repeat Interval.
pub(super) fn timing(group: &action::DecodedGroup) -> Option<bool> {
    let timer_or_empty =
        |list: &[DecodedCondition]| list.is_empty() || (list.len() == 1 && list[0].kind == 1);
    let ending = timer_or_empty(&group.removal)
        || (validation::event_fired(group) && at_once(&group.removal));
    if !ending || !timer_or_empty(&group.rearm) {
        return None;
    }
    // An always-active effect's activation is empty or the Unconditional Check. Its one timer
    // repeats the actions, as the Repeat Interval of a guided Always card did.
    if matches!(
        group.activation.as_slice(),
        [] | [DecodedCondition { kind: 0, .. }]
    ) {
        return group.removal.is_empty().then_some(true);
    }
    Some(false)
}

/// Whether a kill fires this group: a kill condition in its trigger, or one the trigger requires
/// or counts.
pub(super) fn kill_fired(group: &action::DecodedGroup) -> bool {
    fn any(list: &[DecodedCondition]) -> bool {
        list.iter().any(|node| {
            node.kind == 2
                || any(&node.children)
                || node.subgroups.iter().any(|group| any(&group.conditions))
        })
    }
    any(&group.activation)
}

/// Whether an ending is the lone Always that ends an effect as it starts.
pub(super) fn at_once(list: &[DecodedCondition]) -> bool {
    matches!(list, [node] if node.kind == 0 && node.children.is_empty() && node.subgroups.is_empty())
}

const IN_HAND_HINT: &str =
    "Counts only while this weapon is in hand, as Grave Robber's melee kills do.";

/// Under an extra behavior group that starts on an event. Stock perks start extra groups only
/// when always active or from the main behavior's script, and a kill-started one never fired
/// in game, so an event needs a main behavior of its own. The effect list makes the move,
/// since it adds an effect to the perk.
pub(super) fn event_group_warning(ui: &mut egui::Ui, group: usize) {
    let moved = canvas::row(ui, "", "", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Only a main behavior starts on an event in game.")
                        .color(ui.visuals().warn_fg_color),
                )
                .wrap(),
            );
            ui.small_button("Move to Its Own Effect")
                .on_hover_text(
                    "Moves this behavior into a new effect of the perk, as its main behavior.",
                )
                .clicked()
        })
        .inner
    });
    if moved {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new(super::super::MOVE_GROUP), group));
    }
}

/// The In Hand switch on a group's first trigger. A kill that counts whichever weapon is in
/// hand can be held to this one, as Grave Robber holds its melee kill, and a held one let go.
fn in_hand(
    ui: &mut egui::Ui,
    condition: &DecodedCondition,
    list: &List,
    pending: &mut Option<(List, Edit)>,
) {
    let held = structure::held_kill(condition);
    if !held && !structure::loose_kill(condition) {
        return;
    }
    let mut in_hand = held;
    if ui
        .checkbox(&mut in_hand, "In Hand")
        .on_hover_text(IN_HAND_HINT)
        .changed()
    {
        let edit = if in_hand { Edit::Hold } else { Edit::Release };
        *pending = Some((list.clone(), edit));
    }
}
