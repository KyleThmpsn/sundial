//! Recovers an editable program from a decoded stock action when the action lies inside
//! the shape the compiler can emit.
//!
//! A recovered program is the compiler's view of the action, so re-emitting it produces a
//! fresh action rather than a copy. `fidelity` reports every native byte the round trip would
//! not reproduce, so the workbench can say whether a conversion is exact before offering it.

use super::{
    Action, AmmunitionStore, AmmunitionTarget, Asset, EMPTY_KEY, NativeNode, Position, Program,
    Trigger,
};
use crate::package_payload::{i64_at, relative_offset, u64_at};
use crate::sandbox_perk::action::{
    CREATE_ENTITY_FLOAT_LABELS, CREATE_ENTITY_KEY_LABELS, CREATE_ENTITY_MODE_LABEL, DecodedAction,
    DecodedCondition, DecodedEffect, FactValue, NAMED_PROPERTY_LABELS, Probability, decode, layout,
};

const PRECISION: u32 = 0x962E_A19B;
const GRENADE: u32 = 0xC20D_D425;
const MELEE: [u32; 3] = [0xBF39_E12B, 0xE175_76C9, 0x5D3A_7C84];

/// Why a decoded action cannot become an editable program yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Recovers a program from a decoded action.
///
/// `graph_of` maps a referenced stock tag to the graph the program should use, which lets a
/// caller carry existing projectile swaps into the recovered program. It returns the same
/// tag when nothing was swapped.
pub fn decompile(
    action: &DecodedAction,
    name: &str,
    graph_of: impl Fn(u32) -> u32,
) -> Result<Program, Unsupported> {
    if action.groups.len() != 1 {
        return Err(Unsupported(
            "This perk runs more than one program. Parhelion can author one program.".into(),
        ));
    }
    if action.policy != 0 || action.policy_configuration {
        return Err(Unsupported(
            "This perk uses an execution policy Parhelion cannot author.".into(),
        ));
    }
    if action.auxiliary_records != 0 {
        return Err(Unsupported(
            "This perk has auxiliary records the program model cannot preserve.".into(),
        ));
    }
    let group = &action.groups[0];
    let (trigger, chance_permyriad, native_trigger) = trigger_of(&group.activation)?;
    let ending = duration_of(trigger, &group.removal)?;
    let cooldown_ms = cooldown_of(trigger, &group.rearm)?;
    // The native list is stored last to first. Restore authored order.
    let actions = group
        .effects
        .iter()
        .rev()
        .map(|effect| action_of(effect, &graph_of, trigger, group.activation.first()))
        .collect::<Result<Vec<_>, _>>()?;
    let program = Program {
        name: name.to_owned(),
        trigger,
        duration_ms: ending.duration_ms,
        cooldown_ms,
        chance_permyriad,
        actions,
        removal_key: ending.removal_key,
        native_trigger,
        native_removal: ending.native_removal,
        native: None,
    };
    program.validate().map_err(Unsupported)?;
    Ok(program)
}

/// Preserve every native byte and allocation, including unnamed scalar fields.
fn native_condition(condition: &DecodedCondition) -> Option<NativeNode> {
    Some(NativeNode {
        kind: condition.kind,
        bytes: condition.native.clone(),
    })
}

fn native_effect(effect: &DecodedEffect, graph_of: &impl Fn(u32) -> u32) -> Option<NativeNode> {
    use crate::sandbox_perk::action::native::{Graph, schema};
    let mut bytes = effect.native.clone();
    if let Some(original) = effect.referenced_tag {
        let replacement = graph_of(original);
        if replacement != original {
            let mut graph = Graph::read(&bytes, 0, effect.class).ok()?;
            for block in graph.blocks.iter_mut().filter(|block| block.class != 0) {
                let record = schema::record(block.class).ok()?;
                for row in 0..block.count.unwrap_or(1) {
                    for &(field, code) in &record.fields {
                        let field = row * record.size + field;
                        if matches!(code, 4 | 9)
                            && crate::package_payload::u32_at(&block.bytes, field).ok()? == original
                        {
                            block.bytes[field..field + 4]
                                .copy_from_slice(&replacement.to_le_bytes());
                        }
                    }
                }
            }
            bytes = graph.emit().ok()?;
        }
    }
    Some(NativeNode {
        kind: effect.kind,
        bytes,
    })
}

/// How a program ends: a timer, an ending key, a native node, or nothing.
#[derive(Default)]
struct Ending {
    duration_ms: u32,
    removal_key: Option<u32>,
    native_removal: Option<NativeNode>,
}

fn single<'a>(
    list: &'a [DecodedCondition],
    role: &str,
) -> Result<&'a DecodedCondition, Unsupported> {
    match list {
        [condition] => Ok(condition),
        [] => Err(Unsupported(format!("This perk has no {role} condition."))),
        _ => Err(Unsupported(format!(
            "This perk has {} alternative {role} conditions. Parhelion can author one.",
            list.len()
        ))),
    }
}

fn trigger_of(
    activation: &[DecodedCondition],
) -> Result<(Trigger, u16, Option<NativeNode>), Unsupported> {
    // An empty activation list receives event bit 0, the same routing as an unconditional
    // check, so the engine treats both as always active.
    if activation.is_empty() {
        return Ok((Trigger::Always, 10_000, None));
    }
    let condition = single(activation, "activation")?;
    if !condition.children.is_empty() || !condition.subgroups.is_empty() {
        return Ok((Trigger::Native, 10_000, native_condition(condition)));
    }
    let chance = match condition.probability {
        Probability::Always => 10_000,
        Probability::Literal(value) if value.is_finite() && (0.0..=1.0).contains(&value) => {
            (f64::from(value) * 10_000.0).round() as u16
        }
        Probability::Literal(_) => {
            return Err(Unsupported(
                "The activation probability is outside the authored range.".into(),
            ));
        }
        Probability::NativeStat(_) if layout::condition_layout(condition.kind).is_none() => {
            return Ok((Trigger::Native, 10_000, native_condition(condition)));
        }
        Probability::NativeStat(_) => {
            return Err(Unsupported(
                "The activation chance comes from a native stat.".into(),
            ));
        }
    };
    let trigger = match condition.kind {
        0 => Trigger::Always,
        14 => Trigger::Equipped,
        16 => Trigger::Drawn,
        2 => kill_trigger(condition)?,
        _ => match native_condition(condition) {
            Some(node) => return Ok((Trigger::Native, 10_000, Some(node))),
            None => {
                return Err(Unsupported(format!(
                    "Parhelion cannot author a {} trigger yet.",
                    condition.name()
                )));
            }
        },
    };
    if condition.kind != 2 && chance != 10_000 {
        return Err(Unsupported(
            "Only kill triggers carry an activation chance in an authored program.".into(),
        ));
    }
    Ok((trigger, chance, None))
}

