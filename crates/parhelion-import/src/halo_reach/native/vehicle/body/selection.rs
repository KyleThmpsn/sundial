//! Compiled HLMT indices join render states to physics and hit permutations.
use super::*;
use std::collections::BTreeMap;

pub(in super::super) struct Form {
    pub region: String,
    pub source_region: usize,
    pub permutation: usize,
    pub states: Vec<u8>,
}

pub(in super::super) fn forms(scene: &Scene, kind: &str) -> Result<Vec<Form>> {
    ensure!(
        matches!(kind, "physics" | "collision"),
        "Unknown vehicle collision mapping kind"
    );
    let index_key = format!("{kind}_index");
    let source = &scene.object.gameplay["rigid_body"];
    let source = if kind == "collision" {
        &source["shot_collision"]
    } else {
        source
    };
    let source_regions = source["regions"]
        .as_array()
        .context("Source collision regions")?;
    let mapped = scene.object.gameplay["damage"]["collision_regions"]
        .as_array()
        .context("Compiled source collision mappings")?;
    let geometry = scene
        .damage
        .as_ref()
        .context("Source vehicle damage forms")?;
    let mut out = Vec::new();
    let mut covered = BTreeSet::new();
    for region in &geometry.regions {
        let matches = mapped
            .iter()
            .filter(|row| row["name"] == region.name)
            .collect::<Vec<_>>();
        ensure!(matches.len() <= 1, "Ambiguous compiled collision region");
        let Some(mapping) = matches.first() else {
            continue;
        };
        let index = mapping[&index_key]
            .as_i64()
            .context("Compiled collision region index")?;
        if index == -1 {
            continue;
        }
        let index = usize::try_from(index)?;
        let target = source_regions
            .get(index)
            .context("Compiled collision region exceeds table")?;
        ensure!(
            target["name"] == region.name,
            "Compiled collision region name differs"
        );
        ensure!(
            covered.insert(index),
            "Multiple render regions select one collision region"
        );
        let source_permutations = target["permutations"]
            .as_array()
            .context("Collision permutations")?;
        let mappings = mapping["permutations"]
            .as_array()
            .context("Compiled collision permutation mappings")?;
        let mut permutations = BTreeMap::<usize, Vec<u8>>::new();
        for (ordinal, name) in region.states.iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let matches = mappings
                .iter()
                .filter(|row| row["name"] == *name)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "Compiled collision permutation is absent or ambiguous"
            );
            let permutation = matches[0][&index_key]
                .as_i64()
                .context("Compiled collision permutation index")?;
            if permutation == -1 {
                continue;
            }
            let permutation = usize::try_from(permutation)?;
            ensure!(
                permutation < source_permutations.len(),
                "Compiled collision permutation exceeds table"
            );
            permutations
                .entry(permutation)
                .or_default()
                .push(u8::try_from(ordinal)?);
        }
        for (permutation, states) in permutations {
            out.push(Form {
                region: region.name.clone(),
                source_region: index,
                permutation,
                states,
            });
        }
    }
    ensure!(
        covered.len() == source_regions.len(),
        "Unmapped source collision region"
    );
    Ok(out)
}

pub(in super::super) struct Selected {
    pub body: usize,
    pub condition: Option<(String, Vec<u8>)>,
}

pub(in super::super) fn physical(scene: &Scene) -> Result<Vec<Selected>> {
    let source = &scene.object.gameplay["rigid_body"];
    let bodies = source["bodies"].as_array().context("Source rigid bodies")?;
    let mut selected = BTreeMap::new();
    for (index, row) in bodies.iter().enumerate() {
        if row["region"] == -1 && row["permutation"] == -1 {
            selected.insert(index, None);
        }
    }
    for form in forms(scene, "physics")? {
        let rows =
            source["regions"][form.source_region]["permutations"][form.permutation]["bodies"]
                .as_array()
                .context("Source permutation bodies")?;
        for value in rows {
            let index = usize::try_from(value.as_u64().context("Source physical body index")?)?;
            let row = bodies
                .get(index)
                .context("Source physical body exceeds table")?;
            ensure!(
                row["region"] == form.source_region && row["permutation"] == form.permutation,
                "Physical body disagrees with its compiled permutation"
            );
            ensure!(
                selected
                    .insert(index, Some((form.region.clone(), form.states.clone())))
                    .is_none(),
                "Source physical body has multiple activation mappings"
            );
        }
    }
    ensure!(
        !selected.is_empty(),
        "Source vehicle has no reachable physical bodies"
    );
    Ok(selected
        .into_iter()
        .map(|(body, condition)| Selected { body, condition })
        .collect())
}
