//! Evidence linking a perk to named resources in its action and entity graphs.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::Index;
use crate::package_runtime::tft;

#[derive(Clone, Debug, Serialize)]
pub struct PerkAssets {
    pub perk_index: usize,
    pub action: Vec<usize>,
    pub graphs: Vec<usize>,
    pub components: Vec<usize>,
}

impl PerkAssets {
    pub fn references(&self) -> impl Iterator<Item = usize> + '_ {
        self.action
            .iter()
            .chain(&self.graphs)
            .chain(&self.components)
            .copied()
    }
}

/// Values are indices into `names.references`. Component paths describe context,
/// while a path paired with a graph or action identifies that referenced asset.
#[must_use]
pub fn map(perks: &Index, names: &tft::Index) -> Vec<PerkAssets> {
    let mut by_tag = BTreeMap::<u32, BTreeSet<usize>>::new();
    for (index, reference) in names.references.iter().enumerate() {
        by_tag.entry(reference.source).or_default().insert(index);
        by_tag.entry(reference.target).or_default().insert(index);
    }
    perks
        .perks
        .iter()
        .map(|perk| {
            let collect = |tags: Vec<u32>| {
                tags.iter()
                    .filter_map(|tag| by_tag.get(tag))
                    .flatten()
                    .copied()
                    .collect::<BTreeSet<_>>()
            };
            let action = collect(perk.action.into_iter().collect());
            let graphs = collect(perk.graphs.iter().map(|graph| graph.tag).collect())
                .difference(&action)
                .copied()
                .collect::<BTreeSet<_>>();
            let components = collect(
                perk.graphs
                    .iter()
                    .flat_map(|graph| graph.components.iter().map(|component| component.owner))
                    .collect(),
            )
            .into_iter()
            .filter(|index| !action.contains(index) && !graphs.contains(index))
            .collect();
            PerkAssets {
                perk_index: perk.index,
                action: action.into_iter().collect(),
                graphs: graphs.into_iter().collect(),
                components,
            }
        })
        .collect()
}
