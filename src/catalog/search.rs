//! Name search across the catalog's definition tables.

use std::cmp::Ordering;

use super::{Catalog, DefinitionSearchHit, ProgressionDefinition};

/// The table a search entry came from, in result order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Item,
    PackageItem,
    Collectible,
    Record,
    PresentationNode,
    Objective,
    Progression,
    ItemStat,
    ItemTrait,
    UnlockFlag,
    UnlockValue,
}

impl Source {
    const fn order(self) -> u8 {
        match self {
            Self::Item | Self::PackageItem => 0,
            Self::Collectible => 1,
            Self::Record => 2,
            Self::PresentationNode => 3,
            Self::Objective => 4,
            Self::Progression => 5,
            Self::ItemStat => 6,
            Self::ItemTrait => 7,
            Self::UnlockFlag => 8,
            Self::UnlockValue => 9,
        }
    }

    const fn kind(self) -> &'static str {
        match self {
            Self::Item | Self::PackageItem => "Item",
            Self::Collectible => "Collectible",
            Self::Record => "Record",
            Self::PresentationNode => "Presentation Node",
            Self::Objective => "Objective",
            Self::Progression => "Progression",
            Self::ItemStat => "Item Stat",
            Self::ItemTrait => "Item Trait",
            Self::UnlockFlag => "Unlock Flag",
            Self::UnlockValue => "Unlock Value",
        }
    }
}

struct Entry {
    hash: u64,
    source: Source,
    /// Row in the source table. Details are resolved only for returned hits.
    position: u32,
    name: Box<str>,
    lower: Box<str>,
}

/// Lowercased definition names, built on the first search and never serialized.
#[derive(Default)]
pub(super) struct SearchIndex {
    entries: Vec<Entry>,
}

impl SearchIndex {
    fn build(catalog: &Catalog) -> Self {
        let mut entries = Vec::new();
        let mut add = |hash: u64, source: Source, position: usize, name: &str| {
            let name = name.trim();
            if name.is_empty() {
                return;
            }
            entries.push(Entry {
                hash,
                source,
                position: u32::try_from(position).unwrap_or(u32::MAX),
                name: name.into(),
                lower: name.to_lowercase().into_boxed_str(),
            });
        };
        for (position, item) in catalog.items.iter().enumerate() {
            add(item.hash, Source::Item, position, &item.name);
        }
        let mut package_items = catalog
            .names
            .keys()
            .chain(catalog.package_item_names.keys())
            .copied()
            .filter(|hash| !catalog.item_indices.contains_key(hash))
            .collect::<Vec<_>>();
        package_items.sort_unstable();
        package_items.dedup();
        for hash in package_items {
            let name = catalog
                .names
                .get(&hash)
                .map(String::as_str)
                .filter(|name| !name.trim().is_empty())
                .or_else(|| catalog.package_item_name(hash));
            if let Some(name) = name {
                add(hash, Source::PackageItem, 0, name);
            }
        }
        for (position, collectible) in catalog.collectibles.iter().enumerate() {
            add(
                collectible.hash,
                Source::Collectible,
                position,
                &collectible.name,
            );
        }
        for (position, record) in catalog.records.iter().flatten().enumerate() {
            add(record.hash, Source::Record, position, &record.name);
        }
        for (position, node) in catalog.presentation_nodes.iter().enumerate() {
            add(node.hash, Source::PresentationNode, position, &node.name);
        }
        for (position, objective) in catalog.objectives.iter().enumerate() {
            add(objective.hash, Source::Objective, position, &objective.name);
        }
        for (position, definition) in catalog.progression_definitions.iter().enumerate() {
            if let Some(name) = progression_name(catalog, definition) {
                add(definition.hash, Source::Progression, position, name);
            }
        }
        for (position, definition) in catalog.item_stat_definitions.iter().enumerate() {
            add(
                definition.hash,
                Source::ItemStat,
                position,
                &definition.name,
            );
        }
        for (position, definition) in catalog.trait_definitions.iter().enumerate() {
            add(
                definition.hash,
                Source::ItemTrait,
                position,
                &definition.name,
            );
        }
        for (source, definitions) in [
            (Source::UnlockFlag, &catalog.unlock_flag_definitions),
            (Source::UnlockValue, &catalog.unlock_value_definitions),
        ] {
            for (position, definition) in definitions.iter().enumerate() {
                if let Some(name) = &definition.name {
                    add(definition.hash, source, position, name);
                }
            }
        }
        entries.shrink_to_fit();
        Self { entries }
    }
}