fn kill_trigger(condition: &DecodedCondition) -> Result<Trigger, Unsupported> {
    let labels = condition
        .facts
        .iter()
        .find_map(|fact| match &fact.value {
            FactValue::Labels(labels)
                if fact.label == crate::sandbox_perk::action::REQUIRED_LABELS =>
            {
                Some(labels.clone())
            }
            _ => None,
        })
        .unwrap_or_default();
    let weapon = condition
        .facts
        .iter()
        .any(|fact| fact.label == "Requires Owning Weapon" && fact.value == FactValue::Flag(true));
    let mut sorted = labels.clone();
    sorted.sort_unstable();
    let mut melee = MELEE.to_vec();
    melee.sort_unstable();
    match (sorted.as_slice(), weapon) {
        ([], true) => Ok(Trigger::WeaponKill),
        ([], false) => Ok(Trigger::AnyKill),
        ([label], true) if *label == PRECISION => Ok(Trigger::PrecisionKill),
        ([label], false) if *label == GRENADE => Ok(Trigger::GrenadeKill),
        (set, false) if set == melee.as_slice() => Ok(Trigger::MeleeKill),
        _ => Err(Unsupported(
            "The kill filter uses labels Parhelion cannot author yet.".into(),
        )),
    }
}

fn timer_seconds(condition: &DecodedCondition) -> Option<f32> {
    if condition.kind != 1 {
        return None;
    }
    condition.facts.iter().find_map(|fact| match fact.value {
        FactValue::Seconds(value) if fact.label == "Duration" => Some(value),
        _ => None,
    })
}

fn millis(seconds: f32) -> u32 {
    (f64::from(seconds) * 1000.0)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The duration and, for an always-active or native-triggered program, the event key or
/// native node that ends it.
fn duration_of(trigger: Trigger, removal: &[DecodedCondition]) -> Result<Ending, Unsupported> {
    if matches!(trigger, Trigger::Always | Trigger::Native) {
        let condition = match removal {
            [] => return Ok(Ending::default()),
            [condition] => condition,
            _ => {
                return Err(Unsupported(format!(
                    "This perk has {} alternative removal conditions. Parhelion can author one.",
                    removal.len()
                )));
            }
        };
        if trigger == Trigger::Always
            && condition.kind == 30
            && condition.probability == Probability::Always
        {
            return match effect_key(condition) {
                Some(key) => Ok(Ending {
                    removal_key: Some(key),
                    ..Ending::default()
                }),
                None => Err(Unsupported("The ending event key is unreadable.".into())),
            };
        }
        if trigger == Trigger::Native
            && let Some(seconds) = timer_seconds(condition)
        {
            return Ok(Ending {
                duration_ms: millis(seconds),
                ..Ending::default()
            });
        }
        return match native_condition(condition) {
            Some(node) => Ok(Ending {
                native_removal: Some(node),
                ..Ending::default()
            }),
            None => Err(Unsupported(format!(
                "A {} removal condition is not modeled yet.",
                condition.name()
            ))),
        };
    }
    let condition = single(removal, "removal")?;
    let expected = match trigger {
        Trigger::Equipped => Some(15),
        Trigger::Drawn => Some(17),
        _ => None,
    };
    match expected {
        Some(kind) if condition.kind == kind => Ok(Ending::default()),
        Some(_) => Err(Unsupported(format!(
            "The removal condition is {}, which does not pair with the trigger.",
            condition.name()
        ))),
        None => timer_seconds(condition)
            .map(|seconds| Ending {
                duration_ms: millis(seconds),
                ..Ending::default()
            })
            .ok_or_else(|| {
                Unsupported(format!(
                    "The removal condition is {}, not a timer.",
                    condition.name()
                ))
            }),
    }
}

fn effect_key(condition: &DecodedCondition) -> Option<u32> {
    condition.facts.iter().find_map(|fact| match fact.value {
        FactValue::Key(key) if fact.label == "Event Key" => Some(key),
        _ => None,
    })
}

fn cooldown_of(trigger: Trigger, rearm: &[DecodedCondition]) -> Result<u32, Unsupported> {
    match rearm {
        [] => Ok(0),
        [condition] if trigger.supports_cooldown() => timer_seconds(condition)
            .map(millis)
            .ok_or_else(|| Unsupported("The rearm condition is not a timer.".into())),
        _ => Err(Unsupported(
            "This perk has rearm conditions Parhelion cannot author.".into(),
        )),
    }
}

fn action_of(
    effect: &DecodedEffect,
    graph_of: &impl Fn(u32) -> u32,
    trigger: Trigger,
    activation: Option<&DecodedCondition>,
) -> Result<Action, Unsupported> {
    match effect.kind {
        32 => return extend_timers_of(effect, trigger, activation),
        10 => return property_of(effect),
        14 | 15 => return ammunition_of(effect),
        1 | 3 | 26 => {}
        _ => {
            if let Some(node) = native_effect(effect, graph_of) {
                return Ok(Action::Native { node });
            }
        }
    }
    let Some(tag) = effect.referenced_tag else {
        return Err(Unsupported(format!(
            "The {} effect has no entity reference Parhelion can author.",
            effect.name()
        )));
    };
    let asset = Asset {
        graph: graph_of(tag),
        path: effect.referenced_path.clone().unwrap_or_default(),
        values: Vec::new(),
    };
    match effect.kind {
        1 => Ok(Action::Attach {
            asset,
            mode: match effect_fact(effect, CREATE_ENTITY_MODE_LABEL) {
                Some(FactValue::Selector(mode)) => *mode,
                _ => 1,
            },
            keys: CREATE_ENTITY_KEY_LABELS.map(|label| match effect_fact(effect, label) {
                Some(FactValue::Key(key)) => *key,
                _ => EMPTY_KEY,
            }),
            float_bits: CREATE_ENTITY_FLOAT_LABELS.map(|label| match effect_fact(effect, label) {
                Some(FactValue::Number(value)) => value.to_bits(),
                _ => 0,
            }),
        }),
        3 => {
            let event = effect.facts.iter().any(|fact| {
                fact.label == "Position Selector" && fact.value == FactValue::Selector(1)
            });
            Ok(Action::Spawn {
                asset,
                position: if event {
                    Position::Event
                } else {
                    Position::Owner
                },
            })
        }
        26 => Ok(Action::Pattern { asset }),
        _ => Err(Unsupported(format!(
            "Parhelion cannot author the {} effect yet.",
            effect.name()
        ))),
    }
}

fn effect_fact<'a>(effect: &'a DecodedEffect, label: &str) -> Option<&'a FactValue> {
    effect
        .facts
        .iter()
        .find(|fact| fact.label == label)
        .map(|fact| &fact.value)
}

