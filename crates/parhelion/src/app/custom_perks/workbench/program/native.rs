//! Complete native records in the private-perk editor.
use super::*;
use crate::app::custom_perks::workbench::controls::{column, sized};
#[cfg(test)]
use sundial::package_authoring::sandbox_perk::action::native::NodeKind as NativeNodeKind;
use sundial::package_authoring::sandbox_perk::action::native::{
    self, Graph,
    fields::{self, Format},
    schema,
};

mod behavior;
mod masks;
mod structure;
pub(in crate::app::custom_perks::workbench) mod tunings;

pub(in crate::app::custom_perks::workbench) fn insert_catalog_node(
    program: &mut Program,
    group: usize,
    placement: super::super::catalog_insert::Placement,
    node: NativeNode,
) -> Result<(), String> {
    use super::super::catalog_insert::Placement;
    if !program.native_asset_patches.is_empty() {
        return Err("This effect has private resource patches. Finish its native conversion before inserting a catalog node.".into());
    }
    let mut native = sundial::package_authoring::sandbox_perk::program::native_draft(program)?;
    let decoded = sundial::package_authoring::sandbox_perk::action::decode(&native.graph.emit()?)?;
    let behavior = decoded
        .groups
        .get(group)
        .ok_or("The destination behavior no longer exists.")?;
    let (part, edit) = match placement {
        Placement::Action => (structure::Part::Actions, structure::Edit::AddVerbatim(node)),
        Placement::Requirement => (structure::Part::Trigger, structure::Edit::Require(node)),
        Placement::Trigger => (
            structure::Part::Trigger,
            if behavior.activation.is_empty() {
                structure::Edit::Add(node)
            } else {
                structure::Edit::Replace(0, node)
            },
        ),
    };
    structure::List::group(group, part).edit(&mut native.graph, edit)?;
    native.sync_assets()?;
    native.validate()?;
    let changed = program.with_native(native);
    changed.validate_structure()?;
    *program = changed;
    Ok(())
}

pub(in crate::app::custom_perks::workbench) fn reveal(
    ctx: &egui::Context,
    issue: sundial::package_authoring::sandbox_perk::program::NativeIssue,
) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("native-problem-target"), Some(issue)));
}

pub(in crate::app::custom_perks::workbench) fn label_choices(
    ctx: &egui::Context,
    registry: Option<Arc<sundial::investment::discovery::labels::Registry>>,
    error: Option<String>,
) {
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new("installed-label-registry"), (registry, error));
    });
}

/// The first choice a program still needs, naming the field and its node: a key or tag left
/// at zero, or a selection mask left empty (`fields::unset`). The compiler accepts the node,
/// but it never matches or never acts, as a counter with nothing to count never fires.
pub(in crate::app::custom_perks::workbench) fn unset_choice(graph: &Graph) -> Option<String> {
    let graph = Graph::read(
        &graph.emit().ok()?,
        0,
        sundial::package_authoring::sandbox_perk::action::ACTION_ROOT_CLASS,
    )
    .ok()?;
    graph
        .blocks
        .iter()
        .filter(|block| block.class != 0)
        .find_map(|block| {
            let described = fields::describe(block.class).ok()?;
            let rows = block.count.unwrap_or(1);
            let stride = block.bytes.len() / rows.max(1);
            let field = (0..rows).find_map(|row| {
                described.iter().find(|field| {
                    let at = row * stride + field.offset;
                    block
                        .bytes
                        .get(at..at + field.width)
                        .is_some_and(|bytes| fields::unset(block.class, field, bytes))
                })
            })?;
            Some(format!(
                "Choose the {} for {}.",
                plain_field_label(block.class, &field.label),
                node_title(block)
            ))
        })
}