/// The progression's own name, a name every faction shares, or a name from its references.
fn progression_name<'a>(
    catalog: &'a Catalog,
    definition: &'a ProgressionDefinition,
) -> Option<&'a str> {
    let name = definition.name.trim();
    if !name.is_empty() {
        return Some(name);
    }
    let mut factions = definition
        .factions
        .iter()
        .map(|faction| faction.name.trim())
        .filter(|name| !name.is_empty());
    if let Some(first) = factions.next()
        && factions.all(|name| name == first)
    {
        return Some(first);
    }
    catalog
        .progression_names
        .get(&definition.hash)
        .map(String::as_str)
}

/// How closely a lowercased name matches the query. Lower ranks first.
fn rank(name: &str, phrase: &str, words: &[String]) -> u8 {
    if name == phrase {
        0
    } else if name.starts_with(phrase) {
        1
    } else if words.iter().all(|word| starts_word(name, word)) {
        2
    } else {
        3
    }
}

fn starts_word(name: &str, word: &str) -> bool {
    name.match_indices(word).any(|(at, _)| {
        name[..at]
            .chars()
            .next_back()
            .is_none_or(|previous| !previous.is_alphanumeric())
    })
}

fn join_path<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for name in names.map(str::trim).filter(|name| !name.is_empty()) {
        if !text.is_empty() {
            text.push_str(" › ");
        }
        text.push_str(name);
    }
    text
}

impl Catalog {
    /// Case-insensitive, and every word must match. Ranked exact name, prefix, word prefix,
    /// substring, then kind order (items first), name, hash. Empty query returns nothing.
    pub(crate) fn search_definitions(&self, query: &str, limit: usize) -> Vec<DefinitionSearchHit> {
        let words = query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        if words.is_empty() || limit == 0 {
            return Vec::new();
        }
        let phrase = words.join(" ");
        let entries = &self
            .search_index
            .get_or_init(|| SearchIndex::build(self))
            .entries;
        let mut matches = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| words.iter().all(|word| entry.lower.contains(word.as_str())))
            .map(|(position, entry)| (rank(&entry.lower, &phrase, &words), position))
            .collect::<Vec<_>>();
        let order = |left: &(u8, usize), right: &(u8, usize)| -> Ordering {
            let (first, second) = (&entries[left.1], &entries[right.1]);
            left.0
                .cmp(&right.0)
                .then_with(|| first.source.order().cmp(&second.source.order()))
                .then_with(|| first.lower.cmp(&second.lower))
                .then_with(|| first.hash.cmp(&second.hash))
        };
        if matches.len() > limit {
            matches.select_nth_unstable_by(limit - 1, order);
            matches.truncate(limit);
        }
        matches.sort_unstable_by(order);
        matches
            .into_iter()
            .map(|(_, position)| self.search_hit(&entries[position]))
            .collect()
    }

    fn search_hit(&self, entry: &Entry) -> DefinitionSearchHit {
        DefinitionSearchHit {
            hash: entry.hash,
            name: entry.name.to_string(),
            kind: match entry.source {
                Source::Item | Source::PackageItem => self.item_kind_label(entry.hash),
                source => source.kind(),
            },
            detail: self.search_detail(entry),
            icon: match entry.source {
                Source::Item | Source::PackageItem => Some(entry.hash),
                Source::Collectible => self
                    .collectibles
                    .get(entry.position as usize)
                    .map(|collectible| collectible.item_hash),
                _ => None,
            },
        }
    }

    fn search_detail(&self, entry: &Entry) -> String {
        let position = entry.position as usize;
        let detail = match entry.source {
            Source::Item => self
                .items
                .get(position)
                .map(|item| item.type_name.trim().to_owned()),
            Source::PackageItem => self
                .plug_type_name(entry.hash)
                .or_else(|| self.package_item_type_name(entry.hash))
                .map(|name| name.trim().to_owned()),
            Source::Collectible => self.collectibles.get(position).map(|collectible| {
                let path = self.path_text(collectible.hash, collectible.parent_nodes.first());
                if path.is_empty() {
                    collectible.type_name.trim().to_owned()
                } else {
                    path
                }
            }),
            Source::Record => self
                .records()
                .and_then(|records| records.get(position))
                .map(|record| {
                    let path = self.path_text(record.hash, record.parent_nodes.first());
                    if path.is_empty() {
                        record
                            .paths
                            .first()
                            .map(|path| join_path(path.iter().rev().map(String::as_str)))
                            .unwrap_or_default()
                    } else {
                        path
                    }
                }),
            Source::PresentationNode => self
                .presentation_nodes
                .get(position)
                .map(|node| self.path_text(node.hash, node.parents.first())),
            Source::Objective
            | Source::Progression
            | Source::ItemStat
            | Source::ItemTrait
            | Source::UnlockFlag
            | Source::UnlockValue => None,
        };
        detail.unwrap_or_default()
    }

    fn path_text(&self, start: u64, parent: Option<&u64>) -> String {
        join_path(
            self.presentation_chain(start, parent.copied())
                .into_iter()
                .map(|node| node.name.as_str()),
        )
    }
}
