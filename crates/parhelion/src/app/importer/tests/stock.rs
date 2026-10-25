//! Reimport against an installed character overlay, then compile against stock again.
use super::*;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;

#[derive(Deserialize)]
struct Snapshot {
    tag: u32,
    sha256: String,
}

#[derive(Deserialize)]
struct Case {
    packages: PathBuf,
    maps: PathBuf,
    previous: PathBuf,
    vgmstream: PathBuf,
    entry: parhelion_import::halo_reach::browse::Entry,
    stock_character: BTreeMap<String, Snapshot>,
    output: PathBuf,
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_STOCK_CASE with an installed character overlay, source cache and independent stock hashes"]
fn reimport_from_installed_character_bank_stages_against_stock() -> Result<()> {
    let case: Case = serde_json::from_slice(&fs::read(
        std::env::var_os("PARHELION_IMPORT_STOCK_CASE")
            .context("Set PARHELION_IMPORT_STOCK_CASE")?,
    )?)?;
    ensure!(
        !case.output.exists(),
        "Use a fresh stock-source output directory"
    );
    parhelion_import::tools::configure(parhelion_import::tools::Paths {
        vgmstream: Some(case.vgmstream.clone()),
        native_packages: Some(case.packages.clone()),
        ..Default::default()
    });
    let bank = case
        .stock_character
        .get("bank")
        .context("Required stock bank witness")?;
    for key in [
        "bank",
        "parameters",
        "states",
        "layers",
        "pose",
        "directions",
        "skeleton",
        "controls",
    ] {
        ensure!(
            case.stock_character.contains_key(key),
            "Required character witness {key} is absent"
        );
    }
    let manager =
        sundial::package_authoring::open_managed_shadowkeep_package_manager(&case.packages)
            .map_err(anyhow::Error::msg)?;
    let installed = manager
        .read_tag(tiger_pkg::TagHash(bank.tag))
        .map_err(anyhow::Error::msg)?;
    let installed_sha256 = digest(&installed);
    ensure!(
        installed_sha256 != bank.sha256,
        "Case has no modified character bank"
    );
    drop(manager);

    let library = RecipeLibrary::open(case.output.join("library")).map_err(anyhow::Error::msg)?;
    let baseline = WeaponRecipe::load_json(&case.previous)?;
    let identity = baseline.identity.item_hash.parse_u32()?;
    let path = library.save_new(&baseline).map_err(anyhow::Error::msg)?;
    let request = sources::reach_request(
        &case.entry,
        baseline.donor.item_hash.parse_u32()?,
        baseline.donor.expected_name.as_deref().unwrap_or_default(),
        Some(&path),
    )?;
    let source =
        crate::workflow::PackageSource::stock(&case.packages).map_err(anyhow::Error::msg)?;
    let source_path = source.path().to_path_buf();
    let prepared = parhelion_import::halo_reach::browse::import(
        &request,
        &case.maps,
        source.path(),
        &case.output.join("converted"),
    );
    let mut cleanup = Vec::new();
    let prepared = source
        .finish(prepared.map_err(|error| format!("{error:#}")), |warning| {
            cleanup.push(warning)
        })
        .map_err(anyhow::Error::msg)?;
    ensure!(
        cleanup.is_empty() && !source_path.exists(),
        "Import left its isolated package view open"
    );
    save_imported_recipe(&library, &[(path.clone(), identity)], &prepared)
        .map_err(anyhow::Error::msg)?;
    let saved = WeaponRecipe::load_json(&path)?;
    ensure!(
        saved.identity == baseline.identity
            && saved.namespace == baseline.namespace
            && saved.flavor == baseline.flavor
            && saved.overrides.sparrow == baseline.overrides.sparrow,
        "Stock-source reimport changed authored identity, text or vehicle settings"
    );
    let reference = saved
        .overrides
        .imported_graph
        .as_ref()
        .context("Saved imported graph")?;
    let graph: serde_json::Value =
        serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
    let character = &graph["animation"]["character"];
    let mut readback = BTreeMap::new();
    for (key, expected) in &case.stock_character {
        ensure!(
            character[key].as_u64() == Some(u64::from(expected.tag)),
            "Character source tag changed"
        );
        let file = character["files"][key]
            .as_str()
            .context("Character source file")?;
        let hash = digest(&fs::read(reference.directory.join(file))?);
        ensure!(
            hash == expected.sha256,
            "Prepared {key} still contains installed modifications"
        );
        readback.insert(key, hash);
    }
    let graph_sha256 = reference.sha256.clone();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: case.packages.clone(),
        staging_root: case.output.join("stage"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: vec![saved],
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        crate::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    fs::write(
        case.output.join("readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "source_build":"86657.20.08.23.1800.d2_rc",
            "test_executable_sha256":crate::artifact::digest_file(&std::env::current_exe()?)?.sha256,
            "modified_bank_sha256":installed_sha256,"stock_character":readback,
            "saved_recipe":path,"graph_sha256":graph_sha256,"stage":built.run_directory,
            "artifacts":built.artifacts,"source_view_removed":true,
            "limits":"Source snapshots and complete native staging. Live rider alignment and audio require gameplay acceptance."
        }))?,
    )?;
    Ok(())
}
