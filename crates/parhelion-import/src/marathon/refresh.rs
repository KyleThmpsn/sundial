//! Rebuild sight presentation and the source controller in an unpinned graph while preserving
//! its authored identity.
use super::*;

/// Author source sights in an unpinned working copy of a Marathon graph. The caller persists
/// the manifest and creates a new content pin after success, leaving the selected source intact.
pub fn author_sights(
    source: &mut Reader,
    native: &mut Reader,
    directory: &Path,
    graph: &mut Value,
) -> Result<()> {
    ensure!(
        graph["source"] == "marathon" || graph["source"].is_null(),
        "Expected a Marathon presentation"
    );
    // A graph keeps the animation it was imported with. Graphs from before source controllers
    // take the donor's animation.
    let source_animation = graph["animation"]["first_person"]["controller"] == "marathon_source";
    if graph
        .get("source_motion")
        .is_some_and(|value| !value.is_null())
        && graph["live_displays"] == true
        && graph["generated_icon"]["revision"]
            .as_u64()
            .is_some_and(|r| r >= 7)
        && graph["inventory_slot"].is_string()
        && graph["source_motion"]["revision"]
            .as_u64()
            .is_some_and(|r| r >= 3)
        && if source_animation {
            graph["animation"]["first_person"]["state_conversion"]["native_fallback_states"]
                .is_array()
        } else {
            graph["animation"]["controller"] == "donor"
        }
        && graph["reticle_drivers"] == true
        && graph["optics_revision"].as_u64().is_some_and(|r| r >= 1)
    {
        return Ok(());
    }
    let mut presentation = Graph::new(
        directory.to_path_buf(),
        graph["nodes"]
            .as_array()
            .context("Marathon graph nodes")?
            .clone(),
    );
    let template = crate::tiger::shadowkeep::extract(native, hash(graph, "native_item")?)?;
    if graph["inventory_slot"].is_null() {
        let item = native.tag(hash(&template, "item_tag")?, Some(0x80807BEA))?;
        graph["inventory_slot"] = json!(match item
            .u8(sundial::package_authoring::investment_schema::ITEM_INVENTORY_SLOT_OFFSET)?
        {
            0 => "kinetic",
            1 => "energy",
            2 => "power",
            _ => anyhow::bail!("Native weapon has an unsupported inventory slot"),
        });
    }
    let models = graph["source_models"]
        .as_array()
        .context("Marathon source models")?;
    let parts = graph
        .get("source_assembly")
        .unwrap_or(&graph["source_models"])
        .as_array()
        .context("Marathon assembly parts")?
        .clone();
    let (rig, animation) = animate(
        source,
        native,
        &parts,
        hash(&template, "item_tag")?,
        directory,
        source_animation,
    )?;
    let mesh = geometry::read(source, models, &rig)?;
    graph["source"] = json!("marathon");
    // Re-encode source passes together so the physical aperture joins the same geometry,
    // material indices and vertex remapping as the existing assembly.
    let model_tag = hash(graph, "native_model")?;
    let header = native.tag(model_tag, Some(0x808073A5))?;
    let mut carrier = None;
    for at in header.array(16, 136, Some(0x80807378))? {
        let positions = native.tag(header.u32(at)?, None)?;
        let attributes = native.tag(header.u32(at + 4)?, None)?;
        if positions.u16(4)? == 8 && attributes.u16(4)? == 24 && header.u16(at + 42)? != 0 {
            let score = positions.u32(0)?;
            if carrier.is_none_or(|(old, _)| score > old) {
                carrier = Some((score, at));
            }
        }
    }
    let (_, at) = carrier.context("Native body carrier")?;
    let encoded = geometry::encode(&mesh, &header, at)?;
    for (index, name) in ["positions", "attributes", "indices"]
        .into_iter()
        .enumerate()
    {
        let symbol = format!("{name}-header");
        let mut bytes = presentation.read(&symbol)?.0;
        put(
            &mut bytes,
            if index == 2 { 8 } else { 0 },
            &u32::try_from(encoded.streams[index].len())?.to_le_bytes(),
        )?;
        if index == 2 {
            bytes[1] = 1;
        } else if index == 0 {
            put(&mut bytes, 4, &16u16.to_le_bytes())?;
        }
        presentation.replace(&symbol, &bytes, vec![])?;
        presentation.replace(&format!("{name}-data"), &encoded.streams[index], vec![])?;
    }
    presentation.replace("model", &encoded.header, encoded.patches)?;
    material::bind_channels(source, native, &mut presentation, &mesh)?;
    let mut materials = Graph::new(
        directory.to_path_buf(),
        vec![presentation.node("owner")?.clone()],
    );
    let reports = material::build(source, native, &mut materials, &mesh)?;
    for node in materials
        .nodes
        .into_iter()
        .filter(|n| n["symbol"] != "owner")
    {
        if let Some(current) = presentation
            .nodes
            .iter_mut()
            .find(|n| n["symbol"] == node["symbol"])
        {
            *current = node;
        } else {
            presentation.nodes.push(node);
        }
    }
    if graph.get("source_optics").is_none_or(Value::is_null) {
        let (sight, report) = optic::author(
            source,
            native,
            &template,
            &mut presentation,
            &parts,
            hash(graph, "item_hash")?,
        )?;
        graph["kept_parts"] = sight;
        graph["source_optics"] = report;
    }
    graph["nodes"] = json!(presentation.nodes);
    graph["source_motion"] = rig.report;
    graph["animation"] = animation;
    graph["source_assembly"] = json!(parts);
    graph["materials"] = json!(reports);
    graph["omitted_passes"] = json!(mesh.omitted);
    graph["native_draw_parts"] = json!(mesh.groups.len());
    graph["vertices"] = json!(mesh.positions.len());
    graph["triangles"] = json!(
        mesh.groups
            .iter()
            .filter(|(key, _)| matches!(mesh.surfaces[key].stage, 0 | 2 | 8 | 15 | 17))
            .map(|(_, faces)| faces.len())
            .sum::<usize>()
    );
    graph["converter_revision"] = json!(crate::graph::CONVERTER_REVISION);
    graph["live_displays"] = json!(true);
    graph["reticle_drivers"] = json!(true);
    graph["optics_revision"] = json!(1);
    if graph["generated_icon"].is_object() || graph["source_icon_png"].is_null() {
        graph
            .as_object_mut()
            .context("Marathon graph")?
            .remove("source_icon_png");
        crate::io::write_json(&directory.join("asset-graph.json"), graph)?;
        crate::artwork::prepare(
            directory,
            native.packages(),
            crate::artwork::Settings::for_graph(graph, &Value::Null),
        )?;
        *graph = serde_json::from_slice(&fs::read(directory.join("asset-graph.json"))?)?;
    }
    Ok(())
}
