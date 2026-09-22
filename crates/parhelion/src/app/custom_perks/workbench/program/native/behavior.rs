//! Gameplay controls over the existing allocation graph. Unknown bytes stay in that graph.
use super::super::super::canvas;
use super::*;
use crate::app::custom_perks::workbench::controls::{cell, cell_width, sized};
use sundial::package_authoring::sandbox_perk::action::{self, DecodedCondition};

pub(super) mod labels;
mod scripts;
mod trigger;
use super::structure::{self, Edit, List, Part};

/// Room for the longest engine variable a comparison can name.
const VARIABLE_WIDTH: f32 = 230.0;
/// A comparison operator is one or two characters, so it takes only what it needs.
const OPERATION_WIDTH: f32 = 80.0;

pub(super) fn draw_node(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    if graph.blocks[index].class == 0x80803DE7 {
        trigger::draw(ui, graph, index)?;
    }
    controls(ui, graph, index, FieldView::Primary, false)
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
        ui.push_id(("behavior-group", index), |ui| {
            if decoded.groups.len() > 1 {
                ui.horizontal(|ui| {
                ui.strong(if index == 0 {
                    "Main Behavior".into()
                } else {
                    format!("Behavior {}", index + 1)
                });
                if index > 0 {
                    crate::app::style::more_menu(ui, |ui| {
                        if ui.button("Remove Behavior Group").clicked() { remove_group = Some(index); ui.close_menu(); }
                    });
                }
                });
            }
            group_conditions(ui, graph, "Trigger", &group.activation, pick, List::group(index, Part::Trigger), &mut pending)?;
            canvas::row(
                ui,
                "Actions",
                "Actions started by this effect's trigger.",
                |ui| {
                    // Decoding preserves native storage order. Dispatch runs from last to first.
                    for (number, effect) in group.effects.iter().rev().enumerate() {
                        let path = List::group(index, Part::Actions).node(group.effects.len() - 1 - number)?;
                        let response = canvas::block(ui, ("native-action", number), |ui| structure::scoped(graph, &path, |graph, block_index| {
                                ui.set_min_width(ui.available_width());
                                ui.push_id(&path, |ui| {
                                    let mut properties = super::super::super::properties::Panel::new(ui, "action");
                                    let title = super::super::native_action_label(effect.kind, &graph.blocks[block_index].bytes);
                                    let hint = super::super::native_action_reading(effect.kind, &effect.description());
                                    let event = canvas::action_header(ui, &title, &hint, number, group.effects.len(), Some(&mut properties));
                                    if let Some(target) = event.swap_with { pending = Some((List::group(index, Part::Actions), Edit::Move(number, target))); }
                                    if event.remove { pending = Some((List::group(index, Part::Actions), Edit::Remove(number))); }
                                    let class = graph.blocks[block_index].class;
                                    if matches!(class, 0x80803E43 | 0x80803E44 | 0x80803E45 | 0x80803E47 | 0x80803E12) {
                                        let tag = u32::from_le_bytes(graph.blocks[block_index].bytes.get(16..20).ok_or("Missing asset reference.")?.try_into().unwrap());
                                        let mut asset = assets.iter().find(|asset| asset.graph == tag).cloned()
                                            .unwrap_or(Asset { graph: tag, path: effect.referenced_path.clone().unwrap_or_default(), ..Asset::default() });
                                        let scope = match class {
                                            0x80803E12 => AssetScope::Projectiles,
                                            0x80803E43 => AssetScope::Spawnable,
                                            _ => AssetScope::Any,
                                        };
                                        pick(ui, NativeRequest::Asset(&mut asset, scope));
                                        if matches!(asset.graph, 0 | u32::MAX) && class != 0x80803E47 {
                                            ui.colored_label(ui.visuals().error_fg_color, if class == 0x80803E12 { "Projectile: choose a projectile." } else { "Object: choose an object or effect." });
                                        }
                                        if asset.graph != tag {
                                            graph.blocks[block_index].bytes[16..20].copy_from_slice(&asset.graph.to_le_bytes());
                                            graph.create_target(block_index, 8, 0, false)?;
                                            let path = graph.blocks[block_index].links[&8];
                                            graph.blocks[path].bytes = asset.path.as_bytes().to_vec();
                                            graph.blocks[path].bytes.push(0);
                                        }
                                        if !matches!(asset.graph, 0 | u32::MAX) {
                                            if let Some(existing) = assets.iter_mut().find(|existing| existing.graph == asset.graph) { *existing = asset; }
                                            else { assets.push(asset); }
                                        }
                                    }
                                    controls(ui, graph, block_index, FieldView::Primary, true)?;
                                    let mut details = Ok(());
                                    properties.show(ui, |ui| {
                                        details = controls(ui, graph, block_index, FieldView::Details, true)
                                            .and_then(|()| nested(ui, graph, block_index));
                                        if let Some(tag) = effect.referenced_tag {
                                            let tag = if matches!(class, 0x80803E43 | 0x80803E44 | 0x80803E45 | 0x80803E47 | 0x80803E12) {
                                                u32::from_le_bytes(graph.blocks[block_index].bytes[16..20].try_into().unwrap())
                                            } else { tag };
                                            ui.separator();
                                            ui.strong("Referenced Object");
                                            if super::super::super::properties::edit_object(ui, &Asset { graph: tag, ..Asset::default() }) { edit_asset = Some(tag); }
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
                                            group.activation.iter().any(|trigger| trigger.native == condition.native)
                                        });
                                        if repeats {
                                            egui::CollapsingHeader::new("Repeats the Effect's Trigger")
                                                .id_salt("extension-trigger")
                                                .show(ui, |ui| {
                                                    condition_list(ui, graph, "Matching Conditions", &effect.conditions, pick, List { owner: path.clone(), field: 0x18, class: action::CONDITION_ROW_CLASS }, &mut pending)
                                                })
                                                .body_returned
                                                .transpose()?;
                                        } else {
                                            canvas::row(ui, "Trigger", "Checks for this action only. The effect's main trigger keeps its own settings.", |ui| {
                                                condition_list(ui, graph, "Matching Conditions", &effect.conditions, pick, List { owner: path.clone(), field: 0x18, class: action::CONDITION_ROW_CLASS }, &mut pending)
                                            })?;
                                        }
                                    }
                                    Ok::<_, String>(())
                                })
                                .inner
                            }));
                        response.inner?;
                        if reveal.as_ref().is_some_and(|target| target.group == index && target.action == number) {
                            response.response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    let context = Program {
                        trigger: if group.activation.iter().any(|node| node.kind == 2) { Trigger::WeaponKill } else { Trigger::Always },
                        actions: group.effects.iter().map(|effect| Action::Native { node: NativeNode { kind: effect.kind, bytes: effect.native.clone() } }).collect(),
                        ..Program::default()
                    };
                    if let Some(super::super::super::behaviors::Selection::Action(action)) = pick(ui, NativeRequest::Action(&context)) {
                        pending = Some((List::group(index, Part::Actions), Edit::Add(structure::action_node(action, group)?)));
                    }
                    Ok::<_, String>(())
                },
            )?;
            group_conditions(ui, graph, "End Condition", &group.removal, pick, List::group(index, Part::Ending), &mut pending)?;
            {
                let title = if group.rearm.len() == 1 && group.rearm[0].kind == 1 {
                    "Cooldown"
                } else {
                    "Reactivation"
                };
                group_conditions(ui, graph, title, &group.rearm, pick, List::group(index, Part::Rearm), &mut pending)?;
            }
            Ok::<_, String>(())
        })
        .inner?;
    }
    if let Some((list, edit)) = pending {
        list.edit(graph, edit)?;
    }
    if let Some(group) = remove_group {
        structure::remove_group(graph, group)?;
    }
    if ui
        .small_button("Add Behavior Group")
        .on_hover_text("Add a separate set of triggers and actions within this effect.")
        .clicked()
    {
        structure::add_group(graph)?;
    }
    Ok(edit_asset)
}

#[allow(clippy::too_many_arguments)]
fn group_conditions(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    title: &str,
    entries: &[DecodedCondition],
    pick: &mut NativePicker<'_>,
    list: List,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    let hint = match title {
        "Trigger" => canvas::ACTIVATION_HINT,
        "End Condition" => canvas::REMOVAL_HINT,
        _ => canvas::REARM_HINT,
    };
    canvas::row(ui, title, hint, |ui| {
        condition_list(ui, graph, title, entries, pick, list, pending)
    })
}

#[allow(clippy::too_many_arguments)]
fn condition_list(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    title: &str,
    entries: &[DecodedCondition],
    pick: &mut NativePicker<'_>,
    list: List,
    pending: &mut Option<(List, Edit)>,
) -> Result<(), String> {
    for (number, condition) in entries.iter().enumerate() {
        let path = list.node(number)?;
        ui.push_id((&list.owner, list.field, number), |ui| {
            // A contribution is one thing: its condition and its Counter Change share a block.
            let frame = if list.class == 0x80803E32 {
                crate::app::style::block(ui.style())
            } else {
                egui::Frame::new()
            };
            frame.show(ui, |ui| {
            structure::scoped(graph, &path, |graph, index| {
                ui.horizontal_wrapped(|ui| {
                    if list.class == 0x80803E32 {
                        ui.strong(format!("Contribution {}", number + 1));
                    } else if number > 0 {
                        ui.label("Or");
                    }
                    let title = super::super::native_condition_reading(
                        condition.kind,
                        &condition.description(),
                    );
                    if let Some(node) = pick_condition(pick, ui, &title) {
                        *pending = Some((list.clone(), Edit::Replace(number, node)));
                    }
                    crate::app::style::more_menu(ui, |ui| {
                        if ui.button("Remove Condition").clicked() {
                            *pending = Some((list.clone(), Edit::Remove(number)));
                            ui.close_menu();
                        }
                    });
                });
                if condition.kind == 1 {
                    let field = fields::describe(graph.blocks[index].class)?
                        .into_iter()
                        .find(|field| field.label == "Duration")
                        .ok_or("The timer duration field is missing.")?;
                    super::scalar(ui, &field, &mut graph.blocks[index], 0)?;
                } else {
                    if condition.kind == 2 {
                        trigger::draw(ui, graph, index)?;
                    }
                    controls(ui, graph, index, FieldView::Primary, true)?;
                }
                // Conditions and their alternatives remain normal controls at every depth.
                if list.class == 0x80803E32 {
                    // The header says what the row does, so the one number that matters is
                    // never hidden behind it. The engine word rides along for people who know it.
                    let title = format!(
                        "Counter Change (Accumulator) · {}",
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
                            class: if condition.kind == 35 { 0 } else { 0x80803E32 },
                        };
                        ui.indent("conditions", |ui| {
                            if condition.kind == 26 {
                                // The heading owns its add button, so adding a contribution
                                // is not the last thing under an indented list.
                                ui.horizontal(|ui| {
                                    ui.strong("Contributing Conditions");
                                    if let Some(node) = ui
                                        .push_id((&children.owner, children.field, "add"), |ui| {
                                            pick_condition(pick, ui, "Add Condition…")
                                        })
                                        .inner
                                    {
                                        *pending = Some((children.clone(), Edit::Add(node)));
                                    }
                                });
                                if condition.children.is_empty() {
                                    ui.colored_label(
                                        ui.visuals().warn_fg_color,
                                        "Nothing counts yet. Add a condition, such as a kill; each time it passes, the counter goes up by 1.",
                                    );
                                }
                            } else {
                                ui.weak("Required Condition");
                            }
                            condition_list(
                                ui,
                                graph,
                                if condition.kind == 26 {
                                    "Contributing Condition"
                                } else {
                                    "Condition"
                                },
                                &condition.children,
                                pick,
                                children,
                                pending,
                            )
                        })
                        .inner?;
                    }
                    31 => {
                        let subgroups = List {
                            owner: path.clone(),
                            field: 0x10,
                            class: action::SUBGROUP_ROW_CLASS,
                        };
                        if ui.small_button("Add Requirement").clicked() {
                            *pending = Some((subgroups.clone(), Edit::AddGroup));
                        }
                        for (row, subgroup) in condition.subgroups.iter().enumerate() {
                            ui.indent(("requirement", row), |ui| {
                                ui.horizontal(|ui| {
                                    if row > 0 {
                                        ui.label("And");
                                    }
                                    ui.weak(format!("Requirement {}", row + 1));
                                    crate::app::style::more_menu(ui, |ui| {
                                        if ui.button("Remove Requirement").clicked() {
                                            *pending = Some((subgroups.clone(), Edit::Remove(row)));
                                            ui.close_menu();
                                        }
                                    });
                                });
                                row_controls(ui, graph, &subgroups, row)?;
                                let mut owner = path.clone();
                                owner.push(0x10);
                                condition_list(
                                    ui,
                                    graph,
                                    "Requirement",
                                    &subgroup.conditions,
                                    pick,
                                    List {
                                        owner,
                                        field: row * 0x20 + 0x10,
                                        class: action::CONDITION_ROW_CLASS,
                                    },
                                    pending,
                                )
                            })
                            .inner?;
                        }
                    }
                    _ => {}
                }
                advanced(
                    ui,
                    graph,
                    index,
                    (list.class == 0x80803E32).then_some((&list, number)),
                )?;
                Ok::<_, String>(())
            })
            })
            .inner
        })
        .inner?;
    }
    // A contributing list's add button sits beside its heading instead.
    if title != "Contributing Condition" && (list.class != 0 || entries.is_empty()) {
        let label = if title == "End Condition" {
            "Add End Condition…"
        } else if matches!(title, "Reactivation" | "Cooldown") {
            "Add Reactivation Condition…"
        } else if entries.is_empty() && title == "Trigger" {
            "Always Active"
        } else if entries.is_empty() || list.class == 0x80803E32 {
            "Add Condition…"
        } else {
            "Add Alternative…"
        };
        let selected = ui
            .push_id((&list.owner, list.field, "add-condition"), |ui| {
                pick_condition(pick, ui, label)
            })
            .inner;
        if let Some(node) = selected {
            *pending = Some((list, Edit::Add(node)));
        }
    }
    Ok(())
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
        let leads = |field: &fields::Field| {
            matches!(field.offset, 8 | 9 | 12)
                || field
                    .bytes(&graph.blocks[index], row)
                    .is_some_and(|bytes| bytes.iter().any(|byte| *byte != 0))
        };
        let chosen = fields
            .iter()
            .filter(|field| visible(field, list.class) && leads(field) == leading)
            .collect::<Vec<_>>();
        for field in chosen {
            ui.push_id(("contribution", row, field.offset), |ui| {
                super::super::super::properties::field(
                    ui,
                    super::plain_field_label(list.class, &field.label),
                    fields::contract(list.class, field).description,
                    |ui| super::scalar(ui, field, &mut graph.blocks[index], row),
                )
            })
            .inner?;
        }
        Ok(())
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
        Ok(match byte(8) {
            0 => format!("adds {amount}"),
            1 => format!("sets it to {amount}"),
            2 => format!("multiplies it by {amount}"),
            other => format!("operation {other}"),
        })
    })
}

