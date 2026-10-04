//! Opt-in group aliases append masks without consuming category or wire indices.
use super::*;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct GroupAlias {
    pub native: u32,
    pub label: String,
    pub mapped: BTreeSet<u32>,
    pub unresolved: BTreeSet<u32>,
}

fn hash(label: &str) -> u32 {
    label.bytes().fold(0x811C9DC5u32, |value, byte| {
        value.wrapping_mul(0x01000193) ^ u32::from(byte)
    })
}

impl Namespace {
    /// Append deterministic private group records for exact mapped Source sets.
    /// Unmapped members remain explicit and selector conversion retains gates.
    /// The caller must allocate and bind this dictionary in its private package.
    pub fn with_group_aliases(mut self, requested: &BTreeSet<u32>) -> Result<Self> {
        let mut dictionary = Dictionary::read(&self.payload, false)?;
        ensure!(
            dictionary.names == self.names && dictionary.named_groups() == self.groups,
            "category namespace metadata differs from its native payload"
        );
        let indices = dictionary
            .names
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, index))
            .collect::<BTreeMap<_, _>>();
        let mut occupied = self
            .names
            .iter()
            .chain(&self.source_names)
            .copied()
            .chain(self.groups.keys().copied())
            .chain(self.source_groups.keys().copied())
            .collect::<BTreeSet<_>>();
        for source in requested {
            ensure!(
                !self.source_names.contains(source),
                "Source group aliases cannot shadow category names"
            );
            let members = self
                .source_groups
                .get(source)
                .context("requested Source category group missing")?;
            let mapped = members
                .iter()
                .filter(|name| indices.contains_key(*name))
                .copied()
                .collect::<BTreeSet<_>>();
            let unresolved = members
                .difference(&mapped)
                .copied()
                .collect::<BTreeSet<_>>();
            let label = format!("parhelion/source_group/{source:08X}");
            let native = hash(&label);
            if let Some(existing) = self.group_aliases.get(source) {
                ensure!(
                    existing.native == native
                        && existing.label == label
                        && existing.mapped == mapped
                        && existing.unresolved == unresolved
                        && self.groups.get(&native) == Some(&mapped)
                        && !self.names.contains(&native),
                    "existing private category alias differs"
                );
                continue;
            }
            ensure!(
                occupied.insert(native),
                "private category group alias collides with a name or group"
            );
            ensure!(
                dictionary.groups.len() < 4096,
                "private category group capacity exceeded"
            );
            dictionary.groups.push(Group {
                name: native,
                bits: mapped.iter().map(|name| indices[name]).collect(),
            });
            self.group_aliases.insert(
                *source,
                GroupAlias {
                    native,
                    label,
                    mapped,
                    unresolved,
                },
            );
        }
        self.payload = dictionary.emit()?;
        self.groups = dictionary.named_groups();
        Ok(self)
    }
}