/// A node's plain name from its class alone, for messages about one of its fields.
fn node_title(block: &native::Block) -> String {
    if let Some(node) = nodes::CONDITIONS
        .iter()
        .find(|node| node.class == block.class)
    {
        return nodes::condition_title(node.kind).to_owned();
    }
    if let Some(node) = nodes::EFFECTS.iter().find(|node| node.class == block.class) {
        return native_action_label(node.kind, &block.bytes);
    }
    fields::name(block.class)
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
    // The effect menu shows the native structure. It is the view of last resort, so it waits
    // there rather than as a fold under every card.
    structure: bool,
    labels: &BTreeMap<u32, String>,
    pick: &mut NativePicker<'_>,
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
            match behavior::draw(ui, &mut changed.graph, &mut changed.assets, pick) {
                Ok(asset) => edit_asset = asset,
                Err(error) => failed = Some(error),
            }
            let structure = structure
                .then(|| {
                    ui.separator();
                    ui.strong("Native Structure");
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

/// Asks the effect list to move a behavior group into an effect of its own, since that needs
/// the whole perk. The list reads the request back after drawing the card that made it.
pub(in crate::app::custom_perks::workbench) const MOVE_GROUP: &str = "move-behavior-group";

/// Moves behavior group `group` of effect `effect` into a new effect `index` placed after it,
/// as that effect's main behavior. Stock perks start their extra groups only when always
/// active or from the main behavior's own script, never on an event, and a kill-started extra
/// group never fired in game, so an event-started behavior needs a main behavior of its own.
pub(in crate::app::custom_perks::workbench) fn move_group(
    recipe: &mut crate::perk::PerkRecipe,
    effect: u16,
    group: usize,
    index: u16,
) -> Result<(), String> {
    let position = recipe
        .effects
        .iter()
        .position(|candidate| candidate.source_perk_index == effect)
        .ok_or("This effect is no longer in the perk.")?;
    let program = recipe.effects[position]
        .program
        .as_mut()
        .ok_or("Only an authored effect can move a behavior group.")?;
    let name = program.name.clone();
    let source = program
        .native
        .as_mut()
        .ok_or("Only an effect with behavior groups can move one.")?;
    if group == 0 {
        return Err("The main behavior stays with its effect.".into());
    }
    let decoded = sundial::package_authoring::sandbox_perk::action::decode(&source.graph.emit()?)?;
    let moved = decoded
        .groups
        .get(group)
        .ok_or("This behavior group no longer exists.")?;
    let mut target = sundial::package_authoring::sandbox_perk::program::NativeProgram::empty();
    // Actions first, while the new behavior has no ending for a kill's ending rule to fit, so
    // the group moves exactly as it is. Adding in execution order keeps their order.
    for node in moved.effects.iter().rev() {
        structure::List::group(0, structure::Part::Actions).edit(
            &mut target.graph,
            structure::Edit::Add(NativeNode {
                kind: node.kind,
                bytes: node.native.clone(),
            }),
        )?;
    }
    for (part, conditions) in [
        (structure::Part::Trigger, &moved.activation),
        (structure::Part::Ending, &moved.removal),
        (structure::Part::Rearm, &moved.rearm),
    ] {
        for node in conditions {
            structure::List::group(0, part).edit(
                &mut target.graph,
                structure::Edit::Add(NativeNode {
                    kind: node.kind,
                    bytes: node.native.clone(),
                }),
            )?;
        }
    }
    target.assets.clone_from(&source.assets);
    target.sync_assets()?;
    target.validate()?;
    structure::remove_group(&mut source.graph, group)?;
    source.graph.compact();
    source.sync_assets()?;
    source.validate()?;
    let mut added = super::named_effect(index);
    // The moved behavior keeps the tunings its actions apply and drops the rest.
    let mut moved_program = program.with_native(target);
    moved_program.name = format!("{name} · Behavior {}", group + 1);
    moved_program.prune_ability_tunings();
    added.program = Some(moved_program);
    program.prune_ability_tunings();
    recipe.effects.insert(position + 1, added);
    Ok(())
}

/// A separate trigger and its actions within the effect. A guided program takes its complete
/// form first, since only that form holds more than one group.
pub(in crate::app::custom_perks::workbench) fn add_behavior_group(
    program: &mut Program,
) -> Result<(), String> {
    if let Some(native) = &mut program.native {
        return structure::add_group(&mut native.graph);
    }
    program.validate()?;
    let mut native = sundial::package_authoring::sandbox_perk::program::native_draft(program)?;
    structure::add_group(&mut native.graph)?;
    *program = program.with_native(native);
    Ok(())
}

#[cfg(test)]
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
        graph.validate_node(if family == NativeFamily::Condition {
            NativeNodeKind::Condition(node.kind)
        } else {
            NativeNodeKind::Effect(node.kind)
        })?;
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
                ui.label(if count == 1 {
                    "1 Entry".to_owned()
                } else {
                    format!("{count} Entries")
                });
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
/// A hint for a number whose unit the label alone does not give, shown on the value.
fn plain_field_hint(class: u32, label: &str) -> Option<&'static str> {
    Some(match (class, label) {
        // The stock ability energy perks with a constant of 1 use 0.016 to 0.5 here, and the
        // one that scales an input by 2 caps the result at a limit of 1.
        (0x80803E4D, "Scale") => {
            "Multiplies the value. Stock energy perks use 0.016 to 0.5 here, and one uses 2 capped at 1."
        }
        (_, "Owning Slot Amount" | "Slot 1 Amount" | "Slot 2 Amount" | "Slot 3 Amount") => {
            "Share of the capacity chosen under Share Of. 1.0 is all of it."
        }
        _ => return None,
    })
}

pub(super) fn plain_field_label(class: u32, label: &str) -> &'static str {
    match (class, label) {
        (0x80803DDA | 0x80803DF9 | 0x808029E6, "Event Byte") => "Event",
        (0x80803E01 | 0x80803DFD, "Slot Mask") | (0x80803E00, "Selected Bits") => "Ability",
        (0x80803DFF | 0x80803DFE, "Referenced Resource") => "Ability",
        (0x80803DFB, "Second Flag Mask") => "Ammo Type",
        (0x80803DFB, "First Flag Mask") => "Pickup Flags",
        (0x808029E0, "Mode") => "Shots",
        (0x80803E41, "Mode") => "Damage Type",
        (0x80803DCE | 0x80803DCC, "Named Key") => "State",
        (0x80803DCE | 0x80803DCC, "Minimum Value") => "At Least",
        (0x80803DCE | 0x80803DCC, "Maximum Value") => "At Most",
        (0x80803DE5, "Minimum Value") => "At Least",
        (0x80803DE5, "Maximum Value") => "At Most",
        (0x80803E4D, "Scale") => "Multiplier",
        (0x80803E4D, "Limit") => "Stop At",
        (0x80803E42, "Count") => "Orbs",
        (0x80803E42, "Spawn Position") => "Position",
        (0x80803E30, "Trigger Threshold") => "Count Needed",
        (0x80803E30, "Reset Threshold") => "Resets At",
        (0x80803E30, "Minimum Value") => "Lowest Count",
        (0x80803E30, "Maximum Value") => "Highest Count",
        // A contributing condition's Counter Change, read as what happens when it passes or fails.
        (0x80803E32, "Success Operation") => "When It Passes",
        (0x80803E32, "Success Value") => "Pass Amount",
        (0x80803E32, "Success Uses Event Value") => "Pass Uses the Event's Value",
        (0x80803E32, "Failure Operation") => "When It Fails",
        (0x80803E32, "Failure Value") => "Fail Amount",
        (0x80803E32, "Failure Uses Event Value") => "Fail Uses the Event's Value",
        (_, "Value Threshold") => "Required Value",
        (_, "Extend By") => "Added Time",
        (_, "Probability Source") => "Chance Source",
        (0x80803DEA, "Event Value") | (0x80802D00, "Named Event Key") => "Event",
        (0x80803DEA, "Context Key") => "Context",
        (0x80803DEC | 0x80803DEB, "Event Key") => "Signal",
        (0x80803E1C, "Replacement Key") => "Firing Mode",
        (0x808029ED | 0x80803E1D, "Property Key") => "Property",
        (0x80803E1D, "Target Selector") => "Ability",
        (0x80803E39, "Property Key") => "Counter",
        (0x80803E1E, "Property" | "Property Key") => "Signal",
        (0x80803E2E, "Property" | "Property Key") => "Transmat Effect",
        (0x80803E43, "Position Selector") => "Position",
        (0x80803E06, "Hold Duration") => "Stays Met For",
        (_, "Hold Duration") => "Condition Hold",
        (_, "Up To") => "Extension Limit",
        (_, "Storage Path") => "Destination",
        // Every node carries these four. They are what the engine compiled, not what the
        // author chose, so they read as what they are rather than as engine nouns.
        (_, "Effect Kind" | "Condition Kind") => "Kind Number",
        (_, "Evaluation Order") => "Checked in This Order",
        (_, "Linked State") => "Shares Condition State",
        (_, "Retain Effect State") => "Keeps State While Active",
        (_, "Entry Count") => "Number of Rows",
        (_, "Probability") => "Chance",
        (_, "Predicate Mode") => "Label Test",
        (_, "Resource" | "Runtime Resource") => "Asset",
        // The filters an event condition carries, in the game's own words: damage type is
        // what Osmosis and the Change Damage Type action call it, and the slot mask names
        // the same weapon slots the ammunition rows do.
        (_, "Requires Owning Weapon") => "This Weapon Only",
        (_, "Slot Mask") => "Weapon Slots",
        (_, "Ability Slot Mask") => "Ability Slots",
        (_, "Ability Slot Flag") => "Check the Ability Slots",
        (_, "Damage Type Mask") => "Damage Types",
        (_, "Object Filter Selector") => "Object Filter",
        (_, "First Object Flag") => "Check the First Object",
        (_, "Second Object Flag") => "Check the Second Object",
        (_, "Third Object Flag") => "Check the Third Object",
        (_, "Fourth Object Flag") => "Check the Fourth Object",
        // Names an author types or picks.
        (_, "Named Key" | "Binding Key" | "Player Key" | "Target Key") => "Name",
        (_, "Removal Parameter") => "Value Written on Removal",
        (_, "Cleanup Policy Key") => "Lifetime",
        (_, "Action Value Parameter") => "Driven Value",
        (_, "Property Key") => "Property",
        // What an action does to the thing it names.
        (_, "Operation") => "How It Changes",
        (_, "Removal Policy") => "On Removal",
        (_, "Removal Value") => "Value on Removal",
        (_, "Target Selector") => "Applies To",
        (_, "Keep After Removal") => "Keeps the Change",
        (_, "Input Selector" | "Input Source") => "Value Source",
        (_, "Normalize Input") => "Scale to a Fraction",
        (_, "Scale by Action Value") => "Scale by Stacks",
        (_, "Scale by Ammunition Unit") => "Scale by One Round",
        (_, "Capacity Basis" | "Capacity Source") => "Share Of",
        (_, "Attached Entity") => "Attachment",
        (_, "Attachment Target") => "Attach To",
        (0x80803E47, "Spawned Resource") => "Drop Effect",
        (_, "Spawned Entity" | "Spawned Resource") => "What to Spawn",
        (_, "Applied Resource") => "What to Apply",
        (_, "Orb Entity") => "Orb",
        (_, "Projectile Pattern") => "Projectile",
        (_, "Minimum Distance") => "Farther Than",
        (_, "Maximum Distance" | "Maximum Source Distance") => "Within",
        (_, "Damage Multiplier") => "Damage Taken Multiplier",
        (_, "Radar Detection Range") => "Detection Range",
        (_, "Added Value") => "Amount Added",
        // The second wave: labels one class's own traced evidence settles.
        (0x80803E2F, "Value") => "Counter Value",
        (0x80803E2F, "Mode") => "Value Source",
        // Its choices are the attachment set, so it reads as the attach actions do.
        (0x80803E46, "Target Selection") => "Attach To",
        // Kind 33 reads a native stat unless its selector is FF, which is what this picks.
        (0x80803E3C, "Multiplier Stat") => "Use a Stat Instead",
        (_, "Upper Cap") => "Limit",
        (_, "Uses Ability Scalar Cap") => "Uses the Ability Limit",
        (_, "First Target Selector") => "First Target",
        (_, "Second Target Selector") => "Second Target",
        (_, "Third Target Selector") => "Third Target",
        (_, "Owning Slot Amount") => AmmunitionTarget::OwningWeapon.label(),
        (_, "Slot 1 Amount") => AmmunitionTarget::Slot1.label(),
        (_, "Slot 2 Amount") => AmmunitionTarget::Slot2.label(),
        (_, "Slot 3 Amount") => AmmunitionTarget::Slot3.label(),
        (_, "Category 1 Amount") => AmmunitionTarget::Category1.label(),
        (_, "Category 2 Amount") => AmmunitionTarget::Category2.label(),
        (_, "Category 3 Amount") => AmmunitionTarget::Category3.label(),
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
    let current = component_target(*target, 0, 0).unwrap_or("Other Target");
    let evidence = "The numeric selector remains editable.";
    let hover = format!("{current}\n{evidence}");
    // Four named abilities and an unmapped selector are a short vocabulary, and a combo
    // grows to its selected text unless something bounds it, so it keeps a narrow column.
    sized(ui, ui.available_width(), |ui| {
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
            .on_hover_text("Target by number.");
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
        Some(0x08) => Some("Trigger"),
        Some(0x20) => Some("Actions"),
        Some(0x30) => Some("End Condition"),
        Some(0x40) => Some("Reactivation"),
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
            name = behavior::labels::OPERATIONS[(field - source - 8) / 16].into();
        }
    }
    Ok(name)
}

const EFFECT_LIFETIME: &str = "Ends with the Effect";
const OWN_LIFETIME: &str = "Ends on Its Own";
const LIFETIME_HINT: &str = "Ends with the Effect removes the attachment when this effect ends. Ends on Its Own leaves that to the attachment.";
/// The key a new Ends on Its Own choice stores. No stock system reads it.
const OWN_LIFETIME_KEY: &str = "parhelion_own_lifetime";

/// Create Entity's cleanup key as the choice it makes. The game tests only whether the key is
/// empty: empty retires the attachment when the effect ends, and any key leaves its lifetime
/// to the attachment. The stock keys name effect families, so listing them offered choices
/// such as Invisibility that grant nothing. A stored key is kept until the choice changes.
fn lifetime(ui: &mut egui::Ui, value: &mut u32) {
    let empty = sundial::package_authoring::FNV1_EMPTY_HASH;
    let own = *value != 0 && *value != empty;
    let reading = if own { OWN_LIFETIME } else { EFFECT_LIFETIME };
    let hover = if own {
        let key = fields::keys::name(*value).map_or_else(
            || format!("0x{value:08X}"),
            |name| format!("{name} (0x{value:08X})"),
        );
        format!("{reading}\nKey: {key}\n{LIFETIME_HINT}")
    } else {
        format!("{reading}\n{LIFETIME_HINT}")
    };
    let mut choice = own;
    let typed = Typed::new(ui, "native-lifetime", true);
    sized(ui, EVIDENCE_WIDTH, |ui| {
        egui::ComboBox::from_id_salt("native-lifetime")
            .width(ui.available_width())
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                let effect = ui.selectable_value(&mut choice, false, EFFECT_LIFETIME);
                let own = ui.selectable_value(&mut choice, true, OWN_LIFETIME);
                if effect.clicked() || own.clicked() {
                    typed.listed(ui);
                }
                typed.row(ui);
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(ui, "native-lifetime", "Lifetime");
    });
    if typed.shown {
        sized(ui, EVIDENCE_WIDTH, |ui| {
            let raw = hex_key(ui, "native-lifetime-hex", value);
            pickers::name_response(ui, &raw, "Key as Hex");
        });
    }
    if choice != own {
        *value = if choice {
            sundial::package_authoring::fnv1_name_hash(OWN_LIFETIME_KEY)
        } else {
            empty
        };
    }
}

const ORB_CLASS: u32 = 0x80803E42;
/// The orb Masterwork Weapon and Trinity Ghoul Catalyst generate, which a fresh Generate Orbs
/// action starts with.
const MASTERWORK_ORB: u32 = 0x80EF_AE02;
const MASTERWORK_READING: &str = "Masterwork Orb";
const ALLIES_READING: &str = "Orb for Allies";
const ORB_HINT: &str = "Masterwork Orb drops one you can pick up. Orb for Allies is for your allies, as Striking Light's is.";

/// Whether this is Generate Orbs' orb, which the card offers as a choice rather than hiding
/// with the other references.
pub(super) fn orb_entity(class: u32, field: &fields::Field) -> bool {
    class == ORB_CLASS && field.offset == 0x18
}

/// The ability pattern On a Specific Ability and Ends on a Specific Ability store, chosen by
/// name from the stock abilities as a named key is.
pub(super) fn ability_reference(class: u32, field: &fields::Field) -> bool {
    matches!(class, 0x80803DFF | 0x80803DFE) && field.offset == 0x10
}

/// Every ability entity the stock Subclasses equip, named by the nodes that equip it. Empty
/// until a catalog loads, when the seven abilities stock perks name are offered alone.
static ABILITY_KEYS: std::sync::RwLock<&'static [fields::keys::EventKey]> =
    std::sync::RwLock::new(&[]);

/// Records the stock Subclasses' abilities for the Specific Ability conditions: the seven stock
/// perks name, with their evidence, then every other entity a node equips.
pub(in crate::app) fn remember_abilities(subclasses: &[sundial::investment::SubclassSummary]) {
    let mut named = BTreeMap::<u32, std::collections::BTreeSet<(String, String)>>::new();
    for subclass in subclasses {
        for (entry, entity) in &subclass.entry_entities {
            if let Some(name) = subclass.entry_names.get(entry) {
                named
                    .entry(*entity)
                    .or_default()
                    .insert((name.clone(), subclass.name.clone()));
            }
        }
    }
    let mut keys = fields::keys::known(0x8080_3DFF, 0x10).to_vec();
    for (entity, nodes) in named {
        if keys.iter().any(|key| key.hash == entity) {
            continue;
        }
        let mut abilities = nodes
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        abilities.dedup();
        let equipped = nodes
            .iter()
            .map(|(name, subclass)| format!("{name} ({subclass})"))
            .collect::<Vec<_>>()
            .join(", ");
        keys.push(fields::keys::EventKey {
            hash: entity,
            name: leak_label(&abilities.join(" / ")),
            evidence: leak_label(&format!(
                "The ability entity these nodes equip: {equipped}."
            )),
        });
    }
    keys.sort_by_key(|key| key.name);
    if let Ok(mut current) = ABILITY_KEYS.write() {
        *current = Box::leak(keys.into_boxed_slice());
    }
}

/// The abilities a Specific Ability condition can name: every stock one once a catalog loads.
fn ability_keys(class: u32, offset: usize) -> &'static [fields::keys::EventKey] {
    let current = ABILITY_KEYS.read().map_or(&[][..], |keys| *keys);
    if current.is_empty() {
        fields::keys::known(class, offset)
    } else {
        current
    }
}

/// Generate Orbs' orb as the two stock choices. Striking Light and Light of the Fire leave it
/// unset, the default orb Striking Light's own text says is for your allies, and a test perk
/// that dropped it at each kill showed its player no orbs. Any other orb keeps its tag.
fn orb(ui: &mut egui::Ui, value: &mut u32) {
    let allies = matches!(*value, 0 | u32::MAX);
    let reading = match *value {
        MASTERWORK_ORB => MASTERWORK_READING.to_owned(),
        _ if allies => ALLIES_READING.to_owned(),
        other => format!("0x{other:08X}"),
    };
    let mut choice = *value;
    let typed = Typed::new(ui, "native-orb", allies || *value == MASTERWORK_ORB);
    sized(ui, EVIDENCE_WIDTH, |ui| {
        egui::ComboBox::from_id_salt("native-orb")
            .width(ui.available_width())
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                let masterwork =
                    ui.selectable_value(&mut choice, MASTERWORK_ORB, MASTERWORK_READING);
                let allied = ui.selectable_label(allies, ALLIES_READING);
                if allied.clicked() {
                    choice = u32::MAX;
                }
                if masterwork.clicked() || allied.clicked() {
                    typed.listed(ui);
                }
                typed.row(ui);
            })
            .response
            .on_hover_text(ORB_HINT);
        pickers::name_combo(ui, "native-orb", "Orb");
    });
    if typed.shown {
        sized(ui, EVIDENCE_WIDTH, |ui| {
            let raw = hex_key(ui, "native-orb-hex", &mut choice);
            pickers::name_response(ui, &raw, "Orb as Hex");
        });
    }
    *value = choice;
}

