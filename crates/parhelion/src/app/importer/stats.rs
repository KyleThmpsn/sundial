//! Investment stats a reimport takes from the new conversion.
use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

/// A stat the saved recipe still holds at the value its earlier conversion gave it was never
/// edited, so the new conversion's value replaces it, or the donor's own when the new conversion
/// carries none. Edited and removed stats stay as they are, and so does every stat when the
/// earlier conversion's graph cannot be read.
pub(super) fn refresh(
    baseline: &WeaponRecipe,
    prepared: &WeaponRecipe,
    updated: &mut WeaponRecipe,
) {
    let Some(previous) = baseline
        .overrides
        .imported_graph
        .as_ref()
        .and_then(converted)
    else {
        return;
    };
    let stats = &mut updated.overrides.investment_stats;
    stats.retain(|stat| previous.get(&stat.definition_index) != Some(&stat.value));
    for stat in &prepared.overrides.investment_stats {
        if !stats
            .iter()
            .any(|kept| kept.definition_index == stat.definition_index)
            && !updated
                .overrides
                .removed_investment_stats
                .contains(&stat.definition_index)
        {
            stats.push(stat.clone());
        }
    }
}

/// The investment values a conversion wrote into its graph, by stat definition index.
fn converted(reference: &parhelion_import::GraphReference) -> Option<BTreeMap<u16, i32>> {
    let graph: Value =
        serde_json::from_slice(&std::fs::read(reference.directory.join("asset-graph.json")).ok()?)
            .ok()?;
    graph["source_gameplay"]["investment_stats"]
        .as_array()?
        .iter()
        .map(|row| {
            Some((
                u16::try_from(row["definition_index"].as_u64()?).ok()?,
                i32::try_from(row["value"].as_i64()?).ok()?,
            ))
        })
        .collect()
}
