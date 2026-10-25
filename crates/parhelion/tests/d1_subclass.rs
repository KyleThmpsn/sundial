//! Preimplementation end-to-end contract for complete source subclass plans. Failure modes:
//! dropped source nodes or step records, accidental live materialization of an incomplete plan,
//! lost choices after persisted recipe reload and silent changes to a native draft on rejection.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use parhelion::{ItemKind, WeaponRecipe, subclass::d1::Plan};
use parhelion_import::{d1_roi::Source, tiger::reader::package_stamp};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

#[test]
#[ignore = "requires D1_SOURCE_PACKAGES, D1_SUBCLASS_OUTPUT and SUNDIAL_TEST_REVISION"]
fn source_catalog_survives_reload_and_incomplete_materialization_is_atomic() -> Result<()> {
    let packages = configured("D1_SOURCE_PACKAGES")?;
    let output = configured("D1_SUBCLASS_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh output directory");
    fs::create_dir_all(&output)?;
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let source = Source::open(&packages, |_, _| {})?;
    let stamp = package_stamp(&packages)?;
    let catalog = source.subclass_choices()?;
    ensure!(!catalog.is_empty(), "Configured subclass corpus is empty");
    let selected = catalog
        .iter()
        .find(|subclass| subclass.item == 0xDA0B6E1A)
        .context("The configured source fixture must include Sunsinger")?;
    // Independent raw layout oracle for this source fixture, not a second catalog serializer.
    let grid = source.read(0x80A65F5C)?;
    let rows = grid.array(0x10, 64, Some(0x80801728))?;
    ensure!(
        selected.nodes.len() == rows.len(),
        "Every source node is retained"
    );
    for node in &selected.nodes {
        let row = *rows
            .get(usize::try_from(node.node)?)
            .context("Source node ordinal")?;
        ensure!(
            node.group_key == grid.u32(row + 0x38)?,
            "Source group key survives"
        );
        ensure!(
            node.node_flags == grid.u32(row + 0x3C)?,
            "Source node flags survive"
        );
        ensure!(!node.steps.is_empty(), "Source node retains its steps");
    }
    // The original prerequisite and exclusive-group consumers identify this fully
    // unlocked fixture's foundations. Visible singleton foundations are not an
    // assertion about the source game's new-character selections.
    let foundations = selected
        .nodes
        .iter()
        .filter(|node| node.steps.iter().any(|step| step.foundation == Some(true)))
        .map(|node| node.node)
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        foundations == [0, 10, 12, 13, 20, 24].into_iter().collect(),
        "Fully unlocked source foundations follow the qualified prerequisite groups"
    );
    let plan =
        Plan::from_source(selected, &source.report().builds, &stamp).map_err(anyhow::Error::msg)?;
    let saved: serde_json::Value =
        serde_json::from_str(include_str!("../recipes/every-end.parhelion.json"))?;
    let mut recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&json!({
        "schema":1,"namespace":saved["namespace"],"collection_placement":"sunrise_badge",
        "donor":{"item_hash":"0xB9246E3F","expected_name":null},"identity":saved["identity"],
        "name":saved["name"],"flavor":"Source subclass plan verification.","source":"Parhelion"
    }))?)?;
    recipe.kind = ItemKind::Subclass;
    recipe.rename_authored_item("Source Subclass Plan")?;
    recipe
        .overrides
        .subclass_abilities
        .get_or_insert_with(Default::default)
        .d1 = Some(plan);
    let path = output.join("plan.parhelion.json");
    recipe.save_json(&path)?;
    let reloaded = WeaponRecipe::load_json(&path)?;
    let saved = serde_json::to_value(&reloaded)?;
    let original = serde_json::to_value(selected)?;
    ensure!(
        saved["overrides"]["subclass_abilities"]["d1"]["source"] == original,
        "Every decoded source step, grant, perk and stat record survives recipe reload"
    );
    let plan = reloaded
        .overrides
        .subclass_abilities
        .as_ref()
        .unwrap()
        .d1
        .as_ref()
        .unwrap();
    let mut destination = reloaded.clone();
    let error = plan
        .materialize(&mut destination, &[])
        .expect_err("An incomplete source plan cannot become a playable subclass");
    ensure!(
        destination == reloaded,
        "Rejected materialization leaves the full draft unchanged"
    );
    ensure!(
        !error.trim().is_empty(),
        "Incomplete materialization identifies its missing boundary"
    );
    ensure!(
        package_stamp(&packages)? == stamp,
        "Source packages remain unchanged"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"source_builds":source.report().builds,"source_stamp":stamp,
            "source_item":selected.item,"source_grid":selected.grid,"nodes":rows.len(),
            "steps":selected.nodes.iter().map(|node|node.steps.len()).sum::<usize>(),
            "fully_unlocked_foundations":foundations,
            "saved_recipe_sha256":hex::encode(Sha256::digest(fs::read(&path)?)),
            "incomplete_materialization":error,"draft_unchanged":true,"installed":false,"gameplay_verified":false,
            "executable_sha256":hex::encode(Sha256::digest(fs::read(env::current_exe()?)?))
        }))?,
    )?;
    Ok(())
}
