//! Private resource nodes whose package references, as well as their patch words, name other
//! private nodes: particle systems, their materials, shaders, textures and samplers, and the
//! effect graphs that play them. A closure is checked whole before any node joins a graph.
use super::symbol;
use crate::presentation::Graph;
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// A resource's package reference can name another private node, independently of its payload.
/// The containing graph's allocator owns final tags and private-package reservation.
pub struct Node {
    pub symbol: String,
    pub template: u32,
    pub payload: Vec<u8>,
    pub reference: Option<String>,
    pub patches: Vec<(usize, String)>,
}

/// The nodes that `roots` reach through patches and references, in their original order.
/// Systems prepared for an effect graph whose translation was refused are left out.
pub(crate) fn closure(assets: Vec<Node>, roots: &[&str]) -> Vec<Node> {
    let by_symbol = assets
        .iter()
        .map(|node| (node.symbol.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut queue = roots
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<Vec<_>>();
    while let Some(symbol) = queue.pop() {
        let Some(node) = by_symbol.get(symbol.as_str()) else {
            continue;
        };
        if !seen.insert(symbol) {
            continue;
        }
        queue.extend(node.patches.iter().map(|(_, name)| name.clone()));
        queue.extend(node.reference.iter().cloned());
    }
    assets
        .into_iter()
        .filter(|node| seen.contains(&node.symbol))
        .collect()
}

/// Every node has a native template and bytes, each patch is a distinct unresolved word, and
/// `roots` reach every node and nothing outside them through patches and references.
pub(crate) fn validate(assets: &[Node], roots: &[&str]) -> Result<()> {
    ensure!(!roots.is_empty(), "Private resource roots are absent");
    for root in roots {
        symbol(root)?;
    }
    let mut nodes = BTreeMap::new();
    for node in assets {
        symbol(&node.symbol)?;
        ensure!(
            !node.payload.is_empty() && !matches!(node.template, 0 | u32::MAX),
            "Private resource {} lacks its native template or bytes",
            node.symbol
        );
        ensure!(
            nodes.insert(node.symbol.as_str(), node).is_none(),
            "Private resource symbols collide"
        );
        let mut occupied = BTreeSet::new();
        for (offset, symbol) in &node.patches {
            super::symbol(symbol)?;
            let end = offset
                .checked_add(4)
                .context("Private resource patch overflow")?;
            ensure!(
                node.payload.get(*offset..end) == Some(&u32::MAX.to_le_bytes()[..])
                    && (*offset..end).all(|byte| occupied.insert(byte)),
                "Private resource patch is not a distinct unresolved word"
            );
        }
    }
    let mut queue = roots.to_vec();
    let mut seen = BTreeSet::new();
    while let Some(symbol) = queue.pop() {
        let node = nodes
            .get(symbol)
            .with_context(|| format!("Private resource dependency {symbol} is absent"))?;
        if !seen.insert(symbol) {
            continue;
        }
        queue.extend(node.patches.iter().map(|(_, name)| name.as_str()));
        if let Some(reference) = &node.reference {
            queue.push(reference);
        }
    }
    ensure!(
        seen.len() == nodes.len(),
        "Private resource closure contains unused resources"
    );
    Ok(())
}

/// Adds checked `nodes` to `graph`, none of whose symbols it holds yet.
pub(crate) fn append(nodes: Vec<Node>, graph: &mut Graph) -> Result<()> {
    ensure!(
        nodes.iter().all(|node| !graph.contains(&node.symbol)),
        "Private resource namespace collides with its containing graph"
    );
    for node in nodes {
        graph.add(
            &node.symbol,
            node.template,
            &node.payload,
            node.reference.as_deref(),
            node.patches
                .iter()
                .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
                .collect(),
        )?;
    }
    Ok(())
}