fn seconds_fact(effect: &DecodedEffect, label: &str) -> Option<u32> {
    match effect_fact(effect, label) {
        Some(FactValue::Seconds(value)) => Some(millis(*value)),
        _ => None,
    }
}

fn selector_fact(effect: &DecodedEffect, label: &str) -> Option<u8> {
    match effect_fact(effect, label) {
        Some(FactValue::Selector(value)) => Some(*value),
        _ => None,
    }
}

/// An Extend Timers effect is authorable when its one nested condition is the program's own
/// kill trigger, which is the only shape the compiler emits.
fn extend_timers_of(
    effect: &DecodedEffect,
    trigger: Trigger,
    activation: Option<&DecodedCondition>,
) -> Result<Action, Unsupported> {
    let [nested] = effect.conditions.as_slice() else {
        return Err(Unsupported(format!(
            "The Extend Timers effect nests {} conditions. Parhelion nests the trigger once.",
            effect.conditions.len()
        )));
    };
    let Some(activation) = activation.filter(|activation| activation.kind == 2) else {
        return Err(Unsupported(
            "The Extend Timers effect belongs to a perk without a kill trigger.".into(),
        ));
    };
    let same_trigger = nested.kind == 2
        && kill_trigger(nested).ok() == Some(trigger)
        && nested.probability == activation.probability
        && nested.children.is_empty()
        && nested.subgroups.is_empty();
    if !same_trigger {
        return Err(Unsupported(format!(
            "The Extend Timers effect nests a {} condition that differs from the trigger.",
            nested.name()
        )));
    }
    match (
        seconds_fact(effect, "Extend By"),
        seconds_fact(effect, "Up To"),
    ) {
        (Some(extend_ms), Some(cap_ms)) => Ok(Action::ExtendTimers { extend_ms, cap_ms }),
        _ => Err(Unsupported(
            "The Extend Timers effect has no readable timing.".into(),
        )),
    }
}

/// A Named Property effect is authorable when its value program is the surveyed constant.
fn property_of(effect: &DecodedEffect) -> Result<Action, Unsupported> {
    let labels = &NAMED_PROPERTY_LABELS;
    let Some(FactValue::Number(value)) = effect_fact(effect, labels.value) else {
        return Err(Unsupported(
            "The Named Property effect uses a value program Parhelion cannot author yet.".into(),
        ));
    };
    let key = match effect_fact(effect, labels.key) {
        Some(FactValue::Key(key)) => *key,
        _ => EMPTY_KEY,
    };
    let ability_mask = match effect_fact(effect, labels.ability_mask) {
        Some(FactValue::Mask(mask)) => u32::try_from(*mask).unwrap_or(0),
        _ => 0,
    };
    let restore_bits = match effect_fact(effect, labels.restore) {
        Some(FactValue::Number(value)) => value.to_bits(),
        _ => 0,
    };
    Ok(Action::Property {
        key,
        target: selector_fact(effect, labels.target).unwrap_or(0),
        operation_byte: selector_fact(effect, labels.operation).unwrap_or(0),
        removal: selector_fact(effect, labels.removal).unwrap_or(0),
        value_bits: value.to_bits(),
        restore_bits,
        ability_mask,
        input: selector_fact(effect, labels.input).unwrap_or(0),
        flag: selector_fact(effect, labels.flag).unwrap_or(1),
    })
}

/// The amount labels of an ammunition node, in the order of `AmmunitionTarget`.
const AMMUNITION_AMOUNTS: [&str; 7] = [
    "Owning Slot Amount",
    "Slot 1 Amount",
    "Slot 2 Amount",
    "Slot 3 Amount",
    "Category 1 Amount",
    "Category 2 Amount",
    "Category 3 Amount",
];

/// An ammunition node is authorable when its source label filter is empty and one amount
/// carries the value, which is the shape of every stock node outside the ammo pickup perks.
fn ammunition_of(effect: &DecodedEffect) -> Result<Action, Unsupported> {
    if effect
        .facts
        .iter()
        .any(|fact| matches!(fact.value, FactValue::Labels(_)))
    {
        return Err(Unsupported(format!(
            "The {} effect filters on source labels Parhelion cannot author yet.",
            effect.name()
        )));
    }
    let amounts = AMMUNITION_AMOUNTS
        .iter()
        .zip(AmmunitionTarget::ALL)
        .filter_map(|(label, target)| match effect_fact(effect, label) {
            Some(FactValue::Number(value)) if *value != 0.0 => Some((target, f64::from(*value))),
            Some(FactValue::Integer(value)) if *value != 0 => Some((target, f64::from(*value))),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(target, value)] = amounts.as_slice() else {
        return Err(Unsupported(format!(
            "The {} effect fills {} ammunition targets. Parhelion fills one.",
            effect.name(),
            amounts.len()
        )));
    };
    let store_byte = selector_fact(effect, "Storage Path")
        .or_else(|| selector_fact(effect, "Destination"))
        .unwrap_or(1);
    let Some(store) = AmmunitionStore::from_byte(store_byte) else {
        return Err(Unsupported(format!(
            "The {} effect uses storage path {store_byte}, which no stock perk uses.",
            effect.name()
        )));
    };
    let flag = |label: &str| matches!(effect_fact(effect, label), Some(FactValue::Flag(true)));
    let overflow = flag("Allow Magazine Overflow");
    let action_scaled = flag("Scale by Action Value");
    if effect.kind == 14 {
        return Ok(Action::AddRounds {
            rounds: *value as i32,
            target: *target,
            store,
            overflow,
            unit_scaled: flag("Scale by Ammunition Unit"),
            action_scaled,
        });
    }
    let capacity_byte = selector_fact(effect, "Capacity Source").unwrap_or(1);
    let Some(capacity) = AmmunitionStore::from_byte(capacity_byte) else {
        return Err(Unsupported(format!(
            "The {} effect uses capacity source {capacity_byte}, which no stock perk uses.",
            effect.name()
        )));
    };
    Ok(Action::AddFraction {
        fraction_bits: (*value as f32).to_bits(),
        target: *target,
        store,
        capacity,
        overflow,
        action_scaled,
    })
}

/// One native byte range the round trip would not reproduce.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    /// Node kind and role, for the reader.
    pub node: String,
    /// Offset inside the node.
    pub offset: usize,
    /// Stock bytes at that offset.
    pub stock: Vec<u8>,
    /// Bytes the compiler emits instead.
    pub compiled: Vec<u8>,
}

