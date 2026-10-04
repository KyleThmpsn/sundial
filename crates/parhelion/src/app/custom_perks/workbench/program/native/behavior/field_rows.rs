//! A node's fields as rows and tiles, and which of them lead on its card.
use super::*;

pub(super) fn row_controls(
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
                let control = super::super::tile_width(list.class, field, width).unwrap_or(width);
                crate::app::style::tile(
                    ui,
                    width,
                    ("contribution", row, field.offset),
                    super::super::plain_field_label(list.class, &field.label),
                    fields::contract(list.class, field).description,
                    false,
                    |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().interact_size.x = control;
                            ui.spacing_mut().combo_width = control;
                            super::super::scalar(ui, field, &mut graph.blocks[index], row)
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
pub(super) fn contribution_reading(
    graph: &mut Graph,
    list: &List,
    row: usize,
) -> Result<String, String> {
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
pub(super) fn details(
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
pub(super) fn nested(ui: &mut egui::Ui, graph: &mut Graph, parent: usize) -> Result<(), String> {
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
            let context = super::super::reference_name(graph, parent, at % stride, Some(child))?;
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
                                    super::super::super::super::properties::field(
                                        ui,
                                        super::super::plain_field_label(class, &field.label),
                                        fields::contract(class, field).description,
                                        |ui| {
                                            super::super::scalar(
                                                ui,
                                                field,
                                                &mut graph.blocks[child],
                                                row,
                                            )
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
pub(super) enum FieldView {
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
fn compiler_owned(class: u32, offset: usize) -> bool {
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
                    .button(super::super::plain_field_label(block.class, &field.label))
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
    if super::super::orb_entity(block.class, field) {
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
                            super::super::scalar(
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
pub(super) fn chance_leads(
    block: &native::Block,
    view: FieldView,
    demotes: bool,
    trigger: bool,
) -> bool {
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
pub(super) fn chance(ui: &mut egui::Ui, width: f32, bytes: &mut [u8]) {
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

pub(super) fn controls(
    ui: &mut egui::Ui,
    graph: &mut Graph,
    index: usize,
    view: FieldView,
    demotes: bool,
    trigger: bool,
) -> Result<(), String> {
    controls_with_trail(ui, graph, index, view, demotes, trigger, None)
}

/// A tile of the caller's, drawn after a node's own at the width it is given.
type Trail<'a> = &'a mut dyn FnMut(&mut egui::Ui, f32);

/// `controls` with a tile of the caller's drawn after the node's own, on the same line.
pub(super) fn controls_with_trail(
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
    trail: Option<Trail<'_>>,
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
        (ability && field.offset == 2) || super::super::tile_width(class, field, 0.0).is_some()
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
                super::super::plain_field_label(class, &field.label)
            };
            let hint = fields::contract(class, field).description;
            let content = |ui: &mut egui::Ui| {
                if ability && field.offset == 2 {
                    super::super::ability_target(ui, &mut graph.blocks[index].bytes[2]);
                    Ok(())
                } else {
                    super::super::scalar(ui, field, &mut graph.blocks[index], 0)
                }
            };
            // The cell scopes its own widgets. A scope around it in the wrapping line would
            // place it at the cursor without wrapping, and squeeze every row after the first.
            let drawn = if wide {
                cell(ui, field.offset, label, hint, content)
            } else {
                ui.push_id(field.offset, |ui| {
                    super::super::super::super::properties::field(ui, label, hint, content)
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
    if chance_tile || !tiled.is_empty() || trail.is_some() {
        crate::app::style::tiles(ui, |ui, width| {
            if chance_tile {
                chance(ui, width, &mut graph.blocks[index].bytes);
            }
            for field in &tiled {
                let target = ability && field.offset == 2;
                let label = if target {
                    "Ability"
                } else {
                    super::super::plain_field_label(class, &field.label)
                };
                let hint = fields::contract(class, field).description;
                let control = super::super::tile_width(class, field, width).unwrap_or(width);
                let (drawn, _) =
                    crate::app::style::tile(ui, width, field.offset, label, hint, false, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().interact_size.x = control;
                            ui.spacing_mut().combo_width = control;
                            if target {
                                super::super::ability_target(ui, &mut graph.blocks[index].bytes[2]);
                                Ok(())
                            } else {
                                super::super::scalar(ui, field, &mut graph.blocks[index], 0)
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
            if let Some(trail) = trail {
                trail(ui, width);
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

fn visible(field: &fields::Field, class: u32) -> bool {
    field.editable
        && (!matches!(field.format, Format::Bytes | Format::Pointer | Format::Tag)
            || super::super::orb_entity(class, field)
            || super::super::ability_reference(class, field))
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
                .show(ui, |ui| super::super::value(ui, graph, index, offset))
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
