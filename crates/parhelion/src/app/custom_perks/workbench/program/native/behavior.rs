//! Gameplay controls over the existing allocation graph. Unknown bytes stay in that graph.
use super::super::super::canvas;
use super::*;
use crate::app::custom_perks::workbench::controls::{cell, cell_width, sized};
use crate::app::custom_perks::workbench::validation;
use sundial::package_authoring::sandbox_perk::action::{self, DecodedCondition};

mod ammo;
mod comparison;
mod counter;
mod damage;
pub(super) mod labels;
mod logic;
mod requirements;
mod scripts;
mod trigger;
mod weapon_values;
use super::structure::{self, Edit, List, Part};
use comparison::comparison_editor;
use counter::counter_editor;
pub(super) use logic::negation;
use logic::{logic_commands, logic_width, uninverted_title};
use requirements::requirements;
use weapon_values::{WEAPON_VALUES_CLASS, weapon_values};

pub(super) fn draw_node(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    if graph.blocks[index].class == trigger::CLASS {
        let chance = chance_leads(&graph.blocks[index], FieldView::Primary, false, false);
        trigger::draw(ui, graph, index, chance)?;
    }
    controls(ui, graph, index, FieldView::Primary, false, false)
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    assets: &mut Vec<Asset>,
    pick: &mut NativePicker<'_>,
) -> Result<Option<u32>, String> {
    let decoded = action::decode(&graph.emit()?)?;
    let reveal = ui.ctx().data_mut(|data| {
        data.remove_temp::<Option<sundial::package_authoring::sandbox_perk::program::NativeIssue>>(
            egui::Id::new("native-problem-target"),
        )
    }).flatten();
    let mut edit_asset = None;
    let mut pending = None;
    let mut remove_group = None;
    for (index, group) in decoded.groups.iter().enumerate() {
        // Choices that name the other side of the trigger's event read as a kill's, while a
        // kill fires this group.
        super::kill_context(ui, kill_fired(group));
        let drawn = ui.push_id(("behavior-group", index), |ui| {
            // A rule between groups, so where one behavior ends and the next begins reads at a
            // glance rather than from the heading's text alone.
            if index > 0 {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);
            }
            if decoded.groups.len() > 1 {
                ui.horizontal(|ui| {
                    ui.strong(if index == 0 {
                        "Main Behavior".into()
                    } else {
                        format!("Behavior {}", index + 1)
                    });
                    if index > 0 {
                        crate::app::style::more_menu(ui, "Behavior Group", |ui| {
                            if ui.button("Remove Behavior Group").clicked() {
                                remove_group = Some(index);
                                ui.close_menu();
                            }
                        });
                    }
                });
            }
            // Whether the actions hold state until the effect ends, which is what makes a weapon
            // trigger read as While Drawn rather than On Draw.
            let retained = group.effects.is_empty()
                || group
                    .effects
                    .iter()
                    .any(|effect| effect.native.get(1).is_some_and(|byte| *byte != 0));
            group_conditions(
                ui,
                graph,
                Role::Trigger,
                &group.activation,
                pick,
                List::group(index, Part::Trigger),
                Some(retained),
                &mut pending,
            )?;
            if validation::endless(group) {
                let end = canvas::row(ui, "", "", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("Never ends, so it cannot start again.")
                                    .color(ui.visuals().warn_fg_color),
                            )
                            .wrap(),
                        );
                        ui.small_button("End at Once")
                            .on_hover_text("Ends it as it starts, so every trigger fires it.")
                            .clicked()
                    })
                    .inner
                });
                if end {
                    pending = Some((
                        List::group(index, Part::Ending),
                        Edit::Add(structure::always()?),
                    ));
                }
            }
            if index > 0 && validation::event_fired(group) {
                event_group_warning(ui, index);
            }
            if let Some(hint) = group_hint(group) {
                canvas::row(ui, "", "", |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(hint)
                                .color(crate::app::style::secondary(ui.visuals())),
                        )
                        .wrap(),
                    );
                });
            }
            canvas::row(
                ui,
                "Actions",
                "Actions started by this effect's trigger.",
                |ui| {
                    // Decoding preserves native storage order. Dispatch runs from last to first.
                    for (number, effect) in group.effects.iter().rev().enumerate() {
                        let path = List::group(index, Part::Actions)
                            .node(group.effects.len() - 1 - number)?;
                        let response = canvas::block(ui, ("native-action", number), |ui| {
                            structure::scoped(graph, &path, |graph, block_index| {
                                ui.set_min_width(ui.available_width());
                                ui.push_id(&path, |ui| {
                                    let mut properties =
                                        super::super::super::properties::Panel::new(ui, "action");
                                    let title = super::super::native_action_label(
                                        effect.kind,
                                        &graph.blocks[block_index].bytes,
                                    );
                                    let hint = super::super::native_action_reading(
                                        effect.kind,
                                        &effect.description(),
                                    );
                                    let event = canvas::action_header(
                                        ui,
                                        &title,
                                        &hint,
                                        number,
                                        group.effects.len(),
                                        Some(&mut properties),
                                    );
                                    if let Some(target) = event.swap_with {
                                        pending = Some((
                                            List::group(index, Part::Actions),
                                            Edit::Move(number, target),
                                        ));
                                    }
                                    if event.remove {
                                        pending = Some((
                                            List::group(index, Part::Actions),
                                            Edit::Remove(number),
                                        ));
                                    }
                                    let class = graph.blocks[block_index].class;
                                    if matches!(
                                        class,
                                        0x80803E43
                                            | 0x80803E44
                                            | 0x80803E45
                                            | 0x80803E47
                                            | 0x80803E12
                                    ) {
                                        if let Some(tag) = asset_action(
                                            ui,
                                            graph,
                                            block_index,
                                            effect,
                                            assets,
                                            pick,
                                        )? {
                                            edit_asset = Some(tag);
                                        }
                                    }
                                    // What a damage modifier does to damage leads, ahead of the filters that
                                    // choose which damage it applies to.
                                    damage::draw(ui, graph, block_index)?;
                                    ammo::draw(ui, graph, block_index);
                                    controls(
                                        ui,
                                        graph,
                                        block_index,
                                        FieldView::Primary,
                                        true,
                                        false,
                                    )?;
                                    unset_note(ui, &graph.blocks[block_index]);
                                    let mut details = Ok(());
                                    properties.show(ui, |ui| {
                                        details = controls(
                                            ui,
                                            graph,
                                            block_index,
                                            FieldView::Details,
                                            true,
                                            false,
                                        )
                                        .and_then(|()| nested(ui, graph, block_index));
                                        if let Some(tag) = effect.referenced_tag {
                                            let tag = if matches!(
                                                class,
                                                0x80803E43
                                                    | 0x80803E44
                                                    | 0x80803E45
                                                    | 0x80803E47
                                                    | 0x80803E12
                                            ) {
                                                u32::from_le_bytes(
                                                    graph.blocks[block_index].bytes[16..20]
                                                        .try_into()
                                                        .unwrap(),
                                                )
                                            } else {
                                                tag
                                            };
                                            ui.separator();
                                            ui.strong("Referenced Object");
                                            if super::super::super::properties::edit_object(
                                                ui,
                                                &Asset {
                                                    graph: tag,
                                                    ..Asset::default()
                                                },
                                            ) {
                                                edit_asset = Some(tag);
                                            }
                                        }
                                    });
                                    details?;
                                    if effect.kind == 32 {
                                        // A timer extension carries a copy of the trigger that
                                        // started the timers: the compiler re-emits it as the
                                        // nested list, which is the shape Outlaw and the other
                                        // stock kill perks use. Drawn open it restated the whole
                                        // trigger, filter column and all, a few rows under the
                                        // trigger itself. It stays reachable, since a hand
                                        // authored program may have changed it.
                                        let repeats = effect.conditions.iter().all(|condition| {
                                            group
                                                .activation
                                                .iter()
                                                .any(|trigger| trigger.native == condition.native)
                                        });
                                        if repeats {
                                            egui::CollapsingHeader::new(
                                                "Repeats the Effect's Trigger",
                                            )
                                            .id_salt("extension-trigger")
                                            .show(ui, |ui| {
                                                condition_list(
                                                    ui,
                                                    graph,
                                                    Role::Matching,
                                                    &effect.conditions,
                                                    pick,
                                                    List {
                                                        owner: path.clone(),
                                                        field: 0x18,
                                                        class: action::CONDITION_ROW_CLASS,
                                                    },
                                                    None,
                                                    &mut pending,
                                                )
                                            })
                                            .body_returned
                                            .transpose()?;
                                        } else {
                                            canvas::row(
                                                ui,
                                                "Trigger",
                                                "Checks for this action only.",
                                                |ui| {
                                                    condition_list(
                                                        ui,
                                                        graph,
                                                        Role::Matching,
                                                        &effect.conditions,
                                                        pick,
                                                        List {
                                                            owner: path.clone(),
                                                            field: 0x18,
                                                            class: action::CONDITION_ROW_CLASS,
                                                        },
                                                        None,
                                                        &mut pending,
                                                    )
                                                },
                                            )?;
                                        }
                                    }
                                    Ok::<_, String>(())
                                })
                                .inner
                            })
                        });
                        response.inner?;
                        if reveal
                            .as_ref()
                            .is_some_and(|target| target.group == index && target.action == number)
                        {
                            response.response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    // A kill inside a counter or a requirement places the event as a top-level kill does,
                    // so the actions that need one are offered there too.
                    let context = Program {
                        trigger: if group.activation.iter().any(places_event) {
                            Trigger::WeaponKill
                        } else {
                            Trigger::Always
                        },
                        actions: group
                            .effects
                            .iter()
                            .map(|effect| Action::Native {
                                node: NativeNode {
                                    kind: effect.kind,
                                    bytes: effect.native.clone(),
                                },
                            })
                            .collect(),
                        ..Program::default()
                    };
                    match pick(ui, NativeRequest::Action(&context)) {
                        Some(super::super::super::behaviors::Selection::Action(action)) => {
                            pending = Some((
                                List::group(index, Part::Actions),
                                Edit::Add(structure::action_node(action, group)?),
                            ));
                        }
                        // A stock behavior's actions are added together or not at all.
                        Some(super::super::super::behaviors::Selection::Actions(nodes)) => {
                            pending =
                                Some((List::group(index, Part::Actions), Edit::AddAll(nodes)));
                        }
                        _ => {}
                    }
                    Ok::<_, String>(())
                },
            )?;
            // A timer ending is the effect's Duration and a timer reactivation its Cooldown, or
            // its Repeat Interval when nothing triggers it. They lead as one Timing row, and the
            // lists that hold them wait under More Conditions until anything else is set.
            let timing = timing(group);
            let unset_repeat = timing == Some(true) && group.rearm.is_empty();
            if let Some(always) = timing
                && !unset_repeat
            {
                timing_row(ui, graph, index, group, always, &mut pending)?;
            }
            // With a Timing row, its timers are the lists' only entries, so the lists start
            // empty here and a condition added to one joins the timer with Or.
            let shown = |list: &[DecodedCondition]| -> usize {
                if timing.is_some() { list.len() } else { 0 }
            };
            let removal = &group.removal[shown(&group.removal)..];
            let rearm = &group.rearm[shown(&group.rearm)..];
            // The unset Repeat Interval, the ending list and the reactivation list, each drawn
            // where it is asked for.
            let mut lists = |ui: &mut egui::Ui, repeat: bool, ending: bool, reactivation: bool| {
                if repeat {
                    timing_row(ui, graph, index, group, true, &mut pending)?;
                }
                if ending {
                    group_conditions(
                        ui,
                        graph,
                        Role::EndCondition,
                        removal,
                        pick,
                        List::group(index, Part::Ending),
                        None,
                        &mut pending,
                    )?;
                }
                if reactivation {
                    group_conditions(
                        ui,
                        graph,
                        Role::Reactivation,
                        rearm,
                        pick,
                        List::group(index, Part::Rearm),
                        None,
                        &mut pending,
                    )?;
                }
                Ok::<_, String>(())
            };
            if timing.is_none()
                && !implied_ending(group)
                && (!group.removal.is_empty() || !group.rearm.is_empty())
            {
                lists(ui, unset_repeat, true, true)?;
            } else {
                // A condition someone set shows on the card. Only what is still unset, which
                // offers to add one, waits under More Conditions, and with nothing unset the
                // fold goes.
                lists(ui, false, !removal.is_empty(), !rearm.is_empty())?;
                if unset_repeat || removal.is_empty() || rearm.is_empty() {
                    egui::CollapsingHeader::new(egui::RichText::new("More Conditions").small())
                        .id_salt(("more-conditions", index))
                        .show_unindented(ui, |ui| {
                            lists(ui, unset_repeat, removal.is_empty(), rearm.is_empty())
                        })
                        .body_returned
                        .transpose()?;
                }
            }
            Ok::<_, String>(())
        });
        super::kill_context(ui, false);
        drawn.inner?;
    }
    if let Some((list, edit)) = pending {
        list.edit(graph, edit)?;
    }
    if let Some(group) = remove_group {
        structure::remove_group(graph, group)?;
    }
    Ok(edit_asset)
}