/// `slot` is the ability slot of an Ability Property key, whose list also carries the
/// program's tunings on that slot and the row that defines one.
fn key_control(
    ui: &mut egui::Ui,
    label: &str,
    description: &str,
    known: &[fields::keys::EventKey],
    value: &mut u32,
    slot: Option<u8>,
) {
    if known.is_empty() && slot.is_none() {
        let raw = hex_key(ui, "native-event-key-hex", value);
        pickers::name_response(ui, &raw, label);
        return;
    }
    let tuned = slot.and_then(|_| tunings::reading(ui, *value));
    let reading = tuned.clone().unwrap_or_else(|| {
        known
            .iter()
            .find(|key| key.hash == *value)
            .map(|key| key.name)
            .or_else(|| fields::keys::name(*value))
            .map_or_else(
                || {
                    // The FNV-1 basis is the hash of an empty name. Keep its exact value on read.
                    if matches!(*value, 0 | 0x811C9DC5) {
                        "None".to_owned()
                    } else {
                        "Unnamed Key".to_owned()
                    }
                },
                str::to_owned,
            )
    });
    let evidence = tunings::evidence(ui, *value).unwrap_or_else(|| {
        known
            .iter()
            .find(|key| key.hash == *value)
            .or_else(|| fields::keys::entry(*value))
            .map_or(description, |key| key.evidence)
            .to_owned()
    });
    let hover = format!("{reading} (0x{value:08X})\n{evidence}");
    // A long list, such as the states a State Check reads, is searched rather than scrolled,
    // and leads with the everyday states. The query starts empty each time the list opens.
    let searched = known.len() > SEARCHED_KEYS;
    let mut ordered = known.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|key| {
        STATE_LEAD
            .iter()
            .position(|lead| *lead == key.name)
            .unwrap_or(usize::MAX)
    });
    // The FNV-1 basis is the hash of an empty name, which reads as None as zero does.
    let listed = known.iter().any(|key| key.hash == *value)
        || tuned.is_some()
        || matches!(*value, 0 | 0x811C9DC5);
    let typed = Typed::new(ui, "native-event-key", listed);
    let editor = slot.map(|_| tunings::Editor::new(ui, "native-event-key"));
    sized(ui, EVIDENCE_WIDTH, |ui| {
        let query_id = ui.make_persistent_id("native-event-key-query");
        let mut query = ui
            .data(|data| data.get_temp::<String>(query_id))
            .unwrap_or_default();
        let mut picked = false;
        let mut combo = egui::ComboBox::from_id_salt("native-event-key")
            .width(ui.available_width())
            .truncate()
            .selected_text(reading)
            .close_behavior(if searched {
                egui::PopupCloseBehavior::CloseOnClickOutside
            } else {
                egui::PopupCloseBehavior::CloseOnClick
            });
        if slot.is_some() {
            // An ability key list holds the slot's stock keys, the program's tunings and the
            // rows that define one, so it shows them all rather than folding the last few
            // behind a scroll.
            combo = combo.height(TUNED_KEY_LIST_HEIGHT);
        }
        let combo = combo.show_ui(ui, |ui| {
            let focus = query_id.with("focus");
            // The program's own tunings lead the slot's stock keys, with the rows that
            // define one, so they never sit behind the list's scroll.
            if let (Some(slot), Some(editor)) = (slot, &editor) {
                picked |= tunings::rows(ui, editor, slot.into(), value);
            }
            picked |= key_choices(ui, &ordered, value, searched.then_some((&mut query, focus)));
            if picked {
                typed.listed(ui);
            }
            picked |= typed.row(ui);
        });
        if searched && combo.response.clicked() {
            query.clear();
            ui.data_mut(|data| data.insert_temp(query_id.with("focus"), true));
        }
        if searched && picked {
            ui.memory_mut(egui::Memory::close_popup);
        }
        ui.data_mut(|data| data.insert_temp(query_id, query));
        combo.response.on_hover_text(hover);
        pickers::name_combo(ui, "native-event-key", label);
    });
    // A tuning's edit is offered where its reading is, not only inside the list.
    if let (Some(editor), Some(_)) = (&editor, &tuned) {
        sized(ui, EVIDENCE_WIDTH, |ui| {
            ui.scope(|ui| {
                crate::app::style::quiet(ui);
                if ui.small_button("Edit Property…").clicked() {
                    editor.edit(ui, *value);
                }
            });
        });
    }
    if typed.shown {
        sized(ui, EVIDENCE_WIDTH, |ui| {
            let raw = hex_key(ui, "native-event-key-hex", value);
            pickers::name_response(ui, &raw, "Key as Hex");
        });
    }
    if let Some(editor) = &editor {
        editor.show(ui, value);
    }
}