fn advanced(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    contribution: Option<(&List, usize)>,
) -> Result<(), String> {
    egui::CollapsingHeader::new("Advanced")
        .id_salt(("node-advanced", index))
        .show(ui, |ui| {
            controls(ui, graph, index, FieldView::Details, true)?;
            // A contribution's failure side and hold live with the condition they belong
            // to, so one Advanced covers the whole row.
            if let Some((list, row)) = contribution {
                row_fields(ui, graph, list, row, false)?;
            }
            nested(ui, graph, index)
        })
        .body_returned
        .transpose()?;
    Ok(())
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
    for (at, child) in links {
        let class = graph.blocks[child].class;
        if matches!(class, 0 | 0x80803E32 | 0x80803E06)
            || nodes::CONDITIONS
                .iter()
                .chain(&nodes::EFFECTS)
                .any(|n| n.class == class)
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
            let stride = schema::record(graph.blocks[parent].class)?.size;
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
                                        &field.label,
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

fn primary_for(field: &fields::Field, block: &native::Block, ability: bool) -> bool {
    // Reading the chance literally is what 255 means, and it is what every stock condition
    // does, so the row repeated down a card saying what the Chance control beside it already
    // said. It leads only once it names some other source.
    if field.label == "Probability Source" && block.bytes.get(field.offset) == Some(&255) {
        return false;
    }
    // The counter editor draws the count it needs and what happens after it fires. Its
    // clamps start at the stock norm and wait under Advanced, where their sentinels are read.
    if block.class == 0x80803E30 {
        return false;
    }
    if matches!(block.class, 0x80803E3F | 0x80803E3E) {
        return matches!(field.offset, 0x68 | 0x6C..=0x84);
    }
    if matches!(block.class, 0x80803DCE | 0x80803DCC) {
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
            return block.bytes.get(offset..offset + 8).is_some_and(|pair| {
                let minimum = f32::from_le_bytes(pair[..4].try_into().expect("range minimum"));
                let maximum = f32::from_le_bytes(pair[4..].try_into().expect("range maximum"));
                minimum != 0.0 || maximum != unrestricted_max
            });
        }
    }
    // The general predicate's value range bounds the value its named key reads (every keyed
    // stock row keeps the pair ordered, every unkeyed row leaves it at 1 and 1), so the
    // pair leads only once a key is set.
    if matches!(block.class, 0x80803DCE | 0x80803DCC)
        && matches!(field.offset, 0xD8 | 0xDC)
        && block.bytes.get(0xD4..0xD8).is_none_or(|key| {
            matches!(
                u32::from_le_bytes([key[0], key[1], key[2], key[3]]),
                0 | 0x811C_9DC5
            )
        })
    {
        return false;
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
    if field.editable
        && (matches!(field.format, Format::Byte | Format::Mask32) && named_selector()
            || field.format == Format::Key && named_key()
            || matches!(
                field.label.as_str(),
                "On Reload" | "On Second Weapon Event" | "Radar Detection Range"
            )
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
        for comparison in native::predicate::comparisons(graph, index) {
            let field = fields::describe(0x80800090)?
                .into_iter()
                .find(|field| field.offset == 0)
                .ok_or("The comparison constant has no scalar field.")?;
            super::super::super::properties::field(
                ui,
                &comparison.name,
                &format!(
                    "Required comparison: {} {} threshold. Other native restrictions remain in effect.",
                    comparison.name, comparison.operation
                ),
                |ui| super::scalar(ui, &field, &mut graph.blocks[comparison.constant_block], 0),
            )?;
        }
    }
    if class == scripts::CLASS {
        scripts::draw(ui, graph, index)?;
    }
    Ok(())
}

/// A general predicate with exactly one compiled comparison edits as the comparison the
/// game makes: which engine variable, which operation and which threshold. Returns whether
/// it drew, so a predicate with several comparisons keeps the per-comparison thresholds.
/// Fields a guided editor draws itself, so the raw rows never repeat them in either view.
fn owned_by_editor(class: u32, offset: usize) -> bool {
    class == 0x80803E30 && matches!(offset, 0x20 | 0x24)
}

/// What a counter does once it fires, read from and written to its Resets At field. The
/// stock perks use two settings: -1 keeps the count, and a reset equal to Count Needed
/// starts over, which is how the ones that fire every few kills are built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterFiring {
    Keep,
    StartOver,
    Custom,
}

fn after_firing(needed: f32, resets_at: f32) -> AfterFiring {
    if resets_at < 0.0 {
        AfterFiring::Keep
    } else if resets_at == needed {
        AfterFiring::StartOver
    } else {
        AfterFiring::Custom
    }
}

/// The counter's two decisions in plain words: how many, and what happens after it fires.
fn counter_editor(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    let fields = fields::describe(0x80803E30)?;
    let field_at = |offset: usize| {
        fields
            .iter()
            .find(|field| field.offset == offset)
            .ok_or("The counter's fields are missing.")
    };
    let needed_field = field_at(0x20)?;
    let resets_field = field_at(0x24)?;
    let read = |graph: &Graph, field: &fields::Field| {
        field
            .bytes(&graph.blocks[index], 0)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map_or(0.0, f32::from_le_bytes)
    };
    let needed_before = read(graph, needed_field);
    let resets_at = read(graph, resets_field);
    let mode_before = after_firing(needed_before, resets_at);
    let mut needed = needed_before;
    let mut mode = mode_before;
    super::super::super::properties::field(
        ui,
        "Count Needed",
        "How high the counter must reach for this trigger to fire. Each contributing condition below adds to the counter when it passes.",
        |ui| {
            let response = ui.add(
                egui::DragValue::new(&mut needed)
                    .range(1.0..=100_000.0)
                    .speed(0.1)
                    .max_decimals(0),
            );
            pickers::name_response(ui, &response, "Count Needed");
        },
    );
    super::super::super::properties::field(
        ui,
        "After It Fires",
        "Keep counting leaves the count where it is, so the trigger stays satisfied. Start over clears the count when it fires, the way the stock perks that trigger every few kills set Resets At to their Count Needed. A custom reset value is kept as it is.",
        |ui| {
            let label = match mode {
                AfterFiring::Keep => "Keep counting".to_owned(),
                AfterFiring::StartOver => "Start over".to_owned(),
                AfterFiring::Custom => format!("Custom (resets at {resets_at})"),
            };
            egui::ComboBox::from_id_salt("counter-after-firing")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, AfterFiring::Keep, "Keep counting");
                    ui.selectable_value(&mut mode, AfterFiring::StartOver, "Start over");
                    if mode_before == AfterFiring::Custom {
                        ui.selectable_value(
                            &mut mode,
                            AfterFiring::Custom,
                            format!("Custom (resets at {resets_at})"),
                        );
                    }
                });
            pickers::name_combo(ui, "counter-after-firing", "After It Fires");
        },
    );
    if needed != needed_before {
        needed_field.write(&mut graph.blocks[index], 0, &needed.to_le_bytes())?;
    }
    let resets_to = match mode {
        AfterFiring::Keep => -1.0,
        AfterFiring::StartOver => needed,
        AfterFiring::Custom => resets_at,
    };
    if mode != mode_before || (mode == AfterFiring::StartOver && needed != needed_before) {
        resets_field.write(&mut graph.blocks[index], 0, &resets_to.to_le_bytes())?;
    }
    Ok(())
}

