//! Configured source weapons through conversion, recipe reload and selected package actions.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{
    d2_mot::batch,
    tiger::{payload::Payload, reader::Reader, rig},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};
mod art;
mod collection;
mod pose;
mod refresh;
mod selection;

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

fn number(value: &Value) -> Result<u32> {
    if let Some(text) = value.as_str() {
        Ok(u32::from_str_radix(text.trim_start_matches("0x"), 16)?)
    } else {
        Ok(u32::try_from(value.as_u64().context("Integer identity")?)?)
    }
}

fn item(reader: &mut Reader, hash: u32) -> Result<u32> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let table = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let matches = table
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .filter(|&at| table.u32(at).ok() == Some(hash))
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "Authored item is missing or ambiguous");
    table.u32(matches[0] + 16)
}

fn view(native: &Path, output: &Path, run: &Path, names: &[String]) -> Result<PathBuf> {
    let root = output.join("view");
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    for name in names {
        let target = packages.join(name);
        ensure!(
            !target.exists(),
            "Staging would overwrite a stock hard link"
        );
        fs::copy(run.join(name), target)?;
    }
    Ok(packages)
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, SUNDIAL_STOCK_PACKAGES, PARHELION_ANIMATION_PLAN and PARHELION_ANIMATION_OUTPUT"]
fn source_actions_survive_native_aliases_and_portable_staging() -> Result<()> {
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let plan_path = configured("PARHELION_ANIMATION_PLAN")?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_ANIMATION_OUTPUT")?,
        native.parent().context("Native root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, modern.parent().context("Source root")?)?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let plan: Value = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let cases = plan["items"].as_array().context("Required source cases")?;
    ensure!(!cases.is_empty(), "Required source corpus is empty");
    for case in cases {
        ensure!(
            case["actions"].as_array().is_some_and(|a| !a.is_empty()),
            "Required actions are empty"
        );
    }
    fs::create_dir_all(&output)?;
    // A staged readback can be repeated against a completed source conversion.
    // The original conversion receipt remains an explicit input to that run.
    let prepared_path = env::var_os("PARHELION_ANIMATION_PREPARED").map(PathBuf::from);
    let prepared = if let Some(path) = &prepared_path {
        let results: Value = serde_json::from_slice(&fs::read(path)?)?;
        json!({"results":results})
    } else {
        batch::prepare(&plan_path, &modern, &native, &output.join("import"))?
    };
    let results = prepared["results"].as_array().context("Import results")?;
    ensure!(results.len() == cases.len(), "An import case disappeared");
    let mut recipes = Vec::new();
    let mut accepted = Vec::new();
    let mut failures = Vec::new();
    for (index, (case, result)) in cases.iter().zip(results).enumerate() {
        ensure!(
            number(&result["hash"])? == number(&case["hash"])?,
            "Prepared source identity differs from the required case"
        );
        if result["status"] != "prepared" {
            failures.push(format!("Required source import failed: {result}"));
            continue;
        }
        ensure!(
            number(&result["native_item"])? == number(&case["native_item"])?,
            "Prepared donor differs from the required case"
        );
        let prepared_recipe = (|| -> Result<WeaponRecipe> {
            let mut recipe =
                WeaponRecipe::load_json(result["recipe"].as_str().context("Prepared recipe")?)?;
            let item = recipe.identity.item_hash.parse_u32()?;
            let graph = Path::new(result["graph"].as_str().context("Prepared graph")?);
            let reference = parhelion_import::GraphReference::new(graph, item)?;
            let copied = reference.copy_model(
                item,
                item,
                &output.join("graphs").join(format!("{item:08X}")),
            )?;
            recipe.overrides.imported_graph =
                Some(if env::var_os("PARHELION_ANIMATION_REFRESH").is_some() {
                    refresh::animation(&modern, &native, result, copied, item, &output)?
                } else {
                    copied
                });
            let portable = recipe.to_json_pretty()?;
            fs::write(
                output.join(format!("{}.parhelion.json", recipe.identity.item_hash)),
                &portable,
            )?;
            WeaponRecipe::from_json_str(&portable).map_err(anyhow::Error::msg)
        })();
        match prepared_recipe {
            Ok(recipe) => {
                accepted.push(index);
                recipes.push(recipe);
            }
            Err(error) => failures.push(format!("Prepare {}: {error:#}", case["name"])),
        }
    }
    fs::write(
        output.join("preparation.json"),
        serde_json::to_vec_pretty(&json!({"accepted_indices":accepted,"failures":failures}))?,
    )?;
    ensure!(
        !recipes.is_empty(),
        "No prepared cases to stage: {}",
        failures.join("\n")
    );
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
        maps: Vec::new(),
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    let names = built
        .artifacts
        .iter()
        .map(|a| a.file_name.clone())
        .collect::<Vec<_>>();
    let packages = view(&native, &output, &built.run_directory, &names)?;
    let mut source = Reader::new(&modern, &output.join("source-readback"), true)?;
    let mut stock = Reader::new(&native, &output.join("stock-readback"), false)?;
    let mut staged = Reader::new(&packages, &output.join("staged-readback"), false)?;
    let mut receipts = Vec::new();
    for (&index, recipe) in accepted.iter().zip(&recipes) {
        let (case, result) = (&cases[index], &results[index]);
        let checked = (|| -> Result<Value> {
            let folder = Path::new(result["folder"].as_str().context("Source folder")?);
            let exported: Value =
                serde_json::from_slice(&fs::read(folder.join("source/report.json"))?)?;
            let source_rig = rig::inspect(&mut source, number(&exported["item_tag"])?, true)?;
            let donor_tag = item(&mut stock, number(&case["native_item"])?)?;
            let donor_rig = rig::inspect(&mut stock, donor_tag, false)?;
            let emitted_tag = item(&mut staged, recipe.identity.item_hash.parse_u32()?)?;
            let emitted_rig = rig::inspect(&mut staged, emitted_tag, false)?;
            let mut receipt = selection::check(
                &mut source,
                &mut stock,
                &mut staged,
                &source_rig,
                &donor_rig,
                &emitted_rig,
                case,
            )
            .with_context(|| format!("Selected source actions for {}", case["name"]))?;
            if let Some(layers) = case.get("pose_weight_layers") {
                receipt["pose_weights"] =
                    pose::check(&mut source, &mut staged, &source_rig, &emitted_rig, layers)?;
            }
            if case["art_regions"] == true {
                receipt["art_regions"] = art::check(
                    &mut source,
                    &mut staged,
                    number(&exported["item_tag"])?,
                    emitted_tag,
                )?;
            }
            if case["new_collection_family"] == true {
                receipt["collection"] = collection::check(
                    &mut stock,
                    &mut staged,
                    recipe.identity.item_hash.parse_u32()?,
                    number(&case["native_item"])?,
                )?;
            }
            Ok(receipt)
        })();
        match checked {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => {
                let reason = format!("Readback {}: {error:#}", case["name"]);
                receipts.push(json!({"name":case["name"],"error":reason}));
                failures.push(reason);
            }
        }
    }
    source.finish()?;
    stock.finish()?;
    staged.finish()?;
    let hashes = names
        .iter()
        .map(|name| {
            Ok((
                name.clone(),
                hex::encode(Sha256::digest(fs::read(built.run_directory.join(name))?)),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&json!({
            "plan":plan,"items":receipts,"failures":failures,"packages":hashes,"staged":built.run_directory,
            "prepared_source_results":prepared_path,
            "converter_revision":parhelion_import::CONVERTER_REVISION,
            "source_stamp":parhelion_import::d2_mot::service::package_stamp(&modern)?,
            "native_stamp":parhelion_import::d2_mot::service::package_stamp(&native)?,
            "gameplay_verified":false
        }))?,
    )?;
    ensure!(
        failures.is_empty(),
        "Required source coverage failed:\n{}",
        failures.join("\n")
    );
    Ok(())
}
