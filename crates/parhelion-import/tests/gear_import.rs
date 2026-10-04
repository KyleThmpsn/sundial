//! Configured package-to-recipe gear workflow. Never installs its output.
use anyhow::{Context, Result, ensure};
use parhelion_import::cancellation;
use parhelion_import::d2_mot::{
    GraphReference, reader,
    service::{self, Weapon},
};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

#[test]
#[ignore = "Requires configured modern/native packages, PARHELION_GEAR_CASES, PARHELION_GEAR_DONORS and fresh PARHELION_GEAR_OUTPUT"]
fn source_gear_is_planned_before_conversion_and_preserves_its_art() -> Result<()> {
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?.canonicalize()?;
    let native = configured("PARHELION_IMPORT_NATIVE_PACKAGES")?.canonicalize()?;
    let output = reader::outside(
        &configured("PARHELION_GEAR_OUTPUT")?,
        modern.parent().context("Modern root")?,
    )?;
    let output = reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh gear artifact directory");
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_GEAR_CASES")?)?)?;
    let donors: Value = serde_json::from_slice(&fs::read(configured("PARHELION_GEAR_DONORS")?)?)?;
    ensure!(!cases.is_empty(), "Configure at least one gear case");
    fs::create_dir_all(&output)?;
    let mut receipt = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        // Each case supplies the real catalog item and either "prepared" or
        // a required rejection reason. No personal item or installation path.
        let item: Weapon = serde_json::from_value(case["item"].clone())?;
        ensure!(item.family().is_model_gear(), "Expected model gear");
        let folder = output.join(format!("case-{index}"));
        let mut phases = Vec::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_step = format!("Converting {}...", item.name);
        let result = cancellation::run(cancel.clone(), || {
            service::prepare_with_progress(
                &item,
                &modern,
                &native,
                &donors,
                &folder,
                &mut |phase| {
                    if case["expected"] == "cancelled" && phase == cancel_step {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    phases.push(phase);
                },
            )
        });
        let selection_path = folder.join("gear-0/selection.json");
        let selection: Value = serde_json::from_slice(&fs::read(&selection_path)?)?;
        ensure!(
            selection["conversions_started"]
                .as_u64()
                .context("Conversion count")?
                <= 1,
            "Gear conversion retried against another item"
        );
        match (case["expected"].as_str(), result) {
            (Some("prepared"), Ok(path)) => {
                let recipe: Value = serde_json::from_slice(&fs::read(&path)?)?;
                let reference: GraphReference =
                    serde_json::from_value(recipe["overrides"]["imported_graph"].clone())?;
                reference.validate(service::destination_hash(item.hash)?)?;
                let graph: Value = serde_json::from_slice(&fs::read(
                    reference.directory.join("asset-graph.json"),
                )?)?;
                let source: Value =
                    serde_json::from_slice(&fs::read(folder.join("gear-0/source/report.json"))?)?;
                ensure!(recipe["name"] == item.name && graph["source_item"] == item.hash);
                ensure!(graph["gameplay_verified"] == false);
                let source_parts = source["art_parts"].as_array().context("Source art")?;
                ensure!(!source_parts.is_empty(), "prepared gear has no source art");
                let imported_parts = graph["gear_art"]["parts"]
                    .as_array()
                    .context("Imported art")?;
                ensure!(
                    source_parts.len() == imported_parts.len(),
                    "Lost a source art part"
                );
                for source in source_parts {
                    let assignment = u32::from_str_radix(
                        source["assignment"].as_str().context("Source assignment")?,
                        16,
                    )?;
                    ensure!(
                        imported_parts
                            .iter()
                            .any(|part| part["source_assignment"] == assignment)
                    );
                }
                ensure!(selection["conversions_started"] == 1);
                receipt
                    .push(json!({"item":item,"recipe":path,"selection":selection,"phases":phases}));
            }
            (Some("rejected"), Err(error)) => {
                let reason = format!("{error:#}");
                ensure!(
                    reason.contains(
                        case["reason_contains"]
                            .as_str()
                            .context("Expected reason")?
                    ),
                    "Unexpected rejection: {reason}"
                );
                ensure!(
                    !folder.join("result.json").exists(),
                    "Published a failed import"
                );
                receipt.push(
                    json!({"item":item,"rejection":reason,"selection":selection,"phases":phases}),
                );
            }
            (Some("cancelled"), Err(error)) => {
                ensure!(
                    cancellation::is_cancelled(&error),
                    "Cancellation became an import failure: {error:#}"
                );
                ensure!(
                    !folder.join("result.json").exists(),
                    "Published a cancelled import"
                );
                let parts = selection["models"]["art_parts"]
                    .as_array()
                    .context("Selected parts")?;
                ensure!(!parts.is_empty(), "cancelled conversion selected no art");
                for part in parts {
                    let directory =
                        PathBuf::from(part["directory"].as_str().context("Part directory")?);
                    ensure!(
                        !directory.join("prepared").exists(),
                        "Conversion continued after cancellation"
                    );
                }
                receipt.push(
                    json!({"item":item,"cancelled":true,"selection":selection,"phases":phases}),
                );
            }
            (expected, result) => anyhow::bail!("Expected {expected:?}, received {result:?}"),
        }
        reader::write_json(
            &output.join("gear-import-receipt.json"),
            &json!({"cases":receipt,"gameplay_verified":false,"installed":false}),
        )?;
    }
    Ok(())
}
