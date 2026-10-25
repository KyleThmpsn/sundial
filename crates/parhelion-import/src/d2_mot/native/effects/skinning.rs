//! Keep native preview and runtime deformation on the same four bone weights.
use super::*;

pub(super) fn write(c: &mut Effect) -> Result<()> {
    let header = c.graph.read("positions-header")?;
    let stride = usize::from(header.u16(4)?);
    ensure!(
        matches!(stride, 8 | 16),
        "native position stride differs before skinning"
    );
    let original = c.graph.read("positions-data")?;
    ensure!(
        header.u32(0)? as usize == original.0.len() && original.0.len().is_multiple_of(stride),
        "native position header differs before skinning"
    );
    let bones = match c.graph.manifest["rig_mapping"]["bone_map"].as_array() {
        Some(rows) => rows
            .iter()
            .map(|v| Ok(u16::try_from(v.as_u64().context("native bone mapping")?)?))
            .collect::<Result<Vec<_>>>()?,
        None => vec![0],
    };
    let mut weights = Vec::new();
    let mut segments = Vec::new();
    for entry in c.source.report["models"]
        .as_array()
        .context("source skinning models")?
    {
        let model = c
            .source
            .model(entry["model"].as_str().context("source model")?)?;
        let mesh = geometry::selected_mesh(&model, entry)?;
        let streams = geometry::streams(&c.source.root, &c.source.manifest, &model, mesh)?;
        let vertices = crate::d2_mot::skinning::native_vertices(
            &streams.positions,
            &streams.auxiliary,
            &bones,
        )?;
        segments.push(json!({"model":entry["model"],"mesh_index":entry["mesh_index"],"first_vertex":weights.len(),"vertices":vertices.len(),"cloth":entry["cloth"]==true}));
        weights.extend(vertices);
    }
    ensure!(
        weights.len() == original.0.len() / stride,
        "source skinning does not cover the native vertices"
    );
    let mut positions = Vec::with_capacity(weights.len() * 16);
    for (vertex, weight) in original.0.chunks_exact(stride).zip(&weights) {
        positions.extend_from_slice(&vertex[..8]);
        positions.extend_from_slice(weight);
    }
    // All separated models reference the same complete position stream.
    let mut symbols = vec!["model".to_owned()];
    for part in c.graph.manifest["source_parts"]
        .as_array()
        .into_iter()
        .flatten()
    {
        symbols.push(
            part["model"]
                .as_str()
                .context("source part model")?
                .to_owned(),
        );
    }
    symbols.sort();
    symbols.dedup();
    let mut models = Vec::new();
    let palette = weights
        .iter()
        .flat_map(|row| row[4..].iter())
        .copied()
        .max()
        .map_or(1, |bone| u32::from(bone) + 1);
    for symbol in symbols {
        let mut model = c.graph.read(&symbol)?;
        let palette = palette.max(model.u32(0x40)?);
        put(&mut model.0, 0x40, &palette.to_le_bytes())?;
        for mesh in model.array(0x10, 0x88, Some(0x80807378))? {
            for stage in 0..23 {
                let at = mesh + 88 + stage * 2;
                match model.i16(at)? {
                    -1 => {}
                    28 | 139 => put(&mut model.0, at, &28i16.to_le_bytes())?,
                    layout => {
                        anyhow::bail!("native skinning cannot replace vertex layout {layout}")
                    }
                }
            }
        }
        models.push((symbol, model.0));
    }
    let mut header = header.0;
    put(
        &mut header,
        0,
        &u32::try_from(positions.len())?.to_le_bytes(),
    )?;
    put(&mut header, 4, &16u16.to_le_bytes())?;
    c.graph.write("positions-data", &positions)?;
    c.graph.write("positions-header", &header)?;
    for (symbol, bytes) in models {
        c.graph.write(&symbol, &bytes)?;
    }
    c.graph.manifest["native_skinning"] = json!({"layout":28,"position_stride":16,"influences":4,"segments":segments,"source_weights_preserved":true,"gameplay_verified":false});
    Ok(())
}
