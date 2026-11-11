//! Exclusive source upgrade groups and their separate equipped foundation.
//! This intake does not certify the connected effects or a playable native ability.
use super::{
    Choice, Source, SubclassCatalog, SubclassNode, SubclassStep, controller, grants,
    property_choice,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Serialize)]
pub struct Upgrade {
    pub choice: Choice,
    /// Source display row. It orders presentation only and never selects behavior.
    pub row: i16,
    pub keys: Vec<u32>,
    pub resources: Vec<u32>,
    pub revival: bool,
    pub source: Value,
}

#[derive(Clone, Serialize)]
pub struct Selection {
    pub group: u32,
    pub ability_row: u8,
    pub graph: u32,
    pub controller: u32,
    pub bank: u32,
    pub foundation: Choice,
    /// Includes the foundation's other ability grants and action contributions.
    pub foundation_source: Value,
    pub upgrades: Vec<Upgrade>,
}

fn choice(catalog: &SubclassCatalog, node: &SubclassNode, step: &SubclassStep) -> Choice {
    Choice {
        subclass: catalog.item,
        subclass_name: catalog.name.clone(),
        grid: catalog.grid,
        node: node.node,
        step: step.step,
        name: step.name.clone(),
        description: step.description.clone(),
    }
}

impl Selection {
    /// All keys needed by a shared private bank. A path still selects only its upgrade's keys.
    pub fn bank_keys(&self) -> BTreeSet<u32> {
        self.upgrades
            .iter()
            .flat_map(|upgrade| upgrade.keys.iter().copied())
            .collect()
    }

    pub fn resources(&self) -> BTreeSet<u32> {
        self.upgrades
            .iter()
            .flat_map(|upgrade| upgrade.resources.iter().copied())
            .collect()
    }
}

impl Source {
    /// Resolve the complete exclusive group containing a structurally qualified revival choice.
    /// Node, step, group and bank identities drive discovery. Display names never select behavior.
    pub fn radiance_selection(&self, selected: &Choice) -> Result<Selection> {
        let catalogs = self.subclass_choices()?;
        let catalog = catalogs
            .iter()
            .find(|catalog| catalog.item == selected.subclass && catalog.grid == selected.grid)
            .context("Source Radiance subclass changed")?;
        ensure!(
            catalog.controller_class == 0x808025B8,
            "Source Radiance controller family differs"
        );
        let node = catalog
            .nodes
            .iter()
            .find(|node| node.node == selected.node)
            .context("Source Radiance upgrade node changed")?;
        let step = node
            .steps
            .iter()
            .find(|step| step.step == selected.step)
            .context("Source Radiance upgrade step changed")?;
        ensure!(
            choice(catalog, node, step) == *selected,
            "Source Radiance choice metadata changed"
        );
        let (row, keys, _) = property_choice(&step.source)?
            .context("Source Radiance upgrade has unqualified companions")?;
        ensure!(
            row == catalog.ability_row
                && controller::revival_keys(self, catalog.bank, &keys.into_iter().collect())?,
            "Source Radiance group is not anchored by a complete revival choice"
        );
        ensure!(
            node.group_key != 0x811C9DC5 && node.node_flags == 0,
            "Source Radiance upgrade has no exclusive group"
        );
        let mut upgrades = Vec::new();
        let mut distinct = BTreeSet::new();
        for peer in catalog
            .nodes
            .iter()
            .filter(|peer| peer.group_key == node.group_key)
        {
            ensure!(
                peer.node_flags == 0 && peer.steps.len() == 1,
                "Source Radiance group has another progression or selection policy"
            );
            let step = &peer.steps[0];
            ensure!(
                step.foundation == Some(false),
                "Source Radiance upgrade is an automatic contribution"
            );
            let (row, keys, variant) = property_choice(&step.source)?
                .context("Source Radiance group contains an unqualified companion")?;
            ensure!(
                row == catalog.ability_row && keys.iter().all(|key| distinct.insert(*key)),
                "Source Radiance upgrades overlap or target different ability banks"
            );
            let selected_keys = keys.iter().copied().collect();
            upgrades.push(Upgrade {
                choice: choice(catalog, peer, step),
                row: peer.row,
                resources: controller::selected_resources(self, catalog.bank, &selected_keys)?,
                revival: controller::revival_keys(self, catalog.bank, &selected_keys)?,
                keys,
                source: variant,
            });
        }
        ensure!(
            upgrades.len() == 3 && upgrades.iter().filter(|upgrade| upgrade.revival).count() == 1,
            "Source Radiance requires three distinct upgrades and one revival choice"
        );
        upgrades.sort_by_key(|upgrade| (upgrade.row, upgrade.choice.node));
        let mut foundations = Vec::new();
        for node in &catalog.nodes {
            for step in &node.steps {
                if step.foundation != Some(true) {
                    continue;
                }
                let variants = step.source["variants"]
                    .as_array()
                    .context("Source foundation variants")?;
                let mut equips = false;
                for variant in variants {
                    equips |= grants(variant)?
                        .iter()
                        .any(|grant| grant.u8(11).ok() == Some(catalog.ability_row));
                }
                if equips {
                    foundations.push((node, step));
                }
            }
        }
        let [(foundation_node, foundation)] = foundations.as_slice() else {
            anyhow::bail!("Source Radiance has no unique equipped foundation");
        };
        self.unchanged()?;
        Ok(Selection {
            group: node.group_key,
            ability_row: catalog.ability_row,
            graph: catalog.graph,
            controller: catalog.controller,
            bank: catalog.bank,
            foundation: choice(catalog, foundation_node, foundation),
            foundation_source: foundation.source.clone(),
            upgrades,
        })
    }
}
