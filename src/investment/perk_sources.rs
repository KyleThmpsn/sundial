//! Item and plug references are provenance, not names or descriptions of an effect.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerkSource {
    pub hash: u32,
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, Default)]
pub struct PerkSources {
    entries: BTreeMap<u16, Vec<PerkSource>>,
}

impl PerkSources {
    /// Describe what distinct source names have in common, without choosing an alias.
    pub fn shared_label(&self, index: usize) -> Option<String> {
        let names = self.names(index);
        if names.len() < 2 {
            return None;
        }
        if names
            .iter()
            .all(|name| name.ends_with(" Catalyst") || name.starts_with("Masterwork"))
        {
            Some("Shared Masterwork Effect".into())
        } else {
            Some(format!("Shared Effect {index}"))
        }
    }

    pub fn get(&self, index: usize) -> &[PerkSource] {
        u16::try_from(index)
            .ok()
            .and_then(|index| self.entries.get(&index))
            .map_or(&[], Vec::as_slice)
    }

    pub fn label(&self, index: usize) -> String {
        let names = self.names(index);
        match names.as_slice() {
            [] => format!("Effect {index}"),
            [name] => format!("Effect {index} · From {name}"),
            _ => format!("Shared Effect {index}"),
        }
    }

    pub fn summary(&self, index: usize) -> String {
        let names = self.names(index);
        match names.len() {
            0 => "No named item or plug references".to_owned(),
            1..=3 => names.join(", "),
            _ => format!("{} source items and plugs", self.get(index).len()),
        }
    }

    pub fn has_names(&self, index: usize) -> bool {
        !self.names(index).is_empty()
    }

    pub fn matches_query(&self, index: usize, query: &str) -> bool {
        if query.trim().is_empty() {
            return true;
        }
        let text = format!("{} {}", self.label(index), self.details(index)).to_lowercase();
        query.split_whitespace().all(|word| text.contains(word))
    }

    pub fn details(&self, index: usize) -> String {
        self.get(index)
            .iter()
            .map(|source| {
                let kind = if source.type_name.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", source.type_name)
                };
                format!("{}{} · 0x{:08X}", source.name, kind, source.hash)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn names(&self, index: usize) -> Vec<&str> {
        self.get(index)
            .iter()
            .map(|source| source.name.as_str())
            .filter(|name| !name.starts_with("Item 0x"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

impl FromIterator<(u16, PerkSource)> for PerkSources {
    fn from_iter<T: IntoIterator<Item = (u16, PerkSource)>>(iter: T) -> Self {
        let mut result = Self::default();
        for (index, source) in iter {
            result.entries.entry(index).or_default().push(source);
        }
        for sources in result.entries.values_mut() {
            sources.sort_by_cached_key(|source| (source.name.to_lowercase(), source.hash));
            sources.dedup_by_key(|source| source.hash);
        }
        result
    }
}

impl InvestmentCatalog {
    /// Keep every source instead of choosing one item's name for a shared effect.
    /// References do not establish that a source is equipped or its effects are active.
    pub fn perk_sources(&self) -> PerkSources {
        let mut references = Vec::new();
        let hashes = self
            .catalog
            .items
            .iter()
            .map(|item| item.hash)
            .chain(self.catalog.all_plug_options().iter().copied())
            .collect::<BTreeSet<_>>();
        for item_hash in hashes {
            let Ok(hash) = u32::try_from(item_hash) else {
                continue;
            };
            let Some(metadata) = self.catalog.item_package_metadata(item_hash) else {
                continue;
            };
            let source = PerkSource {
                hash,
                name: self
                    .catalog
                    .display_name(item_hash)
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| self.catalog.package_item_name(item_hash))
                    .filter(|name| !name.trim().is_empty())
                    .map_or_else(|| format!("Item 0x{hash:08X}"), str::to_owned),
                type_name: self
                    .catalog
                    .plug_type_name(item_hash)
                    .or_else(|| self.catalog.package_item_type_name(item_hash))
                    .unwrap_or("")
                    .to_owned(),
            };
            for perk in &metadata.sandbox_perks {
                references.push((perk.perk_index, source.clone()));
            }
        }
        references.into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
