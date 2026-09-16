//! Turns a decoded action into readable lines for the workbench.
//!
//! The wording describes the traced operation of each node. It is not a gameplay
//! guarantee, and a perk can still depend on host state this summary cannot see.

use crate::sandbox_perk::nodes::{self, Support};

use super::{
    ConditionRole, DecodedAction, DecodedCondition, DecodedEffect, DecodedGroup, Fact, FactValue,
    facts::trim_number, label_name,
};

/// One readable node line.
#[derive(Clone, Debug, PartialEq)]
pub struct SummaryLine {
    /// Complete native data for the expandable record reader.
    pub native: Option<(bool, crate::sandbox_perk::program::NativeNode)>,
    /// Sentence-case description of the node.
    pub text: String,
    /// Mapped field values, already rendered.
    pub detail: Vec<String>,
    /// Title-case catalog name of the node kind.
    pub kind_name: String,
    /// Authoring support level of the node kind.
    pub support: Support,
    /// Nesting depth for indenting child nodes.
    pub depth: usize,
    /// Entity graph or pattern the node references, when it carries one.
    pub asset: Option<u32>,
}

/// One decoded group, rendered.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSummary {
    /// Title-case heading for this group.
    pub label: String,
    /// Conditions that start the action.
    pub activation: Vec<SummaryLine>,
    /// Effects applied while the action is active.
    pub effects: Vec<SummaryLine>,
    /// Conditions that end the action.
    pub removal: Vec<SummaryLine>,
    /// Conditions that allow the action to start again.
    pub rearm: Vec<SummaryLine>,
}

impl GroupSummary {
    /// The rendered condition list for one role.
    #[must_use]
    pub fn conditions(&self, role: ConditionRole) -> &[SummaryLine] {
        match role {
            ConditionRole::Activation => &self.activation,
            ConditionRole::Removal => &self.removal,
            ConditionRole::Rearm => &self.rearm,
        }
    }
}

/// A readable account of one action.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionSummary {
    /// One sentence naming the trigger and the headline effect.
    pub headline: String,
    /// Every group in the action, primary group first.
    pub groups: Vec<GroupSummary>,
    /// Contract notes and explicit gaps a reader needs.
    pub notes: Vec<String>,
    /// The weakest support level across the action.
    pub support: Support,
}

impl ActionSummary {
    /// Builds the summary of a decoded action.
    #[must_use]
    pub fn new(action: &DecodedAction) -> Self {
        let groups = action
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| summarize_group(index, group))
            .collect();
        Self {
            headline: headline(action),
            groups,
            notes: notes(action),
            support: action.support(),
        }
    }

    /// Plain-text rendering, useful for reports and tests.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = vec![self.headline.clone()];
        for group in &self.groups {
            out.push(format!("[{}]", group.label));
            render_section(
                &mut out,
                ConditionRole::Activation.heading(),
                &group.activation,
            );
            render_section(&mut out, "Then", &group.effects);
            render_section(&mut out, ConditionRole::Removal.heading(), &group.removal);
            render_section(&mut out, ConditionRole::Rearm.heading(), &group.rearm);
        }
        out.extend(self.notes.iter().map(|note| format!("Note: {note}")));
        out.join("\n")
    }
}

fn render_section(out: &mut Vec<String>, heading: &str, lines: &[SummaryLine]) {
    if lines.is_empty() {
        return;
    }
    out.push(format!("{heading}:"));
    for line in lines {
        out.push(format!("{}- {}", "  ".repeat(line.depth + 1), line.text));
    }
}

fn summarize_group(index: usize, group: &DecodedGroup) -> GroupSummary {
    let label = if index == 0 {
        "Main Program".to_owned()
    } else {
        format!("Additional Program {}", index + 1)
    };
    GroupSummary {
        label,
        activation: condition_lines(&group.activation, 0),
        effects: group.effects.iter().map(effect_line).collect(),
        removal: condition_lines(&group.removal, 0),
        rearm: condition_lines(&group.rearm, 0),
    }
}

