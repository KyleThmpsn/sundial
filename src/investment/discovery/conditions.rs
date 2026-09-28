//! On-demand condition inspection, independent of a completed discovery scan.
use crate::sandbox_perk::action::{self, ActionSummary};
use std::path::Path;

/// A behavior family, not an equivalence claim about its configurations. Values,
/// probabilities, filters and child records remain in each complete condition.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Family {
    Kind(u8),
    Kill {
        labels: Vec<u32>,
        owning_weapon: bool,
    },
    Named {
        kind: u8,
        name: String,
    },
}

impl Family {
    pub fn kill(labels: &[u32], owning_weapon: bool) -> Self {
        let mut labels = labels.to_vec();
        labels.sort_unstable();
        Self::Kill {
            labels,
            owning_weapon,
        }
    }

    /// The family a decoded condition belongs to, keyed by the same name `name` gives it.
    pub fn of(condition: &action::DecodedCondition) -> Self {
        if condition.kind == 2 {
            let labels = condition
                .facts
                .iter()
                .find_map(|fact| match &fact.value {
                    action::FactValue::Labels(labels) if fact.label == action::REQUIRED_LABELS => {
                        Some(labels.as_slice())
                    }
                    _ => None,
                })
                .unwrap_or_default();
            let owning_weapon = condition.facts.iter().any(|fact| {
                fact.label == "Requires Owning Weapon"
                    && matches!(fact.value, action::FactValue::Flag(true))
            });
            return Self::kill(labels, owning_weapon);
        }
        match condition.kind {
            6 | 8 | 12 | 20 | 35 => {
                name(condition).map_or(Self::Kind(condition.kind), |name| Self::Named {
                    kind: condition.kind,
                    name,
                })
            }
            kind => Self::Kind(kind),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Condition {
    pub family: Family,
    /// The name `name` gives this configuration, when it has one.
    pub name: Option<String>,
    pub description: String,
    pub source: String,
    pub kind: u8,
    /// Requirements read from this node and its owned conditions.
    pub requirements: Vec<String>,
    pub details: Vec<String>,
    /// Complete relative-pointer record, including its owned conditions and values.
    pub bytes: Vec<u8>,
}

pub fn read(packages: &Path, action: u32) -> Result<Vec<Condition>, String> {
    let manager = super::open_packages(packages)?;
    let payload = manager
        .read_tag(tiger_pkg::TagHash(action))
        .map_err(|error| format!("Could not read action 0x{action:08X}: {error}"))?;
    from_payload(&payload)
}

pub fn from_payload(payload: &[u8]) -> Result<Vec<Condition>, String> {
    let decoded = action::decode(payload)?;
    from_decoded(&decoded)
}

pub(super) fn from_decoded(decoded: &action::DecodedAction) -> Result<Vec<Condition>, String> {
    let summary = ActionSummary::new(decoded);
    let all_conditions = decoded.conditions();
    let mut result = Vec::new();
    for group in summary.groups {
        for (role, lines) in [
            ("Trigger", group.activation),
            ("End Condition", group.removal),
            ("Reactivation", group.rearm),
        ] {
            for line in lines {
                if let Some((true, node)) = line.native {
                    result.push(Condition {
                        family: all_conditions
                            .iter()
                            .find(|condition| condition.native == node.bytes)
                            .map_or(Family::Kind(node.kind), |condition| Family::of(condition)),
                        name: all_conditions
                            .iter()
                            .find(|condition| condition.native == node.bytes)
                            .and_then(|condition| name(condition)),
                        description: line.text,
                        source: format!("{} · {role}", group.label),
                        kind: node.kind,
                        requirements: all_conditions
                            .iter()
                            .find(|condition| condition.native == node.bytes)
                            .map_or_else(Vec::new, |condition| requirements(condition)),
                        details: line.detail,
                        bytes: node.bytes,
                    });
                }
            }
        }
    }
    // Effect-local predicates are also reusable, even when they are absent from
    // the program-level activation/removal lists above.
    for effect in decoded.effects() {
        let mut pending: Vec<_> = effect.conditions.iter().collect();
        while let Some(condition) = pending.pop() {
            result.push(Condition {
                family: Family::of(condition),
                name: name(condition),
                description: condition.description(),
                source: format!(
                    "{} · Matching Condition",
                    effect.catalog().map_or("Action", |kind| kind.name)
                ),
                kind: condition.kind,
                requirements: requirements(condition),
                details: condition.facts.iter().map(|fact| fact.render()).collect(),
                bytes: condition.native.clone(),
            });
            pending.extend(&condition.children);
            pending.extend(
                condition
                    .subgroups
                    .iter()
                    .flat_map(|group| &group.conditions),
            );
        }
    }
    Ok(result)
}

/// The name a condition carries wherever it is offered or shown: the kill trigger it matches,
/// the name of a configuration the stock perks identify, or its kind's name for the kinds
/// named by kind alone. `None` leaves the configuration unnamed.
pub fn name(condition: &action::DecodedCondition) -> Option<String> {
    if condition.kind == 2
        && let Family::Kill {
            labels,
            owning_weapon,
        } = Family::of(condition)
        && let Some(activation) =
            crate::sandbox_perk::activation::PerkActivation::from_filter(&labels, owning_weapon)
    {
        return Some(format!("On {}", activation.label()));
    }
    if condition.kind == 2
        && condition.facts.iter().any(|fact| {
            matches!(&fact.value, action::FactValue::Labels(labels)
                if labels.iter().any(|label| action::label_name(*label).is_none()))
        })
    {
        return None;
    }
    if matches!(condition.kind, 0 | 1 | 14..=17 | 26 | 31 | 35) {
        return Some(crate::sandbox_perk::nodes::condition_title(condition.kind).to_owned());
    }
    let description = condition.description();
    // A state known only by its hash reads as unnamed. The hash stays in the description,
    // which the picker's detail line shows, and on hover over the card's State field.
    if description.starts_with("While in the unnamed state 0x") {
        return Some("While in an Unnamed State".into());
    }
    if description.starts_with("While not in the unnamed state 0x") {
        return Some("While Not in an Unnamed State".into());
    }
    let recognized = condition.kind == 2
        || matches!(condition.kind, 6 | 8 | 12 | 20)
            && condition
                .catalog()
                .is_some_and(|kind| description != kind.summary);
    recognized.then(|| {
        let words = description.split_whitespace().collect::<Vec<_>>();
        words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                // A label such as hammer_throw_melee is the engine's own identifier, and
                // arc_ball and "arc ball" are two different labels, so it stays verbatim.
                if word.contains('_') {
                    return (*word).to_owned();
                }
                crate::sandbox_perk::nodes::title_word(word, index, words.len())
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn requirements(root: &action::DecodedCondition) -> Vec<String> {
    let mut result = std::collections::BTreeSet::new();
    let mut pending = vec![root];
    while let Some(condition) = pending.pop() {
        if let action::Probability::NativeStat(selector) = condition.probability {
            result.insert(format!("Activation chance comes from native stat {selector}, whose gameplay identity is unmapped."));
        }
        if condition.linked_state {
            result.insert("Uses linked condition state supplied by its calling program.".into());
        }
        pending.extend(&condition.children);
        pending.extend(
            condition
                .subgroups
                .iter()
                .flat_map(|group| &group.conditions),
        );
    }
    result.into_iter().collect()
}
