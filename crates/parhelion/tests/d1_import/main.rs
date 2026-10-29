//! Configured source corpus through conversion, portable reload and native package readback.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe,
    build_and_stage_snapshot_with_progress,
};
use parhelion_import::{
    d1_roi::{NativeOptions, Source},
    tiger::{
        payload::Payload,
        reader::{Reader, package_stamp},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};
use sundial::investment::InvestmentCatalog;
mod armor;
mod firing;
mod first_person;
mod perk;
mod readback;

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}
fn number(value: &Value) -> Result<u32> {
    Ok(u32::try_from(value.as_u64().context("Integer identity")?)?)
}
fn item(reader: &mut Reader, hash: u32) -> Result<(u32, Payload)> {
    let tag = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(tag, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let table = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let row = table
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .find(|&r| table.u32(r).ok() == Some(hash))
        .context("Item")?;
    let tag = table.u32(row + 16)?;
    Ok((tag, (*reader.tag(tag, Some(0x80807BEA))?).clone()))
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
        ensure!(!target.exists(), "Staging would replace stock");
        fs::copy(run.join(name), target)?;
    }
    Ok(packages)
}

#[test]
#[ignore = "Requires D1_IMPORT_PLAN, D1_IMPORT_OUTPUT, D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES, PARHELION_D1_XG and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn named_source_corpus_survives_conversion_reload_and_staging() -> Result<()> {
    let began = Instant::now();
    let output = configured("D1_IMPORT_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh output folder");
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let source_packages = configured("D1_SOURCE_PACKAGES")?;
    let source_stamp = package_stamp(&source_packages)?;
    let native_stamp = package_stamp(&native)?;
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    ensure!(!revision.trim().is_empty(), "Source revision is empty");
    let test_binary_sha256 = hex::encode(Sha256::digest(fs::read(env::current_exe()?)?));
    let source = Source::open(&source_packages, |_, _| {})?;
    let plan_bytes = fs::read(configured("D1_IMPORT_PLAN")?)?;
    let plan: Vec<Value> = serde_json::from_slice(&plan_bytes)?;
    ensure!(!plan.is_empty(), "Required source corpus is empty");
    fs::create_dir_all(&output)?;
    fs::write(output.join("plan.json"), &plan_bytes)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let donors = json!({"gear":catalog.gear_donors(ItemKind::Armor.bucket_hashes()).into_iter().map(|d|json!({"hash":d.hash,"name":d.name,"bucket_hash":d.bucket_hash,"class_type":catalog.item_class_type(d.hash),"collection_backed":d.collection_backed})).collect::<Vec<_>>()});
    let options = NativeOptions {
        xg_library: Some(configured("PARHELION_D1_XG")?),
        ..Default::default()
    };
    let weapons = source.weapons()?;
    let armor = source.armor()?;
    let mut stock = Reader::new(&native, &output.join("stock"), false)?;
    let mut recipes = Vec::new();
    let mut receipts = Vec::new();
    let mut failures = Vec::new();
    let mut unchanged = BTreeMap::new();
    for case in &plan {
        let name = case["name"].as_str().context("Case name")?;
        let is_armor = case["kind"] == "armor";
        let candidates = if is_armor {
            armor
                .iter()
                .filter(|a| a.name.trim() == name)
                .map(|a| a.hash)
                .collect::<Vec<_>>()
        } else {
            weapons
                .iter()
                .filter(|a| a.name.trim() == name)
                .map(|a| a.hash)
                .collect::<Vec<_>>()
        };
        ensure!(
            !candidates.is_empty(),
            "Required source item {name} is missing"
        );
        let selected = case
            .get("hash")
            .map(number)
            .transpose()?
            .unwrap_or(candidates[0]);
        ensure!(
            candidates.contains(&selected),
            "Configured source identity differs from {name}"
        );
        let folder = output.join(format!("item-{selected:08X}"));
        let prepared = if is_armor {
            source.prepare_armor(selected, &native, &donors, &folder, &options, &mut |s| {
                println!("{s}")
            })
        } else {
            let bases: Vec<u32> = serde_json::from_value(case["bases"].clone())?;
            source.prepare_weapon_on_base(selected, &native, &bases, &folder, &options, &mut |s| {
                println!("{s}")
            })
        };
        let path = match prepared {
            Ok(path) => {
                ensure!(
                    case.get("unsupported").is_none(),
                    "Previously unsupported {name} converted. Review its acceptance contract"
                );
                path
            }
            Err(error) => {
                let reason = format!("{error:#}");
                let expected = case["unsupported"].as_str().filter(|s| !s.is_empty());
                if !expected.is_some_and(|s| reason.contains(s)) {
                    failures.push(format!("{name}: {reason}"));
                }
                receipts.push(
                    json!({"name":name,"source":selected,"unsupported":reason,"expected":expected}),
                );
                continue;
            }
        };
        let recipe = WeaponRecipe::load_json(&path)?;
        let portable = recipe.to_json_pretty()?;
        fs::write(folder.join("portable.parhelion.json"), &portable)?;
        let reloaded = WeaponRecipe::from_json_str(&portable)?;
        let reference = reloaded
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        if case["require_lifecycle_rejection"] == true {
            reject_retiring_presentation(reference, &graph)?;
        }
        let donor = recipe.donor.item_hash.parse_u32()?;
        let (tag, bytes) = item(&mut stock, donor)?;
        unchanged.insert(tag, bytes.0);
        for variant in &recipe.overrides.socket_plug_variants {
            let (tag, bytes) = item(&mut stock, variant.source_plug_hash.parse_u32()?)?;
            unchanged.insert(tag, bytes.0);
        }
        {
            let report: Value = serde_json::from_slice(&fs::read(folder.join(if is_armor {
                "source/armor.json"
            } else {
                "source/weapon.json"
            }))?)?;
            let mut expected = BTreeSet::new();
            for node in report["talents"]["nodes"]
                .as_array()
                .context("Source talents")?
            {
                for step in node["steps"].as_array().context("Source steps")? {
                    ensure!(
                        expected.insert((number(&node["index"])?, number(&step["index"])?)),
                        "Duplicate source talent"
                    );
                }
            }
            let mut covered = BTreeSet::new();
            for field in ["translated", "unsupported"] {
                for step in graph["perk_translation"][field]
                    .as_array()
                    .context("Talent coverage")?
                {
                    ensure!(
                        covered.insert((number(&step["node"])?, number(&step["step"])?)),
                        "Talent covered twice"
                    );
                    if field == "unsupported" {
                        ensure!(
                            step["reason"].as_str().is_some_and(|s| !s.is_empty()),
                            "Unsupported talent has no reason"
                        );
                    }
                }
            }
            ensure!(
                expected == covered,
                "Source talent choice disappeared during conversion"
            );
            for required in case["required_talents"].as_array().into_iter().flatten() {
                ensure!(
                    graph["perk_translation"]["translated"]
                        .as_array()
                        .context("Translated talents")?
                        .iter()
                        .any(|step| step["name"] == *required),
                    "Required source talent {required} was not translated for {name}"
                );
            }
            for required in case["required_source_actions"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let required = number(required)?;
                ensure!(
                    graph["perk_translation"]["translated"]
                        .as_array()
                        .context("Translated talents")?
                        .iter()
                        .any(|step| {
                            step["source_actions"].as_array().is_some_and(|actions| {
                                actions
                                    .iter()
                                    .any(|tag| tag.as_u64() == Some(u64::from(required)))
                            })
                        }),
                    "Required source action {required:08X} was not translated for {name}"
                );
            }
        }
        if !is_armor {
            let selection: Value =
                serde_json::from_slice(&fs::read(folder.join("native-base-selection.json"))?)?;
            if selection["requires_kinetic"] == true {
                let (_, definition) = item(&mut stock, donor)?;
                ensure!(
                    !definition
                        .array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))?
                        .iter()
                        .any(|&r| definition.u16(r).ok() == Some(68)),
                    "Kinetic source selected a locked damage socket"
                );
            }
        }
        receipts.push(json!({"name":name,"source":selected,"item":recipe.identity.item_hash.parse_u32()?,"donor":donor,"folder":folder,"graph":graph,"portable_sha256":hex::encode(Sha256::digest(portable.as_bytes())),"case":case}));
        recipes.push(reloaded);
    }
    fs::write(
        output.join("conversion.json"),
        serde_json::to_vec_pretty(&json!({"cases":receipts,"failures":failures}))?,
    )?;
    ensure!(!recipes.is_empty(), "No accepted cases to stage");
    let conversion_seconds = began.elapsed().as_secs_f64();
    let staging_began = Instant::now();
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
        .map_err(anyhow::Error::msg)?;
    let packages = view(
        &native,
        &output,
        &built.run_directory,
        &built
            .artifacts
            .iter()
            .map(|a| a.file_name.clone())
            .collect::<Vec<_>>(),
    )?;
    let staging_seconds = staging_began.elapsed().as_secs_f64();
    let readback_began = Instant::now();
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    for (tag, bytes) in unchanged {
        ensure!(staged.tag(tag, None)?.0 == bytes, "Stock donor changed");
    }
    for receipt in &mut receipts {
        let Some(hash) = receipt.get("item").map(number).transpose()? else {
            continue;
        };
        let checked = (|| -> Result<()> {
            receipt["firing_readback"] = firing::check(&source, &mut stock, &mut staged, receipt)?;
            let (tag, _) = item(&mut staged, hash)?;
            receipt["readback"] = readback::check(
                &source,
                &mut staged,
                tag,
                &receipt["graph"],
                receipt["case"]["tessellation"] == true,
            )
            .with_context(|| format!("Readback for {}", receipt["name"]))?;
            {
                let recipe = recipes
                    .iter()
                    .find(|recipe| recipe.identity.item_hash.parse_u32().ok() == Some(hash))
                    .context("Staged recipe")?;
                receipt["native_behaviors"] =
                    perk::check(&source, &mut stock, &mut staged, tag, recipe, receipt).map_err(
                        |error| anyhow::anyhow!("Perks for {}: {error}", receipt["name"]),
                    )?;
                for required in receipt["case"]["required_status_audio_graphs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let required = number(required)?;
                    ensure!(
                        receipt["native_behaviors"]
                            .as_array()
                            .context("Native behavior readback")?
                            .iter()
                            .flat_map(|behavior| {
                                behavior["status_audio"].as_array().into_iter().flatten()
                            })
                            .any(|audio| {
                                audio["source_graph"].as_u64() == Some(u64::from(required))
                                    && audio["persisted"].is_object()
                            }),
                        "Required status audio graph {required:08X} has no persisted and staged readback"
                    );
                }
                if receipt["graph"]["kind"] == "armor" {
                    receipt["armor_talents"] =
                        armor::check(&source, &mut staged, tag, recipe, receipt)?;
                }
            }
            if let Some(actions) = receipt["case"].get("first_person_actions") {
                receipt["first_person_readback"] = first_person::check(
                    &source,
                    &mut stock,
                    &mut staged,
                    tag,
                    &receipt["graph"],
                    actions,
                )
                .with_context(|| format!("First-person actions for {}", receipt["name"]))?;
            }
            Ok(())
        })();
        if let Err(error) = checked {
            let reason = format!("Readback {}: {error:#}", receipt["name"]);
            receipt["readback_error"] = reason.clone().into();
            failures.push(reason);
        }
    }
    stock.finish()?;
    staged.finish()?;
    let source_after = package_stamp(&source_packages)?;
    let native_after = package_stamp(&native)?;
    if source_stamp != source_after || native_stamp != native_after {
        failures.push("Configured source or stock packages changed during verification".into());
    }
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(
            &json!({"source":source.report(),"source_revision":revision,"test_binary_sha256":test_binary_sha256,
                "source_stamp":source_stamp,"source_stamp_after":source_after,
                "native_stamp":native_stamp,"native_stamp_after":native_after,
                "plan_sha256":hex::encode(Sha256::digest(&plan_bytes)),
                "converter_revision":parhelion_import::CONVERTER_REVISION,"cases":receipts,
                "failures":failures,"staged":built.run_directory,"artifacts":built.artifacts,
                "timing_seconds":{"conversion":conversion_seconds,"staging":staging_seconds,
                    "readback":readback_began.elapsed().as_secs_f64(),"total":began.elapsed().as_secs_f64()},
                "limits":"Source conversion, persisted reload and native package readback. Rendering, optics, cloth, audio playback, activation, repeated use, cancellation, expiry and cleanup execution require separate live acceptance. Compilation and link timing belong to the external Release verification receipt."}),
        )?,
    )?;
    ensure!(
        failures.is_empty(),
        "Required source coverage failed: {}",
        failures.join("\n")
    );
    Ok(())
}

