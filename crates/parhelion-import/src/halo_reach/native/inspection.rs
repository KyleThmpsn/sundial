//! Inventory art uses the gear carrier's activation and rig, independently of its summon.
use super::*;

pub(super) fn build(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    native: &mut Reader,
    scene: &Scene,
    entry: &ImportRequest,
    root: &Path,
    graph: &mut Value,
) -> Result<()> {
    if entry.runtime_entity.is_none() || entry.recipe["kind"] != "sparrow" {
        return Ok(());
    }
    let mut inventory = entry.clone();
    inventory.runtime_entity = None;
    inventory.driver_pose = None;
    inventory.animation_bindings.clear();
    inventory.bone_aliases.clear();
    inventory.first_person = None;
    let mut art = super::build(
        cache,
        pages,
        native,
        scene,
        &inventory,
        &root.join("inspection"),
        false,
    )?;
    let prefix = |name: &str| format!("inspection-{name}");
    let nodes = art["nodes"].as_array_mut().context("Inventory art nodes")?;
    let symbols = nodes
        .iter()
        .map(|node| {
            node["symbol"]
                .as_str()
                .map(str::to_owned)
                .context("Inventory art symbol")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    for node in nodes.iter_mut() {
        let name = node["symbol"]
            .as_str()
            .context("Inventory art symbol")?
            .to_owned();
        if name == "model" {
            node["model"] = json!(true);
        }
        node["symbol"] = json!(prefix(&name));
        node["file"] = json!(format!(
            "inspection/{}",
            node["file"].as_str().context("Inventory art file")?
        ));
        for key in ["reference", "shared_owner"] {
            if let Some(name) = node[key].as_str() {
                ensure!(
                    symbols.contains(name),
                    "Inventory art {key} is outside its graph"
                );
                node[key] = json!(prefix(name));
            }
        }
        for patch in node["patches"]
            .as_array_mut()
            .context("Inventory art references")?
        {
            let name = patch["symbol"]
                .as_str()
                .context("Inventory art reference")?;
            ensure!(
                symbols.contains(name),
                "Inventory art reference is outside its graph"
            );
            patch["symbol"] = json!(prefix(name));
        }
    }
    graph["nodes"]
        .as_array_mut()
        .context("Vehicle art nodes")?
        .extend(nodes.iter().cloned());
    for key in ["parent", "companion"] {
        graph[key] = json!(prefix(art[key].as_str().context("Inventory art parent")?));
    }
    for part in art["gear_art"]["parts"]
        .as_array_mut()
        .context("Inventory art parts")?
    {
        part["parent"] = json!(prefix(
            part["parent"]
                .as_str()
                .context("Inventory art assignment")?
        ));
    }
    graph["gear_art"] = art["gear_art"].clone();
    graph["inspection"] = json!({
        "source_entity":art["source_entity"], "source_owner":art["source_owner"],
        "model":"inspection-model", "native_skinning":art["native_skinning"],
        "instance_layout":art["instance_layout"],
        "method":"Inventory gear carrier with independent activation and source geometry"
    });
    Ok(())
}
