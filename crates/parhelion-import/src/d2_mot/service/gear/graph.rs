//! Assemble converted variants into one self-contained, reusable gear graph.
use super::*;

pub(super) fn append(
    folder: &Path,
    target: &Path,
    prefix: &str,
    nodes: &mut Vec<Value>,
) -> Result<Value> {
    let mut graph = load(&folder.join("asset-graph.json"))?;
    let source_nodes = graph["nodes"].as_array().context("converted asset nodes")?;
    let names = source_nodes
        .iter()
        .map(|node| {
            let symbol = node["symbol"].as_str().context("converted asset symbol")?;
            Ok((symbol.to_owned(), format!("{prefix}-{symbol}")))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for node in source_nodes {
        let mut node = node.clone();
        let old_symbol = node["symbol"].as_str().context("asset symbol")?;
        let name = names
            .get(old_symbol)
            .context("asset symbol mapping")?
            .clone();
        let file = format!("{name}.bin");
        let source_file = node["file"].as_str().context("asset payload")?;
        ensure!(
            Path::new(source_file)
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_))),
            "Converted asset path escapes its graph"
        );
        fs::copy(folder.join(source_file), target.join(&file))?;
        node["model"] = json!(old_symbol == "model" || node["model"] == true);
        node["file"] = json!(file);
        for field in ["symbol", "reference", "shared_owner"] {
            if let Some(symbol) = node[field].as_str() {
                node[field] = json!(
                    names
                        .get(symbol)
                        .with_context(|| format!("Unknown asset {field} {symbol}"))?
                );
            }
        }
        for patch in node["patches"]
            .as_array_mut()
            .context("asset relocations")?
        {
            let symbol = patch["symbol"].as_str().context("relocation symbol")?;
            patch["symbol"] = json!(
                names
                    .get(symbol)
                    .context("relocation target outside graph")?
            );
        }
        nodes.push(node);
    }
    for dye in graph["dyes"].as_array_mut().into_iter().flatten() {
        dye["parent"] = json!(
            names
                .get(dye["parent"].as_str().context("dye parent")?)
                .context("dye parent mapping")?
        );
    }
    if let Some(parent) = graph["parent"].as_str() {
        graph["parent"] = json!(names.get(parent).context("art parent mapping")?);
    }
    Ok(graph)
}

pub(super) fn assemble(
    source: &Weapon,
    item: u32,
    donor: u32,
    parts: &[(Value, PathBuf)],
    report: &Value,
    native: &Value,
    output: &Path,
) -> Result<Value> {
    fs::create_dir_all(output)?;
    let mut nodes = Vec::new();
    let mut converted = BTreeMap::new();
    let mut registrations = Vec::new();
    let mut rendering = Vec::new();
    for (index, (part, path)) in parts.iter().enumerate() {
        let parent = if let Some(parent) = converted.get(path) {
            String::clone(parent)
        } else {
            let graph = append(path, output, &format!("part-{index}"), &mut nodes)?;
            let parent = graph["parent"]
                .as_str()
                .context("converted model parent")?
                .to_owned();
            converted.insert(path.clone(), parent.clone());
            let mut evidence = json!({"source_entity":part["entity"],"appearance":graph["appearance"],
                "materials":graph["material_programs"],"effects":graph["effects"],"adapter":graph["attachment_adapter"],
                "source_shader_adapter":graph["source_shader_adapter"],
                "object_channel_adapter":graph["object_channel_adapter"],
                "source_procedural_adapter":graph["source_procedural_adapter"],
                "resting_source_procedures":graph["resting_source_procedures"],
                "native_components":graph["native_components"],
                "runtime_material_draws":graph["source_runtime_material_draws"]});
            for stage in [0, 1, 3, 7, 9, 12, 14, 16] {
                let field = format!("source_stage_{stage}_adapter");
                evidence[&field] = graph[&field].clone();
            }
            rendering.push(evidence);
            parent
        };
        registrations.push(
            json!({"source_assignment":profile::hash(part,"assignment")?,
            "key":private_key(item,&format!("art-{index}")),"parent":parent}),
        );
    }
    let source_rows = report["art_rows"].as_array().context("source art rows")?;
    let native_rows = native["art_rows"].as_array().context("native art rows")?;
    let mut rows = Vec::new();
    for row in source_rows {
        let template = native_rows
            .iter()
            .find(|native| native["class"] == row["class"])
            .or_else(|| native_rows.first())
            .context("native art row template")?;
        let mut row = row.clone();
        row["template_index"] = template["art_index"].clone();
        rows.push(row);
    }
    ensure!(!rows.is_empty(), "Source gear has no art variants");
    let primary = registrations.first().context("converted gear parts")?["parent"].clone();
    Ok(
        json!({"kind":source.family(),"item_hash":item,"native_item":donor,
        "source_item":source.hash,"source_name":source.name,"source_type":source.weapon_type,
        "source_bucket":source.bucket_hash,"source_class":source.class_type,
        "nodes":nodes,"parent":primary,"gear_art":{"rows":rows,"parts":registrations},
        "rendering":rendering,"installable":true,"gameplay_verified":false}),
    )
}
