//! Inventory art uses the gear carrier's activation and rig, independently of its summon.
use super::*;

/// Inventory has no damage controller. Keep the initially selected source
/// sections and let the gear carrier supply their activation group.
pub(super) fn intact(scene: &Scene) -> Result<Scene> {
    let mut scene = scene.clone();
    if let Some(damage) = scene.damage.take() {
        let model = &mut scene
            .models
            .first_mut()
            .context("Inventory source model")?
            .model;
        let mut intact = Vec::new();
        for primitive in std::mem::take(&mut model.primitives) {
            let group = usize::from(damage.group(&primitive)?) - 1;
            if damage.groups[group].states.contains(&0) {
                intact.push(primitive);
            }
        }
        ensure!(
            !intact.is_empty(),
            "Inventory source has no intact geometry"
        );
        model.primitives = intact;
    }
    Ok(scene)
}

/// Gear art occupies a smaller native display envelope than the summoned vehicle.
pub(super) fn fit(
    mesh: &mut Mesh,
    carrier: &Payload,
    entry: &ImportRequest,
    projectile: bool,
) -> Result<Option<Value>> {
    if projectile || entry.runtime_entity.is_some() || entry.recipe["kind"] != "sparrow" {
        return Ok(None);
    }
    ensure!(
        !mesh.positions.is_empty() && mesh.positions.iter().flatten().all(|v| v.is_finite()),
        "Inventory geometry is empty or nonfinite"
    );
    let center = std::array::from_fn::<_, 3, _>(|axis| {
        let lo = mesh
            .positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::INFINITY, f32::min);
        let hi = mesh
            .positions
            .iter()
            .map(|p| p[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        (lo + hi) * 0.5
    });
    let radius = mesh
        .positions
        .iter()
        .map(|p| {
            p.iter()
                .zip(center)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt()
        })
        .fold(0f32, f32::max);
    let target_center = [carrier.f32(0x80)?, carrier.f32(0x84)?, carrier.f32(0x88)?];
    let target_radius = carrier.f32(0x8c)? * entry.inspection_scale;
    ensure!(
        radius.is_finite()
            && radius > 0.
            && target_radius.is_finite()
            && target_radius > 0.
            && target_center.iter().all(|v| v.is_finite()),
        "Inventory display bounds are degenerate"
    );
    let scale = target_radius / radius;
    for point in &mut mesh.positions {
        for axis in 0..3 {
            point[axis] = (point[axis] - center[axis]) * scale + target_center[axis];
        }
    }
    Ok(Some(
        json!({"scale":scale,"display_scale":entry.inspection_scale,"source_center":center,"source_radius":radius,
        "target_center":target_center,"target_radius":target_radius}),
    ))
}

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
        None,
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
        "fit":art["inspection_fit"],
        "method":"Inventory gear carrier with independent activation and source geometry fitted to its native display bounds"
    });
    Ok(())
}