/// The object or effect an attach, spawn, drop or projectile action references: its picker,
/// the reminder while none is chosen, and the stored path and asset list kept in step with the
/// choice. Returns the object whose components the picker asked to edit.
fn asset_action(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    block_index: usize,
    effect: &action::DecodedEffect,
    assets: &mut Vec<Asset>,
    pick: &mut NativePicker<'_>,
) -> Result<Option<u32>, String> {
    let class = graph.blocks[block_index].class;
    let tag = graph.blocks[block_index]
        .bytes
        .get(16..20)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .ok_or("Missing asset reference.")?;
    let mut asset = assets
        .iter()
        .find(|asset| asset.graph == tag)
        .cloned()
        .unwrap_or(Asset {
            graph: tag,
            path: effect.referenced_path.clone().unwrap_or_default(),
            ..Asset::default()
        });
    let scope = match class {
        0x80803E12 => AssetScope::Projectiles,
        0x80803E43 => AssetScope::Spawnable,
        ammo::DROP => AssetScope::DropEffect,
        _ => AssetScope::Any,
    };
    let edit = matches!(
        pick(ui, NativeRequest::Asset(&mut asset, scope)),
        Some(super::super::super::behaviors::Selection::Components)
    )
    .then_some(asset.graph);
    if matches!(asset.graph, 0 | u32::MAX) && class != 0x80803E47 {
        ui.colored_label(
            ui.visuals().error_fg_color,
            if class == 0x80803E12 {
                "Projectile: choose a projectile."
            } else {
                "Object: choose an object or effect."
            },
        );
    }
    if asset.graph != tag {
        graph.blocks[block_index].bytes[16..20].copy_from_slice(&asset.graph.to_le_bytes());
        graph.create_target(block_index, 8, 0, false)?;
        let path = graph.blocks[block_index].links[&8];
        graph.blocks[path].bytes = asset.path.as_bytes().to_vec();
        graph.blocks[path].bytes.push(0);
    }
    if !matches!(asset.graph, 0 | u32::MAX) {
        match assets
            .iter_mut()
            .find(|existing| existing.graph == asset.graph)
        {
            Some(existing) => *existing = asset,
            None => assets.push(asset),
        }
    }
    Ok(edit)
}