/// The named keys in a key list, with a search box above them when `search` holds the query
/// and the slot that asks for its focus. Returns whether a key was picked, by a click or by
/// Enter, which takes the first match.
fn key_choices(
    ui: &mut egui::Ui,
    ordered: &[&fields::keys::EventKey],
    value: &mut u32,
    search: Option<(&mut String, egui::Id)>,
) -> bool {
    let mut words = String::new();
    let mut enter = false;
    if let Some((query, focus)) = search {
        words = query.trim().to_lowercase();
        let search = ui.add(
            egui::TextEdit::singleline(query)
                .hint_text("Search")
                .desired_width(f32::INFINITY),
        );
        pickers::name_response(ui, &search, "Search Choices");
        if ui.data(|data| data.get_temp::<bool>(focus)) == Some(true) {
            search.request_focus();
            ui.data_mut(|data| data.insert_temp(focus, false));
        }
        enter = search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    }
    // A name match leads a match found only in the perks that establish the key.
    let mut shown = ordered
        .iter()
        .filter(|key| pickers::matches(&words, &format!("{} {}", key.name, key.evidence)))
        .collect::<Vec<_>>();
    shown.sort_by_key(|key| !pickers::matches(&words, key.name));
    let mut picked = false;
    if enter && let Some(key) = shown.first() {
        *value = key.hash;
        picked = true;
    }
    if shown.is_empty() {
        ui.label("No Matching Results");
    }
    for key in shown {
        picked |= ui
            .selectable_value(value, key.hash, key.name)
            .on_hover_text(key.evidence)
            .clicked();
    }
    picked
}

