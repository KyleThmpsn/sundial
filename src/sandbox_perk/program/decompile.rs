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

mod actions;
mod fidelity;

use actions::action_of;
pub use fidelity::{Difference, fidelity, native_fidelity};

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
        native_asset_patches: Vec::new(),
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
        ability_tunings: Vec::new(),
        ability_inputs: Vec::new(),
    };
    // Shape only. Stock actions can ship with unfilled references (Sidearm Targeting leaves
    // a Create Entity slot empty), and build readiness is the workbench's job.
    program.validate_structure().map_err(Unsupported)?;
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

#[cfg(test)]
mod tests;
