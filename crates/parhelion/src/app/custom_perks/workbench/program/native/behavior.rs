//! Gameplay controls over the existing allocation graph. Unknown bytes stay in that graph.
use super::super::super::canvas;
use super::*;
use crate::app::custom_perks::workbench::controls::{cell, cell_width, sized};
use sundial::package_authoring::sandbox_perk::action::{self, DecodedCondition};

mod labels;
mod scripts;
mod trigger;

use sundial::package_authoring::sandbox_perk::activation::site_labels as activation_site_labels;

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
    labels: &BTreeMap<u32, String>,
    pick: &mut ConditionPicker<'_>,
) -> Result<Option<u32>, String> {
    let (payload, offsets) = graph.emit_with_offsets()?;
    let decoded = action::decode(&payload)?;
    let blocks = offsets
        .into_iter()
        .map(|(index, offset)| (offset, index))
        .collect();
    let mut edit_asset = None;
    for (index, group) in decoded.groups.iter().enumerate() {
        ui.push_id(("behavior-group", index), |ui| {
            if decoded.groups.len() > 1 {
                ui.strong(if index == 0 {
                    "Main Program".into()
                } else {
                    format!("Program {}", index + 1)
                });
            }
            conditions(ui, graph, &blocks, "Trigger", &group.activation, pick)?;
            canvas::row(
                ui,
                "Actions",
                "Actions started by this effect's trigger.",
                |ui| {
                    for (number, effect) in group.effects.iter().enumerate() {
                        let index = block_at(&blocks, effect.offset)?;
                        crate::app::style::block(ui.style())
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.push_id(index, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        let title = super::super::native_action_label(effect.kind, &graph.blocks[index].bytes);
                                        ui.label(format!("{}. {title}", number + 1));
                                        sundial::investment::draw_authoring_info_icon(
                                            ui,
                                            super::super::native_action_reading(
                                                effect.kind,
                                                &effect.description(),
                                            ),
                                        );
                                        if let Some(tag) = effect.referenced_tag.filter(|tag| labels.contains_key(tag)) {
                                            let label = labels.get(&tag).cloned().unwrap_or_else(|| format!("Asset 0x{tag:08X}"));
                                            if ui.button(label).on_hover_text(format!("Edit this action's components.\nAsset 0x{tag:08X}")).clicked() {
                                                edit_asset = Some(tag);
                                            }
                                        }
                                    });
                                    controls(ui, graph, index, FieldView::Primary, true)?;
                                    advanced(ui, graph, index)?;
                                    if !effect.conditions.is_empty() {
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
                                                    condition_rows(ui, graph, &blocks, "Matching Conditions", &effect.conditions, pick)
                                                })
                                                .body_returned
                                                .transpose()?;
                                        } else {
                                            canvas::row(ui, "Trigger", "Checks for this action only. The effect's main trigger keeps its own settings.", |ui| {
                                                condition_rows(ui, graph, &blocks, "Matching Conditions", &effect.conditions, pick)
                                            })?;
                                        }
                                    }
                                    Ok::<_, String>(())
                                })
                                .inner
                            })
                            .inner?;
                    }
                    Ok::<_, String>(())
                },
            )?;
            if !group.removal.is_empty() {
                conditions(ui, graph, &blocks, "End Condition", &group.removal, pick)?;
            }
            if !group.rearm.is_empty() {
                let title = if group.rearm.len() == 1 && group.rearm[0].kind == 1 {
                    "Cooldown"
                } else {
                    "Reactivation"
                };
                conditions(ui, graph, &blocks, title, &group.rearm, pick)?;
            }
            Ok::<_, String>(())
        })
        .inner?;
    }
    Ok(edit_asset)
}

fn block_at(blocks: &BTreeMap<usize, usize>, offset: usize) -> Result<usize, String> {
    blocks
        .get(&offset)
        .copied()
        .ok_or_else(|| "The decoded behavior no longer matches its native record.".into())
}

