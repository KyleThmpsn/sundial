//! Configured Reach conversion and saved-library projectile ownership.
use super::*;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Deserialize)]
struct Case {
    maps: PathBuf,
    packages: PathBuf,
    previous: PathBuf,
    entry: parhelion_import::halo_reach::browse::Entry,
    projectile_path: String,
    effect_only: parhelion_import::halo_reach::browse::Entry,
    source_revision: String,
    #[serde(default)]
    first_person_actions: BTreeMap<String, Value>,
    #[serde(default)]
    melee_actions: Vec<String>,
    #[serde(default)]
    scoped: bool,
    output: PathBuf,
}

fn read_graph(recipe: &WeaponRecipe) -> Result<Value> {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .context("Imported graph")?;
    reference.validate(recipe.identity.item_hash.parse_u32()?)?;
    Ok(serde_json::from_slice(&fs::read(
        reference.directory.join("asset-graph.json"),
    )?)?)
}

#[test]
#[ignore = "Requires PARHELION_PROJECTILE_IMPORT_CASE with MCC caches, stock packages and a saved weapon recipe"]
fn projectile_import_survives_publication_and_rejects_incomplete_replacement() -> Result<()> {
    let case: Case = serde_json::from_slice(&fs::read(
        std::env::var_os("PARHELION_PROJECTILE_IMPORT_CASE")
            .context("Set PARHELION_PROJECTILE_IMPORT_CASE")?,
    )?)?;
    ensure!(
        !case.output.exists(),
        "Use a fresh projectile import output directory"
    );
    ensure!(
        !case.source_revision.trim().is_empty(),
        "Record the tested source revision in the case"
    );
    let library = RecipeLibrary::open(case.output.join("library")).map_err(anyhow::Error::msg)?;
    let mut baseline = WeaponRecipe::load_json(&case.previous)?;
    baseline.flavor = "Authored text retained through projectile reimport.".into();
    let item = baseline.identity.item_hash.parse_u32()?;
    ensure!(
        item == case.entry.destination()?,
        "Configured source and recipe differ"
    );
    let path = library.save_new(&baseline).map_err(anyhow::Error::msg)?;
    let donor = baseline.donor.item_hash.parse_u32()?;
    let donor_name = baseline.donor.expected_name.as_deref().unwrap_or_default();
    let request = sources::reach_request(&case.entry, donor, donor_name, Some(&path))?;
    ensure!(
        request.projectile_entity.is_none(),
        "Case must use automatic projectile selection"
    );
    let converted = parhelion_import::halo_reach::browse::import(
        &request,
        &case.maps,
        &case.packages,
        &case.output.join("converted"),
    )?;
    let existing = [(path.clone(), item)];
    save_imported_recipe(&library, &existing, &converted).map_err(anyhow::Error::msg)?;
    let saved = WeaponRecipe::load_json(&path)?;
    let graph = read_graph(&saved)?;
    if !case.first_person_actions.is_empty() {
        let actions = graph["animation"]["first_person"]["arm_motion"]["actions"]
            .as_array()
            .context("Published weapon lost its source arm actions")?;
        let next = sources::reach_request(&case.entry, donor, donor_name, Some(&path))?;
        let next = serde_json::to_value(next)?;
        for (state, clip) in &case.first_person_actions {
            ensure!(
                actions.iter().any(|action| action["state"] == *state
                    && action
                        .get("source_selection")
                        .unwrap_or(&action["source_clip"])
                        == clip)
                    && next["first_person"]["actions"][state] == *clip,
                "Published or refreshed weapon lost source action {state}"
            );
        }
        ensure!(
            next["first_person"] == graph["import_settings"]["first_person"],
            "Refresh changed saved arm conversion settings"
        );
    }
    if case.scoped {
        ensure!(
            graph["optic"]["alignment"] == "separate_view"
                && graph["optic"]["scope"]["source_hud"]["layers"]
                    .as_array()
                    .is_some_and(|layers| !layers.is_empty())
                && graph["source_gameplay"]["aim"]["magnification"]
                    .as_f64()
                    .is_some_and(|zoom| zoom > 1.),
            "Published scoped weapon lost its source view or zoom"
        );
    }
    if !case.melee_actions.is_empty() {
        let strikes = graph["animation"]["first_person"]["melee"]["strikes"]
            .as_array()
            .context("Published weapon lost its melee strikes")?;
        let next = serde_json::to_value(sources::reach_request(
            &case.entry,
            donor,
            donor_name,
            Some(&path),
        )?)?;
        for name in &case.melee_actions {
            ensure!(
                strikes.iter().any(|strike| strike["source_clip"] == *name)
                    && next["first_person"]["melee"]
                        .as_array()
                        .is_some_and(|names| names.contains(&json!(name))),
                "Published or refreshed weapon lost source strike {name}"
            );
        }
        ensure!(
            graph["melee"]["activation"]["scope"] == "Drawn weapon"
                && graph["audio"]["contact_damage"]
                    .as_f64()
                    .is_some_and(|amount| amount > 0.),
            "Published melee has no scoped activation and contact profile"
        );
        let saved_value = serde_json::to_value(&saved)?;
        let property = format!(
            "0x{}",
            graph["audio"]["impact_key"]
                .as_str()
                .context("Melee property")?
        );
        ensure!(
            saved_value["overrides"]["socket_plug_variants"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|variant| variant["sandbox_perks"].as_array().into_iter().flatten())
                .flat_map(|perk| perk["program"]["actions"].as_array().into_iter().flatten())
                .any(|action| action["key"] == property),
            "Publication discarded the private melee activation"
        );
    }
    let projectile = &graph["projectile"];
    let root = projectile["root"]
        .as_str()
        .context("Translated projectile root")?;
    ensure!(
        projectile["conversion"]["source"]["path"] == case.projectile_path,
        "Converted the wrong source projectile"
    );
    ensure!(
        saved
            .overrides
            .fired_graph
            .as_ref()
            .and_then(|f| f.imported.as_deref())
            == Some(root),
        "Saved weapon does not select its imported projectile"
    );
    let nodes = projectile["nodes"].as_array().context("Projectile nodes")?;
    let symbols = nodes
        .iter()
        .map(|node| node["symbol"].as_str().context("Node symbol"))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !symbols.is_empty() && symbols.len() == nodes.len(),
        "Invalid projectile node identities"
    );
    let directory = &saved.overrides.imported_graph.as_ref().unwrap().directory;
    let mut payloads = Vec::new();
    for node in nodes {
        let payload = fs::read(directory.join(node["file"].as_str().context("Node file")?))?;
        payloads.push(json!({"symbol":node["symbol"],"file":node["file"],
            "bytes":payload.len(),"sha256":hex::encode(Sha256::digest(&payload))}));
        if node["symbol"] == root {
            ensure!(
                payload.get(0x96..0x98) == Some(18u16.to_le_bytes().as_slice()),
                "Root is not a projectile entity"
            );
        }
        for target in node["patches"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|patch| patch["symbol"].as_str().context("Patch target"))
            .chain(node["reference"].as_str().map(Ok))
        {
            ensure!(
                symbols.contains(target?),
                "Projectile dependency leaves its group"
            );
        }
    }
    ensure!(
        saved.identity == baseline.identity
            && saved.name == baseline.name
            && saved.flavor == baseline.flavor,
        "Reimport changed authored identity or text"
    );
    let prepared = WeaponRecipe::load_json(&converted)?;
    ensure!(
        saved.overrides.socket_columns == prepared.overrides.socket_columns
            && saved.overrides.socket_plug_variants == prepared.overrides.socket_plug_variants
            && saved.overrides.base_sandbox_perks == prepared.overrides.base_sandbox_perks
            && saved.donor.item_hash == prepared.donor.item_hash,
        "Reimport kept the saved perk sockets instead of the conversion's"
    );
    save_imported_recipe(&library, &existing, &converted).map_err(anyhow::Error::msg)?;
    ensure!(
        WeaponRecipe::load_json(&path)?.same_saved_content(&saved),
        "Repeated publication changed the recipe"
    );

    // Real converted payloads are copied and repinned after removing only the projectile group.
    let reference = saved.overrides.imported_graph.as_ref().unwrap();
    let incomplete = case.output.join("incomplete");
    reference.copy_model(item, item, &incomplete)?;
    let mut stripped = graph.clone();
    stripped
        .as_object_mut()
        .context("Graph object")?
        .remove("projectile");
    fs::write(
        incomplete.join("asset-graph.json"),
        serde_json::to_vec_pretty(&stripped)?,
    )?;
    let mut replacement = WeaponRecipe::load_json(&converted)?;
    replacement.overrides.imported_graph =
        Some(parhelion_import::GraphReference::new(&incomplete, item)?);
    replacement.overrides.fired_graph = None;
    let broken = case.output.join("incomplete.parhelion.json");
    replacement.save_json(&broken)?;
    let before = fs::read(&path)?;
    let rejection = save_imported_recipe(&library, &existing, &broken).unwrap_err();
    ensure!(
        fs::read(&path)? == before,
        "Rejected import changed the saved recipe"
    );

    let mut without_selection = saved.clone();
    without_selection.overrides.fired_graph = None;
    library
        .save_existing_if_unchanged(&path, &saved, &without_selection)
        .map_err(anyhow::Error::msg)?;
    save_imported_recipe(&library, &existing, &converted).map_err(anyhow::Error::msg)?;
    ensure!(
        WeaponRecipe::load_json(&path)?.overrides.fired_graph == saved.overrides.fired_graph,
        "A saved recipe without a firing override lost its newly imported projectile"
    );

    let effect_request = case.effect_only.request(donor, donor_name)?;
    let effect_only = parhelion_import::halo_reach::browse::import(
        &effect_request,
        &case.maps,
        &case.packages,
        &case.output.join("effect-only"),
    )?;
    let effect_only = WeaponRecipe::load_json(effect_only)?;
    let effect_graph = read_graph(&effect_only)?;
    ensure!(
        effect_graph["projectile_import"]["status"] == "source_only"
            && effect_graph["projectile_import"]["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty())
            && effect_graph.get("projectile").is_none()
            && effect_only.overrides.fired_graph.is_none(),
        "An effect-only projectile was reported as translated"
    );
    fs::write(
        case.output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "source_build":parhelion_import::halo_reach::BUILD,"source_revision":case.source_revision,
            "crate_version":env!("CARGO_PKG_VERSION"),
            "test_executable_sha256":hex::encode(Sha256::digest(fs::read(std::env::current_exe()?)?)),
            "inputs":{"maps":case.maps,"packages":case.packages,"previous":case.previous},
            "request":request,"saved_recipe":path,"projectile_source":case.projectile_path,
            "projectile_root":root,"projectile_nodes":nodes.len(),"graph_sha256":reference.sha256,
            "converter_revision":graph["converter_revision"],"reach_revision":graph["reach_revision"],
            "projectile_payloads":payloads,
            "rejection":rejection,"rejection_preserved_saved_bytes":true,
            "effect_only_request":effect_request,"effect_only":effect_graph["projectile_import"],
            "limits":["Source conversion and library reload only. Native package linkage and gameplay are not exercised."]
        }))?,
    )?;
    Ok(())
}
