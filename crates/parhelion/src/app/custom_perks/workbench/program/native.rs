//! Complete native records in the private-perk editor.
use super::*;
use crate::app::custom_perks::workbench::controls::{COLUMN_WIDTH, NARROW_COLUMN, column, sized};
use sundial::package_authoring::sandbox_perk::action::native::{
    self, Graph,
    fields::{self, Format},
    schema,
};

mod behavior;
mod masks;

pub(in crate::app::custom_perks::workbench) fn label_choices(
    ctx: &egui::Context,
    registry: Option<Arc<sundial::investment::discovery::labels::Registry>>,
    error: Option<String>,
) {
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("installed-label-registry"), (registry, error));
    });
}

/// Share the current installation's script choices with all nested native editors.
/// Replaced every frame, including while discovery is pending, to avoid stale install data.
pub(in crate::app::custom_perks::workbench) fn script_choices(
    ctx: &egui::Context,
    choices: Option<Arc<Vec<sundial::investment::discovery::scripts::Choice>>>,
) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("installed-behavior-scripts"), choices));
}

/// Room for a traced key name, or for a value with its stock count beside it.
const EVIDENCE_WIDTH: f32 = 230.0;

pub(in crate::app::custom_perks::workbench) fn draw_complete(
    ui: &mut egui::Ui,
    program: &mut sundial::package_authoring::sandbox_perk::program::NativeProgram,
    editing: bool,
    labels: &BTreeMap<u32, String>,
    pick: &mut ConditionPicker<'_>,
) -> Option<usize> {
    let mut changed = program.clone();
    let mut edit_asset = None;
    let result = ui
        .add_enabled_ui(editing, |ui| {
            // One card draws many independent editors. Carrying the first failure out of here
            // rather than returning on it keeps the rest of the card drawn and, more
            // importantly, keeps the edits its other controls made this frame: a truncated
            // field in one node used to discard a label set made in another.
            let mut failed = None;
            match behavior::draw(ui, &mut changed.graph, labels, pick) {
                Ok(asset) => edit_asset = asset,
                Err(error) => failed = Some(error),
            }
            let structure = egui::CollapsingHeader::new("Advanced")
                .id_salt("complete-program-structure")
                .show(ui, |ui| {
                    if !changed.assets.is_empty() {
                        ui.strong("Components");
                        for asset in &changed.assets {
                            let label = labels
                                .get(&asset.graph)
                                .cloned()
                                .unwrap_or_else(|| format!("Asset 0x{:08X}", asset.graph));
                            if ui.button(label).clicked() {
                                edit_asset = Some(asset.graph);
                            }
                        }
                        ui.separator();
                    }
                    allocation(ui, &mut changed.graph, 0, &mut Vec::new())
                })
                .body_returned
                .transpose();
            if let (Err(error), None) = (structure, &failed) {
                failed = Some(error);
            }
            if !editing {
                return (failed, false);
            }
            // Every edit above repointed its allocation rather than writing through one that
            // may be shared, so the replaced allocations are dropped here, where no caller
            // still holds an index into the graph.
            changed.graph.compact();
            match changed.sync_assets().and_then(|()| changed.validate()) {
                // The graph holds together, so this frame's edits are kept even when one of
                // the editors above could not draw. Only a graph that fails to validate is
                // thrown away, since storing that is what would lose the whole program.
                Ok(()) => (failed, true),
                Err(error) => (failed.or(Some(error)), false),
            }
        })
        .inner;
    // A locked card is a reading. Writing the round trip back turned any value that
    // normalises into an edit the reader never made.
    let (failure, commit) = result;
    if commit {
        *program = changed;
    }
    if let Some(error) = failure {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    edit_asset.and_then(|tag| program.assets.iter().position(|asset| asset.graph == tag))
}

pub(super) fn draw(ui: &mut egui::Ui, id: &str, node: &mut NativeNode, family: NativeFamily) {
    let Some(entry) = family.catalog(node.kind) else {
        return;
    };
    let result = Graph::read(&node.bytes, 0, entry.class).and_then(|mut graph| {
        ui.push_id(id, |ui| {
            behavior::draw_node(ui, &mut graph, 0)?;
            egui::CollapsingHeader::new("Advanced")
                .show(ui, |ui| allocation(ui, &mut graph, 0, &mut Vec::new()))
                .body_returned
                .transpose()?;
            Ok::<_, String>(())
        })
        .inner?;
        graph.validate_node(family == NativeFamily::Condition, node.kind)?;
        graph.emit()
    });
    match result {
        Ok(bytes) => node.bytes = bytes,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
}

fn allocation(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    if ancestors.contains(&index) || ancestors.len() >= 64 {
        return Err("The native structure contains a cycle or nests too deeply.".into());
    }
    ancestors.push(index);
    let class = graph.blocks[index].class;
    if class == 0 {
        let bytes = &mut graph.blocks[index].bytes;
        let mut path = String::from_utf8(bytes[..bytes.len() - 1].to_vec())
            .map_err(|_| "Invalid native path.")?;
        if ui.text_edit_singleline(&mut path).changed() && !path.contains('\0') {
            *bytes = path.into_bytes();
            bytes.push(0);
        }
    } else if matches!(class, 0x808093F5 | 0x808093F6 | 0x8080407B) {
        ui.weak("Compiled from this program when it is built.");
    } else {
        let count = graph.blocks[index].count;
        if let Some(count) = count {
            ui.horizontal(|ui| {
                ui.label(format!("{count} Entries"));
                if ui
                    .add_enabled(count < 256, egui::Button::new("Add Entry"))
                    .clicked()
                {
                    graph.resize_array(index, count + 1)?;
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Remove Last Entry"))
                    .clicked()
                {
                    graph.resize_array(index, count.saturating_sub(1))?;
                }
                Ok::<_, String>(())
            })
            .inner?;
        }
        let stride = schema::record(class)?.size;
        for row in 0..graph.blocks[index].count.unwrap_or(1) {
            if count.is_some() {
                egui::CollapsingHeader::new(format!("{} {}", fields::name(class), row + 1))
                    .id_salt((index, row))
                    .show(ui, |ui| record(ui, graph, index, row, stride, ancestors))
                    .body_returned
                    .transpose()?;
            } else {
                record(ui, graph, index, row, stride, ancestors)?;
            }
        }
    }
    egui::CollapsingHeader::new("Native Bytes")
        .id_salt((index, "native-bytes"))
        .show(ui, |ui| {
            for (row, bytes) in graph.blocks[index].bytes.chunks(16).enumerate() {
                ui.monospace(format!("{:04X}  {}", row * 16, hex(bytes)));
            }
        });
    ancestors.pop();
    Ok(())
}

fn record(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    row: usize,
    stride: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    let views = schema::inline(class)?;
    for (at, child, _) in &views {
        if *child == native::value::CLASS {
            egui::CollapsingHeader::new(format!("Value Program +0x{at:02X}"))
                .id_salt((index, row, at, "value"))
                .show(ui, |ui| value(ui, graph, index, row * stride + at))
                .body_returned
                .transpose()?;
        }
    }
    for field in fields::describe(class)? {
        let in_program = views.iter().any(|(at, child, _)| {
            *child == native::value::CLASS && (field.offset >= *at && field.offset < at + 48)
        });
        let generated_predicate = views.iter().any(|(at, child, _)| {
            *child == native::labels::PREDICATE_CLASS
                && field.offset >= *at
                && field.offset < at + 16
        });
        let added_mask = matches!(class, 0x80803E1A) && (0xA8..0xD0).contains(&field.offset)
            || matches!(class, 0x8080281C) && (0x78..0xA0).contains(&field.offset);
        if in_program || generated_predicate || added_mask {
            continue;
        }
        let at = row * stride + field.offset;
        ui.push_id((index, row, field.offset), |ui| {
            if field.format == Format::Pointer {
                pointer(ui, graph, index, at, field.offset, ancestors)
            } else {
                // The selector alone names the ability, whatever the flag and option bytes
                // hold (see `action::roles`), so the picker is not gated on them.
                let component_target = field.editable && class == 0x80803E4D && field.offset == 2;
                super::super::properties::field(
                    ui,
                    &field.label,
                    fields::contract(class, &field).description,
                    |ui| {
                        if component_target {
                            let target = graph.blocks[index]
                                .bytes
                                .get_mut(at)
                                .ok_or("Ability target extends past the component record")?;
                            ability_target(ui, target);
                            return Ok(());
                        }
                        scalar(ui, &field, &mut graph.blocks[index], row)
                    },
                )
            }
        })
        .inner?;
    }
    Ok(())
}

/// The label a field shows in the guided editor: the traced name, or the plain word where
/// the field's values are named. A field named by its values reads by what it selects, so
/// "Event Byte" with Aiming Started and Aiming Stopped reads "Event", and the general
/// predicate's key with Charged with Light behind it reads "State".
pub(super) fn plain_field_label(class: u32, label: &str) -> &'static str {
    match (class, label) {
        (0x80803DDA | 0x80803DF9 | 0x808029E6, "Event Byte") => "Event",
        (0x80803E01 | 0x80803DFD, "Slot Mask") | (0x80803E00, "Selected Bits") => "Ability",
        (0x80803DFB, "Second Flag Mask") => "Ammo Type",
        (0x80803DFB, "First Flag Mask") => "Pickup Flags",
        (0x808029E0, "Mode") => "Shots",
        (0x80803E41, "Mode") => "Damage Type",
        (0x80803DCE | 0x80803DCC, "Named Key") => "State",
        (0x80803DCE | 0x80803DCC, "Minimum Value") => "At Least",
        (0x80803DCE | 0x80803DCC, "Maximum Value") => "At Most",
        (0x80803DE5, "Minimum Value") => "At Least",
        (0x80803DE5, "Maximum Value") => "At Most",
        (0x80803E44, "Input Source") | (0x80803E4D, "Input Selector") => "Value Source",
        (0x80803E4D, "Scale") => "Multiplier",
        (0x80803E4D, "Limit") => "Stop At",
        (0x80803E42, "Count") => "Orbs",
        (0x80803E42, "Spawn Position") => "Position",
        (0x80803E30, "Trigger Threshold") => "Count Needed",
        (0x80803E30, "Reset Threshold") => "Resets At",
        (0x80803E30, "Minimum Value") => "Lowest Count",
        (0x80803E30, "Maximum Value") => "Highest Count",
        (_, "Value Threshold") => "Required Value",
        (_, "Extend By") => "Added Time",
        (_, "Probability Source") => "Chance Source",
        (0x80803DEA, "Event Value") => "Event",
        (0x80803DEA, "Context Key") => "Context",
        (0x80803DEC | 0x80803DEB, "Event Key") => "Signal",
        (0x80803E1C, "Replacement Key") => "Firing Mode",
        (0x808029ED | 0x80803E1D, "Property Key") => "Property",
        (0x80803E1D, "Target Selector") => "Ability",
        (0x80803E45, "First Key") => "Effect",
        (0x80803E39, "Property Key") => "Counter",
        (0x80803E43, "Position Selector") => "Position",
        (_, "Hold Duration") => "Condition Hold",
        (_, "Up To") => "Extension Limit",
        (_, "Storage Path") => "Destination",
        (_, "Owning Slot Amount") => "This Weapon",
        (_, "Slot 1 Amount") => "Weapon Slot 1",
        (_, "Slot 2 Amount") => "Weapon Slot 2",
        (_, "Slot 3 Amount") => "Weapon Slot 3",
        (_, "Category 1 Amount") => "Ammo Type 1",
        (_, "Category 2 Amount") => "Ammo Type 2",
        (_, "Category 3 Amount") => "Ammo Type 3",
        _ => leak_label(label),
    }
}

/// Labels come from the schema as owned strings. The few that reach here are the fixed
/// vocabulary of `fields::describe`, so interning them once is bounded.
fn leak_label(label: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static INTERNED: OnceLock<Mutex<std::collections::BTreeMap<String, &'static str>>> =
        OnceLock::new();
    let mut map = INTERNED
        .get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(interned) = map.get(label) {
        return interned;
    }
    let interned: &'static str = Box::leak(label.to_owned().into_boxed_str());
    map.insert(label.to_owned(), interned);
    interned
}

fn ability_target(ui: &mut egui::Ui, target: &mut u8) {
    use sundial::package_authoring::sandbox_perk::action::component_target;
    let current = component_target(*target, 0, 0).unwrap_or("Unmapped Target");
    let evidence = "Source-derived from Innervation and Bomber, Invigoration and Outreach, Insulation and Perpetuation, and for Super from Ashes to Assets, Hands-On and Heavy Lifting. The numeric selector remains editable.";
    let hover = format!("{current}\n{evidence}");
    // Four named abilities and an unmapped selector are a short vocabulary, and a combo
    // grows to its selected text unless something bounds it, so it keeps a narrow column.
    sized(ui, NARROW_COLUMN, |ui| {
        egui::ComboBox::from_id_salt("ability-target")
            .width(ui.available_width())
            .truncate()
            .selected_text(current)
            .show_ui(ui, |ui| {
                for selector in [0, 1, 2, 7] {
                    ui.selectable_value(
                        target,
                        selector,
                        component_target(selector, 0, 0).unwrap(),
                    );
                }
                ui.selectable_value(target, 255, "Other Target…");
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(ui, "ability-target", "Ability Target");
    });
    if component_target(*target, 0, 0).is_none() {
        ui.add(egui::DragValue::new(target).range(0..=255))
            .on_hover_text("Native target selector. Other values have no identified ability role.");
    }
}

fn pointer(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    at: usize,
    field: usize,
    ancestors: &mut Vec<usize>,
) -> Result<(), String> {
    let class = graph.blocks[index].class;
    if class == 0x808040B5 && field == 0xB0 {
        ui.weak("Group routing is compiled from the condition lists.");
        return Ok(());
    }
    let target = graph.blocks[index].links.get(&at).copied();
    let name = reference_name(graph, index, field, target)?;
    egui::CollapsingHeader::new(format!("{name} +0x{field:02X}"))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut choices = schema::choices(class, field);
                if let Some((_, code)) = schema::record(class)?
                    .fields
                    .iter()
                    .find(|(offset, _)| *offset == field)
                {
                    if matches!(code, 1 | 2) {
                        choices = vec![(0, false)];
                    }
                }
                ui.menu_button("Choose Reference", |ui| {
                    for (class, array) in choices {
                        if ui.button(fields::name(class)).clicked() {
                            graph.create_target(index, at, class, array)?;
                            ui.close_menu();
                        }
                    }
                    Ok::<_, String>(())
                })
                .inner
                .transpose()?;
                if let Some(target) = target
                    && ui.button("Clear Reference").clicked()
                {
                    if graph.blocks[target].count.is_some() {
                        graph.blocks[index].bytes[at - 8..at].fill(0);
                    }
                    graph.blocks[index].links.remove(&at);
                }
                Ok::<_, String>(())
            })
            .inner?;
            if let Some(target) = graph.blocks[index].links.get(&at).copied() {
                allocation(ui, graph, target, ancestors)?;
            } else {
                ui.weak("No reference selected.");
            }
            Ok::<_, String>(())
        })
        .body_returned
        .transpose()?;
    Ok(())
}

