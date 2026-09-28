//! Preserve ordered source art regions for the client's stat-driven selection.
use super::*;
use std::collections::BTreeSet;

pub(super) fn regions(source: &Value) -> Result<BTreeSet<u64>> {
    let mut positions = BTreeMap::<u64, BTreeSet<u64>>::new();
    for part in source["art_parts"].as_array().into_iter().flatten() {
        for placement in part["placements"].as_array().into_iter().flatten() {
            if let (Some(selector), Some(position)) = (
                placement["selector"].as_u64(),
                placement["position"].as_u64(),
            ) {
                positions.entry(selector).or_default().insert(position);
            }
        }
    }
    positions
        .into_iter()
        .filter(|(_, values)| values.len() > 1)
        .map(|(selector, values)| {
            ensure!(
                values.iter().copied().eq(0..values.len() as u64),
                "source art region {selector} has discontinuous alternatives"
            );
            ensure!(
                selector != 0,
                "alternative weapon bodies require separate runtime composition"
            );
            Ok(selector)
        })
        .collect()
}