/// The contribution rows of a counter, each a condition with its Counter Change.
pub(super) const CONTRIBUTION_ROW_CLASS: u32 = 0x8080_3E32;

/// What a condition list is for on the card. The role names the row, chooses its hint and its
/// add command, and says which logic commands the list offers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
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
fn group_conditions(
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
fn condition_list(
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
                                    ui.close_menu();
                                }
                                if ui.button("Remove Condition").clicked() {
                                    *pending = Some((list.clone(), Edit::Remove(number)));
                                    ui.close_menu();
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
                                                    ui.colored_label(
                                                        ui.visuals().warn_fg_color,
                                                        "No contributing conditions.",
                                                    );
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
        super::super::super::behaviors::Selection::Trigger(trigger) => {
            Some(Picked::Preset(trigger))
        }
        super::super::super::behaviors::Selection::Condition(node) => Some(Picked::Node(node)),
        super::super::super::behaviors::Selection::Action(_)
        | super::super::super::behaviors::Selection::Actions(_)
        | super::super::super::behaviors::Selection::Components => None,
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
fn group_hint(group: &action::DecodedGroup) -> Option<&'static str> {
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

/// The field a node still needs, beside the node, as an empty counter's note is. A zero key,
/// tag or selection compiles, but the node never matches or never acts.
fn unset_note(ui: &mut egui::Ui, block: &native::Block) {
    let Ok(described) = fields::describe(block.class) else {
        return;
    };
    let unset = described.iter().find(|field| {
        block
            .bytes
            .get(field.offset..field.offset + field.width)
            .is_some_and(|bytes| fields::unset(block.class, field, bytes))
    });
    if let Some(field) = unset {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "Choose the {}.",
                super::plain_field_label(block.class, &field.label)
            ),
        );
    }
}

/// Whether a trigger reports a place to spawn at: a kill or damage dealt, directly or inside a
/// counter, a "while" check or a requirement. Stock perks spawn at the event after all but the
/// last: String of Curses counts kills, Beacons of Empowerment and Striking Light check a state
/// around a kill, and Poison Arrows and Cellular Suppression spawn where damage lands.
fn places_event(condition: &DecodedCondition) -> bool {
    matches!(condition.kind, 2 | 4)
        || condition.children.iter().any(places_event)
        || condition
            .subgroups
            .iter()
            .any(|subgroup| subgroup.conditions.iter().any(places_event))
}

/// Whether the group's only ending is the one its weapon trigger implies, the unequip after
/// an equip or the holster after a draw. It folds as a guided card's default ending did.
fn implied_ending(group: &action::DecodedGroup) -> bool {
    matches!(
        (group.activation.as_slice(), group.removal.as_slice()),
        ([trigger], [ending])
            if matches!((trigger.kind, ending.kind), (14, 15) | (16, 17))
    ) && group.rearm.is_empty()
}

/// Whether a group's timing reads as one row: its ending and reactivation are each empty or a
/// single timer, or its ending is at once where an event fires it. The value says whether
/// nothing triggers it, when the one timer is a Repeat Interval.
fn timing(group: &action::DecodedGroup) -> Option<bool> {
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
fn kill_fired(group: &action::DecodedGroup) -> bool {
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
fn at_once(list: &[DecodedCondition]) -> bool {
    matches!(list, [node] if node.kind == 0 && node.children.is_empty() && node.subgroups.is_empty())
}

const IN_HAND_HINT: &str =
    "Counts only while this weapon is in hand, as Grave Robber's melee kills do.";

/// Under an extra behavior group that starts on an event. Stock perks start extra groups only
/// when always active or from the main behavior's script, and a kill-started one never fired
/// in game, so an event needs a main behavior of its own. The effect list makes the move,
/// since it adds an effect to the perk.
fn event_group_warning(ui: &mut egui::Ui, group: usize) {
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
            .data_mut(|data| data.insert_temp(egui::Id::new(super::MOVE_GROUP), group));
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
const DURATION_HINT: &str =
    "How long this effect stays active. Zero keeps it until the perk is removed.";
const EVENT_DURATION_HINT: &str =
    "How long this effect stays active. At Once lets every trigger fire it again.";
const COOLDOWN_HINT: &str = "The delay before this effect can trigger again.";
const REPEAT_HINT: &str = "The interval at which an always-active effect runs its actions again.";

/// Duration and Cooldown, or the Repeat Interval of an effect nothing triggers, as tiles.
fn timing_row(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    group_index: usize,
    group: &action::DecodedGroup,
    always: bool,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    // An effect an event fires ends at once at zero, as stock kill perks whose actions happen
    // once do. With no ending it would run once and never again.
    let event = validation::event_fired(group);
    canvas::row(ui, "Timing", "", |ui| {
        crate::app::style::tiles(ui, |ui, width| {
            if !always {
                timer_tile(
                    ui,
                    graph,
                    width,
                    List::group(group_index, Part::Ending),
                    &group.removal,
                    (
                        "Duration",
                        if event {
                            EVENT_DURATION_HINT
                        } else {
                            DURATION_HINT
                        },
                    ),
                    Timer::Duration { event },
                    pending,
                )?;
            }
            let (name, hint) = if always {
                ("Repeat Interval", REPEAT_HINT)
            } else {
                ("Cooldown", COOLDOWN_HINT)
            };
            timer_tile(
                ui,
                graph,
                width,
                List::group(group_index, Part::Rearm),
                &group.rearm,
                (name, hint),
                Timer::Rearm,
                pending,
            )
        })
    })
}

/// Which timer a tile sets.
#[derive(Clone, Copy)]
enum Timer {
    /// The ending. Where an event fires the effect, zero ends it at once.
    Duration {
        event: bool,
    },
    Rearm,
}

/// One timer as a tile. Zero removes the timer, or ends at once where an event fires the effect,
/// and a value on an empty list, or on an ending at once, sets one.
#[allow(clippy::too_many_arguments)]
fn timer_tile(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    width: f32,
    list: List,
    entries: &[DecodedCondition],
    (name, hint): (&str, &str),
    timer: Timer,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    if entries.is_empty() || at_once(entries) {
        if let Some(seconds) = unset_timer(ui, width, (name, hint), at_once(entries)) {
            let node = timer_node(seconds, matches!(timer, Timer::Rearm))?;
            *pending = Some((
                list,
                if entries.is_empty() {
                    Edit::Add(node)
                } else {
                    Edit::Replace(0, node)
                },
            ));
        }
        return Ok(());
    }
    let path = list.node(0)?;
    let cleared = structure::scoped(graph, &path, |graph, index| {
        let class = graph.blocks[index].class;
        let field = fields::describe(class)?
            .into_iter()
            .find(|field| field.label == "Duration")
            .ok_or("The timer duration field is missing.")?;
        let control = super::tile_width(class, &field, width).unwrap_or(width);
        let before = graph.blocks[index].bytes.clone();
        crate::app::style::tile(ui, width, name, name, hint, false, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().interact_size.x = control;
                super::scalar(ui, &field, &mut graph.blocks[index], 0)
            })
            .inner
        })
        .0?;
        // Only setting zero removes the timer. A zero timer that is only drawn stays, since
        // a stock effect or a newly added condition may hold one.
        Ok(graph.blocks[index].bytes != before
            && super::timer_seconds(&graph.blocks[index]) == Some(0.0))
    })?;
    if cleared {
        *pending = Some((
            list,
            match timer {
                Timer::Duration { event: true } => Edit::Replace(0, structure::always()?),
                _ => Edit::Remove(0),
            },
        ));
    }
    Ok(())
}