fn reference_name(
    graph: &Graph,
    index: usize,
    field: usize,
    target: Option<usize>,
) -> Result<String, String> {
    let class = graph.blocks[index].class;
    let mut name = target.map_or_else(
        || "Reference".into(),
        |i| fields::name(graph.blocks[i].class),
    );
    let group_field = if class == 0x808040B5 && (0x20..0x68).contains(&field) {
        Some(field - 0x20)
    } else if class == 0x8080407D {
        Some(field)
    } else {
        None
    };
    if let Some(label) = match group_field {
        Some(0x08) => Some("Starts When"),
        Some(0x20) => Some("Effects"),
        Some(0x30) => Some("Ends When"),
        Some(0x40) => Some("Ready Again When"),
        _ => None,
    } {
        name = label.into();
    }
    if class == 0x80802F16 {
        match field {
            0x128 => name = "Assignments".into(),
            0x138 => name = "Multipliers".into(),
            _ => {}
        }
    }
    if class == 0x808040B5 {
        match field {
            0x18 => name = "Auxiliary Records".into(),
            0x70 => name = "Additional Groups".into(),
            0x78 => name = "Execution Policy Settings".into(),
            _ => {}
        }
    }
    for (source, kind, _) in schema::inline(class)? {
        if kind == native::labels::SOURCE_CLASS
            && field >= source + 8
            && (field - source - 8) % 16 == 0
            && field < source + 64
        {
            name = [
                "Any Labels",
                "All Labels",
                "Excluded Labels",
                "Not All Labels",
            ][(field - source - 8) / 16]
                .into();
        }
    }
    Ok(name)
}