/// Compare complete allocation graphs, excluding only compiler-owned routing metadata.
/// Policies, auxiliary records, all groups and every unknown scalar remain in the comparison.
pub fn native_fidelity(stock: &[u8], compiled: &[u8]) -> Result<Vec<Difference>, String> {
    use crate::sandbox_perk::action::{ACTION_ROOT_CLASS, native::Graph};
    let canonical = |payload: &[u8]| -> Result<Vec<u8>, String> {
        let mut graph = Graph::read(payload, 0, ACTION_ROOT_CLASS)?;
        graph.validate_program()?;
        let root = &mut graph.blocks[0].bytes;
        root[..8].fill(0);
        root[0x88..0xA8].fill(0);
        root[0xCC..0xCE].fill(0);
        for block in &mut graph.blocks {
            if crate::sandbox_perk::nodes::CONDITIONS
                .iter()
                .any(|node| node.observed() && node.class == block.class)
            {
                block.bytes[6..8].fill(0);
            }
        }
        graph.emit()
    };
    let mut out = Vec::new();
    compare_bytes(
        "Complete Program".into(),
        &canonical(stock)?,
        &canonical(compiled)?,
        &[],
        &mut out,
    );
    Ok(out)
}

/// Compares a stock action with the action compiled from its recovered program.
///
/// Pointer fields, list descriptors, compiled ordinals and linked-state flags are derived data
/// and are masked. Every other byte of every node is compared, so a difference here names a
/// native setting the program model does not carry.
pub fn fidelity(stock: &[u8], compiled: &[u8]) -> Result<Vec<Difference>, String> {
    let stock_action = decode(stock)?;
    let compiled_action = decode(compiled)?;
    let mut out = Vec::new();
    if stock == compiled {
        return Ok(out);
    }
    if stock_action.groups.len() != 1
        || compiled_action.groups.len() != 1
        || stock_action.policy != 0
        || compiled_action.policy != 0
        || stock_action.policy_configuration
        || compiled_action.policy_configuration
        || stock_action.auxiliary_records != 0
        || compiled_action.auxiliary_records != 0
    {
        return Err("Exact conversion cannot be checked for additional groups, execution policies or auxiliary records.".into());
    }
    if stock_action
        .conditions()
        .iter()
        .chain(compiled_action.conditions().iter())
        .any(|node| {
            node.catalog().is_none_or(|entry| {
                entry.support != crate::sandbox_perk::nodes::Support::Authorable
            })
        })
        || stock_action
            .effects()
            .chain(compiled_action.effects())
            .any(|node| {
                node.catalog().is_none_or(|entry| {
                    entry.support != crate::sandbox_perk::nodes::Support::Authorable
                })
            })
    {
        return Err(
            "Exact conversion cannot be checked for a node outside the authored program model."
                .into(),
        );
    }
    compare_bytes(
        "Action Routing and State".into(),
        &stock[0x88..0xD0],
        &compiled[0x88..0xD0],
        &[(0x20, 16)],
        &mut out,
    );
    compare_bytes(
        "Action Identity".into(),
        &stock[8..0x10],
        &compiled[8..0x10],
        &[],
        &mut out,
    );
    let (Some(left), Some(right)) = (stock_action.groups.first(), compiled_action.groups.first())
    else {
        return Err("An action without a program cannot be compared".into());
    };
    for (role, a, b) in [
        ("activation", &left.activation, &right.activation),
        ("removal", &left.removal, &right.removal),
        ("rearm", &left.rearm, &right.rearm),
    ] {
        for (x, y) in a.iter().zip(b) {
            compare_condition(stock, compiled, x, y, role, &mut out);
        }
        if a.len() != b.len() {
            out.push(Difference {
                node: format!("{role} list length"),
                offset: 0,
                stock: (a.len() as u64).to_le_bytes().to_vec(),
                compiled: (b.len() as u64).to_le_bytes().to_vec(),
            });
        }
    }
    for (x, y) in left.effects.iter().zip(&right.effects) {
        compare_effect(stock, compiled, x, y, &mut out);
    }
    if left.effects.len() != right.effects.len() {
        out.push(Difference {
            node: "Effect List Length".into(),
            offset: 0,
            stock: (left.effects.len() as u64).to_le_bytes().to_vec(),
            compiled: (right.effects.len() as u64).to_le_bytes().to_vec(),
        });
    }
    Ok(out)
}

fn node_bytes(payload: &[u8], offset: usize, size: usize) -> &[u8] {
    payload.get(offset..offset + size).unwrap_or(&[])
}

fn condition_mask(kind: u8, size: usize) -> Vec<(usize, usize)> {
    // Ordinals are rebuilt. Linked state changes the evaluator contract.
    let mut mask = vec![(7, 1)];
    match kind {
        // Label globals reference pointers, label rows and the predicate pointer.
        2 => mask.extend([(0x48, 8), (0xD0, 16), (0x110, 8), (0x130, 8)]),
        // Weapon events keep a label globals pointer.
        13..=19 => mask.push((0x50, 8)),
        _ => {}
    }
    mask.retain(|(start, len)| start + len <= size);
    mask
}

fn effect_mask(kind: u8, size: usize) -> Vec<(usize, usize)> {
    let mut mask = Vec::new();
    match kind {
        1 | 3 | 26 => mask.push((8, 8)),
        // The nested condition list descriptor.
        32 => mask.push((0x10, 16)),
        // The value program's two array pointers. Their contents are compared separately.
        10 => mask.extend([(0x20, 8), (0x30, 8)]),
        // The label globals path pointer inside the source label filter.
        14 | 15 => mask.push((0x48, 8)),
        _ => {}
    }
    mask.retain(|(start, len)| start + len <= size);
    mask
}

/// The bytecode and constant rows of a value program, for comparison outside the node.
fn program_bytes(payload: &[u8], program: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (descriptor, stride) in [(program, 1), (program + 0x10, 16)] {
        let count = usize::try_from(u64_at(payload, descriptor).ok()?).ok()?;
        let rows =
            relative_offset(descriptor + 8, 0, i64_at(payload, descriptor + 8).ok()?).ok()? + 16;
        out.extend_from_slice(payload.get(rows..rows + count * stride)?);
    }
    Some(out)
}