fn conditions(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    blocks: &BTreeMap<usize, usize>,
    title: &str,
    entries: &[DecodedCondition],
    pick: &mut ConditionPicker<'_>,
) -> Result<(), String> {
    canvas::row(ui, title, "Conditions for this part of the effect.", |ui| {
        condition_rows(ui, graph, blocks, title, entries, pick)
    })
}

fn condition_rows(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    blocks: &BTreeMap<usize, usize>,
    title: &str,
    entries: &[DecodedCondition],
    pick: &mut ConditionPicker<'_>,
) -> Result<(), String> {
    if entries.is_empty() {
        ui.label("Always");
    }
    for (number, condition) in entries.iter().enumerate() {
        let index = block_at(blocks, condition.offset)?;
        ui.push_id((title, index), |ui| {
            let mut replacement = None;
            // A kill condition draws a column of filters. Its picker would sit beside that
            // column and leave the height of it as empty space, so it goes underneath.
            let stacked = condition.kind == 2;
            ui.horizontal_wrapped(|ui| {
                if number > 0 {
                    ui.label("Or");
                }
                if condition.kind == 2 {
                    trigger::draw(ui, graph, index)?;
                } else if condition.kind == 1 {
                    ui.label("After").on_hover_text(
                        "Time before this condition passes, measured from when its timer starts.",
                    );
                    let field = fields::describe(graph.blocks[index].class)?
                        .into_iter()
                        .find(|field| field.label == "Duration")
                        .ok_or("The timer duration field is missing.")?;
                    super::scalar(ui, &field, &mut graph.blocks[index], 0)?;
                } else {
                    ui.label(super::super::native_condition_reading(
                        condition.kind,
                        &condition.description(),
                    ));
                }
                if !stacked {
                    replacement = pick(ui);
                }
                Ok::<_, String>(())
            })
            .inner?;
            if stacked {
                replacement =
                    crate::app::custom_perks::workbench::controls::cell(ui, "", "", |ui| pick(ui));
            }
            if let Some(node) = replacement {
                let class = nodes::condition(node.kind)
                    .ok_or("Unknown condition kind.")?
                    .class;
                let copied = Graph::read(&node.bytes, 0, class)?;
                copied.validate_node(true, node.kind)?;
                let root = graph.append(&copied)?;
                graph.blocks[index] = graph.blocks[root].clone();
                return Ok::<_, String>(());
            }
            if condition.kind != 1 {
                controls(ui, graph, index, FieldView::Primary, true)?;
            }
            advanced(ui, graph, index)?;
            if !condition.children.is_empty() {
                ui.indent("children", |ui| {
                    ui.weak("Contributing Conditions");
                    condition_rows(
                        ui,
                        graph,
                        blocks,
                        "Contributing Conditions",
                        &condition.children,
                        pick,
                    )
                })
                .inner?;
            }
            for (number, subgroup) in condition.subgroups.iter().enumerate() {
                ui.indent(("subgroup", number), |ui| {
                    ui.weak(format!("Requirement {}, met by any of", number + 1));
                    condition_rows(
                        ui,
                        graph,
                        blocks,
                        &format!("Requirement {}", number + 1),
                        &subgroup.conditions,
                        pick,
                    )
                })
                .inner?;
            }
            Ok::<_, String>(())
        })
        .inner?;
    }
    Ok(())
}

fn advanced(ui: &mut egui::Ui, graph: &mut Graph, index: usize) -> Result<(), String> {
    egui::CollapsingHeader::new("Advanced")
        .id_salt(("node-advanced", index))
        .show(ui, |ui| {
            controls(ui, graph, index, FieldView::Details, true)?;
            nested(ui, graph, index)
        })
        .body_returned
        .transpose()?;
    Ok(())
}