/// Use the same named native choices for typed actions and complete native records.
pub(super) fn byte_field(ui: &mut egui::Ui, class: u32, offset: usize, value: &mut u8) {
    let field = fields::describe(class)
        .expect("typed action native schema")
        .into_iter()
        .find(|field| field.offset == offset && field.format == Format::Byte)
        .expect("typed action byte field");
    let contract = fields::contract(class, &field);
    ui.push_id((class, offset), |ui| {
        properties::field(
            ui,
            plain_field_label(class, &field.label),
            contract.description,
            |ui| {
                *value = u8::try_from(selector(ui, class, &field, &contract, u32::from(*value)))
                    .expect("byte selector is bounded to 255");
            },
        );
    });
}

pub(super) fn ability_property(ui: &mut egui::Ui, slot: u8, value: &mut u32) {
    properties::field(
        ui,
        "Property",
        "The property must be defined by this ability. Hover a choice for its supported abilities.",
        |ui| {
            key_control(
                ui,
                "Property",
                "An ability-bank property. Its behavior depends on the selected ability.",
                fields::keys::ability_properties(slot),
                value,
            );
        },
    );
}

fn key_control(
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
    known: &[fields::keys::EventKey],
    value: &mut u32,
) {
    if known.is_empty() {
        let raw = hex_key(ui, "native-event-key-hex", value);
        pickers::name_response(ui, &raw, label);
        return;
    }
    let reading = fields::keys::name(*value).map_or_else(
        || {
            // The FNV-1 basis is the hash of an empty name. Keep its exact value on read.
            if matches!(*value, 0 | 0x811C9DC5) {
                "None".to_owned()
            } else {
                format!("0x{value:08X}")
            }
        },
        str::to_owned,
    );
    let evidence = known
        .iter()
        .find(|key| key.hash == *value)
        .or_else(|| fields::keys::entry(*value))
        .map_or(description, |key| key.evidence);
    let hover = format!("{reading}\n{evidence}");
    sized(ui, EVIDENCE_WIDTH, |ui| {
        egui::ComboBox::from_id_salt("native-event-key")
            .width(EVIDENCE_WIDTH)
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                for key in known {
                    ui.selectable_value(value, key.hash, key.name)
                        .on_hover_text(key.evidence);
                }
                egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                    let raw = hex_key(ui, "native-event-key-hex", value);
                    pickers::name_response(ui, &raw, "Key as Hex");
                });
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(ui, "native-event-key", label);
    });
}

