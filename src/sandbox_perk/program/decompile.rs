//! Recovers an editable program from a decoded stock action when the action lies inside
//! the shape the compiler can emit.
//!
//! A recovered program is the compiler's view of the action, so re-emitting it produces a
//! fresh action rather than a copy. `fidelity` reports every native byte the round trip would
//! not reproduce, so the workbench can say whether a conversion is exact before offering it.

use super::{
    Action, AmmunitionStore, AmmunitionTarget, Asset, EMPTY_KEY, NativeGroup, NativeNode,
    NativeRecord, Policy, Position, Program, Trigger,
};
use crate::package_payload::{i64_at, relative_offset, u64_at};
use crate::sandbox_perk::action::{
    CREATE_ENTITY_FLOAT_LABELS, CREATE_ENTITY_KEY_LABELS, CREATE_ENTITY_MODE_LABEL, DecodedAction,
    DecodedCondition, DecodedEffect, DecodedGroup, FactValue, NAMED_PROPERTY_LABELS, Probability,
    decode, layout,
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
    let Some(group) = action.groups.first() else {
        return Err(Unsupported("This perk runs no program.".into()));
    };
    // The first condition of each list is the typed reading. The engine accepts any condition
    // in a list, so the rest are alternatives carried verbatim in their stock order.
    let primary_activation = &group.activation[..group.activation.len().min(1)];
    let primary_removal = &group.removal[..group.removal.len().min(1)];
    let (mut trigger, chance_permyriad, mut native_trigger) = trigger_of(primary_activation)?;
    if matches!(trigger, Trigger::Equipped | Trigger::Drawn) && group.removal.is_empty() {
        // The paired triggers always compile an ending. A stock weapon event without one is
        // carried as a native trigger, whose program may run until the perk is removed.
        trigger = Trigger::Native;
        native_trigger = Some(native_condition(&group.activation[0]));
    }
    let ending = duration_of(trigger, primary_removal)?;
    let rearm = cooldown_of(trigger, &group.rearm[..group.rearm.len().min(1)]);
    let alternatives = |list: &[DecodedCondition]| -> Vec<NativeNode> {
        list.iter().skip(1).map(native_condition).collect()
    };
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
        cooldown_ms: rearm.cooldown_ms,
        chance_permyriad,
        actions,
        removal_key: ending.removal_key,
        native_trigger,
        native_removal: ending.native_removal,
        native: None,
        auxiliary: action.auxiliary.iter().map(NativeRecord::from).collect(),
        policy: policy_of(action),
        alternative_triggers: alternatives(&group.activation),
        alternative_removals: alternatives(&group.removal),
        native_rearm: rearm.native_rearm,
        alternative_rearms: alternatives(&group.rearm),
        additional_groups: action
            .groups
            .iter()
            .skip(1)
            .map(|group| native_group(group, &graph_of))
            .collect::<Result<_, _>>()?,
    };
    program.validate().map_err(Unsupported)?;
    Ok(program)
}

/// How a stock action was recovered for editing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Recovery {
    /// The typed program model holds the whole action, so the workbench shows typed controls.
    Typed,
    /// The action lies outside the typed model and is carried in its complete native form.
    /// The message says which shape the model does not hold yet.
    NativeForm(String),
}

/// Recovers a stock action for editing without ever refusing it.
///
/// The typed program is preferred because it gives the same controls a custom perk has. When
/// the action lies outside that model the complete native form carries it instead, so every
/// stock perk opens and the caller can say which representation it received.
pub fn recover(
    payload: &[u8],
    name: &str,
    graph_of: impl Fn(u32) -> u32,
) -> Result<(Program, Recovery), String> {
    let decoded = decode(payload)?;
    match decompile(&decoded, name, graph_of) {
        Ok(program) => Ok((program, Recovery::Typed)),
        Err(Unsupported(reason)) => Ok((
            Program::from_native(payload, name)?,
            Recovery::NativeForm(reason),
        )),
    }
}

/// A further program of the action, every node carried verbatim in native list order.
fn native_group(
    group: &DecodedGroup,
    graph_of: &impl Fn(u32) -> u32,
) -> Result<NativeGroup, Unsupported> {
    let conditions = |list: &[DecodedCondition]| list.iter().map(native_condition).collect();
    Ok(NativeGroup {
        activation: conditions(&group.activation),
        effects: group
            .effects
            .iter()
            .map(|effect| {
                native_effect(effect, graph_of).ok_or_else(|| {
                    Unsupported(format!(
                        "The {} effect of a further program cannot be carried.",
                        effect.name()
                    ))
                })
            })
            .collect::<Result<_, _>>()?,
        removal: conditions(&group.removal),
        rearm: conditions(&group.rearm),
    })
}