fn condition_lines(conditions: &[DecodedCondition], depth: usize) -> Vec<SummaryLine> {
    let mut out = Vec::new();
    for condition in conditions {
        out.push(condition_line(condition, depth));
        out.extend(condition_lines(&condition.children, depth + 1));
        for (index, subgroup) in condition.subgroups.iter().enumerate() {
            out.push(SummaryLine {
                native: None,
                text: format!("Requirement {}, met by any of", index + 1),
                detail: vec![format!(
                    "Hold: {}",
                    FactValue::Seconds(subgroup.hold).render()
                )],
                kind_name: "Requirement".to_owned(),
                support: Support::Readable,
                depth: depth + 1,
                asset: None,
            });
            out.extend(condition_lines(&subgroup.conditions, depth + 2));
        }
    }
    out
}

fn condition_line(condition: &DecodedCondition, depth: usize) -> SummaryLine {
    let catalog = condition.catalog();
    let mut text = describe_condition(condition);
    if let Some(chance) = condition.probability.describe() {
        text.push_str(&format!(" ({chance})"));
    }
    let mut detail = condition.facts.iter().map(Fact::render).collect::<Vec<_>>();
    if let Some(row) = condition.accumulator_row {
        detail.push(format!(
            "On pass: operation {} with {}",
            row.success_operation,
            if row.success_uses_event_value {
                "the event value".into()
            } else {
                FactValue::Number(row.success_value).render()
            }
        ));
        detail.push(format!(
            "On fail: operation {} with {}",
            row.failure_operation,
            if row.failure_uses_event_value {
                "the event value".into()
            } else {
                FactValue::Number(row.failure_value).render()
            }
        ));
        detail.push(format!(
            "Hold: {}",
            FactValue::Seconds(row.hold_seconds).render()
        ));
    }
    SummaryLine {
        native: Some((
            true,
            crate::sandbox_perk::program::NativeNode {
                kind: condition.kind,
                bytes: condition.native.clone(),
            },
        )),
        text,
        detail,
        kind_name: condition.name(),
        support: catalog.map_or(Support::Structural, |node| node.support),
        depth,
        asset: None,
    }
}

fn effect_line(effect: &DecodedEffect) -> SummaryLine {
    let catalog = effect.catalog();
    let mut detail = effect.facts.iter().map(Fact::render).collect::<Vec<_>>();
    if effect.kind == 8
        && let Some(role) = component_role(effect)
    {
        detail.insert(0, format!("Target Role: {role}"));
    }
    if let Some(tag) = effect.referenced_tag {
        detail.insert(0, format!("Asset: 0x{tag:08X}"));
    }
    if let Some(path) = &effect.referenced_path
        && !path.is_empty()
    {
        detail.push(format!("Native Path: {path}"));
    }
    if !effect.conditions.is_empty() {
        detail.push("Timer extension conditions:".to_owned());
        for line in condition_lines(&effect.conditions, 0) {
            detail.push(format!("{}{}", "  ".repeat(line.depth), line.text));
            detail.extend(
                line.detail
                    .into_iter()
                    .map(|field| format!("{}{}", "  ".repeat(line.depth + 1), field)),
            );
        }
    }
    SummaryLine {
        native: Some((
            false,
            crate::sandbox_perk::program::NativeNode {
                kind: effect.kind,
                bytes: effect.native.clone(),
            },
        )),
        text: describe_effect(effect),
        detail,
        kind_name: effect.name(),
        support: catalog.map_or(Support::Structural, |node| node.support),
        depth: 0,
        asset: effect.referenced_tag,
    }
}

fn fact_value<'a>(facts: &'a [Fact], label: &str) -> Option<&'a FactValue> {
    facts
        .iter()
        .find(|fact| fact.label == label)
        .map(|fact| &fact.value)
}

fn labels_of(condition: &DecodedCondition) -> Vec<u32> {
    match fact_value(&condition.facts, super::REQUIRED_LABELS) {
        Some(FactValue::Labels(values)) => values.clone(),
        _ => Vec::new(),
    }
}

fn requires_weapon(condition: &DecodedCondition) -> bool {
    matches!(
        fact_value(&condition.facts, "Requires Owning Weapon"),
        Some(FactValue::Flag(true))
    )
}

