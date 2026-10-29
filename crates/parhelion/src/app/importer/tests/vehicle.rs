//! Configured vehicle reimport through the browser request, library and native build.
use super::*;
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct Case {
    maps: PathBuf,
    packages: PathBuf,
    previous: PathBuf,
    entry: parhelion_import::halo_reach::browse::Entry,
    expected_name: String,
    expected_default_entity: u32,
    /// Independently supplied authored dimensions, including legacy defaults to repair.
    expected_scale: f32,
    expected_inspection_scale: f32,
    /// A separately authored image to retain after the generated image has refreshed.
    #[serde(default)]
    custom_icon: Option<PathBuf>,
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
    ensure!(
        baseline.icon_donor.is_none(),
        "Case must exercise vehicle artwork without an explicit icon donor"
    );
    if let Some(image) = &baseline.overrides.icon_edit.imported_image {
        ensure!(
            parhelion_import::artwork::generated_image(
                baseline
                    .overrides
                    .imported_graph
                    .as_ref()
                    .context("Prior generated artwork graph")?,
                &serde_json::to_value(image)?,
            )?,
            "Case must start with missing or previously generated artwork"
        );
    }
    baseline.name = format!("Reach {}", case.expected_name);
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
        default.recipe["name"] == case.expected_name,
        "Imported name must not add the source game's name"
    );
    ensure!(
        default.scale == 0.8 && default.inspection_scale == 2.,
        "New vehicle imports use the wrong world or inventory size"
    );
    ensure!(
        default.runtime_entity == Some(case.expected_default_entity),
        "New import selected the wrong native vehicle for the source physics"
    );
    ensure!(
        default.runtime_entity != Some(selected),
        "Case must exercise a saved selection different from the browser default"
    );
    let path = library.save_new(&baseline).map_err(anyhow::Error::msg)?;
    let request = sources::reach_request(&case.entry, donor, donor_name, Some(&path))?;
    ensure!(
        request.scale == case.expected_scale
            && request.inspection_scale == case.expected_inspection_scale,
        "Reimport lost the authored dimensions"
    );
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
            && saved.namespace == baseline.namespace
            && saved.name == case.expected_name
            && saved.flavor == baseline.flavor,
        "Reimport failed to remove the generated prefix or lost authored identity or text"
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
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(graph.directory.join("asset-graph.json"))?)?;
    let source_png = fs::read(
        graph.directory.join(
            document["source_icon_png"]
                .as_str()
                .context("Vehicle graph has no inventory artwork")?,
        ),
    )?;
    let icon = saved
        .overrides
        .icon_edit
        .imported_image
        .as_ref()
        .context("Vehicle reimport did not save its rendered icon")?;
    let encoded = serde_json::to_value(icon)?;
    let png = STANDARD.decode(encoded["png_base64"].as_str().context("Saved icon PNG")?)?;
    let pixels = image::load_from_memory(&png)?.to_rgba8();
    ensure!(
        pixels.dimensions() == (96, 96)
            && pixels.pixels().any(|pixel| pixel[3] > 0)
            && pixels.pixels().any(|pixel| pixel[3] == 0),
        "Vehicle artwork must contain visible geometry on a transparent 96 by 96 canvas"
    );
    ensure!(
        pixels == image::load_from_memory(&source_png)?.to_rgba8(),
        "Saved vehicle artwork differs from its published graph"
    );
    let icon_path = case.output.join("vehicle-icon.png");
    fs::write(&icon_path, &png)?;
    // The saved graph must also supply its artwork when the recipe has no embedded image.
    let mut document: serde_json::Value = serde_json::from_str(&saved.to_json_pretty()?)?;
    document["overrides"]["icon_edit"]
        .as_object_mut()
        .context("Saved icon settings")?
        .remove("imported_image");
    let graph_only = WeaponRecipe::from_json_str(&serde_json::to_string(&document)?)?;
    ensure!(
        graph_only.overrides.icon_edit.imported_image.as_ref() == Some(icon),
        "Saved vehicle graph did not restore its rendered artwork"
    );
    let next_request = sources::reach_request(&case.entry, donor, donor_name, Some(&path))?;
    ensure!(
        next_request.scale == case.expected_scale
            && next_request.inspection_scale == case.expected_inspection_scale,
        "Saved publication lost the dimensions needed for the next import"
    );
    save_imported_recipe(&library, &existing, &prepared).map_err(anyhow::Error::msg)?;
    let repeated = WeaponRecipe::load_json(&path)?;
    ensure!(
        repeated.same_saved_content(&saved),
        "Repeated reimport changed the saved recipe"
    );
    let mut renamed = repeated.clone();
    renamed.name = "My Custom Vehicle".into();
    if let Some(path) = &case.custom_icon {
        renamed.overrides.icon_edit.imported_image = Some(
            crate::icon_edit::ImportedIcon::from_bytes(&fs::read(path)?)
                .map_err(anyhow::Error::msg)?,
        );
    }
    library
        .save_existing_if_unchanged(&path, &repeated, &renamed)
        .map_err(anyhow::Error::msg)?;
    save_imported_recipe(&library, &existing, &prepared).map_err(anyhow::Error::msg)?;
    let authored = WeaponRecipe::load_json(&path)?;
    ensure!(
        authored.same_saved_content(&renamed),
        "Reimport overwrote a custom name or custom artwork"
    );
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: case.packages,
        staging_root: case.output.join("stage"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: vec![authored],
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
            "world_scale":next_request.scale,"inspection_scale":next_request.inspection_scale,
            "inventory_icon":icon_path,"graph_and_saved_icon_pixels_match":true,
            "graph_only_artwork_restored":true,
            "default_name":case.expected_name,"custom_name_preserved":true,
            "repeat_reimport_preserved":true,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