/// A dropdown whose value can also be typed. Its list ends in Other Value…, which closes the
/// list and shows the value's own editor under the dropdown. The editor stays while the value
/// is one the list does not hold, and choosing a listed value puts it away. An Advanced fold
/// inside the list closed the list when clicked and opened below where the list could show.
pub(super) struct Typed {
    id: egui::Id,
    /// Whether the editor shows under the dropdown this frame.
    pub shown: bool,
}

impl Typed {
    /// `listed` says whether the list holds the current value.
    pub(super) fn new(ui: &egui::Ui, salt: &str, listed: bool) -> Self {
        let id = ui.make_persistent_id((salt, "typed"));
        let chosen = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
        Self {
            id,
            shown: !listed || chosen,
        }
    }

    /// The list's last row. Returns whether it was chosen.
    pub(super) fn row(&self, ui: &mut egui::Ui) -> bool {
        ui.separator();
        let chosen = ui.selectable_label(false, "Other Value…").clicked();
        if chosen {
            ui.data_mut(|data| data.insert_temp(self.id, true));
        }
        chosen
    }

    /// Puts the editor away once a listed value is chosen.
    pub(super) fn listed(&self, ui: &egui::Ui) {
        ui.data_mut(|data| data.remove::<bool>(self.id));
    }
}