/// Follow owned value/filter allocations, stopping at independently edited nodes.
/// All edits still use the same byte-exact scalar writer as Native Structure.
fn nested(ui: &mut egui::Ui, graph: &mut Graph, parent: usize) -> Result<(), String> {
    let mut visited = std::collections::BTreeSet::new();
    let mut pending = vec![parent];
    while let Some(parent) = pending.pop() {
        if !visited.insert(parent) {
            continue;
        }
        let links = graph.blocks[parent].links.clone();
        for (at, child) in links {
            let block = graph
                .blocks
                .get(child)
                .ok_or("Missing nested native record.")?;
            let class = block.class;
            if class == 0
                || nodes::CONDITIONS
                    .iter()
                    .chain(&nodes::EFFECTS)
                    .any(|n| n.class == class)
            {
                continue;
            }
            let count = block.count.unwrap_or(1);
            let fields = fields::describe(class)?;
            let editable: Vec<_> = fields
                .iter()
                .filter(|field| visible(field, class))
                .collect();
            let has_labels = native::labels::bindings(class)?
                .iter()
                .any(|(source, _)| !activation_site_labels(class, *source).is_empty());
            if !editable.is_empty() || has_labels {
                let stride = schema::record(graph.blocks[parent].class)?.size;
                let context = super::reference_name(graph, parent, at % stride, Some(child))?;
                egui::CollapsingHeader::new(context)
                    .id_salt(("nested-fields", parent, at))
                    .show(ui, |ui| {
                        for row in 0..count {
                            if count > 1 {
                                ui.strong(format!("Entry {}", row + 1));
                            }
                            for field in &editable {
                                ui.push_id((child, row, field.offset), |ui| {
                                    super::super::super::properties::field(
                                        ui,
                                        &field.label,
                                        fields::contract(class, field).description,
                                        |ui| {
                                            // This record can be one another node points at
                                            // too, so an edit lands on a copy that only this
                                            // owner reaches. The link is re-read because an
                                            // earlier field in this same pass may have made
                                            // that copy already.
                                            let target = graph.blocks[parent]
                                                .links
                                                .get(&at)
                                                .copied()
                                                .unwrap_or(child);
                                            let before = graph.blocks[target].bytes.clone();
                                            let drawn = super::scalar(
                                                ui,
                                                field,
                                                &mut graph.blocks[target],
                                                row,
                                            );
                                            if drawn.is_ok() && graph.blocks[target].bytes != before
                                            {
                                                let after = std::mem::replace(
                                                    &mut graph.blocks[target].bytes,
                                                    before,
                                                );
                                                let private = graph.make_unique(parent, at)?;
                                                graph.blocks[private].bytes = after;
                                            }
                                            drawn
                                        },
                                    )
                                })
                                .inner?;
                            }
                            // Nested records carry label sites too: the weapon-family lists
                            // inside the ammunition and finder nodes, for example.
                            ui.push_id((child, row, "labels"), |ui| {
                                labels::draw_row_sites(ui, graph, child, row)
                            })
                            .inner?;
                        }
                        Ok::<_, String>(())
                    })
                    .body_returned
                    .transpose()?;
            }
            pending.push(child);
        }
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
        // The selector byte names the ability. The input selector at +8 is 255 in nearly
        // every stock action and has no traced name, so it waits under Advanced.
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
            | "Modifier Value"
            | "Modifier Limit"
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
    if matches!(block.class, 0x80803E3F | 0x80803E3E) {
        return matches!(field.offset, 0x68 | 0x6C..=0x84);
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
    if field.editable
        && (matches!(field.format, Format::Byte | Format::Mask32)
            && !fields::contract(block.class, field).choices.is_empty()
            || field.format == Format::Key
                && !fields::keys::known(block.class, field.offset).is_empty()
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
        for field in fields.iter().filter(|field| visible(field, class)) {
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
    // Every label list the stock perks author on this node kind, from that site's own
    // vocabulary. The kill node draws its sites beside its presets instead.
    if matches!(view, FieldView::Primary) {
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