pub(super) fn describe_condition(condition: &DecodedCondition) -> String {
    if condition.kind == 20
        && let Ok(graph) = super::native::Graph::read(&condition.native, 0, condition.class)
        && let Some(name) = super::native::predicate::describe(&graph)
    {
        return name;
    }
    if matches!(condition.kind, 20 | 35)
        && let Some(name) = state_description(condition.class, &condition.native)
    {
        return name;
    }
    if let Some(name) = recognized_condition(condition) {
        return name.to_owned();
    }
    match condition.kind {
        0 => "Always".to_owned(),
        1 => match fact_value(&condition.facts, "Duration") {
            Some(value) => format!("After {}", value.render()),
            None => "After a timer".to_owned(),
        },
        2 => describe_kill(condition),
        // Innervation, Invigoration, Insulation and Absolution share both keys.
        // Their separate native item descriptions all identify Orb of Light pickup.
        12 if matches!(
            fact_value(&condition.facts, "Event Value"),
            Some(FactValue::Key(0x6CEC7A87))
        ) && matches!(
            fact_value(&condition.facts, "Context Key"),
            Some(FactValue::Key(0x18CCBF24))
        ) =>
        {
            "Pick up an Orb of Light".into()
        }
        14 => "The weapon is attached".to_owned(),
        15 => "The weapon is detached".to_owned(),
        16 => "The weapon is drawn".to_owned(),
        17 => "The weapon is holstered".to_owned(),
        26 => "A counter built from the rows below reaches its threshold".to_owned(),
        31 => "Every requirement below is met".to_owned(),
        35 => "A predicate passes and its nested condition also passes".to_owned(),
        _ => condition
            .catalog()
            .map_or_else(|| condition.name(), |node| node.summary.to_owned()),
    }
}

/// A general predicate read as the state it checks: the named key at +D4, inverted by the
/// flag at +F8, and the equipped weapon labels of any weapon record it carries. Both are
/// named from the stock perks that use them, so a node with neither keeps its traced name.
pub fn state_description(class: u32, native: &[u8]) -> Option<String> {
    let key = native
        .get(0xD4..0xD8)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))?;
    let inverted = native.get(0xF8) == Some(&1);
    let state = super::native::fields::keys::known(class, 0xD4)
        .iter()
        .find(|entry| entry.hash == key)
        .map(|entry| entry.name.to_owned())
        .or_else(|| {
            // The inline player and weapon states, named the same way (see `values.rs`).
            let player = match native.get(0x38) {
                Some(2) => Some("Airborne"),
                Some(4) => Some("Sliding"),
                Some(8) => Some("Sprinting"),
                _ => None,
            };
            let weapon = (native.get(0x81) == Some(&4)).then_some("Aiming Down Sights");
            match (player, weapon) {
                (Some(player), Some(weapon)) => Some(format!("{player} and {weapon}")),
                (Some(one), None) | (None, Some(one)) => Some(one.to_owned()),
                (None, None) => None,
            }
        });
    let weapons = equipped_weapon_labels(class, native);
    let mut text = String::new();
    if let Some(state) = state {
        text = if inverted {
            format!("While not {state}")
        } else {
            format!("While {state}")
        };
    }
    if !weapons.is_empty() {
        let list = weapons.join(" or ");
        if text.is_empty() {
            text = format!("While a {list} is equipped");
        } else {
            text.push_str(&format!(" with a {list} equipped"));
        }
    }
    (!text.is_empty()).then_some(text)
}