fn selector(
    ui: &mut egui::Ui,
    class: u32,
    field: &fields::Field,
    contract: &fields::ValueContract,
    before: u32,
) -> u32 {
    let mut selected = before;
    // Preserve unnamed stock choices alongside the recovered names.
    let unnamed = contract
        .observed
        .iter()
        .filter(|(value, _, _)| !contract.choices.iter().any(|(named, _)| named == value))
        .collect::<Vec<_>>();
    let stock = |value: u32| {
        contract
            .observed
            .iter()
            .find(|(candidate, _, _)| u32::from(*candidate) == value)
            .map(|(_, count, _)| *count)
    };
    let reading: String = contract
        .choices
        .iter()
        .find(|(value, _)| u32::from(*value) == selected)
        .map_or_else(
            || match stock(selected) {
                Some(count) => format!("{selected} · {count} stock perks"),
                // Zero is the template's own value and selects nothing named.
                // Any other unnamed value is shown with what is known about
                // it, which is only that no stock perk sets it.
                None if selected == 0 => "Not Set".to_owned(),
                None => format!("{selected} · not used by stock perks"),
            },
            |(_, name)| (*name).into(),
        );
    let hover = format!("{reading}\n{}", contract.description);
    // A value's name can be a sentence's worth of words, and a combo takes the width
    // of its selected text, so it is held to the shared value column with the whole
    // reading and the field's description on hover.
    column(ui, |ui| {
        egui::ComboBox::from_id_salt("native-value-choice")
            .width(COLUMN_WIDTH)
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                for (value, name) in contract.choices {
                    ui.selectable_value(&mut selected, u32::from(*value), *name)
                        .on_hover_text(match stock(u32::from(*value)) {
                            Some(count) => format!("{count} stock perks set this value."),
                            None => "No stock perk sets this value.".to_owned(),
                        });
                }
                for (value, count, perks) in unnamed {
                    ui.selectable_value(
                        &mut selected,
                        u32::from(*value),
                        format!("{value} · {count} stock perks"),
                    )
                    .on_hover_text(if perks.is_empty() {
                        "What this value selects is not established. No named stock perk sets it."
                            .to_owned()
                    } else {
                        format!("What this value selects is not established. Set by {perks}.")
                    });
                }
                egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Native Value");
                        let mut drag = egui::DragValue::new(&mut selected);
                        if field.width == 1 {
                            drag = drag.range(0..=255);
                        }
                        ui.add(drag);
                    });
                });
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(
            ui,
            "native-value-choice",
            plain_field_label(class, &field.label),
        );
    });
    selected
}