fn compare_bytes(
    node: String,
    stock: &[u8],
    compiled: &[u8],
    mask: &[(usize, usize)],
    out: &mut Vec<Difference>,
) {
    let size = stock.len().max(compiled.len());
    let differs = (0..size)
        .map(|offset| {
            let masked = mask
                .iter()
                .any(|(start, len)| (*start..start + len).contains(&offset));
            !masked && byte_at(stock, offset) != byte_at(compiled, offset)
        })
        .collect::<Vec<_>>();
    let mut offset = 0;
    while offset < size {
        if !differs[offset] {
            offset += 1;
            continue;
        }
        let start = offset;
        while offset < size && differs[offset] {
            offset += 1;
        }
        out.push(Difference {
            node: node.clone(),
            offset: start,
            stock: stock.get(start..offset).unwrap_or(&[]).to_vec(),
            compiled: compiled.get(start..offset).unwrap_or(&[]).to_vec(),
        });
    }
}

fn byte_at(bytes: &[u8], offset: usize) -> u8 {
    bytes.get(offset).copied().unwrap_or(0)
}

fn condition_size(condition: &DecodedCondition) -> usize {
    condition
        .catalog()
        .map_or(8, |node| node.struct_size as usize)
}

fn compare_condition(
    stock: &[u8],
    compiled: &[u8],
    a: &DecodedCondition,
    b: &DecodedCondition,
    role: &str,
    out: &mut Vec<Difference>,
) {
    let node = format!("{} ({role})", a.name());
    if a.kind != b.kind {
        out.push(Difference {
            node,
            offset: 5,
            stock: vec![a.kind],
            compiled: vec![b.kind],
        });
        return;
    }
    if layout::condition_layout(a.kind).is_none() && !matches!(a.kind, 2 | 14..=17) {
        compare_native(&node, a.class, &a.native, &b.native, out);
        return;
    }
    let size = condition_size(a);
    compare_bytes(
        node.clone(),
        node_bytes(stock, a.offset, size),
        node_bytes(compiled, b.offset, size),
        &condition_mask(a.kind, size),
        out,
    );
    compare_label_facts(&node, &a.facts, &b.facts, out);
    if a.kind == 2 {
        compare_predicate(
            stock,
            compiled,
            a.offset + 0x130,
            b.offset + 0x130,
            &node,
            out,
        );
    }
}

fn compare_native(node: &str, class: u32, left: &[u8], right: &[u8], out: &mut Vec<Difference>) {
    let canonical = |bytes: &[u8]| {
        let mut graph = crate::sandbox_perk::action::native::Graph::read(bytes, 0, class)?;
        for block in &mut graph.blocks {
            if crate::sandbox_perk::nodes::CONDITIONS
                .iter()
                .any(|entry| entry.observed() && entry.class == block.class)
            {
                block.bytes[7] = 0;
            }
        }
        graph.emit()
    };
    match (canonical(left), canonical(right)) {
        (Ok(left), Ok(right)) => compare_bytes(
            format!("{node} Complete Native Data"),
            &left,
            &right,
            &[],
            out,
        ),
        _ => out.push(Difference {
            node: format!("{node} Unreadable Native Data"),
            offset: 0,
            stock: left.to_vec(),
            compiled: right.to_vec(),
        }),
    }
}