fn reject_retiring_presentation(
    reference: &parhelion_import::GraphReference,
    graph: &Value,
) -> Result<()> {
    let item = number(&graph["item_hash"])?;
    parhelion_import::GraphReference::new(&reference.directory, item)?;
    let mut exercised = 0;
    for node in graph["nodes"].as_array().context("Graph nodes")? {
        let path = reference
            .directory
            .join(node["file"].as_str().context("Node file")?);
        let bytes = fs::read(&path)?;
        let p = Payload(bytes.clone());
        let Ok(instance) = p.pointer(16) else {
            continue;
        };
        if instance < 4 || p.u32(instance - 4).ok() != Some(0x808084D7) {
            continue;
        }
        let definition = usize::try_from(p.u64(instance + 8)?)?;
        let mut retiring = bytes.clone();
        retiring[definition + 0x2e0] |= 2;
        fs::write(&path, retiring)?;
        let result = parhelion_import::GraphReference::new(&reference.directory, item);
        fs::write(&path, bytes)?;
        let error = result
            .err()
            .context("Unsafe appearance graph was accepted")?;
        ensure!(
            format!("{error:#}").contains("retire its appearance part"),
            "Unexpected graph rejection: {error:#}"
        );
        exercised += 1;
    }
    ensure!(
        exercised > 0,
        "Required presentation lifecycle corpus is empty"
    );
    parhelion_import::GraphReference::new(&reference.directory, item)?;
    Ok(())
}
