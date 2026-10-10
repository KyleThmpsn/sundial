//! Gameplay controls over the existing allocation graph. Unknown bytes stay in that graph.
use super::super::super::canvas;
use super::*;
use crate::app::custom_perks::workbench::controls::{cell, cell_width, sized};
use crate::app::custom_perks::workbench::validation;
use sundial::package_authoring::sandbox_perk::action::{self, DecodedCondition};

mod ammo;
mod comparison;
mod conditions;
mod counter;
mod damage;
mod field_rows;
pub(super) mod labels;
mod logic;
mod requirements;
mod scripts;
mod timers;
mod trigger;
mod weapon_values;
use super::structure::{self, Edit, List, Part};
use comparison::comparison_editor;
use conditions::*;
use counter::counter_editor;
use field_rows::*;
pub(super) use logic::negation;
use logic::{logic_commands, logic_width, uninverted_title};
use requirements::requirements;
use timers::*;
use weapon_values::{WEAPON_VALUES_CLASS, weapon_values};

#[cfg(test)]
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
                                ui.close();
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
                                    if problem::contains(ui, &path) {
                                        properties.reveal(ui);
                                    }
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
                                    ) && let Some(tag) =
                                        asset_action(ui, graph, block_index, effect, assets, pick)?
                                    {
                                        edit_asset = Some(tag);
                                    }
                                    // What a damage modifier does to damage leads, ahead of the filters that
                                    // choose which damage it applies to.
                                    damage::draw(ui, graph, block_index)?;
                                    ammo::draw(ui, graph, block_index);
                                    // An attachment's length sits with its Lifetime, the other
                                    // thing that decides when it ends.
                                    let attached = matches!(class, 0x80803E44 | 0x80803E45)
                                        .then(|| referenced_tag(&graph.blocks[block_index].bytes))
                                        .flatten();
                                    let mut length = |ui: &mut egui::Ui, width: f32| {
                                        if let Some(tag) = attached {
                                            length_tile(ui, width, tag, effect, assets, pick);
                                        }
                                    };
                                    controls_with_trail(
                                        ui,
                                        graph,
                                        block_index,
                                        FieldView::Primary,
                                        true,
                                        false,
                                        if attached.is_some() {
                                            Some(&mut length as &mut dyn FnMut(&mut egui::Ui, f32))
                                        } else {
                                            None
                                        },
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
                                            .open(problem::open_header(
                                                ui,
                                                ui.make_persistent_id("extension-trigger"),
                                                problem::contains(ui, &path),
                                            ))
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
                            // An action chosen with its asset's edits, as Change Weapon
                            // Properties is, keeps them: the node holds only the asset.
                            if let Some(asset) =
                                action.asset().filter(|asset| !asset.values.is_empty())
                            {
                                match assets
                                    .iter_mut()
                                    .find(|existing| existing.graph == asset.graph)
                                {
                                    Some(existing) => *existing = asset.clone(),
                                    None => assets.push(asset.clone()),
                                }
                            }
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
                        .open(problem::open_header(
                            ui,
                            ui.make_persistent_id(("more-conditions", index)),
                            [Part::Ending, Part::Rearm].into_iter().any(|part| {
                                let list = List::group(index, part);
                                let mut path = list.owner;
                                path.push(list.field);
                                problem::contains(ui, &path)
                            }),
                        ))
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

/// The asset an attach, spawn, drop or projectile action references.
fn referenced_tag(bytes: &[u8]) -> Option<u32> {
    bytes
        .get(16..20)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
}

/// The attachment's length tile, drawn after the action's own tiles. The asset row it edits is
/// the action's, made when the first edit needs one.
fn length_tile(
    ui: &mut egui::Ui,
    width: f32,
    tag: u32,
    effect: &action::DecodedEffect,
    assets: &mut Vec<Asset>,
    pick: &mut NativePicker<'_>,
) {
    let position = assets.iter().position(|asset| asset.graph == tag);
    let mut asset = position.map_or_else(
        || Asset {
            graph: tag,
            path: effect.referenced_path.clone().unwrap_or_default(),
            ..Asset::default()
        },
        |position| assets[position].clone(),
    );
    pick(ui, NativeRequest::AssetLength(&mut asset, width));
    match position {
        Some(position) => assets[position] = asset,
        None if !asset.values.is_empty() => assets.push(asset),
        None => {}
    }
}

/// The contribution rows of a counter, each a condition with its Counter Change.
pub(super) const CONTRIBUTION_ROW_CLASS: u32 = 0x8080_3E32;