/// Key lists longer than this are searched rather than scrolled.
const SEARCHED_KEYS: usize = 12;

/// The popup height of an ability slot's key list: up to twelve stock keys, a few tunings and
/// the three rows under them.
const TUNED_KEY_LIST_HEIGHT: f32 = 460.0;

/// The states a requirement most often checks, in the order Suggested lists them. The others
/// follow in the order the key table gives, which leads with what the stock perks check most.
const STATE_LEAD: &[&str] = &[
    "Aiming Down Sights for a Moment",
    "Weapon Firing",
    "Reloading",
    "Sliding",
    "Super Active",
    "Charged with Light Stacks",
    "Nearby Enemy Count",
    "Magazine Fraction",
    "Rounds Loaded",
    "Weapon Equipped",
    "Weapon Holstered",
    "Charging Grenade",
    "Guarding",
    "Subclass Is Arc",
    "Subclass Is Solar",
    "Subclass Is Void",
    "Inside Well of Radiance",
    "Drawing a Bow",
    "Fully Spun Up",
];

const KILL_CONTEXT: &str = "native-kill-fired";

/// Marks whether a kill fires the behavior group being drawn, so a choice that names the other
/// side of the trigger's event reads as the kill's.
pub(super) fn kill_context(ui: &egui::Ui, killed: bool) {
    let id = egui::Id::new(KILL_CONTEXT);
    ui.ctx().data_mut(|data| {
        if killed {
            data.insert_temp(id, true);
        } else {
            data.remove::<bool>(id);
        }
    });
}

/// A choice's name where it is drawn. Where a kill fires the effect, the other combatant an
/// action attaches to or applies to is the target it killed, as Firefly's explosion and
/// Cosmology's and Judgment's attachments are.
fn choice_name(
    ui: &egui::Ui,
    class: u32,
    field: &fields::Field,
    value: u8,
    name: &'static str,
) -> &'static str {
    let other_side = matches!(class, 0x80803E44..=0x80803E46) && field.offset == 2 && value == 3;
    let killed = || {
        ui.ctx()
            .data(|data| data.get_temp::<bool>(egui::Id::new(KILL_CONTEXT)))
            .unwrap_or(false)
    };
    if other_side && killed() {
        "Killed Target"
    } else {
        name
    }
}