/// A timer tile with no timer yet, reading zero, or At Once for an ending at once. Returns the
/// seconds a change sets.
fn unset_timer(
    ui: &mut egui::Ui,
    width: f32,
    (name, hint): (&str, &str),
    ends_at_once: bool,
) -> Option<f32> {
    let mut seconds = 0.0_f32;
    // The same widgets, in the same order and at the same width, as the tile of a set timer,
    // so the drag that adds the timer carries on into the timer it added instead of stopping
    // at its first tick when the tile is redrawn around the new condition.
    let control = nodes::condition(1)
        .and_then(|kind| {
            let field = fields::describe(kind.class)
                .ok()?
                .into_iter()
                .find(|field| field.label == "Duration")?;
            super::tile_width(kind.class, &field, width)
        })
        .unwrap_or(width);
    let (changed, _) = crate::app::style::tile(ui, width, name, name, hint, false, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size.x = control;
            let response = ui.add(
                egui::DragValue::new(&mut seconds)
                    .range(0.0..=3600.0)
                    .clamp_existing_to_range(false)
                    .custom_formatter(|value, _| {
                        if ends_at_once && value == 0.0 {
                            "At Once".to_owned()
                        } else {
                            seconds_text(value)
                        }
                    })
                    .custom_parser(|text| {
                        let text = text.trim();
                        if text.eq_ignore_ascii_case("at once") {
                            Some(0.0)
                        } else {
                            text.parse().ok()
                        }
                    })
                    .suffix(if ends_at_once { "" } else { " s" }),
            );
            pickers::name_response(ui, &response, name);
            response.changed()
        })
        .inner
    });
    (changed && seconds > 0.0).then_some(seconds)
}

/// A timer condition's seconds, sized for the value, on the line beside its title. Any other
/// condition kind draws nothing here.
fn timer_seconds_inline(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    kind: u8,
) -> Result<(), String> {
    if kind != 1 {
        return Ok(());
    }
    let class = graph.blocks[index].class;
    let field = fields::describe(class)?
        .into_iter()
        .find(|field| field.label == "Duration")
        .ok_or("The timer duration field is missing.")?;
    // An allocation wraps with the condition's line where a scope would not.
    sized(ui, 64.0, |ui| {
        ui.spacing_mut().interact_size.x = 64.0;
        super::scalar(ui, &field, &mut graph.blocks[index], 0)
    })
}

/// Seconds with no trailing zeros, as a set timer reads.
fn seconds_text(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A timer condition of the given length, as the compiler writes one for a guided Duration or
/// Cooldown.
fn timer_node(seconds: f32, rearm: bool) -> Result<NativeNode, String> {
    let millis = ((seconds * 1000.0).round() as u32).max(1);
    let draft = Program {
        trigger: Trigger::WeaponKill,
        duration_ms: millis,
        cooldown_ms: if rearm { millis } else { 0 },
        ..Program::default()
    };
    let payload = sundial::package_authoring::sandbox_perk::program::native_draft(&draft)?
        .graph
        .emit()?;
    let decoded = action::decode(&payload)?;
    let group = decoded
        .groups
        .first()
        .ok_or("The timer could not be built.")?;
    let node = if rearm {
        group.rearm.first()
    } else {
        group.removal.first()
    }
    .ok_or("The timer could not be built.")?;
    Ok(NativeNode {
        kind: node.kind,
        bytes: node.native.clone(),
    })
}

fn row_controls(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    list: &List,
    row: usize,
) -> Result<(), String> {
    let mut path = list.owner.clone();
    path.push(list.field);
    row_fields(ui, graph, list, row, true)
}

/// A contribution's fields: what happens when the condition passes is the row, and its
/// hold and failure side wait in the condition's Advanced until one of them holds something.
fn row_fields(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    list: &List,
    row: usize,
    leading: bool,
) -> Result<(), String> {
    let mut path = list.owner.clone();
    path.push(list.field);
    structure::scoped(graph, &path, |graph, index| {
        let fields = fields::describe(list.class)?;
        // A requirement's hold is its only setting, so it shows even at zero.
        let leads = |field: &fields::Field| {
            list.class == action::SUBGROUP_ROW_CLASS
                || matches!(field.offset, 8 | 9 | 12)
                || field
                    .bytes(&graph.blocks[index], row)
                    .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0))
        };
        let chosen = fields
            .iter()
            .filter(|field| visible(field, list.class) && leads(field) == leading)
            .collect::<Vec<_>>();
        if chosen.is_empty() {
            return Ok(());
        }
        crate::app::style::tiles(ui, |ui, width| {
            for field in chosen {
                let control = super::tile_width(list.class, field, width).unwrap_or(width);
                crate::app::style::tile(
                    ui,
                    width,
                    ("contribution", row, field.offset),
                    super::plain_field_label(list.class, &field.label),
                    fields::contract(list.class, field).description,
                    false,
                    |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().interact_size.x = control;
                            ui.spacing_mut().combo_width = control;
                            super::scalar(ui, field, &mut graph.blocks[index], row)
                        })
                        .inner
                    },
                )
                .0?;
            }
            Ok(())
        })
    })
}

/// What a contributing condition does to the counter when it passes, as the row's header.
fn contribution_reading(graph: &mut Graph, list: &List, row: usize) -> Result<String, String> {
    let mut path = list.owner.clone();
    path.push(list.field);
    structure::scoped(graph, &path, |graph, index| {
        let block = &graph.blocks[index];
        let fields = fields::describe(list.class)?;
        let at = |offset: usize| {
            fields
                .iter()
                .find(|field| field.offset == offset)
                .and_then(|field| field.bytes(block, row))
        };
        let byte = |offset: usize| {
            at(offset)
                .and_then(|bytes| bytes.first().copied())
                .unwrap_or(0)
        };
        let amount = if byte(9) != 0 {
            "the event's value".to_owned()
        } else {
            let value = at(12)
                .and_then(|bytes| bytes.try_into().ok())
                .map_or(0.0, f32::from_le_bytes);
            if value.fract() == 0.0 && value.abs() < 1.0e9 {
                format!("{}", value as i64)
            } else {
                format!("{value}")
            }
        };
        let change = match byte(8) {
            0 => format!("adds {amount}"),
            1 => format!("sets it to {amount}"),
            2 => format!("multiplies it by {amount}"),
            other => format!("operation {other}"),
        };
        // How long a pass keeps counting is half of what the row means, as Harbinger's Pulse
        // counts each kill for 2 seconds, so it reads beside the change rather than folded.
        let hold = at(0x10)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0.0, f32::from_le_bytes);
        Ok(if hold > 0.0 {
            format!("{change} for {} s", seconds_text(f64::from(hold)))
        } else {
            change
        })
    })
}

/// A condition's less used fields, shown once its menu asks for them.
fn details(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    contribution: Option<(&List, usize)>,
    trigger: bool,
) -> Result<(), String> {
    ui.indent("node-details", |ui| {
        controls(ui, graph, index, FieldView::Details, true, trigger)?;
        // A contribution's failure side and hold live with the condition they belong to, so
        // one set of properties covers the whole row.
        if let Some((list, row)) = contribution {
            row_fields(ui, graph, list, row, false)?;
        }
        nested(ui, graph, index)
    })
    .inner
}

/// Follow owned value/filter allocations, stopping at independently edited nodes.
/// All edits still use the same byte-exact scalar writer as Native Structure.
fn nested(ui: &mut egui::Ui, graph: &mut Graph, parent: usize) -> Result<(), String> {
    let path = structure::path_to(graph, parent)?;
    nested_at(ui, graph, &path, 0)
}