/// The root's policy settings, when any differs from what the compiler writes by default.
fn policy_of(action: &DecodedAction) -> Option<Policy> {
    (action.policy != 0
        || action.policy_modifier != 0
        || action.root_key != EMPTY_KEY
        || action.policy_configuration.is_some())
    .then(|| Policy {
        selector: action.policy,
        modifier: action.policy_modifier,
        key: action.root_key,
        configuration: action.policy_configuration.as_ref().map(NativeRecord::from),
    })
}

/// Preserve every native byte and allocation, including unnamed scalar fields.
fn native_condition(condition: &DecodedCondition) -> NativeNode {
    NativeNode {
        kind: condition.kind,
        bytes: condition.native.clone(),
    }
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
        return Ok((Trigger::Native, 10_000, Some(native_condition(condition))));
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
            return Ok((Trigger::Native, 10_000, Some(native_condition(condition))));
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
        // A kill filter outside the named trigger set keeps its labels and chance in the node.
        2 => match kill_trigger(condition) {
            Ok(trigger) => trigger,
            Err(_) => return Ok((Trigger::Native, 10_000, Some(native_condition(condition)))),
        },
        _ => return Ok((Trigger::Native, 10_000, Some(native_condition(condition)))),
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
        return Ok(Ending {
            native_removal: Some(native_condition(condition)),
            ..Ending::default()
        });
    }
    let condition = single(removal, "removal")?;
    let expected = match trigger {
        Trigger::Equipped => Some(15),
        Trigger::Drawn => Some(17),
        _ => None,
    };
    if expected == Some(condition.kind) {
        return Ok(Ending::default());
    }
    if expected.is_none()
        && let Some(seconds) = timer_seconds(condition)
    {
        return Ok(Ending {
            duration_ms: millis(seconds),
            ..Ending::default()
        });
    }
    // Any other ending stays native: kill perks that end on an unconditional check, drawn
    // perks that end on a different weapon event.
    Ok(Ending {
        native_removal: Some(native_condition(condition)),
        ..Ending::default()
    })
}

fn effect_key(condition: &DecodedCondition) -> Option<u32> {
    condition.facts.iter().find_map(|fact| match fact.value {
        FactValue::Key(key) if fact.label == "Event Key" => Some(key),
        _ => None,
    })
}

/// How a program becomes ready again: a cooldown timer, a native node, or nothing.
struct Rearm {
    cooldown_ms: u32,
    native_rearm: Option<NativeNode>,
}

/// The primary rearm condition. A timer on a trigger with a cooldown is the typed reading.
/// Anything else, including a rearm on a trigger without a cooldown, stays native.
fn cooldown_of(trigger: Trigger, rearm: &[DecodedCondition]) -> Rearm {
    match rearm {
        [] => Rearm {
            cooldown_ms: 0,
            native_rearm: None,
        },
        [condition, ..] => match timer_seconds(condition).filter(|_| trigger.supports_cooldown()) {
            Some(seconds) => Rearm {
                cooldown_ms: millis(seconds),
                native_rearm: None,
            },
            None => Rearm {
                cooldown_ms: 0,
                native_rearm: Some(native_condition(condition)),
            },
        },
    }
}