fn scalar(
    ui: &mut egui::Ui,
    field: &fields::Field,
    block: &mut native::Block,
    row: usize,
) -> Result<(), String> {
    let bytes = field
        .bytes(block, row)
        .ok_or("Truncated native field.")?
        .to_vec();
    if !field.editable {
        ui.weak(hex(&bytes));
        return Ok(());
    }
    let contract = fields::contract(block.class, field);
    if block.class == 0x808094B3 && field.offset == 0 {
        let mut value = u32::from_le_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid label width.")?,
        );
        let before = value;
        behavior::labels::draw_single(ui, &mut value);
        if value != before {
            field.write(block, row, &value.to_le_bytes())?;
        }
        return Ok(());
    }
    if contract.bitmask {
        return masks::draw(ui, field, block, row, &contract);
    }
    // A selector with no recovered names still has the values the game itself sets. Offering
    // those keeps the control a choice rather than a blind 0 to 255 spinner, and says plainly
    // that a count is evidence of use and not of meaning.
    if field.format == Format::Byte && contract.choices.is_empty() && !contract.observed.is_empty()
    {
        let mut selected = bytes[0];
        let stock = |value: u8| {
            contract
                .observed
                .iter()
                .find(|(candidate, _, _)| *candidate == value)
                .map(|(_, count, _)| *count)
        };
        let reading = match stock(selected) {
            Some(count) => format!("{selected} · {count} stock perks"),
            None => format!("{selected} · not used by stock perks"),
        };
        let hover = format!(
            "{reading}\n{} What this byte selects is not established. The listed values are the ones the game's own perks set, so they are the values the engine is known to accept here.",
            contract.description
        );
        // `width` is only a floor, so the evidence beside the value would otherwise push
        // the control across the pane. The allocation bounds it and the full reading and
        // the field's own description stay on hover.
        sized(ui, EVIDENCE_WIDTH, |ui| {
            egui::ComboBox::from_id_salt("native-observed-value")
                .selected_text(reading)
                .width(EVIDENCE_WIDTH)
                .truncate()
                .show_ui(ui, |ui| {
                    for (value, count, perks) in contract.observed {
                        ui.selectable_value(
                            &mut selected,
                            *value,
                            format!("{value} · {count} stock perks"),
                        )
                        .on_hover_text(if perks.is_empty() {
                            "No named stock perk sets this value.".to_owned()
                        } else {
                            format!("Set by {perks}.")
                        });
                    }
                    egui::CollapsingHeader::new("Advanced").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Native Value");
                            ui.add(egui::DragValue::new(&mut selected).range(0..=255));
                        });
                    });
                })
                .response
                .on_hover_text(hover);
            pickers::name_combo(
                ui,
                "native-observed-value",
                plain_field_label(block.class, &field.label),
            );
        });
        if selected != bytes[0] {
            field.write(block, row, &[selected])?;
        }
        return Ok(());
    }
    // A byte selector or a 32-bit mask whose values are named. Both read as one number here,
    // and the write goes back at the field's own width.
    if matches!(field.format, Format::Byte | Format::Mask32) && !contract.choices.is_empty() {
        let before = if bytes.len() == 1 {
            u32::from(bytes[0])
        } else {
            u32::from_le_bytes(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid native mask width.")?,
            )
        };
        let selected = selector(ui, block.class, field, &contract, before);
        if selected != before {
            if bytes.len() == 1 {
                let value =
                    u8::try_from(selected).map_err(|_| "A byte selector holds 0 to 255.")?;
                field.write(block, row, &[value])?;
            } else {
                field.write(block, row, &selected.to_le_bytes())?;
            }
        }
        return Ok(());
    }
    // A key whose stock values are named is chosen by name, with the hex value kept under
    // Advanced for any other key.
    if field.format == Format::Key {
        let known = if block.class == 0x80803E1D && field.offset == 4 {
            let stride = schema::record(block.class)?.size;
            let slot = *block
                .bytes
                .get(row * stride + 2)
                .ok_or("Missing ability slot.")?;
            fields::keys::ability_properties(slot)
        } else {
            fields::keys::known(block.class, field.offset)
        };
        if !known.is_empty() {
            let mut value = u32::from_le_bytes(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid native key width.")?,
            );
            let before = value;
            key_control(
                ui,
                plain_field_label(block.class, &field.label),
                contract.description,
                known,
                &mut value,
            );
            if value != before {
                field.write(block, row, &value.to_le_bytes())?;
            }
            return Ok(());
        }
    }
    let mapped = nodes::CONDITIONS
        .iter()
        .find(|entry| entry.class == block.class)
        .and_then(|entry| layout::condition_layout(entry.kind))
        .or_else(|| {
            nodes::EFFECTS
                .iter()
                .find(|entry| entry.class == block.class)
                .and_then(|entry| layout::effect_layout(entry.kind))
        })
        .and_then(|layout| {
            layout.fields.iter().find(|mapped| {
                mapped.offset == field.offset && mapped.format.width() == field.width
            })
        });
    if let Some(mapped) = mapped {
        let start = row * schema::record(block.class)?.size;
        super::draw_native_field(ui, mapped, &mut block.bytes[start..]);
        return Ok(());
    }
    // The visible label sits beside the control without being linked to it, so each control
    // is given that label as its accessible name here.
    let name = plain_field_label(block.class, &field.label);
    let changed = match field.format {
        Format::Flag => {
            let mut v = bytes[0] != 0;
            let response = ui.checkbox(&mut v, "");
            pickers::name_response(ui, &response, name);
            response.changed().then(|| vec![u8::from(v)])
        }
        Format::Float => {
            let mut bits = u32::from_le_bytes(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid float field.")?,
            );
            let before = bits;
            let response = float_field(ui, &mut bits);
            pickers::name_response(ui, &response, name);
            if block.class == 0x80803E4D && field.offset == 12 && f32::from_bits(bits) < 0.0 {
                ui.weak("No Limit");
            }
            if !contract.suffix.is_empty() {
                ui.label(contract.suffix.trim());
            }
            (bits != before).then(|| bits.to_le_bytes().to_vec())
        }
        Format::Key | Format::Tag | Format::Mask32 => {
            let mut value = u32::from_le_bytes(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid native key width.")?,
            );
            let before = value;
            let response = hex_key(ui, "native-key", &mut value);
            pickers::name_response(ui, &response, name);
            (value != before).then(|| value.to_le_bytes().to_vec())
        }
        Format::Byte => {
            let mut v = bytes[0];
            let response = ui.add(egui::DragValue::new(&mut v));
            pickers::name_response(ui, &response, name);
            response.changed().then(|| vec![v])
        }
        Format::Integer => {
            let mut value =
                i32::from_le_bytes(bytes.try_into().map_err(|_| "Invalid integer field.")?);
            let response = ui.add(egui::DragValue::new(&mut value));
            pickers::name_response(ui, &response, name);
            response.changed().then(|| value.to_le_bytes().to_vec())
        }
        Format::Unsigned => {
            let mut value = u32::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| "Invalid unsigned integer field.")?,
            );
            let response = ui.add(egui::DragValue::new(&mut value));
            pickers::name_response(ui, &response, name);
            response.changed().then(|| value.to_le_bytes().to_vec())
        }
        _ => hex_input(ui, &bytes, false),
    };
    if let Some(bytes) = changed {
        field.write(block, row, &bytes)?;
    }
    Ok(())
}