fn nested_at(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    path: &[usize],
    depth: usize,
) -> Result<(), String> {
    if depth >= 64 {
        return Err("The native structure nests too deeply.".into());
    }
    let parent = structure::resolve(graph, path)?;
    let links = graph.blocks[parent].links.clone();
    // A value program's constants and instructions belong to its tile or expression editor
    // (`values`). Folding them again showed Reload from Reserves' five shares a second time,
    // as five folds all named Constant Vector.
    let program_size = schema::record(native::value::CLASS)?.size;
    let stride = schema::record(graph.blocks[parent].class)?.size.max(1);
    let programs = schema::inline(graph.blocks[parent].class)?
        .into_iter()
        .filter(|(_, child, _)| *child == native::value::CLASS)
        .map(|(offset, _, _)| offset..offset + program_size)
        .collect::<Vec<_>>();
    for (at, child) in links {
        let class = graph.blocks[child].class;
        if matches!(class, 0 | CONTRIBUTION_ROW_CLASS | 0x80803E06)
            || nodes::CONDITIONS
                .iter()
                .chain(&nodes::EFFECTS)
                .any(|n| n.class == class)
            || programs
                .iter()
                .any(|program| program.contains(&(at % stride)))
        {
            continue;
        }
        let mut child_path = path.to_vec();
        child_path.push(at);
        let fields: Vec<_> = fields::describe(class)?
            .into_iter()
            .filter(|field| visible(field, class))
            .collect();
        let has_labels = !native::labels::bindings(class)?.is_empty();
        if !fields.is_empty() || has_labels {
            let context = super::reference_name(graph, parent, at % stride, Some(child))?;
            egui::CollapsingHeader::new(context)
                .id_salt(("nested-fields", &child_path))
                .show(ui, |ui| {
                    structure::scoped(graph, &child_path, |graph, child| {
                        let count = graph.blocks[child].count.unwrap_or(1);
                        for row in 0..count {
                            if count > 1 {
                                ui.strong(format!("Entry {}", row + 1));
                            }
                            for field in &fields {
                                ui.push_id((row, field.offset), |ui| {
                                    super::super::super::properties::field(
                                        ui,
                                        super::plain_field_label(class, &field.label),
                                        fields::contract(class, field).description,
                                        |ui| {
                                            super::scalar(ui, field, &mut graph.blocks[child], row)
                                        },
                                    )
                                })
                                .inner?;
                            }
                            ui.push_id((row, "labels"), |ui| {
                                labels::draw_row_sites(ui, graph, child, row)
                            })
                            .inner?;
                        }
                        Ok(())
                    })
                })
                .body_returned
                .transpose()?;
        }
        nested_at(ui, graph, &child_path, depth + 1)?;
    }
    Ok(())
}
/// All recovered scalar formats share the native field writer. Storage and policy
/// fields remain in the complete record, instead of masquerading as gameplay settings.
#[derive(Clone, Copy)]
enum FieldView {
    Primary,
    Details,
}

fn primary(field: &fields::Field, ability: bool) -> bool {
    if ability {
        // Named state, version and input choices are handled by primary_for. Keep the
        // ability target visible even when its current value has no recovered name.
        return field.offset == 2;
    }
    matches!(
        field.label.as_str(),
        "Duration"
            | "Hold Duration"
            | "Extend By"
            | "Up To"
            | "Count"
            | "Spawn Position"
            | "Scale"
            | "Limit"
            | "Damage Multiplier"
            | "Maximum Source Distance"
            | "Upper Cap"
            | "Value Threshold"
            | "Trigger Threshold"
            | "Reset Threshold"
            | "Minimum Value"
            | "Maximum Value"
    ) || field.label.ends_with("Amount")
}

/// A setting at the value stock nodes give it, which leads only once changed. Every counter
/// setter sets the counter to its Counter Value, so its source said only that. Most stock energy
/// grants move the current ability's energy in any state by a fixed amount: 50 of 67 read any
/// state, 61 of 65 the current ability and 42 of 67 no value source.
fn stock_setting(field: &fields::Field, block: &native::Block) -> bool {
    let value = block.bytes.get(field.offset).copied();
    match (block.class, field.offset) {
        (0x80803E2F, 2) => value == Some(1),
        (0x80803E4D, 3 | 4) => value == Some(0),
        (0x80803E4D, 0x48) => value == Some(255),
        _ => false,
    }
}

/// Fields the compiler rewrites from the label lists drawn above them: a label predicate's
/// mode, and the words of an added-label node's compiled label set. They read as a number and
/// as hex beside the labels they restate, so they wait under Show Properties. A predicate
/// inlined in its node, as the Event Label Filter's at +58, holds the same mode.
pub(super) fn compiler_owned(class: u32, offset: usize) -> bool {
    let compiled_set = match class {
        native::labels::PREDICATE_CLASS => return true,
        0x8080281C => (0x78..0xA0).contains(&offset),
        0x80803E1A => (0xA8..0xD0).contains(&offset),
        _ => false,
    };
    compiled_set
        || schema::inline(class).is_ok_and(|inline| {
            inline
                .iter()
                .any(|(at, child, _)| *child == native::labels::PREDICATE_CLASS && *at == offset)
        })
}

/// A negative limit is the no-limit setting stock energy grants and fresh actions keep, so the
/// limit leads only once one is set.
fn energy_limit_set(block: &native::Block) -> bool {
    block
        .bytes
        .get(0x0C..0x10)
        .is_some_and(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) >= 0.0)
}

/// Where an ammo adjustment keeps its seven target amounts, This Weapon first.
const AMMO_TARGETS: std::ops::RangeInclusive<usize> = 0x6C..=0x84;

/// The ammo targets added from Add Target, which stay on the card while they still hold zero.
fn revealed_targets(ui: &egui::Ui, class: u32) -> Vec<usize> {
    if !matches!(class, 0x80803E3F | 0x80803E3E) {
        return Vec::new();
    }
    ui.data(|data| data.get_temp(ui.make_persistent_id("ammo-targets")))
        .unwrap_or_default()
}

/// The ammo adjustment's quiet menu of the targets its card leaves out.
fn add_target_menu(
    ui: &mut egui::Ui,
    fields: &[fields::Field],
    block: &native::Block,
    revealed: &[usize],
) {
    if !matches!(block.class, 0x80803E3F | 0x80803E3E) {
        return;
    }
    let hidden = fields
        .iter()
        .filter(|field| {
            AMMO_TARGETS.contains(&field.offset)
                && !ammo_target_leads(block, field.offset)
                && !revealed.contains(&field.offset)
        })
        .collect::<Vec<_>>();
    if hidden.is_empty() {
        return;
    }
    let id = ui.make_persistent_id("ammo-targets");
    ui.scope(|ui| {
        crate::app::style::quiet(ui);
        ui.menu_button("Add Target…", |ui| {
            for field in hidden {
                if ui
                    .button(super::plain_field_label(block.class, &field.label))
                    .clicked()
                {
                    let mut shown = revealed.to_vec();
                    shown.push(field.offset);
                    ui.data_mut(|data| data.insert_temp(id, shown));
                    ui.close_menu();
                }
            }
        });
    });
}

/// An ammo adjustment keeps an amount for each target, and most perks fill one: Triple Tap
/// returns a round to This Weapon and leaves the other six at zero. A target leads once it
/// holds an amount, and This Weapon leads while none does. Add Target reveals the rest.
fn ammo_target_leads(block: &native::Block, offset: usize) -> bool {
    let held = |at: usize| {
        block
            .bytes
            .get(at..at + 4)
            .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0))
    };
    AMMO_TARGETS.contains(&offset)
        && (held(offset) || offset == 0x6C && !AMMO_TARGETS.step_by(4).any(held))
}

