//! Turns decoded stock effects into typed actions and keeps every other shape as a native node.
use super::*;

pub(super) fn action_of(
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
        damage_type: None,
        rows: Vec::new(),
        hud_status: None,
        script: None,
    };
    match effect.kind {
        1 => Ok(Action::Attach {
            asset,
            mode: (match effect_fact(effect, CREATE_ENTITY_MODE_LABEL) {
                Some(FactValue::Selector(mode)) => *mode,
                _ => 1,
            })
            .into(),
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
        target: (selector_fact(effect, "Target Selector").unwrap_or(0)).into(),
        flag: (selector_fact(effect, "Ability State").unwrap_or(0)).into(),
        option: (selector_fact(effect, "Ability Version").unwrap_or(0)).into(),
        scale_bits,
        limit_bits,
        value_bits,
        input: (selector_fact(effect, "Input Selector").unwrap_or(0xFF)).into(),
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
            mode: (n[2]).into(),
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
        target: (n[2]).into(),
        key: *key,
        option: (n[8]).into(),
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