/// A value no name was recovered for, read by a stock perk that sets it, since that is the
/// one thing known about it and a name a reader knows. A value no named perk sets reads by
/// its number.
fn observed_reading(value: u32, perks: &str) -> String {
    let first = perks
        .split([',', ';'])
        .flat_map(|part| part.split(" and "))
        .map(str::trim)
        .find(|name| !name.is_empty());
    match first {
        Some(name) => format!("As {name}"),
        None => format!("Value {value}"),
    }
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
    let perks_setting = |value: u32| {
        contract
            .observed
            .iter()
            .find(|(candidate, _, _)| u32::from(*candidate) == value)
            .map_or("", |(_, _, perks)| *perks)
    };
    let reading: String = contract
        .choices
        .iter()
        .find(|(value, _)| u32::from(*value) == selected)
        .map_or_else(
            || match stock(selected) {
                // Zero is the template's own value and selects nothing named. Any other
                // unnamed value reads by a perk that sets it, and its stock use is on hover.
                None if selected == 0 => "Not Set".to_owned(),
                _ => observed_reading(selected, perks_setting(selected)),
            },
            |(value, name)| choice_name(ui, class, field, *value, name).to_owned(),
        );
    let usage = match stock(selected) {
        _ if contract
            .choices
            .iter()
            .any(|(value, _)| u32::from(*value) == selected) =>
        {
            String::new()
        }
        Some(count) => {
            format!("\n{count} stock perks set this value. What it selects is not established.")
        }
        None => "\nNo stock perk sets this value.".to_owned(),
    };
    let hover = format!("{reading}{usage}\n{}", contract.description);
    let listed = contract
        .choices
        .iter()
        .any(|(value, _)| u32::from(*value) == selected)
        || unnamed
            .iter()
            .any(|(value, _, _)| u32::from(*value) == selected);
    let typed = Typed::new(ui, "native-value-choice", listed);
    // A value's name can be a sentence's worth of words, and a combo takes the width
    // of its selected text, so it is held to the shared value column with the whole
    // reading and the field's description on hover.
    column(ui, |ui| {
        egui::ComboBox::from_id_salt("native-value-choice")
            .width(ui.available_width())
            .truncate()
            .selected_text(reading)
            .show_ui(ui, |ui| {
                let mut chose = false;
                for (value, name) in contract.choices {
                    let name = choice_name(ui, class, field, *value, name);
                    chose |= ui
                        .selectable_value(&mut selected, u32::from(*value), name)
                        .on_hover_text(match stock(u32::from(*value)) {
                            Some(count) => format!("{count} stock perks set this value."),
                            None => "No stock perk sets this value.".to_owned(),
                        })
                        .clicked();
                }
                for (value, count, perks) in &unnamed {
                    chose |= ui
                        .selectable_value(
                            &mut selected,
                            u32::from(*value),
                            format!(
                                "{} · {count} stock perks",
                                observed_reading(u32::from(*value), perks)
                            ),
                        )
                        .on_hover_text(if perks.is_empty() {
                            "No named stock perk sets this value.".to_owned()
                        } else {
                            format!("Set by {perks}.")
                        })
                        .clicked();
                }
                if chose {
                    typed.listed(ui);
                }
                typed.row(ui);
            })
            .response
            .on_hover_text(hover);
        pickers::name_combo(
            ui,
            "native-value-choice",
            plain_field_label(class, &field.label),
        );
    });
    if typed.shown {
        column(ui, |ui| {
            let mut drag = egui::DragValue::new(&mut selected);
            if field.width == 1 {
                drag = drag.range(0..=255);
            }
            let raw = ui.add(drag).on_hover_text("Raw Value");
            pickers::name_response(ui, &raw, "Raw Value");
        });
    }
    selected
}