/// Whether one of the general predicate's ranges leads. `None` for any other field, which the
/// general rules decide.
fn predicate_range_leads(field: &fields::Field, block: &native::Block) -> Option<bool> {
    if !matches!(block.class, 0x80803DCE | 0x80803DCC) {
        return None;
    }
    // Lead with the meaningful restrictions a stock/custom predicate actually uses.
    // Unrestricted ranges stay editable in Advanced without adding fourteen controls
    // to every ordinary state check. Keep both bounds together when either is changed.
    let range = match field.offset {
        0x18..=0x34 => Some((field.offset & !7, 0x30)),
        0x9C..=0xB0 => Some((0x9C + ((field.offset - 0x9C) / 8) * 8, usize::MAX)),
        _ => None,
    };
    if let Some((offset, count_offset)) = range {
        let unrestricted_max = if offset == count_offset { 32.0f32 } else { 1.0 };
        return Some(block.bytes.get(offset..offset + 8).is_some_and(|pair| {
            let minimum = f32::from_le_bytes(pair[..4].try_into().expect("range minimum"));
            let maximum = f32::from_le_bytes(pair[4..].try_into().expect("range maximum"));
            minimum != 0.0 || maximum != unrestricted_max
        }));
    }
    (matches!(field.offset, 0xD8 | 0xDC) && keyed_range_implied(block)).then_some(false)
}

/// The general predicate's value range bounds the value its named key reads (every keyed
/// stock row keeps the pair ordered, every unkeyed row leaves it at 1 and 1), so the pair
/// leads only once a key is set. An unnamed key waits under Advanced, so the range it bounds
/// waits with it rather than leading as a range of nothing on the card. A switch such as
/// Weapon Holstered reads 1 while it holds and 0 while it does not, so 1 to 1 and 0 to 0 are
/// what the title already says. A count keeps its range, which is the test.
fn keyed_range_implied(block: &native::Block) -> bool {
    block.bytes.get(0xD4..0xD8).is_none_or(|key| {
        let key = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let switch_holds = || {
            !fields::keys::NUMERIC.contains(&key)
                && block.bytes.get(0xD8..0xE0).is_some_and(|pair| {
                    pair[..4] == pair[4..]
                        && (pair[..4] == 1.0f32.to_le_bytes() || pair[..4] == [0; 4])
                })
        };
        matches!(key, 0 | 0x811C_9DC5)
            || !fields::keys::known(block.class, 0xD4)
                .iter()
                .any(|entry| entry.hash == key)
            || switch_holds()
    })
}

fn primary_for(field: &fields::Field, block: &native::Block, ability: bool) -> bool {
    // Reading the chance literally is what 255 means, and it is what every stock condition
    // does, so the row repeated down a card saying what the Chance control beside it already
    // said. It leads only once it names some other source.
    if field.label == "Probability Source" && block.bytes.get(field.offset) == Some(&255) {
        return false;
    }
    // A zero hold or threshold is what most stock conditions keep, so each leads once set,
    // as Vengeance's 0.1 threshold does, rather than as a 0.0 on every damage condition.
    if matches!(field.label.as_str(), "Hold Duration" | "Value Threshold")
        && field
            .bytes(block, 0)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
    {
        return false;
    }
    // The counter editor draws the count it needs and what happens after it fires. Its
    // clamps start at the stock norm and wait under Advanced, where their sentinels are read.
    if block.class == 0x80803E30 {
        return false;
    }
    // Which orb decides whether you see the orbs at all.
    if super::orb_entity(block.class, field) {
        return true;
    }
    // A kill's Target State and Remembered Target are filters most kills leave empty, so each
    // leads once it names something. Drawn empty they put two None pickers under every kill
    // condition, and a trigger with Or alternatives repeated them for each.
    if block.class == trigger::CLASS
        && matches!(field.offset, 0x144 | 0x148)
        && field.bytes(block, 0).is_none_or(|bytes| {
            <[u8; 4]>::try_from(bytes)
                .map(u32::from_le_bytes)
                .is_ok_and(|key| matches!(key, 0 | 0x811C_9DC5))
        })
    {
        return false;
    }
    if block.class == 0x80803E4D && field.offset == 0x0C {
        return energy_limit_set(block);
    }
    if matches!(block.class, 0x80803E3F | 0x80803E3E) {
        return field.offset == 0x68 || ammo_target_leads(block, field.offset);
    }
    if let Some(leads) = predicate_range_leads(field, block) {
        return leads;
    }
    // A selector whose values are named, or a key whose stock values are named, is what
    // its node's plain title is about, so it leads rather than sitting under Advanced.
    // A selector whose values are named is what its node's plain title is about, so it leads
    // even while a fresh node still holds an unnamed value: On Picking Up Ammo is about its
    // ammo type before one is chosen. The general predicate is the exception. Its title is
    // the comparison, and its player state, weapon state and named key are incidental, so
    // there an unnamed value would only lead as "Native Value 0" and an unknown key as hex.
    // Those wait under Advanced until they name something.
    let incidental = matches!(block.class, 0x80803DCE | 0x80803DCC);
    let current = field.bytes(block, 0);
    let named_selector = || {
        let choices = fields::contract(block.class, field).choices;
        !choices.is_empty()
            && (!incidental
                || field.format == Format::Mask32
                || current
                    .and_then(|bytes| bytes.first().copied())
                    .is_some_and(|value| choices.iter().any(|(choice, _)| *choice == value)))
    };
    let named_key = || {
        let known = fields::keys::known(block.class, field.offset);
        !known.is_empty()
            && (!incidental
                || current
                    .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                    .map(u32::from_le_bytes)
                    .is_some_and(|key| known.iter().any(|candidate| candidate.hash == key)))
    };
    // The unnamed second event leads only once it is chosen. Most reload conditions leave
    // it clear, and an unnamed switch beside On Reload said nothing a reader could use.
    let second_event = field.label == "On Second Weapon Event"
        && current.is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0));
    // A set reload flag is what the condition's title, On Reloading, already says, so the box
    // leads only while it is clear.
    let reload_clear = field.label == "On Reload"
        && current.is_none_or(|bytes| bytes.iter().all(|byte| *byte == 0));
    if field.editable
        && !stock_setting(field, block)
        && (matches!(field.format, Format::Byte | Format::Mask32) && named_selector()
            || matches!(field.format, Format::Key | Format::Tag) && named_key()
            || second_event
            || reload_clear
            || field.label == "Radar Detection Range"
            || field.label.contains(" Ammo "))
    {
        return true;
    }
    primary(field, ability)
}

/// The selector byte alone names the ability, whatever the flag and option bytes hold
/// (see `action::roles`), so this is not gated on them.
fn ability_adjustment(block: &native::Block) -> bool {
    block.class == 0x80803E4D && matches!(block.bytes.get(2), Some(0 | 1 | 2 | 7))
}

