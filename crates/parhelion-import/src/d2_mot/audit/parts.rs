//! Validate independent source attachment identities and geometry in emitted packages.
use super::*;
use std::collections::BTreeMap;

pub(super) fn audit(
    graph: &Value,
    symbols: &Value,
    assignments: &Payload,
    slots: &[(u64, Vec<u32>)],
    read: &impl Fn(u32) -> Result<Payload>,
) -> Result<BTreeMap<u32, u32>> {
    let mut bindings = BTreeMap::new();
    let tag = |name: &str| -> Result<u32> {
        Ok(u32::try_from(
            symbols[name].as_u64().context("source part symbol")?,
        )?)
    };
    let rows = assignments.array(8, 8, None)?;
    let mut total = graph["native_draw_parts"]
        .as_u64()
        .context("main draw count")?;
    let mut sources = BTreeSet::new();
    for part in graph["source_parts"].as_array().into_iter().flatten() {
        let assignment = u32::try_from(part["assignment"].as_u64().context("part assignment")?)?;
        let key = u32::try_from(part["key"].as_u64().context("part key")?)?;
        let selector = part["selector"].as_u64().context("part selector")?;
        let position = usize::try_from(part["position"].as_u64().context("part position")?)?;
        ensure!(
            part["source_region"] == true
                || slots
                    .iter()
                    .filter(|(s, keys)| *s == selector && keys.get(position) == Some(&assignment))
                    .count()
                    == 1,
            "source part does not match the validated attachment position"
        );
        if part["source_region"] != true {
            ensure!(
                bindings.insert(assignment, key).is_none(),
                "duplicate source attachment"
            );
        }
        let parent = tag(part["parent"].as_str().context("part parent")?)?;
        let entity = tag(part["entity"].as_str().context("part entity")?)?;
        let owner = tag(part["owner"].as_str().context("part owner")?)?;
        let model = tag(part["model"].as_str().context("part model")?)?;
        ensure!(
            rows.iter()
                .filter(|row| assignments.u32(**row).ok() == Some(key)
                    && assignments.u32(**row + 4).ok() == Some(parent))
                .count()
                == 1,
            "source part assignment is unresolved"
        );
        ensure!(
            read(parent)?.u32(16)? == entity,
            "source part parent is unresolved"
        );
        let entity = read(entity)?;
        ensure!(
            entity
                .array(16, 12, Some(0x80809C04))?
                .iter()
                .any(|at| entity.u32(*at).ok() == Some(owner)),
            "source part owner is unresolved"
        );
        let owner = read(owner)?;
        ensure!(
            owner.u32(owner.pointer(24)? + 0x1DC)? == model,
            "source part model is unresolved"
        );
        let model = read(model)?;
        let meshes = model.array(16, 136, Some(0x80807378))?;
        ensure!(
            meshes.len() == 1 && model.f32(0x6C)? > 0.0,
            "source part has no visible geometry"
        );
        let draws = model.array(meshes[0] + 24, 32, Some(0x8080737E))?;
        super::draws::validate(&model, meshes[0], &draws)?;
        ensure!(
            part["draw_count"].as_u64() == Some(draws.len() as u64),
            "source part draw count differs"
        );
        total += draws.len() as u64;
        for source in part["source_models"]
            .as_array()
            .context("part source models")?
        {
            ensure!(
                sources.insert(source.as_str().context("source model")?),
                "source geometry occurs in multiple attachments"
            );
        }
    }
    if graph["source_parts"]
        .as_array()
        .is_some_and(|parts| !parts.is_empty())
    {
        ensure!(
            graph["assembled_draw_parts"].as_u64() == Some(total),
            "source geometry partition is incomplete"
        );
    }
    Ok(bindings)
}