#[allow(clippy::cognitive_complexity)]
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
        // The closed control reads by a stock perk that sets the value. How many stock perks
        // set it is evidence for choosing, so it stays in the list and on hover.
        let perks_setting = contract
            .observed
            .iter()
            .find(|(candidate, _, _)| *candidate == selected)
            .map_or("", |(_, _, perks)| *perks);
        let reading = observed_reading(u32::from(selected), perks_setting);
        let usage = match stock(selected) {
            Some(count) => format!("{count} stock perks set this value."),
            None => "No stock perk sets this value.".to_owned(),
        };
        let hover = format!("{reading}\n{usage}\n{}", contract.description);
        let listed = contract
            .observed
            .iter()
            .any(|(value, _, _)| *value == selected);
        let typed = Typed::new(ui, "native-observed-value", listed);
        // `width` is only a floor, so the evidence beside the value would otherwise push
        // the control across the pane. The allocation bounds it and the full reading and
        // the field's own description stay on hover.
        sized(ui, EVIDENCE_WIDTH, |ui| {
            egui::ComboBox::from_id_salt("native-observed-value")
                .selected_text(reading)
                .width(ui.available_width())
                .truncate()
                .show_ui(ui, |ui| {
                    let mut chose = false;
                    for (value, count, perks) in contract.observed {
                        chose |= ui
                            .selectable_value(
                                &mut selected,
                                *value,
                                format!(
                                    "{} · {count} stock perks",
                                    observed_reading(u32::from(*value), perks)
                                ),
                            )
                            .on_hover_text(if perks.is_empty() {
                                "No named stock perk sets this value.".to_owned()
                            } else {
                                format!("Set by {perks}.")
                            })
                            .clicked();
                    }
                    if chose {
                        typed.listed(ui);
                    }
                    typed.row(ui);
                })
                .response
                .on_hover_text(hover);
            pickers::name_combo(
                ui,
                "native-observed-value",
                plain_field_label(block.class, &field.label),
            );
        });
        if typed.shown {
            sized(ui, EVIDENCE_WIDTH, |ui| {
                let raw = ui
                    .add(egui::DragValue::new(&mut selected).range(0..=255))
                    .on_hover_text("Raw Value");
                pickers::name_response(ui, &raw, "Raw Value");
            });
        }
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
    if (block.class == 0x80803E45 && field.offset == 0x18) || orb_entity(block.class, field) {
        let mut value = u32::from_le_bytes(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid native key width.")?,
        );
        let before = value;
        if block.class == ORB_CLASS {
            orb(ui, &mut value);
        } else {
            lifetime(ui, &mut value);
        }
        if value != before {
            field.write(block, row, &value.to_le_bytes())?;
        }
        return Ok(());
    }
    // A key whose stock values are named is chosen by name, with the hex value kept under
    // Advanced for any other key. The ability conditions' pattern reference is one such key.
    if matches!(field.format, Format::Key | Format::Tag) {
        // An Ability Property's key also offers the program's own tunings on the slot.
        let mut slot = None;
        let known = if block.class == 0x80803E1D && field.offset == 4 {
            let stride = schema::record(block.class)?.size;
            let chosen = *block
                .bytes
                .get(row * stride + 2)
                .ok_or("Missing ability slot.")?;
            slot = Some(chosen);
            fields::keys::ability_properties(chosen)
        } else if ability_reference(block.class, field) {
            ability_keys(block.class, field.offset)
        } else {
            fields::keys::known(block.class, field.offset)
        };
        if !known.is_empty() || slot.is_some() {
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
                slot,
            );
            if value != before {
                field.write(block, row, &value.to_le_bytes())?;
            }
            return Ok(());
        }
    }
    if let Some(mapped) = mapped(block.class, field) {
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
            let response = super::super::controls::float_field_with(ui, &mut bits, contract.suffix);
            pickers::name_response(ui, &response, name);
            if let Some(hint) = plain_field_hint(block.class, &field.label) {
                response.on_hover_text(hint);
            }
            if block.class == 0x80803E4D && field.offset == 12 && f32::from_bits(bits) < 0.0 {
                ui.weak("No Limit");
            }
            // The counter's sentinels, read as the stock perks mean them: a reset at -1 never
            // fires, and the stock lower clamp of -9998 is no bound at all.
            if block.class == 0x80803E30 {
                let value = f32::from_bits(bits);
                if field.offset == 0x24 && value < 0.0 {
                    ui.weak("Never");
                } else if field.offset == 0x28 && value <= -9998.0 {
                    ui.weak("No Minimum");
                }
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

/// The guided layout field drawn for a native field, when its kind has one.
fn mapped(class: u32, field: &fields::Field) -> Option<&'static layout::Field> {
    nodes::CONDITIONS
        .iter()
        .find(|entry| entry.class == class)
        .and_then(|entry| layout::condition_layout(entry.kind))
        .or_else(|| {
            nodes::EFFECTS
                .iter()
                .find(|entry| entry.class == class)
                .and_then(|entry| layout::effect_layout(entry.kind))
        })
        .and_then(|layout| {
            layout.fields.iter().find(|mapped| {
                mapped.offset == field.offset && mapped.format.width() == field.width
            })
        })
}

/// The width a field's control takes in a tile `tile` wide, or `None` for a hex key, which
/// keeps its row beside the key's name. A unit or a second bound shares the tile with the
/// value. The cases follow `scalar`, which draws the control.
fn tile_width(class: u32, field: &fields::Field, tile: f32) -> Option<f32> {
    let contract = fields::contract(class, field);
    if !field.editable || (class == 0x808094B3 && field.offset == 0) {
        return None;
    }
    // The orb is a choice like Position beside it, though it is stored as a reference.
    if orb_entity(class, field) {
        return Some(tile);
    }
    // A key with named values is a dropdown like any other choice. A raw key keeps its row,
    // where the hex and its name both have room.
    if matches!(field.format, Format::Key | Format::Tag)
        && ((class == 0x80803E1D && field.offset == 4)
            || !fields::keys::known(class, field.offset).is_empty())
    {
        return Some(tile);
    }
    if contract.bitmask
        || (field.format == Format::Byte && !contract.observed.is_empty())
        || (matches!(field.format, Format::Byte | Format::Mask32) && !contract.choices.is_empty())
    {
        return Some(tile);
    }
    match mapped(class, field).map(|mapped| mapped.format) {
        Some(FieldFormat::Range) => Some((tile - 32.0) / 2.0),
        Some(FieldFormat::Key | FieldFormat::Mask32) => Some(tile),
        Some(_) => Some(tile),
        None => match field.format {
            Format::Flag
            | Format::Byte
            | Format::Integer
            | Format::Unsigned
            | Format::Key
            | Format::Mask32 => Some(tile),
            Format::Float => Some(tile),
            _ => None,
        },
    }
}

/// A timer condition's length in seconds, read as its Duration control shows it.
fn timer_seconds(block: &native::Block) -> Option<f32> {
    let field = fields::describe(block.class)
        .ok()?
        .into_iter()
        .find(|field| field.label == "Duration")?;
    match mapped(block.class, &field) {
        Some(layout_field) => match layout_field.read(&block.bytes)? {
            FactValue::Seconds(seconds) | FactValue::Number(seconds) => Some(seconds),
            _ => None,
        },
        None => field
            .bytes(block, 0)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(f32::from_le_bytes),
    }
}

/// The Not of a general predicate, drawn on its condition's line.
#[cfg(test)]
pub(super) fn negation(ui: &mut egui::Ui, node: &mut NativeNode) {
    let Some(entry) = nodes::condition(node.kind) else {
        return;
    };
    let Ok(mut graph) = Graph::read(&node.bytes, 0, entry.class) else {
        return;
    };
    let before = graph.blocks[0].bytes.clone();
    behavior::negation(ui, &mut graph.blocks[0]);
    if graph.blocks[0].bytes != before
        && let Ok(bytes) = graph.emit()
    {
        node.bytes = bytes;
    }
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
    // The stored bytes, the text beside them, and the error Apply Bytes last reported.
    let mut state = ui
        .data(|data| data.get_temp::<(Vec<u8>, String, Option<String>)>(id))
        .filter(|state| state.0 == bytes)
        .unwrap_or_else(|| (bytes.to_vec(), hex(bytes), None));
    let text = if multiline {
        ui.text_edit_multiline(&mut state.1)
    } else {
        ui.add(egui::TextEdit::singleline(&mut state.1).desired_width(120.0))
    };
    if text.changed() {
        state.2 = None;
    }
    let mut result = None;
    if ui.button("Apply Bytes").clicked() {
        match parse_hex(&state.1) {
            Ok(value) => {
                state.2 = None;
                result = Some(value);
            }
            Err(error) => state.2 = Some(error),
        }
    }
    if let Some(error) = &state.2 {
        ui.colored_label(ui.visuals().error_fg_color, error);
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