/// The controls a node's primary view leads with, before its scalar fields: the general
/// predicate's named comparisons and the behavior script of kind 48.
fn leading_controls(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    let class = graph.blocks[index].class;
    if class == 0x80803E30 {
        counter_editor(ui, graph, index)?;
        return Ok(());
    }
    if matches!(class, 0x80803DCE | 0x80803DCC) && comparison_editor(ui, graph, index)? {
        return Ok(());
    }
    if class == 0x80803DCE {
        let comparisons = native::predicate::comparisons(graph, index);
        if !comparisons.is_empty() {
            let field = fields::describe(0x80800090)?
                .into_iter()
                .find(|field| field.offset == 0)
                .ok_or("The comparison constant has no scalar field.")?;
            crate::app::style::tiles(ui, |ui, width| {
                for comparison in &comparisons {
                    let hint = format!(
                        "Required comparison: {} {} threshold. Other native restrictions remain in effect.",
                        comparison.name, comparison.operation
                    );
                    crate::app::style::tile(
                        ui,
                        width,
                        ("comparison", comparison.constant_block),
                        &comparison.name,
                        &hint,
                        false,
                        |ui| {
                            ui.spacing_mut().interact_size.x = ui.available_width();
                            super::scalar(
                                ui,
                                &field,
                                &mut graph.blocks[comparison.constant_block],
                                0,
                            )
                        },
                    )
                    .0?;
                }
                Ok::<(), String>(())
            })?;
        }
    }
    if class == scripts::CLASS {
        scripts::draw(ui, graph, index)?;
    }
    if class == WEAPON_VALUES_CLASS {
        weapon_values(ui, &mut graph.blocks[index])?;
    }
    Ok(())
}

/// A general predicate with exactly one compiled comparison edits as the comparison the
/// game makes: which engine variable, which operation and which threshold. Returns whether
/// it drew, so a predicate with several comparisons keeps the per-comparison thresholds.
/// Fields a guided editor draws itself, so the raw rows never repeat them in either view.
fn owned_by_editor(class: u32, offset: usize) -> bool {
    (class == 0x80803E30 && matches!(offset, 0x20 | 0x24))
        || (matches!(class, 0x80803DCE | 0x80803DCC) && offset == 0xF8)
        || ammo::owns(class, offset)
}

/// Whether a condition's chance is a tile in this view. A condition that always passes says
/// nothing by repeating so on every card, and a card can hold three of them. Certainty waits
/// in its properties, anything else leads.
fn chance_leads(block: &native::Block, view: FieldView, demotes: bool, trigger: bool) -> bool {
    if !nodes::CONDITIONS
        .iter()
        .any(|kind| kind.class == block.class)
        || block.bytes.get(4) != Some(&255)
    {
        return false;
    }
    let Some(chance) = block
        .bytes
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(f32::from_le_bytes)
    else {
        return false;
    };
    let certain = demotes && !trigger && (chance - 1.0).abs() < f32::EPSILON;
    let leads = matches!(view, FieldView::Primary) != certain;
    leads && (0.0..=1.0).contains(&chance)
}

/// A condition's chance as a percentage tile.
fn chance(ui: &mut egui::Ui, width: f32, bytes: &mut [u8]) {
    let mut percent = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) * 100.0;
    crate::app::style::tile(
        ui,
        width,
        "chance",
        "Chance",
        "Chance that this condition passes when its requirements match.",
        false,
        |ui| {
            let chance = ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                egui::DragValue::new(&mut percent)
                    .range(0.0..=100.0)
                    .clamp_existing_to_range(false)
                    .suffix("%"),
            );
            pickers::name_response(ui, &chance, "Chance");
            if chance.changed() {
                bytes[..4].copy_from_slice(&(percent / 100.0).to_le_bytes());
            }
        },
    );
}

fn controls(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    view: FieldView,
    // Whether this caller also draws an Advanced pass. A node drawn on its own has nowhere
    // to put a demoted row, so demoting there would hide the control rather than move it.
    demotes: bool,
    // Whether the node starts its group. Its chance leads even when certain, since how often
    // a trigger fires is the first thing a reader tunes.
    trigger: bool,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    // A kill condition draws its leading chance beside its Kill Trigger preset.
    let chance_tile = chance_leads(&graph.blocks[index], view, demotes, trigger)
        && !(class == trigger::CLASS && matches!(view, FieldView::Primary));
    if matches!(view, FieldView::Primary) {
        leading_controls(ui, graph, index)?;
    }
    let ability = class == 0x80803E4D;
    // A literal chance is the Chance tile in one view or the other, never also a raw number.
    let literal_chance = nodes::CONDITIONS.iter().any(|kind| kind.class == class)
        && graph.blocks[index].bytes.get(4) == Some(&255);
    let fields = fields::describe(class)?;
    // A value the compiler derives, such as a label predicate's mode, is rewritten on build, so
    // no view offers it as a control.
    let editable = fields
        .iter()
        .filter(|field| {
            visible(field, class)
                && !owned_by_editor(class, field.offset)
                && !compiler_owned(class, field.offset)
        })
        .filter(|field| !(literal_chance && field.offset == 0))
        .collect::<Vec<_>>();
    let unexplained = nodes::EFFECTS.iter().any(|kind| kind.class == class)
        || nodes::CONDITIONS
            .iter()
            .any(|kind| kind.class == class && nodes::plain_condition_title(kind.kind).is_none());
    let mut labelled = false;
    for (offset, _) in native::labels::bindings(class)? {
        labelled |= native::labels::source(graph, index, offset)?
            .iter()
            .any(|labels| !labels.is_empty());
    }
    let other_content = labelled
        || matches!(
            class,
            0x80803E43
                | 0x80803E44
                | 0x80803E45
                | 0x80803E47
                | 0x80803E12
                | 0x80803E30
                | 0x80803DCE
                | 0x80803DCC
        )
        || class == damage::CLASS
        || ammo::leads(class)
        || class == scripts::CLASS
        || schema::inline(class)?
            .iter()
            .any(|(_, child, _)| *child == native::value::CLASS);
    let promoted = unexplained
        && !other_content
        && !editable
            .iter()
            .any(|field| primary_for(field, &graph.blocks[index], ability));
    let revealed = revealed_targets(ui, class);
    let multiplier = ability && energy_multiplier_leads(graph, index);
    let chosen = editable
        .into_iter()
        .filter(|field| {
            // A literal chance source repeats the Chance tile, so it never leads.
            let leads = (promoted
                && field.label != "Probability Source"
                && !stock_setting(field, &graph.blocks[index]))
                || primary_for(field, &graph.blocks[index], ability)
                || (multiplier && field.offset == 8)
                || revealed.contains(&field.offset);
            matches!(view, FieldView::Primary) == leads
        })
        .collect::<Vec<_>>();
    // Values read as tiles, a name over each, three or four to a line, so a node of short
    // fields fills the card's width rather than running down one column. Hex keys keep their
    // rows, where the key and its name both have room.
    let (tiled, rows): (Vec<_>, Vec<_>) = chosen.into_iter().partition(|field| {
        (ability && field.offset == 2) || super::tile_width(class, field, 0.0).is_some()
    });
    // Two fields fit side by side once the pane affords two cells. Each row measures its own
    // label column against the whole line, so however wide the pane grew a card of short
    // fields ran down a single column with the rest of every line empty. A narrow pane keeps
    // the rows, where the label column gives way as the pane tightens, rather than cells that
    // hold their label width and squeeze the control instead.
    let wide = ui.available_width() >= cell_width(ui) * 2.0;
    let mut failed = None;
    let mut draw_fields = |ui: &mut egui::Ui| {
        for field in &rows {
            let label = if ability && field.offset == 2 {
                "Ability"
            } else {
                super::plain_field_label(class, &field.label)
            };
            let hint = fields::contract(class, field).description;
            let content = |ui: &mut egui::Ui| {
                if ability && field.offset == 2 {
                    super::ability_target(ui, &mut graph.blocks[index].bytes[2]);
                    Ok(())
                } else {
                    super::scalar(ui, field, &mut graph.blocks[index], 0)
                }
            };
            // The cell scopes its own widgets. A scope around it in the wrapping line would
            // place it at the cursor without wrapping, and squeeze every row after the first.
            let drawn = if wide {
                cell(ui, field.offset, label, hint, content)
            } else {
                ui.push_id(field.offset, |ui| {
                    super::super::super::properties::field(ui, label, hint, content)
                })
                .inner
            };
            if let Err(error) = drawn
                && failed.is_none()
            {
                failed = Some(error);
            }
        }
    };
    // An empty wrapping line still takes a line's height, which left a blank row above the
    // tiles of every node without label-left fields. With no rows, draw_fields draws nothing.
    if wide && !rows.is_empty() {
        ui.horizontal_wrapped(&mut draw_fields);
    } else {
        draw_fields(ui);
    }
    if chance_tile || !tiled.is_empty() {
        crate::app::style::tiles(ui, |ui, width| {
            if chance_tile {
                chance(ui, width, &mut graph.blocks[index].bytes);
            }
            for field in &tiled {
                let target = ability && field.offset == 2;
                let label = if target {
                    "Ability"
                } else {
                    super::plain_field_label(class, &field.label)
                };
                let hint = fields::contract(class, field).description;
                let control = super::tile_width(class, field, width).unwrap_or(width);
                let (drawn, _) =
                    crate::app::style::tile(ui, width, field.offset, label, hint, false, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().interact_size.x = control;
                            ui.spacing_mut().combo_width = control;
                            if target {
                                super::ability_target(ui, &mut graph.blocks[index].bytes[2]);
                                Ok(())
                            } else {
                                super::scalar(ui, field, &mut graph.blocks[index], 0)
                            }
                        })
                        .inner
                    });
                if let Err(error) = drawn
                    && failed.is_none()
                {
                    failed = Some(error);
                }
            }
        });
    }
    if let Some(error) = failed {
        return Err(error);
    }
    if matches!(view, FieldView::Primary) {
        add_target_menu(ui, &fields, &graph.blocks[index], &revealed);
    }
    values(ui, graph, index, view)?;
    // Every label list on this node. The kill node draws its sites beside its presets. A node
    // whose filters are all it has, such as a fresh Event Label Filter, leads with their fold
    // rather than reading empty.
    let bindings = native::labels::bindings(class)?;
    let mut populated_labels = false;
    for (offset, _) in &bindings {
        populated_labels |= native::labels::source(graph, index, *offset)?
            .iter()
            .any(|labels| !labels.is_empty());
    }
    let labels_lead = populated_labels || (promoted && !bindings.is_empty());
    if !demotes || matches!(view, FieldView::Primary) == labels_lead {
        labels::draw_sites(ui, graph, index)?;
    }
    Ok(())
}

