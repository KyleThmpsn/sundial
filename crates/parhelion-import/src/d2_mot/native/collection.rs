//! Assemble complete source graphs without regenerating or mutating payloads.
use super::*;

pub fn assemble(plan: &Path, out: &Path) -> Result<Value> {
    let out = outside(out, plan)?;
    ensure!(!out.exists(), "collection output already exists");
    let plan = load(plan)?;
    let entries = plan["graphs"].as_array().context("collection graphs")?;
    let recipes = plan["recipes"].as_array().context("collection recipes")?;
    ensure!(
        !entries.is_empty() && !recipes.is_empty(),
        "empty collection"
    );
    let mut identities = std::collections::BTreeSet::new();
    let mut private_keys = std::collections::BTreeSet::new();
    let mut graphs = Vec::new();
    for entry in entries {
        let root = Path::new(entry["graph"].as_str().context("collection graph path")?);
        outside(&out, root)?;
        let graph = Graph {
            root: root.to_owned(),
            manifest: load(&root.join("asset-graph.json"))?,
        };
        ensure!(
            graph.manifest["source_shader_adapter"]["stages"] == json!([0, 1, 3, 7, 9, 12])
                && graph.manifest["attachment_adapter"]["omitted_stages"] == json!([]),
            "collection graph lacks complete source rendering"
        );
        let hash = graph.manifest["item_hash"]
            .as_u64()
            .context("graph item identity")?;
        ensure!(
            identities.insert(hash),
            "duplicate graph item identity {hash:08X}"
        );
        let art = graph.manifest["art_key"]
            .as_u64()
            .context("graph art key")?;
        ensure!(
            private_keys.insert(art),
            "duplicate private art key {art:08X}"
        );
        for dye in graph.manifest["dyes"].as_array().context("graph dyes")? {
            let key = dye["manifest"].as_u64().context("dye manifest key")?;
            ensure!(
                private_keys.insert(key),
                "duplicate private dye key {key:08X}"
            );
        }
        let mut files = std::collections::BTreeSet::new();
        for node in graph.manifest["nodes"].as_array().context("graph nodes")? {
            let path = graph.path(node["symbol"].as_str().context("node symbol")?)?;
            ensure!(
                files.insert(path.clone()) && path.is_file(),
                "duplicate or missing graph payload"
            );
        }
        graphs.push(graph);
    }
    let mut recipe_files = std::collections::BTreeSet::new();
    let mut prepared_recipes = Vec::new();
    let mut recipe_identities = std::collections::BTreeSet::new();
    let mut native_recipes = 0;
    for recipe in recipes {
        let path = Path::new(recipe.as_str().context("collection recipe path")?);
        outside(&out, path)?;
        let name = path.file_name().context("recipe filename")?.to_owned();
        ensure!(
            recipe_files.insert(name.clone()),
            "duplicate recipe filename"
        );
        let bytes = fs::read(path)?;
        let value: Value = serde_json::from_slice(&bytes)?;
        let hash = crate::d2_mot::profile::hash(&value["identity"], "item_hash")? as u64;
        ensure!(
            recipe_identities.insert(hash),
            "duplicate recipe item identity {hash:08X}"
        );
        if !identities.contains(&hash) {
            native_recipes += 1;
        }
        prepared_recipes.push((name, bytes));
    }
    fs::create_dir_all(out.join("recipes"))?;
    for (name, bytes) in &prepared_recipes {
        fs::write(out.join("recipes").join(name), bytes)?;
    }
    let mut paths = Vec::new();
    for (i, graph) in graphs.iter().enumerate() {
        let root = out.join(format!("graph-{i:02}/graph"));
        fs::create_dir_all(&root)?;
        for node in graph.manifest["nodes"].as_array().unwrap() {
            let path = graph.path(node["symbol"].as_str().unwrap())?;
            fs::copy(&path, root.join(path.file_name().unwrap()))?;
        }
        fs::copy(
            graph.root.join("asset-graph.json"),
            root.join("asset-graph.json"),
        )?;
        paths.push(root);
    }
    write_json(&out.join("graphs.json"), &json!(paths))?;
    write_json(&out.join("assembly.json"), &plan)?;
    let result = json!({"graph_count":graphs.len(),"recipe_count":prepared_recipes.len(),"recipes_without_private_graph":native_recipes,
        "source_graphs_preserved":true,"recipes_preserved":true,"private_keys_unique":true,
        "implementation":"Rust","installable":false,"gameplay_verified":false});
    write_json(&out.join("coverage.json"), &result)?;
    Ok(result)
}
