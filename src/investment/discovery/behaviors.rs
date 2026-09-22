//! Optional discovery of complete condition and effect records from native perk actions.
use super::conditions;
use crate::sandbox_perk::action;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub struct Source {
    pub perk: u16,
    pub role: String,
}

pub struct Condition {
    pub condition: conditions::Condition,
    pub sources: Vec<Source>,
}

pub struct Effect {
    pub kind: u8,
    pub bytes: Vec<u8>,
    pub description: String,
    pub details: Vec<String>,
    pub sources: Vec<Source>,
}

#[derive(Default)]
pub struct Catalog {
    pub conditions: Vec<Condition>,
    pub effects: Vec<Effect>,
    pub errors: Vec<String>,
}

/// Inspection callers can still read one action without creating this index.
pub fn read(packages: &Path, sources: &[(u16, u32)]) -> Result<Catalog, String> {
    let manager = super::open_packages(packages)?;
    read_with_manager(&manager, sources)
}

/// Build the behavior catalog without waiting for asset names or projectile discovery.
pub fn discover(packages: &Path) -> Result<Catalog, String> {
    let manager = super::open_packages(packages)?;
    let index = crate::sandbox_perk::dependencies::cached(packages, &manager, |_, _| {})?;
    let sources = index
        .perks
        .iter()
        .filter_map(|perk| Some((u16::try_from(perk.index).ok()?, perk.action?)))
        .collect::<Vec<_>>();
    read_with_manager(&manager, &sources)
}

fn read_with_manager(
    manager: &crate::package_runtime::reader::PackageManager,
    sources: &[(u16, u32)],
) -> Result<Catalog, String> {
    let mut actions = BTreeMap::<u32, Vec<u16>>::new();
    for &(perk, tag) in sources {
        actions.entry(tag).or_default().push(perk);
    }
    let actions = actions.into_iter().collect::<Vec<_>>();
    let loaded = crate::package_runtime::parallel::map_jobs(&actions, |(tag, _)| {
        manager
            .read_tag(tiger_pkg::TagHash(*tag))
            .map_err(|error| format!("Could not read action 0x{tag:08X}: {error}"))
            .and_then(|payload| action::decode(&payload))
    });
    let mut catalog = Catalog::default();
    let mut conditions_seen = BTreeMap::new();
    let mut effects_seen = BTreeMap::new();
    for ((tag, perks), result) in actions.into_iter().zip(loaded) {
        match result {
            Ok(decoded) => {
                catalog.add(&decoded, &perks, &mut conditions_seen, &mut effects_seen)?
            }
            Err(error) => catalog.errors.push(format!("Action 0x{tag:08X}: {error}")),
        }
    }
    Ok(catalog)
}

impl Catalog {
    fn add(
        &mut self,
        decoded: &action::DecodedAction,
        perks: &[u16],
        conditions_seen: &mut BTreeMap<(u8, Vec<u8>), usize>,
        effects_seen: &mut BTreeMap<(u8, Vec<u8>), usize>,
    ) -> Result<(), String> {
        for condition in conditions::from_decoded(decoded)? {
            let mut identity = condition.bytes.clone();
            // Ignore only the receiving program's evaluation ordinal for grouping.
            // Keep every original byte in the selected record.
            if let Some(ordinal) = identity.get_mut(7) {
                *ordinal = 0;
            }
            let index = *conditions_seen
                .entry((condition.kind, identity))
                .or_insert_with(|| {
                    let index = self.conditions.len();
                    self.conditions.push(Condition {
                        condition: condition.clone(),
                        sources: Vec::new(),
                    });
                    index
                });
            add_sources(
                &mut self.conditions[index].sources,
                perks,
                &condition.source,
            );
        }
        for (group, program) in decoded.groups.iter().enumerate() {
            for (number, effect) in program.effects.iter().enumerate() {
                let index = *effects_seen
                    .entry((effect.kind, effect.native.clone()))
                    .or_insert_with(|| {
                        let index = self.effects.len();
                        self.effects.push(Effect {
                            kind: effect.kind,
                            bytes: effect.native.clone(),
                            description: effect.description(),
                            details: effect.facts.iter().map(|fact| fact.render()).collect(),
                            sources: Vec::new(),
                        });
                        index
                    });
                add_sources(
                    &mut self.effects[index].sources,
                    perks,
                    &format!("Program {} · Action {}", group + 1, number + 1),
                );
            }
        }
        Ok(())
    }
}

fn add_sources(sources: &mut Vec<Source>, perks: &[u16], role: &str) {
    for &perk in perks {
        if !sources
            .iter()
            .any(|source| source.perk == perk && source.role == role)
        {
            sources.push(Source {
                perk,
                role: role.to_owned(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_behavior_entries_preserve_complete_records_and_all_sources() {
        let payload = action::fixtures::precision_kill_action();
        let decoded = action::decode(&payload).unwrap();
        let mut catalog = Catalog::default();
        let mut conditions = BTreeMap::new();
        let mut effects = BTreeMap::new();
        catalog
            .add(&decoded, &[421], &mut conditions, &mut effects)
            .unwrap();
        let counts = (catalog.conditions.len(), catalog.effects.len());
        catalog
            .add(&decoded, &[422], &mut conditions, &mut effects)
            .unwrap();
        assert_eq!(counts, (catalog.conditions.len(), catalog.effects.len()));
        for entry in &catalog.conditions {
            assert!(entry.sources.iter().any(|source| source.perk == 421));
            assert!(entry.sources.iter().any(|source| source.perk == 422));
            let class = crate::sandbox_perk::nodes::condition(entry.condition.kind)
                .unwrap()
                .class;
            let graph = action::native::Graph::read(&entry.condition.bytes, 0, class).unwrap();
            assert_eq!(graph.emit().unwrap(), entry.condition.bytes);
            if entry.condition.kind == 2 {
                assert_eq!(
                    entry.condition.family,
                    conditions::Family::kill(&[0x962E_A19B], true)
                );
            }
        }
        for entry in &catalog.effects {
            let class = crate::sandbox_perk::nodes::effect(entry.kind)
                .unwrap()
                .class;
            let graph = action::native::Graph::read(&entry.bytes, 0, class).unwrap();
            assert_eq!(graph.emit().unwrap(), entry.bytes);
        }
    }
}