fn value(ui: &mut egui::Ui, graph: &mut Graph, index: usize, at: usize) -> Result<(), String> {
    let mut program = native::value::Program::read(graph, index, at)?;
    let original = program.clone();
    let mut fast_path = program.fast_path == 1;
    if ui
        .checkbox(&mut fast_path, "Polynomial Fast Path")
        .on_hover_text("This mode uses constant vector zero to evaluate a clamped cubic.")
        .changed()
    {
        program.fast_path = u32::from(fast_path);
    }
    for (row, constant) in program.constants.iter_mut().enumerate() {
        super::super::properties::field(
            ui,
            &format!("Constant {row}"),
            "Stored four-lane constant used by this value program.",
            |ui| {
                for lane in constant {
                    let mut v = f32::from_bits(*lane);
                    if ui.add(egui::DragValue::new(&mut v).speed(0.01)).changed() && v.is_finite() {
                        *lane = v.to_bits();
                    }
                }
            },
        );
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                program.constants.len() < 256,
                egui::Button::new("Add Constant"),
            )
            .clicked()
        {
            program.constants.push([0; 4]);
        }
        if ui
            .add_enabled(
                !program.constants.is_empty(),
                egui::Button::new("Remove Last Constant"),
            )
            .clicked()
        {
            program.constants.pop();
        }
    });
    ui.small("Instructions use hexadecimal opcode and operand bytes.");
    let code = program
        .instructions
        .iter()
        .flat_map(|i| std::iter::once(i.opcode).chain(i.operand))
        .collect::<Vec<_>>();
    if let Some(bytes) = hex_input(ui, &code, true) {
        let mut instructions = Vec::new();
        let mut p = 0;
        while p < bytes.len() {
            let opcode = bytes[p];
            p += 1;
            let operand = if opcode == 34 || opcode >= 52 {
                let operand = *bytes.get(p).ok_or("Instruction needs an operand.")?;
                p += 1;
                Some(operand)
            } else {
                None
            };
            instructions.push(native::value::Instruction { opcode, operand });
        }
        program.instructions = instructions;
        program.fast_path = 0;
    }
    for instruction in &program.instructions {
        ui.weak(native::value::instruction_name(instruction.opcode));
    }
    if program != original {
        program.write(graph, index, at)?;
    }
    Ok(())
}

fn hex_input(ui: &mut egui::Ui, bytes: &[u8], multiline: bool) -> Option<Vec<u8>> {
    let id = ui.id().with("native-hex-input");
    let mut state = ui
        .data(|data| data.get_temp::<(Vec<u8>, String)>(id))
        .filter(|state| state.0 == bytes)
        .unwrap_or_else(|| (bytes.to_vec(), hex(bytes)));
    if multiline {
        ui.text_edit_multiline(&mut state.1);
    } else {
        ui.add(egui::TextEdit::singleline(&mut state.1).desired_width(120.0));
    }
    let mut result = None;
    if ui.button("Apply Bytes").clicked() {
        match parse_hex(&state.1) {
            Ok(value) => {
                result = Some(value);
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    result
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.split_whitespace().collect::<String>();
    if !digits.is_ascii() || digits.len() % 2 != 0 {
        return Err("Enter complete hexadecimal bytes.".into());
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| "Enter hexadecimal bytes.".into())
        })
        .collect()
}