fn action_of(
    effect: &DecodedEffect,
    graph_of: &impl Fn(u32) -> u32,
    trigger: Trigger,
    activation: Option<&DecodedCondition>,
) -> Result<Action, Unsupported> {
    // Shapes outside the typed actions stay verbatim in a native node: timer extensions on
    // perks without a kill trigger, value programs, source label filters, multi-target fills.
    let native = |refusal: Unsupported| {
        native_effect(effect, graph_of)
            .map(|node| Action::Native { node })
            .ok_or(refusal)
    };
    match effect.kind {
        32 => return extend_timers_of(effect, trigger, activation).or_else(native),
        10 => return property_of(effect).or_else(native),
        8 => return adjust_component_of(effect).or_else(native),
        42 => return accumulator_update_of(effect).or_else(native),
        7 => return ability_property_of(effect).or_else(native),
        47 => return scalar_effect_of(effect, 8, &[2, 3]).or_else(native),
        35 => return scalar_effect_of(effect, 12, &[9, 10, 11]).or_else(native),
        6 => return scalar_effect_of(effect, 4, &[]).or_else(native),
        30 => return scalar_effect_of(effect, 3, &[]).or_else(native),
        14 | 15 => return ammunition_of(effect).or_else(native),
        1 | 3 | 26 => {}
        _ => {
            if let Some(node) = native_effect(effect, graph_of) {
                return Ok(Action::Native { node });
            }
        }
    }
    let Some(tag) = effect.referenced_tag else {
        // An entity effect without a reference has no asset to edit. It stays verbatim.
        return native(Unsupported(format!(
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

fn number_bits(effect: &DecodedEffect, label: &str) -> Option<u32> {
    match effect_fact(effect, label) {
        Some(FactValue::Number(value)) => Some(value.to_bits()),
        _ => None,
    }
}

/// A Component Value Adjustment is typed when its value program is the plain constant and
/// the two unmapped words hold the value every stock node stores, so the compiler's template
/// reproduces the node exactly.
fn adjust_component_of(effect: &DecodedEffect) -> Result<Action, Unsupported> {
    let Some(value_bits) = number_bits(effect, crate::sandbox_perk::action::CONSTANT_VALUE) else {
        return Err(Unsupported(
            "The Component Value Adjustment effect uses a value program Parhelion cannot author yet.".into(),
        ));
    };
    let word = |at: usize| {
        effect
            .native
            .get(at..at + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    if effect.native.get(1) != Some(&0) || word(0x38) != Some(1) || word(0x40) != Some(1) {
        return Err(Unsupported(
            "The Component Value Adjustment effect sets bytes outside the stock template.".into(),
        ));
    }
    let (Some(scale_bits), Some(limit_bits)) =
        (number_bits(effect, "Scale"), number_bits(effect, "Limit"))
    else {
        return Err(Unsupported(
            "The Component Value Adjustment effect has no readable scale.".into(),
        ));
    };
    Ok(Action::AdjustComponent {
        target: selector_fact(effect, "Target Selector").unwrap_or(0),
        flag: selector_fact(effect, "Flag Byte").unwrap_or(0),
        option: selector_fact(effect, "Option Byte").unwrap_or(0),
        scale_bits,
        limit_bits,
        value_bits,
        input: selector_fact(effect, "Input Selector").unwrap_or(0xFF),
    })
}

/// The small fixed-layout effects whose every field is named. The compiler's template writes
/// the retained byte and the named lanes and leaves the rest zero, so a node that sets any
/// byte outside that stays native and keeps its own bytes.
///
/// The retained byte is compared against what the compiler would write for the recovered
/// action rather than against a fixed value, since it differs by kind: every stock Transmat
/// Context node clears it while the other three set it.
fn scalar_effect_of(
    effect: &DecodedEffect,
    size: usize,
    spare: &[usize],
) -> Result<Action, Unsupported> {
    let n = &effect.native;
    let refused = || {
        Unsupported(format!(
            "The {} effect sets bytes outside the stock template.",
            effect.name()
        ))
    };
    if n.len() != size || spare.iter().any(|at| n.get(*at) != Some(&0)) {
        return Err(refused());
    }
    let key = |at: usize| u32::from_le_bytes([n[at], n[at + 1], n[at + 2], n[at + 3]]);
    let action = match effect.kind {
        47 => Action::TransmatContext { key: key(4) },
        35 => Action::OverrideHostKey {
            target: n[2],
            interface: n[3],
            key: key(4),
            apply_to_player: n[8] != 0,
        },
        6 => Action::SetDamageType {
            mode: n[2],
            keep_after_removal: n[3] != 0,
        },
        _ => Action::WeaponReferenceCount { selector: n[2] },
    };
    if n[1] != u8::from(action.retained()) {
        return Err(refused());
    }
    Ok(action)
}

/// An Ability Property is three fields. The compiler's template writes the rest as zero, so
/// a node with anything else there stays native.
fn ability_property_of(effect: &DecodedEffect) -> Result<Action, Unsupported> {
    let n = &effect.native;
    if n.len() != 12
        || n[1] != 1
        || n[3] != 0
        || n.get(9..12).is_none_or(|tail| tail.iter().any(|b| *b != 0))
    {
        return Err(Unsupported(
            "The Ability Property effect sets bytes outside the stock template.".into(),
        ));
    }
    let Some(FactValue::Key(key)) = effect_fact(effect, "Property Key") else {
        return Err(Unsupported(
            "The Ability Property effect has no readable property key.".into(),
        ));
    };
    Ok(Action::AbilityProperty {
        target: n[2],
        key: *key,
        option: n[8],
    })
}

/// An Accumulator Update is two fields. The retained byte must be clear, as in every stock
/// node, for the compiler's template to reproduce it.
fn accumulator_update_of(effect: &DecodedEffect) -> Result<Action, Unsupported> {
    if effect.native.get(1) != Some(&0) || effect.native.get(3) != Some(&0) {
        return Err(Unsupported(
            "The Accumulator Update effect sets bytes outside the stock template.".into(),
        ));
    }
    let Some(value_bits) = number_bits(effect, "Value") else {
        return Err(Unsupported(
            "The Accumulator Update effect has no readable value.".into(),
        ));
    };
    Ok(Action::UpdateAccumulator {
        mode: selector_fact(effect, "Mode").unwrap_or(1),
        value_bits,
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
    compare_auxiliary(&stock_action, &compiled_action, &mut out);
    compare_policy(&stock_action, &compiled_action, &mut out);
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
    if stock_action.groups.is_empty() || compiled_action.groups.is_empty() {
        return Err("An action without a program cannot be compared".into());
    }
    for (left, right) in stock_action.groups.iter().zip(&compiled_action.groups) {
        compare_group(stock, compiled, left, right, &mut out);
    }
    if stock_action.groups.len() != compiled_action.groups.len() {
        out.push(Difference {
            node: "Program Count".into(),
            offset: 0,
            stock: (stock_action.groups.len() as u64).to_le_bytes().to_vec(),
            compiled: (compiled_action.groups.len() as u64).to_le_bytes().to_vec(),
        });
    }
    Ok(out)
}

/// Compares one program's four lists node by node, then their lengths.
fn compare_group(
    stock: &[u8],
    compiled: &[u8],
    left: &DecodedGroup,
    right: &DecodedGroup,
    out: &mut Vec<Difference>,
) {
    for (role, a, b) in [
        ("activation", &left.activation, &right.activation),
        ("removal", &left.removal, &right.removal),
        ("rearm", &left.rearm, &right.rearm),
    ] {
        for (x, y) in a.iter().zip(b) {
            compare_condition(stock, compiled, x, y, role, out);
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
        compare_effect(stock, compiled, x, y, out);
    }
    if left.effects.len() != right.effects.len() {
        out.push(Difference {
            node: "Effect List Length".into(),
            offset: 0,
            stock: (left.effects.len() as u64).to_le_bytes().to_vec(),
            compiled: (right.effects.len() as u64).to_le_bytes().to_vec(),
        });
    }
}

fn node_bytes(payload: &[u8], offset: usize, size: usize) -> &[u8] {
    payload.get(offset..offset + size).unwrap_or(&[])
}

/// The policy selector and modifier sit inside the routing range compared elsewhere. The root
/// key and the configuration record are carried verbatim, so they are compared byte for byte.
fn compare_policy(stock: &DecodedAction, compiled: &DecodedAction, out: &mut Vec<Difference>) {
    compare_bytes(
        "Root Key".into(),
        &stock.root_key.to_le_bytes(),
        &compiled.root_key.to_le_bytes(),
        &[],
        out,
    );
    match (&stock.policy_configuration, &compiled.policy_configuration) {
        (None, None) => {}
        (Some(x), Some(y)) => {
            if x.class != y.class {
                out.push(Difference {
                    node: "Policy Configuration Class".into(),
                    offset: 0,
                    stock: x.class.to_le_bytes().to_vec(),
                    compiled: y.class.to_le_bytes().to_vec(),
                });
            }
            compare_bytes(
                format!("Policy Configuration 0x{:08X}", x.class),
                &x.bytes,
                &y.bytes,
                &[],
                out,
            );
        }
        (x, y) => out.push(Difference {
            node: "Policy Configuration Presence".into(),
            offset: 0,
            stock: vec![u8::from(x.is_some())],
            compiled: vec![u8::from(y.is_some())],
        }),
    }
}

/// Auxiliary records are carried verbatim, so every byte of every record is compared.
fn compare_auxiliary(stock: &DecodedAction, compiled: &DecodedAction, out: &mut Vec<Difference>) {
    for (index, (x, y)) in stock.auxiliary.iter().zip(&compiled.auxiliary).enumerate() {
        let node = format!("Auxiliary Record {index} 0x{:08X}", x.class);
        if x.class != y.class {
            out.push(Difference {
                node: format!("{node} Class"),
                offset: 0,
                stock: x.class.to_le_bytes().to_vec(),
                compiled: y.class.to_le_bytes().to_vec(),
            });
        }
        compare_bytes(node, &x.bytes, &y.bytes, &[], out);
    }
    if stock.auxiliary.len() != compiled.auxiliary.len() {
        out.push(Difference {
            node: "Auxiliary Record List Length".into(),
            offset: 0,
            stock: (stock.auxiliary.len() as u64).to_le_bytes().to_vec(),
            compiled: (compiled.auxiliary.len() as u64).to_le_bytes().to_vec(),
        });
    }
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
    fn auxiliary_records_are_carried_verbatim_through_the_round_trip() {
        use crate::sandbox_perk::action::{AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS};
        // The three leaf classes stock actions list at the root, with representative bytes.
        let key_record = {
            let mut bytes = [0_u8; 24];
            bytes[..4].copy_from_slice(&2_u32.to_le_bytes());
            bytes[4..8].copy_from_slice(&0xA43A_8C2E_u32.to_le_bytes());
            bytes[8..12].copy_from_slice(&1.5_f32.to_le_bytes());
            bytes
        };
        // Offset 0x08 is a declared reference and stays null in every stock record. The
        // resource tag sits at 0x10.
        let mut tag_record = [0_u8; 24];
        tag_record[..4].copy_from_slice(&3_u32.to_le_bytes());
        tag_record[16..20].copy_from_slice(&0x80BC_5810_u32.to_le_bytes());
        let mut small_record = [0_u8; 8];
        small_record[..4].copy_from_slice(&0x10FF_0000_u32.to_le_bytes());
        small_record[4..].copy_from_slice(&0x599B_031D_u32.to_le_bytes());

        let mut out = Builder {
            bytes: drawn_pattern_action(),
        };
        let mut nodes = Vec::new();
        for (class, bytes) in [
            (0x8080_4085, &key_record[..]),
            (0x8080_4087, &tag_record[..]),
            (0x8080_2A20, &small_record[..]),
        ] {
            let at = out.node(class, bytes.len());
            out.bytes[at..at + bytes.len()].copy_from_slice(bytes);
            nodes.push(at);
        }
        out.pointer_list(AUXILIARY_RECORDS, AUXILIARY_ROW_CLASS, &nodes);
        let payload = out.finish();

        let action = decode(&payload).unwrap();
        assert_eq!(
            action
                .auxiliary
                .iter()
                .map(|record| (record.class, record.bytes.as_slice()))
                .collect::<Vec<_>>(),
            vec![
                (0x8080_4085, &key_record[..]),
                (0x8080_4087, &tag_record[..]),
                (0x8080_2A20, &small_record[..]),
            ]
        );
        let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
        assert_eq!(recovery, Recovery::Typed);
        assert_eq!(program.auxiliary.len(), 3);
        assert_eq!(program.auxiliary[0].bytes, key_record.to_vec());

        let compiled = super::super::compiler::assemble(&program, None).unwrap();
        assert_eq!(
            decode(&compiled.payload).unwrap().auxiliary,
            action.auxiliary
        );
        // The fixture is not byte-faithful to compiler routing metadata. The records are.
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(
            differences
                .iter()
                .all(|difference| !difference.node.starts_with("Auxiliary")),
            "{differences:?}"
        );

        // Dropping a record is a reported difference, not a refusal.
        let mut trimmed = program.clone();
        trimmed.auxiliary.pop();
        let compiled = super::super::compiler::assemble(&trimmed, None).unwrap();
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(
            differences
                .iter()
                .any(|difference| difference.node == "Auxiliary Record List Length"),
            "{differences:?}"
        );

        // A record of the wrong size for its class is refused before it reaches the compiler.
        let mut wrong = program;
        wrong.auxiliary[2].bytes.push(0);
        assert!(wrong.validate().unwrap_err().contains("8"));
    }

    #[test]
    fn alternative_conditions_are_carried_verbatim_beside_the_typed_reading() {
        let mut out = Builder::new();
        let draw = out.draw();
        let also_starts = out.event_key(0x1234_5678);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[draw, also_starts],
        );
        let pattern = out.pattern(0x8161_F73A, "content/sandbox/weapons/demo/demo.pattern.tft");
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[pattern]);
        let holster = out.holster();
        let also_ends = out.timer(4.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[holster, also_ends],
        );
        let payload = out.finish();

        let action = decode(&payload).unwrap();
        let program = decompile(&action, "Demo", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Drawn);
        assert_eq!(program.alternative_triggers.len(), 1);
        assert_eq!(program.alternative_triggers[0].kind, 30);
        assert_eq!(program.alternative_removals.len(), 1);
        assert_eq!(program.alternative_removals[0].kind, 1);
        assert_eq!(
            program.alternative_removals[0].bytes,
            action.groups[0].removal[1].native
        );

        let compiled = super::super::compiler::assemble(&program, None).unwrap();
        let again = decode(&compiled.payload).unwrap();
        let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&again.groups[0].activation), vec![16, 30]);
        assert_eq!(kinds(&again.groups[0].removal), vec![17, 1]);
        // The alternative timer takes a timer slot, and both lists route their events.
        assert_eq!(again.timer_budget, 1);
        assert_eq!(again.activation_event_mask, (1 << 16) | (1 << 30));
        assert_eq!(again.removal_event_mask, (1 << 17) | (1 << 1));
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(
            differences
                .iter()
                .all(|difference| !difference.node.contains("list length")),
            "{differences:?}"
        );

        // Dropping an alternative is reported, not refused.
        let mut trimmed = program.clone();
        trimmed.alternative_removals.clear();
        let compiled = super::super::compiler::assemble(&trimmed, None).unwrap();
        assert!(
            fidelity(&payload, &compiled.payload)
                .unwrap()
                .iter()
                .any(|difference| difference.node == "removal list length")
        );

        // An alternative of a kind without a recovered layout is refused before it reaches the
        // compiler.
        let mut wrong = program;
        wrong.alternative_removals[0].kind = 200;
        assert!(
            wrong
                .validate()
                .unwrap_err()
                .contains("no recovered native layout")
        );
    }

    #[test]
    fn unnamed_label_filters_are_carried_in_native_nodes() {
        let mut out = Builder::new();
        let activation = out.kill(&[0x599B_031D], false, 0.5);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_ACTIVATION,
            CONDITION_ROW_CLASS,
            &[activation],
        );
        let rounds = out.fixed_ammunition(&[0x599B_031D], 1, 0);
        out.pointer_list(PRIMARY_GROUP + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[rounds]);
        let duration = out.timer(5.0);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REMOVAL,
            CONDITION_ROW_CLASS,
            &[duration],
        );
        let payload = out.finish();

        let action = decode(&payload).unwrap();
        let program = decompile(&action, "Demo", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::Native);
        let trigger = program.native_trigger.as_ref().unwrap();
        assert_eq!(trigger.kind, 2);
        assert_eq!(trigger.bytes, action.groups[0].activation[0].native);
        assert!(program.has_kill_trigger());
        assert_eq!(program.duration_ms, 5_000);
        let [Action::Native { node }] = program.actions.as_slice() else {
            panic!("{:?}", program.actions);
        };
        assert_eq!(node.kind, action.groups[0].effects[0].kind);
        assert_eq!(node.bytes, action.groups[0].effects[0].native);

        let compiled = super::super::compiler::assemble(&program, None).unwrap();
        let again = decode(&compiled.payload).unwrap();
        assert_eq!(
            again.groups[0].activation[0].native,
            action.groups[0].activation[0].native
        );
        assert_eq!(
            again.groups[0].effects[0].native,
            action.groups[0].effects[0].native
        );
    }

    #[test]
    fn execution_policies_are_carried_verbatim_through_the_round_trip() {
        use crate::sandbox_perk::action::{POLICY_CONFIGURATION, POLICY_SELECTOR, ROOT_KEY};
        let mut out = Builder {
            bytes: drawn_pattern_action(),
        };
        out.bytes[POLICY_SELECTOR] = 1;
        out.bytes[POLICY_SELECTOR + 1] = 1;
        out.u32(ROOT_KEY, 0xC767_798C);
        // Selector 1 uses an 8-byte record: a key and a flag word.
        let configuration = out.node(0x8080_3E07, 8);
        out.u32(configuration, 0xDB33_855E);
        out.u32(configuration + 4, 1);
        out.pointer(POLICY_CONFIGURATION, configuration);
        let payload = out.finish();

        let action = decode(&payload).unwrap();
        let record = action.policy_configuration.as_ref().unwrap();
        assert_eq!(record.class, 0x8080_3E07);
        assert_eq!(record.bytes.len(), 8);
        let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
        assert_eq!(recovery, Recovery::Typed);
        let policy = program.policy.as_ref().unwrap();
        assert_eq!(
            (policy.selector, policy.modifier, policy.key),
            (1, 1, 0xC767_798C)
        );
        assert_eq!(policy.configuration.as_ref().unwrap().bytes, record.bytes);

        let compiled = super::super::compiler::assemble(&program, None).unwrap();
        let again = decode(&compiled.payload).unwrap();
        assert_eq!(again.policy, 1);
        assert_eq!(again.policy_modifier, 1);
        assert_eq!(again.root_key, 0xC767_798C);
        assert_eq!(again.policy_configuration, action.policy_configuration);
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(
            differences
                .iter()
                .all(|difference| !difference.node.starts_with("Policy")
                    && difference.node != "Root Key"
                    && !(difference.node == "Action Routing and State"
                        && (0x30..0x32).contains(&difference.offset))),
            "{differences:?}"
        );

        // Dropping the policy is reported, not refused.
        let mut plain = program.clone();
        plain.policy = None;
        let compiled = super::super::compiler::assemble(&plain, None).unwrap();
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(
            differences
                .iter()
                .any(|difference| difference.node == "Policy Configuration Presence"),
            "{differences:?}"
        );

        // A default-policy action carries no policy, so the recipe stays minimal.
        let plain =
            decompile(&decode(&drawn_pattern_action()).unwrap(), "Demo", |tag| tag).unwrap();
        assert!(plain.policy.is_none());
    }

    #[test]
    fn native_endings_and_rearms_are_carried_beside_kill_triggers() {
        let mut out = Builder::new();
        let activation = out.kill(&[PRECISION], true, 1.0);
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
        // The stock shape behind 51 perks: a kill perk that ends on an unconditional check.
        let ends = out.unconditional();
        out.pointer_list(PRIMARY_GROUP + GROUP_REMOVAL, CONDITION_ROW_CLASS, &[ends]);
        // A rearm that is not a timer, with a second alternative.
        let rearm = out.draw();
        let also = out.event_key(0x1234_5678);
        out.pointer_list(
            PRIMARY_GROUP + GROUP_REARM,
            CONDITION_ROW_CLASS,
            &[rearm, also],
        );
        let payload = out.finish();

        let action = decode(&payload).unwrap();
        let program = decompile(&action, "Demo", |tag| tag).unwrap();
        assert_eq!(program.trigger, Trigger::PrecisionKill);
        assert_eq!(program.duration_ms, 0);
        assert_eq!(
            program.native_removal.as_ref().map(|node| node.kind),
            Some(0)
        );
        assert_eq!(program.cooldown_ms, 0);
        assert_eq!(
            program.native_rearm.as_ref().map(|node| node.kind),
            Some(16)
        );
        assert_eq!(program.alternative_rearms.len(), 1);

        let mask = Some((&[PRECISION][..], [0_u8; 40]));
        let compiled = super::super::compiler::assemble(&program, mask).unwrap();
        let again = decode(&compiled.payload).unwrap();
        let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&again.groups[0].removal), vec![0]);
        assert_eq!(kinds(&again.groups[0].rearm), vec![16, 30]);
        assert_eq!(again.rearm_event_mask, (1 << 16) | (1 << 30));

        // A cooldown and a native rearm cannot both apply.
        let mut both = program;
        both.cooldown_ms = 1_000;
        assert!(both.validate().unwrap_err().contains("not both"));
    }

    #[test]
    fn recovery_prefers_the_typed_program_and_never_refuses() {
        let payload = drawn_pattern_action();
        let (typed, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
        assert_eq!(recovery, Recovery::Typed);
        assert!(typed.native.is_none());
        assert_eq!(
            typed,
            decompile(&decode(&payload).unwrap(), "Demo", |tag| tag).unwrap()
        );

        // An activation probability outside 0 to 1 is outside the typed model. The same action
        // still opens, carried in native form, and the reason names the shape that was refused.
        let mut out_of_range = payload;
        let at = decode(&out_of_range).unwrap().groups[0].activation[0].offset;
        out_of_range[at..at + 4].copy_from_slice(&2.0_f32.to_le_bytes());
        let (native, recovery) = recover(&out_of_range, "Demo", |tag| tag).unwrap();
        assert!(native.native.is_some());
        assert!(native.actions.is_empty());
        match recovery {
            Recovery::NativeForm(reason) => {
                assert!(reason.contains("probability"), "{reason}")
            }
            Recovery::Typed => panic!("an out-of-range probability cannot be typed"),
        }
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
        // A nested condition that is not the trigger lies outside the typed action and is
        // carried verbatim, nested condition included.
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
        let decoded = decode(&out.finish()).unwrap();
        let program = decompile(&decoded, "Refresh", |tag| tag).unwrap();
        let [Action::Native { node }] = program.actions.as_slice() else {
            panic!("{:?}", program.actions);
        };
        assert_eq!(node.kind, 32);
        assert_eq!(node.bytes, decoded.groups[0].effects[0].native);
    }

    /// Whether a fidelity difference is about list or program structure rather than bytes.
    fn structural(difference: &Difference) -> bool {
        difference.node == "Program Count"
            || difference.node.contains("list length")
            || difference.node == "Effect List Length"
    }

    /// The drawn pattern action plus a second program: always active, spawning one entity,
    /// ending on an event key.
    fn two_program_action() -> Vec<u8> {
        use crate::sandbox_perk::action::{ADDITIONAL_GROUPS, GROUP_ROW_CLASS, GROUP_SIZE};
        let mut out = Builder {
            bytes: drawn_pattern_action(),
        };
        let rows = out.rows(ADDITIONAL_GROUPS, GROUP_ROW_CLASS, 1, GROUP_SIZE);
        let activation = out.unconditional();
        out.pointer_list(rows + GROUP_ACTIVATION, CONDITION_ROW_CLASS, &[activation]);
        let spawn = out.spawn(0x80BC_2F21, "content/sandbox/effects/demo/demo.entity.tft");
        out.pointer_list(rows + GROUP_EFFECTS, EFFECT_ROW_CLASS, &[spawn]);
        let ends = out.event_key(0x1234_5678);
        out.pointer_list(rows + GROUP_REMOVAL, CONDITION_ROW_CLASS, &[ends]);
        out.finish()
    }

    #[test]
    fn further_programs_are_recovered_beside_the_typed_one() {
        let payload = two_program_action();
        assert_eq!(decode(&payload).unwrap().groups.len(), 2);
        let (program, recovery) = recover(&payload, "Demo", |tag| tag).unwrap();
        assert_eq!(recovery, Recovery::Typed);
        assert_eq!(program.trigger, Trigger::Drawn);
        assert_eq!(program.actions.len(), 2);
        assert_eq!(program.additional_groups.len(), 1);
        let group = &program.additional_groups[0];
        assert_eq!(group.activation[0].kind, 0);
        assert_eq!(group.effects[0].kind, 3);
        assert_eq!(group.removal[0].kind, 30);
        assert!(group.rearm.is_empty());
    }

    #[test]
    fn further_programs_compile_verbatim_with_their_routing_records() {
        let payload = two_program_action();
        let action = decode(&payload).unwrap();
        let program = decompile(&action, "Demo", |tag| tag).unwrap();
        let compiled = super::super::compiler::assemble(&program, None).unwrap();
        let again = decode(&compiled.payload).unwrap();
        assert_eq!(again.groups.len(), 2);
        let kinds = |list: &[DecodedCondition]| list.iter().map(|c| c.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&again.groups[1].activation), vec![0]);
        assert_eq!(kinds(&again.groups[1].removal), vec![30]);
        assert_eq!(again.groups[1].effects[0].kind, 3);
        assert_eq!(
            again.groups[1].effects[0].native,
            action.groups[1].effects[0].native
        );
        // The compiled routing records exist, one per further program, for the rebuild.
        let (count, _, _, class) =
            crate::package_payload::native_array_at(&compiled.payload, 0xA8).unwrap();
        assert_eq!((count, class), (1, 0x8080_407B));
        let differences = fidelity(&payload, &compiled.payload).unwrap();
        assert!(!differences.iter().any(structural), "{differences:?}");

        // Dropping the further program is reported, not refused.
        let mut single = program;
        single.additional_groups.clear();
        let compiled = super::super::compiler::assemble(&single, None).unwrap();
        assert!(
            fidelity(&payload, &compiled.payload)
                .unwrap()
                .iter()
                .any(|difference| difference.node == "Program Count")
        );
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
        // A nonzero fast-path selector means the program is not the plain constant, so the
        // node is carried verbatim instead of as a typed property.
        let mut other = payload.clone();
        other[property + 0x44] = 1;
        let decoded = decode(&other).unwrap();
        let program = decompile(&decoded, "Flag", |tag| tag).unwrap();
        assert!(
            matches!(&program.actions[0], Action::Native { node } if node.kind == 10),
            "{:?}",
            program.actions
        );
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
    fn ammunition_effects_recover_their_one_amount_and_carry_filters_and_spreads_natively() {
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
        // A source label filter lies outside the typed action and is carried verbatim.
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
        let decoded = decode(&out.finish()).unwrap();
        let program = decompile(&decoded, "Ammo", |tag| tag).unwrap();
        let [Action::Native { node }] = program.actions.as_slice() else {
            panic!("{:?}", program.actions);
        };
        assert_eq!(node.bytes, decoded.groups[0].effects[0].native);
        // Two amounts in one node exceed the typed action, which carries one, so the node stays
        // native.
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
        let decoded = decode(&out.finish()).unwrap();
        let program = decompile(&decoded, "Ammo", |tag| tag).unwrap();
        assert!(
            matches!(&program.actions[0], Action::Native { node } if node.bytes == decoded.groups[0].effects[0].native),
            "{:?}",
            program.actions
        );
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
