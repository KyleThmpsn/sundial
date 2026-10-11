//! Real Marathon conversion through recipe persistence and independent model copying.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use parhelion::WeaponRecipe;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

#[test]
#[ignore = "Requires PARHELION_MARATHON_PLAN with recipes, PARHELION_MARATHON_PACKAGES, PARHELION_EXISTING_ICON, SUNDIAL_STOCK_PACKAGES and fresh PARHELION_ICON_OUTPUT"]
fn converted_artwork_survives_reload_and_model_copy_without_replacing_authored_icons() -> Result<()>
{
    let plan_path = configured("PARHELION_MARATHON_PLAN")?;
    let output = configured("PARHELION_ICON_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let existing = fs::read(configured("PARHELION_EXISTING_ICON")?)?;
    image::load_from_memory(&existing)?;
    let report = parhelion_import::marathon::prepare(
        &plan_path,
        &configured("PARHELION_MARATHON_PACKAGES")?,
        &configured("SUNDIAL_STOCK_PACKAGES")?,
        &output.join("converted"),
    )?;
    let plan: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let entries = plan
        .as_array()
        .filter(|v| !v.is_empty())
        .context("Empty required corpus")?;
    let mut records = Vec::new();
    for entry in entries {
        let slug = entry["slug"].as_str().context("Plan slug")?;
        let path = output
            .join("converted")
            .join(slug)
            .join("weapon.parhelion.json");
        let mut recipe = WeaponRecipe::load_json(&path)?;
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?
            .clone();
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        let source = fs::read(
            reference
                .directory
                .join(graph["source_icon_png"].as_str().context("Generated PNG")?),
        )?;
        let image = image::load_from_memory(&source)?.to_rgba8();
        ensure!(
            image.dimensions() == (96, 96),
            "Wrong inventory artwork size"
        );
        let generated = recipe
            .overrides
            .icon_edit
            .imported_image
            .clone()
            .context("No automatic artwork")?;
        let saved = output.join(format!("{slug}.parhelion.json"));
        recipe.save_json(&saved)?;
        ensure!(
            WeaponRecipe::load_json(&saved)?
                .overrides
                .icon_edit
                .imported_image
                == Some(generated.clone()),
            "Inventory artwork was lost on disk"
        );

        // A copied graph must bring its PNG along. Removing the original folder from reach
        // makes this a persistence contract rather than agreement between two file lists.
        let item = recipe.identity.item_hash.parse_u32()?;
        let mut copied = reference.copy_model(item, item, &output.join("copies").join(slug))?;
        fs::rename(
            &reference.directory,
            reference.directory.with_file_name("unavailable-graph"),
        )?;
        copied.validate(item)?;
        recipe.overrides.imported_graph = Some(copied.clone());
        recipe.overrides.icon_edit.imported_image = None;
        recipe.save_json(&saved)?;
        let reloaded = WeaponRecipe::load_json(&saved)?;
        ensure!(
            reloaded.overrides.icon_edit.imported_image == Some(generated),
            "Copied graph failed to supply its artwork during save and reload"
        );

        let mut custom = serde_json::to_value(&recipe)?;
        custom["overrides"]["icon_edit"]["imported_image"] =
            json!({"png_base64":STANDARD.encode(&existing)});
        let protected = custom.clone();
        parhelion_import::artwork::apply(&mut custom, &copied.directory)?;
        ensure!(
            custom == protected,
            "Existing Destiny or custom artwork was replaced"
        );
        let custom_recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&custom)?)?;
        custom_recipe.save_json(&saved)?;
        ensure!(
            WeaponRecipe::load_json(&saved)?.overrides.icon_edit
                == custom_recipe.overrides.icon_edit,
            "Existing artwork changed on reload"
        );

        let mut donor = serde_json::to_value(&recipe)?;
        donor["icon_donor"] = donor["donor"].clone();
        let protected = donor.clone();
        parhelion_import::artwork::apply(&mut donor, &copied.directory)?;
        ensure!(donor == protected, "Explicit icon donor was replaced");
        ensure!(
            WeaponRecipe::from_json_str(&serde_json::to_string(&donor)?)?
                .overrides
                .icon_edit
                .imported_image
                .is_none(),
            "Recipe load replaced the icon donor"
        );
        // Source-provided Destiny artwork has no generated marker. It must take the same
        // precedence when the recipe has not embedded it yet, including after model reuse.
        let graph_path = copied.directory.join("asset-graph.json");
        let mut source_graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
        source_graph
            .as_object_mut()
            .context("Source graph")?
            .remove("generated_icon");
        fs::write(
            copied.directory.join(
                source_graph["source_icon_png"]
                    .as_str()
                    .context("Source PNG")?,
            ),
            &existing,
        )?;
        fs::write(&graph_path, serde_json::to_vec_pretty(&source_graph)?)?;
        copied = parhelion_import::GraphReference::new(&copied.directory, item)?;
        recipe.overrides.imported_graph = Some(copied.clone());
        let mut from_source = serde_json::to_value(&recipe)?;
        parhelion_import::artwork::apply(&mut from_source, &copied.directory)?;
        ensure!(
            from_source["overrides"]["icon_edit"]["imported_image"]
                == json!({"png_base64":STANDARD.encode(&existing)}),
            "Source Destiny artwork was not adopted"
        );
        recipe.save_json(&saved)?;
        ensure!(
            WeaponRecipe::load_json(&saved)?
                .overrides
                .icon_edit
                .imported_image
                == custom_recipe.overrides.icon_edit.imported_image,
            "Source Destiny artwork was replaced during recipe reload"
        );
        fs::write(output.join(format!("{slug}-icon.png")), &source)?;
        records.push(json!({"slug":slug,"item":item,"copied_graph_sha256":copied.sha256,
            "png_sha256":hex::encode(Sha256::digest(&source)),"reload":true,
            "original_graph_unavailable":true,"existing_artwork_preserved":true,"icon_donor_preserved":true,
            "source_destiny_icon_preserved":true}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
        "conversion":report,"plan":plan_path,"items":records,"installed":false,
        "limits":"Persistence and graph-copy acceptance. Visual review and native package readback are separate."}))?,
    )?;
    Ok(())
}