fn comparison_editor(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<bool, String> {
    use sundial::package_authoring::sandbox_perk::action::native::predicate;
    let blocks = {
        let mut pending = vec![index];
        let mut seen = std::collections::BTreeSet::new();
        let mut found = Vec::new();
        while let Some(at) = pending.pop() {
            if !seen.insert(at) {
                continue;
            }
            let Some(block) = graph.blocks.get(at) else {
                continue;
            };
            if predicate::read(graph, at).is_some() {
                found.push(at);
            }
            pending.extend(block.links.values().copied());
        }
        found
    };
    let [block] = blocks.as_slice() else {
        return Ok(false);
    };
    let block = *block;
    let current = predicate::read(graph, block).ok_or("The comparison no longer reads.")?;
    let source = graph.blocks[block].links[&0];
    let raw = String::from_utf8_lossy(&graph.blocks[source].bytes)
        .trim_end_matches('\0')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut variable = raw.clone();
    let mut operation = current.operation;
    let mut threshold = f32::from_bits(current.threshold);
    let known = predicate::variable(&raw);
    super::super::super::properties::field(
        ui,
        "Compared Value",
        "The engine variable this condition compares. Each choice is one the game's own perks compare, named from the client's compiled source string.",
        |ui| {
            let selected = known.map_or_else(|| raw.clone(), |v| v.plain.to_owned());
            // A combo takes the width of its selected text and `width` only sets a floor, so
            // the longest variable name would run to the edge of the pane. The allocation
            // bounds it and the evidence for the choice stays on hover.
            let hover = known.map_or_else(
                || format!("{selected}\nNo stock perk compares this variable."),
                |v| format!("{selected}\n{}", v.evidence),
            );
            sized(ui, VARIABLE_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("predicate-variable")
                    .width(VARIABLE_WIDTH)
                    .truncate()
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for candidate in predicate::VARIABLES {
                            ui.selectable_value(
                                &mut variable,
                                candidate.name.to_owned(),
                                candidate.plain,
                            )
                            .on_hover_text(candidate.evidence);
                        }
                    })
                    .response
                    .on_hover_text(hover);
                pickers::name_combo(ui, "predicate-variable", "Compared Value");
            });
            Ok::<(), String>(())
        },
    )?;
    super::super::super::properties::field(
        ui,
        "Comparison",
        "How the value is compared with the threshold. These are the four operations the stock predicates compile.",
        |ui| {
            // The four operations are all one or two characters, so this control keeps its
            // own small width and leaves the row's remaining room to the threshold.
            let hover = format!("Comparison: {operation}");
            sized(ui, OPERATION_WIDTH, |ui| {
                egui::ComboBox::from_id_salt("predicate-operation")
                    .width(OPERATION_WIDTH)
                    .truncate()
                    .selected_text(operation)
                    .show_ui(ui, |ui| {
                        for (name, _) in predicate::OPERATIONS {
                            ui.selectable_value(&mut operation, name, name);
                        }
                    })
                    .response
                    .on_hover_text(hover);
                pickers::name_combo(ui, "predicate-operation", "Comparison");
            });
            let threshold_drag = ui.add(egui::DragValue::new(&mut threshold).speed(0.1));
            pickers::name_response(ui, &threshold_drag, "Threshold");
            Ok::<(), String>(())
        },
    )?;
    if variable != raw || operation != current.operation || threshold.to_bits() != current.threshold
    {
        predicate::rewrite(graph, block, &variable, operation, threshold)?;
    }
    Ok(true)
}