pub(super) fn visible(field: &fields::Field, class: u32) -> bool {
    field.editable
        && (!matches!(field.format, Format::Bytes | Format::Pointer | Format::Tag)
            || super::orb_entity(class, field)
            || super::ability_reference(class, field))
        && !field.label.starts_with("Native Value")
        && field.label != "Retain Effect State"
        && !(nodes::CONDITIONS.iter().any(|node| node.class == class)
            && field.offset < 8
            && !matches!(field.offset, 0 | 4))
}

/// The one constant a value program pushes, when pushing it is all the program does.
fn simple_constant(program: &native::value::Program) -> Option<u32> {
    let simple = program.fast_path == 0
        && matches!(program.instructions.as_slice(), [a, b] if a.opcode == 52 && a.operand == Some(0) && b.opcode == 62 && b.operand == Some(0))
        && program.constants.len() == 1
        && program.constants[0]
            .iter()
            .all(|lane| *lane == program.constants[0][0]);
    simple.then(|| program.constants[0][0])
}

/// Stock energy grants keep their amount in the multiplier over a value of 1, and authored ones
/// keep it in the value over a multiplier of 1. The value hides while it is 1, so the multiplier
/// leads unless the value carries the amount on its own. Before, neither led on a stock card.
fn energy_multiplier_leads(graph: &Graph, index: usize) -> bool {
    let one = 1.0f32.to_bits();
    let multiplier = graph.blocks[index]
        .bytes
        .get(8..12)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    let value = native::value::Program::read(graph, index, 0x18)
        .ok()
        .as_ref()
        .and_then(simple_constant);
    multiplier != Some(one) || value.is_none_or(|value| value == one)
}

/// Reload from Reserves, whose five value programs are the share of capacity each weapon
/// reloads.
const RESERVE_TRANSFER_CLASS: u32 = 0x808029EC;

/// Reload from Reserves' shares in the order its card leads with them: the perk's own weapon,
/// which most stock perks fill, then the equipped weapon, then the Kinetic, Energy and Power
/// slots.
const RESERVE_SHARES: [usize; 5] = [0xB0, 0x78, 0xE8, 0x120, 0x158];

/// A value program's place and name on its card, where a node names its programs.
fn value_name(class: u32, offset: usize) -> Option<(usize, &'static str)> {
    if class != RESERVE_TRANSFER_CLASS {
        return None;
    }
    let rank = RESERVE_SHARES.iter().position(|at| *at == offset)?;
    action::RESERVE_TRANSFER_PROGRAMS
        .iter()
        .find(|(at, _, _)| *at == offset)
        .map(|(_, label, _)| (rank, label.trim_end_matches(" Value")))
}

/// A node's value expressions. A constant is a tile, and a node's constants share one line of
/// tiles, so Reload from Reserves' five shares sit side by side rather than one under another.
/// An expression keeps its own fold. A constant that only scales by one, as an ability
/// adjustment's often does, waits under Show Properties.
fn values(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    view: FieldView,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    let mut offsets = schema::inline(class)?
        .into_iter()
        .filter(|(_, child, _)| *child == native::value::CLASS)
        .map(|(offset, _, _)| offset)
        .collect::<Vec<_>>();
    offsets.sort_by_key(|offset| {
        (
            value_name(class, *offset).map_or(usize::MAX, |(rank, _)| rank),
            *offset,
        )
    });
    let mut constants = Vec::new();
    for offset in offsets {
        let program = native::value::Program::read(graph, index, offset)?;
        let constant = simple_constant(&program);
        let identity_scale =
            ability_adjustment(&graph.blocks[index]) && constant == Some(1.0f32.to_bits());
        let primary = constant.is_some() && !identity_scale;
        if primary != matches!(view, FieldView::Primary) {
            continue;
        }
        let label = value_name(class, offset).map(|(_, label)| label.to_owned());
        match constant {
            Some(bits) => constants.push((offset, bits, label.unwrap_or_else(|| "Value".into()))),
            None => {
                egui::CollapsingHeader::new(
                    label.unwrap_or_else(|| format!("Value Expression at +0x{offset:X}")),
                )
                .id_salt(("value-expression", offset))
                .show(ui, |ui| super::value(ui, graph, index, offset))
                .body_returned
                .transpose()?;
            }
        }
    }
    if constants.is_empty() {
        return Ok(());
    }
    let hint = if class == RESERVE_TRANSFER_CLASS {
        "Share of the capacity to reload. 0.1 reloads 10%, as far as reserves allow."
    } else {
        "Value supplied to the action's scale or operation."
    };
    let edited = crate::app::style::tiles(ui, |ui, width| {
        let mut edited = Vec::new();
        for (offset, bits, label) in &constants {
            let mut value = *bits;
            crate::app::style::tile(ui, width, ("constant", *offset), label, hint, false, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().interact_size.x = ui.available_width();
                    let constant = float_field(ui, &mut value);
                    pickers::name_response(ui, &constant, label);
                });
            });
            if value != *bits {
                edited.push((*offset, value));
            }
        }
        edited
    });
    for (offset, bits) in edited {
        let mut program = native::value::Program::read(graph, index, offset)?;
        program.constants[0] = [bits; 4];
        program.write(graph, index, offset)?;
    }
    Ok(())
}