/// The weapon type labels of the equipped weapon records a predicate carries. Every stock
/// use of the record is an Ammo Finder mod reading "while you have an X equipped", and the
/// record's label site holds the weapon type vocabulary.
fn equipped_weapon_labels(class: u32, native: &[u8]) -> Vec<String> {
    let Ok(graph) = super::native::Graph::read(native, 0, class) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for (index, block) in graph.blocks.iter().enumerate() {
        if block.class != 0x8080_29DB {
            continue;
        }
        let Ok(lists) = super::native::labels::source(&graph, index, 8) else {
            continue;
        };
        for hash in &lists[0] {
            let name = crate::sandbox_perk::activation::site_label_name(*hash)
                .map_or_else(|| format!("label 0x{hash:08X}"), str::to_owned);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Names require the complete identifying selectors, never just a node kind.
/// The underlying condition and all of its restrictions remain intact.
pub(super) fn recognized_condition(condition: &DecodedCondition) -> Option<&'static str> {
    let bytes = &condition.native;
    match condition.kind {
        // Lead from Gold and Overflow agree on the recipient mask. Their second
        // mask distinguishes Heavy from Special-or-Heavy pickups.
        6 if bytes.get(8) == Some(&7) => match bytes.get(9) {
            Some(4) => Some("Pick up Heavy ammunition"),
            Some(6) => Some("Pick up Special or Heavy ammunition"),
            _ => None,
        },
        // Osmosis/Demolitionist and Bomber/Outreach/Perpetuation identify these
        // ability-use masks. Only the unfiltered player form gets this name.
        8 if bytes
            .get(9..32)
            .is_some_and(|tail| tail.iter().all(|&byte| byte == 0)) =>
        {
            match bytes.get(8) {
                Some(1) => Some("Use a grenade ability"),
                Some(128) => Some("Use a class ability"),
                _ => None,
            }
        }
        12 if bytes.get(8..16) == Some(&[0x87, 0x7A, 0xEC, 0x6C, 0x24, 0xBF, 0xCC, 0x18]) => {
            Some("Pick up an Orb of Light")
        }
        _ => None,
    }
}

fn describe_kill(condition: &DecodedCondition) -> String {
    let labels = labels_of(condition);
    let source = if requires_weapon(condition) {
        "this weapon"
    } else {
        "any credited source"
    };
    if labels.is_empty() {
        return format!("A kill from {source}");
    }
    let names = labels
        .iter()
        .map(|hash| label_name(*hash).map_or_else(|| format!("label 0x{hash:08X}"), str::to_owned))
        .collect::<Vec<_>>()
        .join(" or ");
    format!("A {names} kill from {source}")
}

pub(super) fn describe_effect(effect: &DecodedEffect) -> String {
    let asset = effect
        .referenced_path
        .as_deref()
        .map(asset_label)
        .or_else(|| effect.referenced_tag.map(|tag| format!("0x{tag:08X}")));
    match (effect.kind, asset) {
        (1, Some(name)) => format!("Attach {name} for as long as the effect lasts"),
        (1, None) => "Attach an entity for as long as the effect lasts".to_owned(),
        (2, Some(name)) => format!("Attach {name} and drive one of its values"),
        (3, Some(name)) => format!("Spawn {name} once"),
        (3, None) => "Spawn an entity once".to_owned(),
        (8, _) if component_role(effect).is_some() => {
            let target = component_role(effect).unwrap().to_lowercase();
            let scale = fact_value(&effect.facts, "Scale").map(FactValue::render);
            match scale {
                Some(scale) => format!("Adjust {target} using a scale of {scale}"),
                None => format!("Adjust {target}"),
            }
        }
        (26, Some(name)) => format!("Replace the weapon projectile pattern with {name}"),
        (26, None) => "Replace the weapon projectile pattern".to_owned(),
        (32, _) => describe_extend(effect),
        (14 | 15, _) => describe_ammunition(effect),
        (40, _) => describe_event_modifier(effect),
        _ => effect
            .catalog()
            .map_or_else(|| effect.name(), |node| node.summary.to_owned()),
    }
}

fn component_role(effect: &DecodedEffect) -> Option<&'static str> {
    let selector = |label| match fact_value(&effect.facts, label) {
        Some(FactValue::Selector(value)) => Some(*value),
        _ => None,
    };
    super::component_target(
        selector("Target Selector")?,
        selector("Flag Byte")?,
        selector("Option Byte")?,
    )
}

fn describe_event_modifier(effect: &DecodedEffect) -> String {
    let Some(multiplier) = fact_value(&effect.facts, "Default Scalar Multiplier") else {
        return "Adjust the filtered event values".into();
    };
    let mut text = format!(
        "Multiply the filtered event scalar by {}",
        multiplier.render()
    );
    if let Some(alternate) = fact_value(&effect.facts, "Alternate Scalar Multiplier") {
        text.push_str(&format!(
            ", or {} when the alternate event context applies",
            alternate.render()
        ));
    }
    if fact_value(&effect.facts, "Uses Ability Scalar Cap").is_some() {
        text.push_str(", subject to the ability scalar cap");
    }
    text
}

fn describe_extend(effect: &DecodedEffect) -> String {
    let extend = fact_value(&effect.facts, "Extend By").map(FactValue::render);
    let cap = fact_value(&effect.facts, "Up To").map(FactValue::render);
    match (extend, cap) {
        (Some(extend), Some(cap)) => {
            format!("Extend the running timers by {extend}, up to {cap}")
        }
        _ => "Extend the running timers".to_owned(),
    }
}

/// The amount labels of an ammunition node with the reading of each target.
const AMMUNITION_TARGETS: [(&str, &str); 7] = [
    ("Owning Slot Amount", "this weapon"),
    ("Slot 1 Amount", "weapon slot 1"),
    ("Slot 2 Amount", "weapon slot 2"),
    ("Slot 3 Amount", "weapon slot 3"),
    ("Category 1 Amount", "ammo type 1"),
    ("Category 2 Amount", "ammo type 2"),
    ("Category 3 Amount", "ammo type 3"),
];

/// An ammunition adjustment as a sentence: the amounts it adds and where they go. The
/// magazine and reserves readings of the storage byte come from how stock perks use it.
fn describe_ammunition(effect: &DecodedEffect) -> String {
    let store = match fact_value(&effect.facts, "Storage Path")
        .or_else(|| fact_value(&effect.facts, "Destination"))
    {
        Some(FactValue::Selector(0)) => "reserves",
        Some(FactValue::Selector(1)) => "magazine",
        _ => "ammunition",
    };
    let fraction = effect.kind == 15;
    let amounts = AMMUNITION_TARGETS
        .iter()
        .filter_map(|(label, target)| match fact_value(&effect.facts, label) {
            Some(FactValue::Integer(value)) if *value != 0 => Some(format!(
                "{value} {} to {target}",
                if value.unsigned_abs() == 1 {
                    "round"
                } else {
                    "rounds"
                }
            )),
            Some(FactValue::Number(value)) if *value != 0.0 => Some(if fraction {
                format!(
                    "{}% of the capacity to {target}",
                    trim_number(value * 100.0)
                )
            } else if value.abs() == 1.0 {
                format!("{} round to {target}", trim_number(*value))
            } else {
                format!("{} rounds to {target}", trim_number(*value))
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    if amounts.is_empty() {
        return format!("Add nothing to the {store}");
    }
    let overflow = matches!(
        fact_value(&effect.facts, "Allow Magazine Overflow"),
        Some(FactValue::Flag(true))
    );
    format!(
        "Add {} in the {store}{}",
        amounts.join(" and "),
        if overflow { ", past its capacity" } else { "" }
    )
}

/// Last path segment without its extensions, for a readable asset name.
fn asset_label(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file.split('.').next().unwrap_or(file);
    if stem.is_empty() {
        path.to_owned()
    } else {
        stem.to_owned()
    }
}

fn headline(action: &DecodedAction) -> String {
    let Some(group) = action.groups.first() else {
        return "This perk has no action program.".to_owned();
    };
    let trigger = group
        .activation
        .first()
        .map_or_else(|| "It runs from the start".to_owned(), describe_condition);
    let effects = group.effects.len();
    let count = match effects {
        0 => "no effects".to_owned(),
        1 => "one effect".to_owned(),
        value => format!("{value} effects"),
    };
    format!("{trigger}, then applies {count}.")
}

fn notes(action: &DecodedAction) -> Vec<String> {
    let mut notes = Vec::new();
    if action
        .groups
        .first()
        .is_some_and(|group| group.activation.len() > 1)
    {
        notes.push(
            "Conditions in one list are alternatives. The first one that passes starts the action."
                .to_owned(),
        );
    }
    if action.groups.len() > 1 {
        notes.push(
            "This perk has more than one program. Each program keeps its own conditions and effects."
                .to_owned(),
        );
    }
    if let Some(policy) = nodes::policy(action.policy).filter(|policy| policy.kind != 0) {
        notes.push(format!("{}. {}", policy.name, policy.summary));
    }
    notes.extend(unmapped_note(action));
    notes.push(
        "This describes the compiled action. It does not prove the perk behaves this way on every weapon."
            .to_owned(),
    );
    notes
}

fn unmapped_note(action: &DecodedAction) -> Option<String> {
    let mut names = action
        .conditions()
        .into_iter()
        .filter_map(DecodedCondition::catalog)
        .chain(action.effects().filter_map(DecodedEffect::catalog))
        .filter(|node| node.support >= Support::Structural)
        .map(|node| node.name)
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "Parhelion reads the structure of {} but not its individual fields.",
        names.join(", ")
    ))
}