fn controls(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    view: FieldView,
    // Whether this caller also draws an Advanced pass. A node drawn on its own has nowhere
    // to put a demoted row, so demoting there would hide the control rather than move it.
    demotes: bool,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    if nodes::CONDITIONS.iter().any(|kind| kind.class == class)
        && graph.blocks[index].bytes.get(4) == Some(&255)
    {
        let bytes = &mut graph.blocks[index].bytes;
        let chance =
            f32::from_le_bytes(bytes[..4].try_into().map_err(|_| "Missing chance value.")?);
        // A condition that always passes says nothing by repeating so on every card, and a
        // card can hold three of them. Certainty waits under Advanced, anything else leads.
        let certain = demotes && (chance - 1.0).abs() < f32::EPSILON;
        let leads = matches!(view, FieldView::Primary) != certain;
        if leads && (0.0..=1.0).contains(&chance) {
            let mut percent = chance * 100.0;
            super::super::super::properties::field(
                ui,
                "Chance",
                "Chance that this condition passes when its requirements match.",
                |ui| {
                    let chance = ui.add(
                        egui::DragValue::new(&mut percent)
                            .range(0.0..=100.0)
                            .suffix("%"),
                    );
                    pickers::name_response(ui, &chance, "Chance");
                    if chance.changed() {
                        bytes[..4].copy_from_slice(&(percent / 100.0).to_le_bytes());
                    }
                },
            );
        }
    }
    if matches!(view, FieldView::Primary) {
        leading_controls(ui, graph, index)?;
    }
    let ability = class == 0x80803E4D;
    let fields = fields::describe(class)?;
    // Two fields fit side by side once the pane affords two cells. Each row measures its own
    // label column against the whole line, so however wide the pane grew a card of short
    // fields ran down a single column with the rest of every line empty. A narrow pane keeps
    // the rows, where the label column gives way as the pane tightens, rather than cells that
    // hold their label width and squeeze the control instead.
    let wide = ui.available_width() >= cell_width(ui) * 2.0;
    let mut failed = None;
    let mut draw_fields = |ui: &mut egui::Ui| {
        for field in fields
            .iter()
            .filter(|field| visible(field, class) && !owned_by_editor(class, field.offset))
        {
            if !match view {
                FieldView::Primary => primary_for(field, &graph.blocks[index], ability),
                FieldView::Details => !primary_for(field, &graph.blocks[index], ability),
            } {
                continue;
            }
            let drawn = ui
                .push_id(field.offset, |ui| {
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
                    if wide {
                        cell(ui, label, hint, content)
                    } else {
                        super::super::super::properties::field(ui, label, hint, content)
                    }
                })
                .inner;
            if let Err(error) = drawn
                && failed.is_none()
            {
                failed = Some(error);
            }
        }
    };
    if wide {
        ui.horizontal_wrapped(&mut draw_fields);
    } else {
        draw_fields(ui);
    }
    if let Some(error) = failed {
        return Err(error);
    }
    let mut inline = schema::inline(class)?;
    inline.sort_by_key(|(offset, _, _)| *offset);
    for (offset, child, _) in inline {
        if child == native::value::CLASS {
            ui.push_id(("expression", offset), |ui| {
                constant(ui, graph, index, offset, view)
            })
            .inner?;
        }
    }
    // Every label list on this node. The kill node draws its sites beside its presets.
    let mut populated_labels = false;
    for (offset, _) in native::labels::bindings(class)? {
        populated_labels |= native::labels::source(graph, index, offset)?
            .iter()
            .any(|labels| !labels.is_empty());
    }
    if !demotes || matches!(view, FieldView::Primary) == populated_labels {
        labels::draw_sites(ui, graph, index)?;
    }
    Ok(())
}

