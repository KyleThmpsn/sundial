//! Keep unsupported source passes visible after supported materials have been translated.
use super::*;
use std::collections::BTreeSet;

pub(super) fn record(c: &mut Effect, progress: &mut dyn FnMut(String)) -> Result<()> {
    let supported = [0, 1, 3, 7, 9, 12, 14, 16];
    let mut unsupported = Vec::new();
    let mut omitted = BTreeSet::new();
    let mut stages = BTreeSet::new();
    for entry in c.source.report["models"]
        .as_array()
        .context("source models")?
    {
        let tag = entry["model"].as_str().context("source model")?;
        let model = c.source.model(tag)?;
        let mesh = geometry::selected_mesh(&model, entry)?;
        let parts = model.array(mesh + 32, 36, Some(0x80806ECB))?;
        // Stage 23 is compute skinning, lowered separately to native geometry and skinning.
        for stage in (0..23).filter(|stage| !supported.contains(stage)) {
            let start = usize::from(model.u16(mesh + 48 + stage * 2)?);
            let end = usize::from(model.u16(mesh + 50 + stage * 2)?);
            for &part in parts
                .get(start..end)
                .context("source stage exceeds draw table")?
            {
                if model.u32(part + 12)? == 0 {
                    continue;
                }
                let absent = c.draws.records[stage].is_empty();
                stages.insert(stage);
                if absent {
                    omitted.insert(stage);
                }
                unsupported.push(json!({"model":tag,"mesh":entry["mesh_index"],
                    "stage":stage,"material":format!("{:08X}",model.u32(part)?),
                    "source_part":(part-parts[0])/36,"native_stage_has_draws":!absent,
                    "reason":"unsupported source rendering stage"}));
            }
        }
    }
    let runtime_missing = c.graph.manifest["source_runtime_material_draws"]
        .as_array()
        .is_some_and(|draws| !draws.is_empty());
    let substituted = supported.iter().any(|stage| {
        c.graph.manifest[format!("source_stage_{stage}_adapter")]["materials"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|material| {
                material["source_vertex_equations_retained"] == false
                    || material["source_pixel_equations_retained"] == false
            })
    });
    if !unsupported.is_empty() || runtime_missing || substituted {
        c.graph.manifest["source_shader_adapter"]["source_shader_equations_retained"] =
            json!(false);
    }
    if !unsupported.is_empty() || runtime_missing {
        c.graph.manifest["source_shader_adapter"]["source_material_equations_retained"] =
            json!(false);
    }
    let mut limitations = Vec::new();
    if stages.remove(&8) {
        limitations.push("Source distortion effects are not fully reproduced.");
    }
    if !stages.is_empty() {
        limitations.push("Some source visual effects use unsupported rendering features.");
    }
    for limitation in &limitations {
        progress((*limitation).into());
    }
    c.graph.manifest["source_unsupported_material_draws"] = json!(unsupported);
    c.graph.manifest["attachment_adapter"]["omitted_stages"] = json!(omitted);
    c.graph.manifest["material_limitations"] = json!(limitations);
    Ok(())
}
