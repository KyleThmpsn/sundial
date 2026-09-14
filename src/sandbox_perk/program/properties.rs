//! Key usage read from the selected installation. Cached observations contain no item names.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::sandbox_perk::action::{
    DecodedAction, DecodedCondition, Fact, FactValue, NAMED_PROPERTY_LABELS,
};

mod cache;
pub use cache::{KeyIndex, cached, cached_only};

/// Numeric observations from one action resource.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct KeyUsage {
    properties: Vec<Property>,
    removals: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Property {
    key: u32,
    value: Option<u32>,
    target: u8,
    operation: u8,
    removal: u8,
}

fn fact<'a>(facts: &'a [Fact], label: &str) -> Option<&'a FactValue> {
    facts
        .iter()
        .find(|fact| fact.label == label)
        .map(|fact| &fact.value)
}

impl KeyUsage {
    fn read(action: &DecodedAction) -> Self {
        let labels = &NAMED_PROPERTY_LABELS;
        let mut seen = BTreeSet::new();
        let properties = action
            .effects()
            .filter_map(|node| {
                if node.kind != 10 || !seen.insert(node.offset) {
                    return None;
                }
                let FactValue::Key(key) = fact(&node.facts, labels.key)? else {
                    return None;
                };
                let FactValue::Selector(target) = fact(&node.facts, labels.target)? else {
                    return None;
                };
                let FactValue::Selector(operation) = fact(&node.facts, labels.operation)? else {
                    return None;
                };
                let FactValue::Selector(removal) = fact(&node.facts, labels.removal)? else {
                    return None;
                };
                let value = match fact(&node.facts, labels.value) {
                    Some(FactValue::Number(value)) if value.is_finite() => Some(value.to_bits()),
                    _ => None,
                };
                Some(Property {
                    key: *key,
                    value,
                    target: *target,
                    operation: *operation,
                    removal: *removal,
                })
            })
            .collect();
        let mut removals = Vec::new();
        seen.clear();
        for group in &action.groups {
            removal_keys(&group.removal, &mut seen, &mut removals);
        }
        Self {
            properties,
            removals,
        }
    }
}

fn removal_keys(nodes: &[DecodedCondition], seen: &mut BTreeSet<usize>, out: &mut Vec<u32>) {
    for node in nodes {
        if !seen.insert(node.offset) {
            continue;
        }
        if node.kind == 30
            && let Some(FactValue::Key(key)) = fact(&node.facts, "Event Key")
        {
            out.push(*key);
        }
        removal_keys(&node.children, seen, out);
        for subgroup in &node.subgroups {
            removal_keys(&subgroup.conditions, seen, out);
        }
    }
}

/// One key and its uses in installed actions. Names are references, not gameplay meanings.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvidence {
    pub key: String,
    pub nodes: u32,
    pub perks: Vec<String>,
    pub values: Vec<f32>,
    pub targets: Vec<u8>,
    pub operations: Vec<u8>,
    pub removals: Vec<u8>,
}

impl KeyEvidence {
    #[must_use]
    pub fn hash(&self) -> u32 {
        u32::from_str_radix(&self.key, 16).unwrap_or(0)
    }

    #[must_use]
    pub fn seen_in_as(&self, what: &str) -> String {
        if self.perks.is_empty() {
            format!("{} {what}, no named perk", self.nodes)
        } else {
            self.perks.join(", ")
        }
    }

    #[must_use]
    pub fn seen_in(&self) -> String {
        self.seen_in_as("installed nodes")
    }

    #[must_use]
    pub fn single(values: &[u8]) -> Option<u8> {
        match values {
            [value] => Some(*value),
            _ => None,
        }
    }
}

/// Lookup tables assembled once from cached actions and the selected catalog's names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyCatalog {
    properties: Vec<KeyEvidence>,
    removals: Vec<KeyEvidence>,
}

impl KeyCatalog {
    #[must_use]
    pub fn from_index(index: &KeyIndex, mut names: impl FnMut(usize) -> Vec<String>) -> Self {
        let mut sources = BTreeMap::<u32, BTreeSet<String>>::new();
        for &(perk, tag) in &index.perks {
            if index.actions.contains_key(&tag) {
                sources.entry(tag).or_default().extend(
                    names(perk)
                        .into_iter()
                        .filter(|name| !name.trim().is_empty()),
                );
            }
        }
        let mut properties = BTreeMap::<u32, Aggregate>::new();
        let mut removals = BTreeMap::<u32, Aggregate>::new();
        for (tag, usage) in &index.actions {
            let names = sources.get(tag).cloned().unwrap_or_default();
            for node in &usage.properties {
                let entry = properties.entry(node.key).or_default();
                entry.nodes += 1;
                entry.names.extend(names.iter().cloned());
                entry.values.extend(node.value);
                entry.targets.insert(node.target);
                entry.operations.insert(node.operation);
                entry.removals.insert(node.removal);
            }
            for key in &usage.removals {
                let entry = removals.entry(*key).or_default();
                entry.nodes += 1;
                entry.names.extend(names.iter().cloned());
            }
        }
        Self {
            properties: finish(properties),
            removals: finish(removals),
        }
    }

    #[must_use]
    pub fn property_keys(&self) -> &[KeyEvidence] {
        &self.properties
    }

    #[must_use]
    pub fn property_key(&self, key: u32) -> Option<&KeyEvidence> {
        self.properties.iter().find(|entry| entry.hash() == key)
    }

    #[must_use]
    pub fn removal_keys(&self) -> &[KeyEvidence] {
        &self.removals
    }

    #[must_use]
    pub fn removal_key(&self, key: u32) -> Option<&KeyEvidence> {
        self.removals.iter().find(|entry| entry.hash() == key)
    }
}

#[derive(Default)]
struct Aggregate {
    nodes: u32,
    names: BTreeSet<String>,
    values: BTreeSet<u32>,
    targets: BTreeSet<u8>,
    operations: BTreeSet<u8>,
    removals: BTreeSet<u8>,
}

fn finish(entries: BTreeMap<u32, Aggregate>) -> Vec<KeyEvidence> {
    let mut result = entries
        .into_iter()
        .map(|(key, entry)| {
            let mut values = entry
                .values
                .into_iter()
                .map(f32::from_bits)
                .collect::<Vec<_>>();
            values.sort_by(f32::total_cmp);
            KeyEvidence {
                key: format!("{key:08X}"),
                nodes: entry.nodes,
                perks: entry.names.into_iter().collect(),
                values,
                targets: entry.targets.into_iter().collect(),
                operations: entry.operations.into_iter().collect(),
                removals: entry.removals.into_iter().collect(),
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| b.nodes.cmp(&a.nodes).then_with(|| a.key.cmp(&b.key)));
    result
}

#[cfg(test)]
mod tests;
