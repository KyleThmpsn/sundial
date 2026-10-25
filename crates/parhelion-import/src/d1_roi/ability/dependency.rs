//! Typed private resource closure shared by ability animation and effect preparation.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Patch {
    pub offset: usize,
    pub symbol: String,
    pub class: u32,
}

#[derive(Clone)]
pub struct Asset {
    pub symbol: String,
    pub class: u32,
    pub payload: Payload,
    pub patches: Vec<Patch>,
}

/// A private payload after the package allocator has supplied every resource identity.
pub struct Linked {
    pub symbol: String,
    pub tag: u32,
    pub class: u32,
    pub payload: Payload,
}

/// Resolve the complete typed closure in memory. Allocation and publication stay with the
/// caller. No payload is returned until every dependency and allocated identity is checked.
pub fn link(
    assets: &[Asset],
    roots: &[String],
    allocations: &BTreeMap<String, u32>,
) -> Result<Vec<Linked>> {
    validate(assets, roots)?;
    let expected = assets
        .iter()
        .map(|asset| &asset.symbol)
        .collect::<BTreeSet<_>>();
    ensure!(
        expected == allocations.keys().collect(),
        "Ability allocations do not cover exactly the private closure"
    );
    let mut tags = BTreeSet::new();
    for &tag in allocations.values() {
        ensure!(
            !matches!(tag, 0 | u32::MAX) && tags.insert(tag),
            "Ability allocations contain an absent or duplicate identity"
        );
    }
    assets
        .iter()
        .map(|asset| {
            let mut payload = asset.payload.clone();
            for patch in &asset.patches {
                crate::presentation::put(
                    &mut payload.0,
                    patch.offset,
                    &allocations[&patch.symbol].to_le_bytes(),
                )?;
            }
            Ok(Linked {
                symbol: asset.symbol.clone(),
                tag: allocations[&asset.symbol],
                class: asset.class,
                payload,
            })
        })
        .collect()
}

pub fn symbol(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
        "Invalid private ability asset symbol"
    );
    Ok(())
}

/// Follow the converter's typed reference slots. Cyclic effects retain their identity, while
/// unresolved symbols, wrong resource classes, overlapping patches and orphan assets fail.
/// This validates declared references, not arbitrary untyped bytes or native gameplay.
pub fn validate(assets: &[Asset], roots: &[String]) -> Result<Value> {
    ensure!(
        !assets.is_empty() && !roots.is_empty(),
        "Ability dependency closure is empty"
    );
    let mut nodes = BTreeMap::new();
    for asset in assets {
        symbol(&asset.symbol)?;
        ensure!(
            !asset.payload.0.is_empty() && !matches!(asset.class, 0 | u32::MAX),
            "Ability dependency has no payload or class"
        );
        ensure!(
            nodes.insert(asset.symbol.as_str(), asset).is_none(),
            "Duplicate private ability asset"
        );
        let mut occupied = BTreeSet::new();
        for patch in &asset.patches {
            symbol(&patch.symbol)?;
            ensure!(
                asset.payload.u32(patch.offset)? == u32::MAX,
                "Ability dependency patch is not an unresolved resource slot"
            );
            for byte in patch.offset
                ..patch
                    .offset
                    .checked_add(4)
                    .context("Dependency patch overflow")?
            {
                ensure!(occupied.insert(byte), "Ability dependency patches overlap");
            }
        }
    }
    let mut queue = roots.iter().map(String::as_str).collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(key) = queue.pop() {
        let asset = nodes
            .get(key)
            .with_context(|| format!("Missing ability dependency {key}"))?;
        if !seen.insert(key) {
            continue;
        }
        for patch in &asset.patches {
            let target = nodes
                .get(patch.symbol.as_str())
                .with_context(|| format!("Missing {} dependency {}", asset.symbol, patch.symbol))?;
            ensure!(
                target.class == patch.class,
                "Ability dependency {} has class {:08X}, expected {:08X}",
                patch.symbol,
                target.class,
                patch.class
            );
            queue.push(patch.symbol.as_str());
        }
    }
    ensure!(
        seen.len() == nodes.len(),
        "Ability dependency closure contains unreachable assets"
    );
    Ok(
        json!({"roots":roots,"assets":assets.iter().map(|asset| json!({
        "symbol":asset.symbol,"class":asset.class,"bytes":asset.payload.0.len(),
        "sha256":hex::encode(Sha256::digest(&asset.payload.0)),
        "dependencies":asset.patches.iter().map(|patch| json!({
            "offset":patch.offset,"symbol":patch.symbol,"class":patch.class
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>(),"gameplay_verified":false}),
    )
}