fn compare_label_facts(
    node: &str,
    a: &[crate::sandbox_perk::action::Fact],
    b: &[crate::sandbox_perk::action::Fact],
    out: &mut Vec<Difference>,
) {
    let labels = |facts: &[crate::sandbox_perk::action::Fact]| {
        facts
            .iter()
            .filter_map(|fact| {
                if let FactValue::Labels(values) = &fact.value {
                    Some((
                        fact.label,
                        values
                            .iter()
                            .flat_map(|value| value.to_le_bytes())
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                }
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let a = labels(a);
    let b = labels(b);
    for label in a
        .keys()
        .chain(b.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
    {
        let left = a.get(label).cloned().unwrap_or_default();
        let right = b.get(label).cloned().unwrap_or_default();
        if left != right {
            out.push(Difference {
                node: format!("{node} {label}"),
                offset: 0,
                stock: left,
                compiled: right,
            });
        }
    }
}

fn compare_predicate(
    stock: &[u8],
    compiled: &[u8],
    a: usize,
    b: usize,
    node: &str,
    out: &mut Vec<Difference>,
) {
    let read = |payload: &[u8], at| -> Option<Vec<u8>> {
        let relative = i64_at(payload, at).ok()?;
        if relative == 0 {
            return Some(Vec::new());
        }
        let target = relative_offset(at, 0, relative).ok()?;
        let marker = target.checked_sub(4)?;
        let class = crate::package_payload::u32_at(payload, marker).ok()?;
        let size = match class {
            0x8080_93F6 => 84,
            0x8080_93F5 => 164,
            _ => return None,
        };
        Some(payload.get(marker..target.checked_add(size)?)?.to_vec())
    };
    let left = read(stock, a);
    let right = read(compiled, b);
    // An unreadable predicate must never become an empty successful comparison.
    if left.is_none() || right.is_none() || left != right {
        out.push(Difference {
            node: format!("{node} Compiled Predicate"),
            offset: 0x130,
            stock: left.unwrap_or_else(|| b"Unreadable".to_vec()),
            compiled: right.unwrap_or_else(|| b"Unreadable".to_vec()),
        });
    }
}

fn compare_effect(
    stock: &[u8],
    compiled: &[u8],
    a: &DecodedEffect,
    b: &DecodedEffect,
    out: &mut Vec<Difference>,
) {
    let node = a.name();
    if a.kind != b.kind {
        out.push(Difference {
            node,
            offset: 0,
            stock: vec![a.kind],
            compiled: vec![b.kind],
        });
        return;
    }
    if layout::effect_layout(a.kind).is_none() && !matches!(a.kind, 1 | 3 | 10 | 14 | 15 | 26 | 32)
    {
        compare_native(&node, a.class, &a.native, &b.native, out);
        return;
    }
    let size = a.catalog().map_or(2, |node| node.struct_size as usize);
    compare_bytes(
        node.clone(),
        node_bytes(stock, a.offset, size),
        node_bytes(compiled, b.offset, size),
        &effect_mask(a.kind, size),
        out,
    );
    compare_label_facts(&node, &a.facts, &b.facts, out);
    match a.kind {
        32 => {
            for (x, y) in a.conditions.iter().zip(&b.conditions) {
                compare_condition(stock, compiled, x, y, "nested", out);
            }
            if a.conditions.len() != b.conditions.len() {
                out.push(Difference {
                    node: format!("{node} nested list length"),
                    offset: 0x10,
                    stock: (a.conditions.len() as u64).to_le_bytes().to_vec(),
                    compiled: (b.conditions.len() as u64).to_le_bytes().to_vec(),
                });
            }
        }
        10 => {
            let left = program_bytes(stock, a.offset + 0x18).unwrap_or_default();
            let right = program_bytes(compiled, b.offset + 0x18).unwrap_or_default();
            if left != right {
                out.push(Difference {
                    node: format!("{node} value program"),
                    offset: 0x18,
                    stock: left,
                    compiled: right,
                });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod coverage;
    use crate::sandbox_perk::action::fixtures::{
        Builder, drawn_pattern_action, precision_kill_action,
    };
    use crate::sandbox_perk::action::{
        CONDITION_ROW_CLASS, EFFECT_ROW_CLASS, GROUP_ACTIVATION, GROUP_EFFECTS, GROUP_REARM,
        GROUP_REMOVAL, PRIMARY_GROUP,
    };

    fn kill_attach_action(chance: f32) -> Vec<u8> {
        let mut out = Builder::new();
        let activation = out.kill(&[PRECISION], true, chance);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let attach = out.attach(
            0x80BC_5810,
            "content/sandbox/effects/trail/trail.entity.tft",
        );
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
        let duration = out.timer(5.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let cooldown = out.timer(2.5);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REARM,
            CONDITION_ROW_CLASS,
            &[cooldown],
        );
        out.finish()
    }

    #[test]
    fn a_drawn_pattern_action_recovers_its_program_in_authored_order() {
        let action = decode(&drawn_pattern_action()).unwrap();
        let program = decompile(&action, "Demo", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Drawn);
        assert_eq!(program.duration_ms, 0);
        assert_eq!(program.cooldown_ms, 0);
        assert_eq!(program.chance_permyriad, 10_000);
        // The payload stores effects last to first, so the authored order is reversed.
        assert!(matches!(program.actions[0], Action::Spawn { .. }));
        assert!(matches!(program.actions[1], Action::Pattern { .. }));
        assert_eq!(program.actions[0].asset().unwrap().graph, 0x80BC_2F21);
        assert_eq!(
            program.actions[1].asset().unwrap().path,
            "content/sandbox/weapons/demo/demo.pattern.tft"
        );
    }

    #[test]
    fn an_extend_timers_effect_recovers_when_it_nests_the_trigger() {
        let mut out = Builder::new();
        let activation = out.kill(&[PRECISION], true, 1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let nested = out.kill(&[PRECISION], true, 1.0);
        let extend = out.extend_timers(5.0, 10.0, &[nested]);
        let attach = out.attach(0x80BC_5810, "content/sandbox/effects/buff/buff.entity.tft");
        // Stored last to first: the attach runs first, then the extension.
        out.pointer_list(
            PRIMARY_GROUP + GROUP_EFFECTS,
            EFFECT_ROW_CLASS,
            &[extend, attach],
        );
        let duration = out.timer(5.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let action = decode(&out.finish()).unwrap();
        let program = decompile(&action, "Refresh", |tag| tag).unwrap();
        assert!(matches!(program.actions[0], Action::Attach { .. }));
        assert_eq!(
            program.actions[1],
            Action::ExtendTimers {
                extend_ms: 5_000,
                cap_ms: 10_000
            }
        );
        // A nested condition that is not the trigger is refused by name.
        let mut out = Builder::new();
        let activation = out.kill(&[PRECISION], true, 1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let nested = out.kill(&[GRENADE], false, 1.0);
        let extend = out.extend_timers(5.0, 5.0, &[nested]);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[extend]);
        let duration = out.timer(5.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let error = decompile(&decode(&out.finish()).unwrap(), "Refresh", |tag| tag).unwrap_err();
        assert!(error.0.contains("differs from the trigger"), "{error}");
    }

    #[test]
    fn a_named_property_effect_recovers_its_constant_and_refuses_other_programs() {
        let mut out = Builder::new();
        let activation = out.unconditional();
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let property = out.named_property(0x5EE2_66FC, 1.0, 2, 0, 1);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[property]);
        let payload = out.finish();
        let program = decompile(&decode(&payload).unwrap(), "Flag", |tag| tag).unwrap();
        assert_eq!(
            program.actions[0],
            Action::Property {
                key: 0x5EE2_66FC,
                target: 2,
                operation_byte: 0,
                removal: 1,
                value_bits: 1.0_f32.to_bits(),
                restore_bits: 0,
                ability_mask: 0,
                input: 0,
                flag: 1,
            }
        );
        let json = serde_json::to_string(&program.actions[0]).unwrap();
        assert!(json.contains("\"key\":\"0x5EE266FC\""), "{json}");
        assert!(json.contains("\"value_bits\":1.0"), "{json}");
        assert_eq!(
            serde_json::from_str::<Action>(&json).unwrap(),
            program.actions[0]
        );
        // A nonzero fast-path selector means the program is not the plain constant.
        let mut other = payload.clone();
        other[property + 0x44] = 1;
        let error = decompile(&decode(&other).unwrap(), "Flag", |tag| tag).unwrap_err();
        assert!(error.0.contains("value program"), "{error}");
        // Fidelity compares the program's bytes, not only the node.
        let mut changed = payload.clone();
        let action = decode(&changed).unwrap();
        let constants = crate::package_payload::relative_offset(
            property + 0x30,
            0,
            crate::package_payload::i64_at(&changed, property + 0x30).unwrap(),
        )
        .unwrap()
            + 16;
        changed[constants..constants + 4].copy_from_slice(&2.0_f32.to_le_bytes());
        drop(action);
        let differences = fidelity(&payload, &changed).unwrap();
        assert_eq!(differences.len(), 1);
        assert!(differences[0].node.ends_with("value program"));
    }

    #[test]
    fn ammunition_effects_recover_their_one_amount_and_refuse_filters_and_spreads() {
        let mut out = Builder::new();
        let activation = out.kill(&[], true, 1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        // Triple Tap's shape: one round into this weapon's magazine.
        let rounds = out.fixed_ammunition(&[], 1, 0);
        // A kill-to-reload shape: half the magazine capacity into the magazine.
        let fraction = out.proportional_ammunition(1, 1, 0.5);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_EFFECTS,
            EFFECT_ROW_CLASS,
            &[fraction, rounds],
        );
        let duration = out.timer(1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let payload = out.finish();
        let program = decompile(&decode(&payload).unwrap(), "Ammo", |tag| tag).unwrap();
        assert_eq!(program.actions[0], Action::add_rounds(1));
        assert_eq!(program.actions[1], Action::add_fraction(0.5));
        let json = serde_json::to_string(&program.actions[1]).unwrap();
        assert!(json.contains("\"fraction_bits\":0.5"), "{json}");
        assert!(!json.contains("overflow"), "{json}");
        assert_eq!(
            serde_json::from_str::<Action>(&json).unwrap(),
            program.actions[1]
        );
        // A source label filter is refused by name.
        let mut out = Builder::new();
        let activation = out.kill(&[], true, 1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let filtered = out.fixed_ammunition(&[PRECISION], 1, 0);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[filtered]);
        let duration = out.timer(1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let error = decompile(&decode(&out.finish()).unwrap(), "Ammo", |tag| tag).unwrap_err();
        assert!(error.0.contains("source labels"), "{error}");
        // Two amounts in one node are refused, since the program carries one.
        let mut out = Builder::new();
        let activation = out.kill(&[], true, 1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let spread = out.fixed_ammunition(&[], 2, -1);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[spread]);
        let duration = out.timer(1.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let error = decompile(&decode(&out.finish()).unwrap(), "Ammo", |tag| tag).unwrap_err();
        assert!(error.0.contains("2 ammunition targets"), "{error}");
    }

    fn always_attach_action(repeat: Option<f32>) -> Vec<u8> {
        let mut out = Builder::new();
        let activation = out.unconditional();
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
        if let Some(seconds) = repeat {
            let timer = out.timer(seconds);
            out.pointer_list(PRIMARY_GROUP + GROUP_REARM, CONDITION_ROW_CLASS, &[timer]);
        }
        out.finish()
    }

    #[test]
    fn an_always_active_action_recovers_with_its_repeat_interval() {
        let action = decode(&always_attach_action(Some(3.0))).unwrap();
        let program = decompile(&action, "Aura", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Always);
        assert_eq!(program.duration_ms, 0);
        assert_eq!(program.cooldown_ms, 3_000);
        assert!(matches!(program.actions[0], Action::Attach { .. }));
        let plain = decompile(
            &decode(&always_attach_action(None)).unwrap(),
            "Aura",
            |tag| tag,
        )
        .unwrap();
        assert_eq!(plain.cooldown_ms, 0);
    }

    #[test]
    fn attach_technical_fields_survive_the_round_trip_and_default_when_absent() {
        let mut out = Builder::new();
        let activation = out.unconditional();
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let attach = out.attach_with(
            0x80BC_5810,
            "content/sandbox/effects/aura/aura.entity.tft",
            3,
            [0x4113_6E32, 0x95E7_400C],
            [1.0; 4],
        );
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
        let action = decode(&out.finish()).unwrap();
        let program = decompile(&action, "Aura", |tag| tag).unwrap();
        let Action::Attach {
            mode,
            keys,
            float_bits,
            ..
        } = &program.actions[0]
        else {
            panic!("expected an attach action");
        };
        assert_eq!(*mode, 3);
        assert_eq!(*keys, [0x4113_6E32, 0x95E7_400C]);
        assert_eq!(*float_bits, [1.0_f32.to_bits(); 4]);
        let json = serde_json::to_string(&program).unwrap();
        assert!(json.contains("\"mode\":3"), "{json}");
        assert!(json.contains("\"0x41136E32\""), "{json}");
        assert!(json.contains("\"float_bits\":[1.0,1.0,1.0,1.0]"), "{json}");
        assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
        // A recipe written before these fields existed reads back as the compiler's old output.
        let legacy = r#"{"operation":"attach","asset":{"graph":1}}"#;
        let action = serde_json::from_str::<Action>(legacy).unwrap();
        assert_eq!(
            action,
            Action::attach(Asset {
                graph: 1,
                path: String::new(),
                values: Vec::new(),
            })
        );
        assert_eq!(serde_json::to_string(&action).unwrap(), legacy);
    }

    #[test]
    fn an_always_active_action_recovers_its_ending_event_key() {
        let mut out = Builder::new();
        let activation = out.unconditional();
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
        let ending = out.event_key(0xA628_8DD1);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[ending],
        );
        let timer = out.timer(3.0);
        out.pointer_list(PRIMARY_GROUP + GROUP_REARM, CONDITION_ROW_CLASS, &[timer]);
        let program = decompile(&decode(&out.finish()).unwrap(), "Aura", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Always);
        assert_eq!(program.removal_key, Some(0xA628_8DD1));
        assert_eq!(program.cooldown_ms, 3_000);
        let json = serde_json::to_string(&program).unwrap();
        assert!(json.contains("\"removal_key\":\"0xA6288DD1\""), "{json}");
        assert_eq!(serde_json::from_str::<Program>(&json).unwrap(), program);
        let mut plain = program.clone();
        plain.removal_key = None;
        assert!(
            !serde_json::to_string(&plain)
                .unwrap()
                .contains("removal_key")
        );
        plain.trigger = Trigger::Drawn;
        plain.removal_key = Some(1);
        assert!(plain.validate().unwrap_err().contains("always-active"));
    }

    #[test]
    fn an_empty_activation_list_reads_as_always_active() {
        let mut out = Builder::new();
        let attach = out.attach(0x80BC_5810, "content/sandbox/effects/aura/aura.entity.tft");
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[attach]);
        let action = decode(&out.finish()).unwrap();
        let program = decompile(&action, "Aura", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Always);
    }

    #[test]
    fn a_kill_action_recovers_its_trigger_timing_and_chance() {
        let action = decode(&kill_attach_action(0.25)).unwrap();
        let program = decompile(&action, "Trail", |tag| tag + 1).unwrap();
        assert_eq!(program.trigger, Trigger::PrecisionKill);
        assert_eq!(program.duration_ms, 5_000);
        assert_eq!(program.cooldown_ms, 2_500);
        assert_eq!(program.chance_permyriad, 2_500);
        assert_eq!(program.actions.len(), 1);
        assert_eq!(program.actions[0].asset().unwrap().graph, 0x80BC_5811);
    }

    #[test]
    fn a_weighted_spawn_effect_recovers_its_complete_native_record() {
        // The fixture's nested condition matches its trigger, so the extension is authorable.
        let action = decode(&precision_kill_action()).unwrap();
        let program = decompile(&action, "Outlaw", |tag| tag).unwrap();
        assert!(matches!(program.actions[0], Action::ExtendTimers { .. }));
        // Weighted spawn records now have a complete native authoring path.
        let mut out = Builder::new();
        let activation = out.unconditional();
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        // A label-free weighted spawn record is preserved even with empty choices.
        let unknown = out.node(0x8080_3E47, 64);
        out.bytes[unknown] = 13;
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[unknown]);
        let decoded = decode(&out.finish()).unwrap();
        let program = decompile(&decoded, "Weighted Spawn", |tag| tag).unwrap();
        assert!(
            matches!(&program.actions[0],Action::Native{node} if node.kind==13 && node.bytes==decoded.groups[0].effects[0].native)
        );
    }

    #[test]
    fn plain_scalar_nodes_recover_verbatim_as_native_trigger_ending_and_effect() {
        let mut out = Builder::new();
        // A Two Event Flag Masks trigger with masks 3 and 4.
        let activation = out.condition(0x8080_3DFB, 6, 12);
        out.bytes[activation + 8] = 3;
        out.bytes[activation + 9] = 4;
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        // Three Weapon Float Overrides, retained, and a one-shot Publish Named Player Event.
        let overrides = out.node(0x8080_3E0B, 16);
        out.bytes[overrides] = 29;
        out.bytes[overrides + 1] = 1;
        out.f32(overrides + 4, 0.5);
        out.f32(overrides + 12, 2.0);
        let publish = out.node(0x8080_3E1E, 8);
        out.bytes[publish] = 43;
        out.u32(publish + 4, 0x5EE2_66FC);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_EFFECTS,
            EFFECT_ROW_CLASS,
            &[publish, overrides],
        );
        // An Event Key Match of kind 29 ends it, which the ending-key model does not cover.
        let ending = out.condition(0x8080_3DEC, 29, 12);
        out.u32(ending + 8, 0xA628_8DD1);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[ending],
        );
        let payload = out.finish();
        let decoded = decode(&payload).unwrap();
        let program = decompile(&decoded, "Native", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Native);
        let trigger = program.native_trigger.as_ref().unwrap();
        assert_eq!(trigger.kind, 6);
        assert_eq!(&trigger.bytes, &payload[activation..activation + 12]);
        let removal = program.native_removal.as_ref().unwrap();
        assert_eq!(removal.kind, 29);
        assert_eq!(&removal.bytes[8..], &payload[ending + 8..ending + 12]);
        assert_eq!(program.actions.len(), 2);
        let Action::Native { node } = &program.actions[0] else {
            panic!("expected a native effect");
        };
        assert_eq!(node.kind, 29);
        assert_eq!(&node.bytes, &payload[overrides..overrides + 16]);
        assert_eq!(program.actions[1].label(), "Publish Named Player Event");
        assert!(!program.actions[1].retained());
        // The rebuilt nodes compile back to the same bytes, header included.
        let mut compiled = crate::sandbox_perk::program::compiler::Payload::new();
        let node = compiled
            .native_condition(program.native_trigger.as_ref().unwrap(), 0)
            .unwrap();
        assert_eq!(
            &compiled.bytes[node..node + 12],
            &payload[activation..activation + 12]
        );
        assert!(fidelity(&payload, &payload).unwrap().is_empty());
    }

    #[test]
    fn fidelity_reports_only_unmasked_native_differences() {
        let stock = kill_attach_action(1.0);
        let mut compiled = kill_attach_action(1.0);
        assert!(fidelity(&stock, &compiled).unwrap().is_empty());
        let action = decode(&compiled).unwrap();
        let attach = action.groups[0].effects[0].offset;
        compiled[attach + 0x20] = 0x40;
        let differences = fidelity(&stock, &compiled).unwrap();
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].offset, 0x20);
        assert_eq!(differences[0].node, "Create Entity");
        // The compiled ordinal byte is derived and never counts as a difference.
        let activation = action.groups[0].activation[0].offset;
        compiled[activation + 7] = 9;
        assert_eq!(fidelity(&stock, &compiled).unwrap().len(), 1);
    }

    #[test]
    #[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with a clean Shadowkeep package directory"]
    fn installed_actions_report_how_many_are_editable_today() {
        use crate::investment_schema::{
            GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag,
        };
        use crate::sandbox_perk::{
            SANDBOX_PERK_RUNTIME_MAP_TAG, finished_sandbox_perk_at, finished_sandbox_perk_count,
            sandbox_perk_runtime_assignment,
        };
        use std::collections::{BTreeMap, BTreeSet};
        use tiger_pkg::TagHash;

        let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must name a clean package directory");
        let install = std::path::Path::new(&packages)
            .parent()
            .expect("install root");
        let manager = crate::package_runtime::open_shadowkeep_packages(install).expect("open");
        let globals_tag =
            crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
                .expect("resolve investment_globals");
        let globals = manager.read_tag(globals_tag).expect("read globals");
        let catalog_tag = TagHash(
            investment_globals_table_tag(&globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)
                .expect("resolve finished perk catalog"),
        );
        let catalog = manager.read_tag(catalog_tag).expect("read catalog");
        let runtime_map = manager
            .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
            .expect("read runtime map");
        let mut actions = BTreeSet::new();
        for index in 0..finished_sandbox_perk_count(&catalog).expect("count") {
            let perk = finished_sandbox_perk_at(&catalog, index).expect("perk");
            if let Some(assignment) =
                sandbox_perk_runtime_assignment(&runtime_map, perk.runtime_key).expect("assign")
            {
                actions.insert(assignment.runtime_tag);
            }
        }
        let mut total = 0;
        let mut refusals = BTreeMap::<String, usize>::new();
        let mut compile_refused = 0;
        let mut exact = 0;
        let mut approximate = 0;
        let mut differences = BTreeMap::<(String, usize), usize>::new();
        let mut action_kinds = BTreeMap::<&str, usize>::new();
        for tag in actions {
            let Ok(payload) = manager.read_tag(TagHash(tag)) else {
                continue;
            };
            total += 1;
            let decoded = decode(&payload).expect("decode");
            let program = match decompile(&decoded, "Census", |tag| tag) {
                Ok(program) => program,
                Err(reason) => {
                    *refusals.entry(reason.0).or_default() += 1;
                    continue;
                }
            };
            for action in &program.actions {
                *action_kinds.entry(action.label()).or_default() += 1;
            }
            let compiled = match super::super::compile(&manager, &program) {
                Ok(compiled) => compiled,
                Err(reason) => {
                    compile_refused += 1;
                    let key = format!(
                        "compiler: {}",
                        reason.split(" 0x").next().unwrap_or(&reason)
                    );
                    *refusals.entry(key).or_default() += 1;
                    continue;
                }
            };
            let found = fidelity(&payload, &compiled.payload).expect("fidelity");
            if found.is_empty() {
                exact += 1;
            } else {
                approximate += 1;
                for difference in found {
                    *differences
                        .entry((difference.node, difference.offset))
                        .or_default() += 1;
                }
            }
        }
        eprintln!(
            "{total} actions: {exact} exact round trips, {approximate} approximate, {compile_refused} refused by the compiler"
        );
        eprintln!("refusals: {refusals:#?}");
        eprintln!("recovered actions by kind: {action_kinds:?}");
        let mut ranked = differences.into_iter().collect::<Vec<_>>();
        ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        for ((node, offset), count) in ranked.iter().take(25) {
            eprintln!("  {count:4} x {node} +0x{offset:X}");
        }
        assert!(
            exact + approximate > 0,
            "no installed action recovered a program"
        );
    }
}