fn visible(field: &fields::Field, class: u32) -> bool {
    field.editable
        && !matches!(field.format, Format::Bytes | Format::Pointer | Format::Tag)
        && !field.label.starts_with("Native Value")
        && field.label != "Retain Effect State"
        && !(nodes::CONDITIONS.iter().any(|node| node.class == class)
            && field.offset < 8
            && !matches!(field.offset, 0 | 4))
}

fn constant(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    offset: usize,
    view: FieldView,
) -> Result<(), String> {
    let mut program = native::value::Program::read(graph, index, offset)?;
    let simple = program.fast_path == 0
        && matches!(program.instructions.as_slice(), [a, b] if a.opcode == 52 && a.operand == Some(0) && b.opcode == 62 && b.operand == Some(0))
        && program.constants.len() == 1
        && program.constants[0]
            .iter()
            .all(|lane| *lane == program.constants[0][0]);
    let reserve_transfer = graph.blocks[index].class == 0x808029EC;
    let identity_scale = ability_adjustment(&graph.blocks[index])
        && simple
        && program.constants[0][0] == 1.0f32.to_bits();
    let primary = simple && !reserve_transfer && !identity_scale;
    if primary != matches!(view, FieldView::Primary) {
        return Ok(());
    }
    let label = if reserve_transfer {
        action::RESERVE_TRANSFER_PROGRAMS
            .iter()
            .find(|(at, _, _)| *at == offset)
            .map(|(_, label, _)| label.replace("Value", "Fraction"))
    } else {
        None
    }
    .unwrap_or_else(|| {
        if simple {
            "Constant Value".into()
        } else {
            format!("Value Expression at +0x{offset:X}")
        }
    });
    if simple {
        let mut bits = program.constants[0][0];
        super::super::super::properties::field(
            ui,
            &label,
            if reserve_transfer {
                "Applicable slot fractions are added, then multiplied by Capacity Basis. 0.1 requests 10% of that capacity, subject to ammunition-unit rounding and available reserves. The two selected-slot identities remain unresolved."
            } else {
                "Value supplied to the action's scale or operation."
            },
            |ui| {
                let constant = float_field(ui, &mut bits);
                pickers::name_response(ui, &constant, &label);
            },
        );
        if bits != program.constants[0][0] {
            program.constants[0] = [bits; 4];
            program.write(graph, index, offset)?;
        }
    } else {
        egui::CollapsingHeader::new(label)
            .show(ui, |ui| super::value(ui, graph, index, offset))
            .body_returned
            .transpose()?;
    }
    Ok(())
}
