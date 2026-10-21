//! Configured vehicle reimport through the browser request, library and native build.
use super::*;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Case {
    maps: PathBuf,
    packages: PathBuf,
    previous: PathBuf,
    entry: parhelion_import::halo_reach::browse::Entry,
    output: PathBuf,
}

#[test]
#[ignore = "Requires PARHELION_VEHICLE_REIMPORT_CASE with real source caches, stock packages and a prior vehicle recipe"]
fn vehicle_reimport_preserves_selection_through_native_staging() -> Result<()> {
    let case: Case = serde_json::from_slice(&fs::read(
        std::env::var_os("PARHELION_VEHICLE_REIMPORT_CASE")
            .context("Set PARHELION_VEHICLE_REIMPORT_CASE")?,
    )?)?;
    ensure!(
        !case.output.exists(),
        "Use a fresh reimport output directory"
    );
    let library = RecipeLibrary::open(case.output.join("library")).map_err(anyhow::Error::msg)?;
    let mut baseline = WeaponRecipe::load_json(&case.previous)?;
    baseline.flavor = "Authored description retained through reimport.".into();
    let tuning = baseline
        .overrides
        .sparrow
        .as_mut()
        .context("Prior vehicle settings")?;
    tuning.durability.health_percent = 125;
    tuning.vehicle_icon = false;
    let selected = tuning
        .summon
        .entity()
        .map_err(anyhow::Error::msg)?
        .context("Prior native vehicle")?;
    let donor = baseline.donor.item_hash.parse_u32()?;
    let donor_name = baseline.donor.expected_name.as_deref().unwrap_or_default();
    let default = case.entry.request(donor, donor_name)?;
    ensure!(
        default.runtime_entity != Some(selected),
        "Case must exercise a saved selection different from the browser default"
    );
    let path = library.save_new(&baseline).map_err(anyhow::Error::msg)?;
    let request = sources::reach_request(&case.entry, donor, donor_name, Some(&path))?;
    let request_path = case.output.join("request.json");
    fs::write(&request_path, serde_json::to_vec_pretty(&request)?)?;
    let prepared = parhelion_import::halo_reach::browse::import(
        &request,
        &case.maps,
        &case.packages,
        &case.output.join("converted"),
    )?;
    let identity = baseline.identity.item_hash.parse_u32()?;
    let existing = [(path.clone(), identity)];
    save_imported_recipe(&library, &existing, &prepared).map_err(anyhow::Error::msg)?;
    let saved = WeaponRecipe::load_json(&path)?;
    ensure!(
        saved.identity == baseline.identity
            && saved.name == baseline.name
            && saved.flavor == baseline.flavor,
        "Reimport lost authored identity or text"
    );
    ensure!(
        saved.overrides.sparrow == baseline.overrides.sparrow,
        "Reimport changed authored vehicle settings"
    );
    saved.validate()?;
    let graph = saved
        .overrides
        .imported_graph
        .as_ref()
        .context("Reimported graph")?;
    graph.validate_vehicle(selected)?;
    save_imported_recipe(&library, &existing, &prepared).map_err(anyhow::Error::msg)?;
    let repeated = WeaponRecipe::load_json(&path)?;
    ensure!(
        repeated.same_saved_content(&saved),
        "Repeated reimport changed the saved recipe"
    );
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: case.packages,
        staging_root: case.output.join("stage"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: vec![repeated],
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        crate::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    fs::write(
        case.output.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "prior_recipe":case.previous,"entry":case.entry,"request":request_path,
            "saved_recipe":path,"selected_entity":format!("{selected:08X}"),
            "graph_sha256":graph.sha256,"stage":built.run_directory,"artifacts":built.artifacts,
            "identity_and_text_preserved":true,"vehicle_settings_preserved":true,
            "repeat_reimport_preserved":true,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
