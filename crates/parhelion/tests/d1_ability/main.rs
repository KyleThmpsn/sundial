//! Configured source selection through an atomic draft import, reload and staged native readback.
//! Failure boundaries: stale drafts, invalid target roles, empty source discovery, incomplete source
//! groups, retained donor modifiers, wrong property recipients, priority or value loss and stock mutation.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use parhelion::subclass::{
    import::{self, Request},
    layout,
};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe};
use parhelion_import::{
    d1_roi::Source,
    tiger::{
        payload::Payload,
        reader::{Reader, package_stamp},
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use sundial::investment::InvestmentCatalog;
mod count;

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
struct SourceRow {
    class: u32,
    value: Vec<u8>,
    name: u32,
    priority: u32,
    mode: u32,
    handler: u32,
}
fn native_view(native: &Path, output: &Path, run: &Path, names: &[String]) -> Result<PathBuf> {
    let root = output.join("view");
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    fs::write(root.join("destiny2.exe"), [])?;
    fs::copy(
        native
            .parent()
            .context("Native installation")?
            .join("bin/x64/oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for file in fs::read_dir(native)? {
        let file = file?;
        if file.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(file.path(), packages.join(file.file_name()))?;
        }
    }
    for name in names {
        ensure!(!packages.join(name).exists(), "Overlay collides with stock");
        fs::copy(run.join(name), packages.join(name))?;
    }
    Ok(packages)
}
fn action(reader: &mut Reader, index: u16) -> Result<Payload> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash;
    let globals = reader.tag(globals.0, None)?;
    let finished = reader.tag(globals.u32(16 + 71 * 16)?, Some(0x80805C97))?;
    let rows = finished.array(8, 24, Some(0x80805C9D))?;
    let key = finished.u32(*rows.get(usize::from(index)).context("Finished perk")? + 4)?;
    let runtime = reader.tag(0x80EC3F60, Some(0x80809780))?;
    let found = runtime
        .array(8, 8, None)?
        .into_iter()
        .filter(|&r| runtime.u32(r).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(found.len() == 1, "Private action assignment is ambiguous");
    Ok((*reader.tag(runtime.u32(found[0] + 4)?, Some(0x808040B5))?).clone())
}

// Compare the bank's observable stock recipients and multiplicity without depending on moved
// block addresses or the bank writer's own decoder.
fn stock_rows(bank: &Payload, keys: &[u32]) -> Result<Vec<Vec<u8>>> {
    let mut result = Vec::new();
    for row in bank.array(bank.pointer(24)? + 0x88, 56, Some(0x8080453B))? {
        if !keys.contains(&bank.u32(row + 16)?) {
            continue;
        }
        let body = bank.pointer(row + 48)?;
        let class = bank.u32(body + 4)?;
        let size = match class {
            0x808044D2 => 1,
            0x808042A5 | 0x808044CA => 4,
            0x808044D4 => 8,
            0x808044D6 => 16,
            other => anyhow::bail!("Unverified stock recipient {other:08X}"),
        };
        let mut record = bank.0[row + 16..row + 40].to_vec();
        record.extend_from_slice(&class.to_le_bytes());
        record.extend_from_slice(&bank.0[body + 16..body + 16 + size]);
        if bank.u64(row + 40)? != 0 {
            let gate = bank.pointer(row + 40)?;
            record.extend_from_slice(&bank.0[gate - 4..gate + 8]);
        }
        result.push(record);
    }
    Ok(result)
}

#[test]
#[ignore = "Requires D1_SOURCE_PACKAGES, SUNDIAL_STOCK_PACKAGES, D1_ABILITY_OUTPUT, PARHELION_D1_XG and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn triple_jump_survives_selected_entry_import_reload_and_staging() -> Result<()> {
    let output = configured("D1_ABILITY_OUTPUT")?;
    ensure!(!output.exists(), "Use fresh output");
    let source_path = configured("D1_SOURCE_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let revision = env::var("SUNDIAL_TEST_REVISION")?;
    let source = Source::open(&source_path, |_, _| {})?;
    let choices = source.ability_choices()?;
    ensure!(
        !choices.is_empty(),
        "Required source ability corpus is empty"
    );
    let selected = choices
        .iter()
        .filter(|c| c.subclass_name == "Gunslinger" && c.name == "Triple Jump")
        .collect::<Vec<_>>();
    ensure!(
        selected.len() == 1,
        "Required source Triple Jump choice is missing or ambiguous"
    );
    fs::create_dir_all(&output)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().context("Native root")?,
        &output.join("catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let subclasses = catalog.subclasses(|_| true);
    let base = subclasses
        .iter()
        .find(|s| s.name == "Gunslinger")
        .context("Native Gunslinger")?;
    let saved: serde_json::Value =
        serde_json::from_str(include_str!("../../recipes/every-end.parhelion.json"))?;
    let mut recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&json!({
        "schema":1,"namespace":saved["namespace"],"collection_placement":"sunrise_badge",
        "donor":{"item_hash":format!("0x{:08X}",base.hash),"expected_name":base.name},
        "identity":saved["identity"],"name":saved["name"],"flavor":"Private ability verification.","source":"Parhelion"
    }))?)?;
    recipe.kind = ItemKind::Subclass;
    recipe.rename_authored_item("Imported Triple Jump E2E")?;
    let baseline = recipe.clone();
    let stamp = package_stamp(&native)?;
    let mut stock = Reader::new(&native, &output.join("stock"), false)?;
    // These build-pinned original bank identities are independent of the lowering report.
    let original = source.read(0x80A74698)?;
    let before = stock.tag(0x80BC1D56, Some(0x80809C36))?;
    let invalid_output = output.join("invalid-target");
    ensure!(
        import::prepare(&Request {
            source_packages: &source_path,
            native_packages: &native,
            output: &invalid_output,
            recipe: &recipe,
            choice: selected[0],
            entry: layout::SUPER
        })
        .is_err(),
        "Movement import accepted a super target"
    );
    ensure!(
        recipe == baseline && !invalid_output.exists(),
        "Invalid preparation changed its draft or created an artifact"
    );
    let mut expected = Vec::new();
    for row in original.array(original.pointer(24)? + 0xA0, 40, Some(0x80802A84))? {
        if original.u32(row)? != 0x53722F50 {
            continue;
        }
        ensure!(
            original.u64(row + 24)? == 0,
            "Source group unexpectedly gated"
        );
        let body = original.pointer(row + 32)?;
        let (class, size) = match original.u32(body - 4)? {
            0x808027E0 => (0x808044D2, 1),
            0x8080209C => (0x808042A5, 4),
            0x80802330 => (0x808044D4, 8),
            other => anyhow::bail!("Unverified source recipient {other:08X}"),
        };
        let witnesses = before
            .array(before.pointer(24)? + 0x88, 56, Some(0x8080453B))?
            .into_iter()
            .filter(|&at| {
                before.u32(at + 16).ok() == Some(0x53722F50)
                    && before.u32(at + 20).ok() == original.u32(row + 4).ok()
                    && before.u32(at + 24).ok() == original.u32(row + 8).ok()
                    && before.u64(at + 40).ok() == Some(0)
                    && before
                        .pointer(at + 48)
                        .ok()
                        .and_then(|body| before.u32(body + 4).ok())
                        == Some(class)
            })
            .collect::<Vec<_>>();
        ensure!(witnesses.len() == 1, "Independent native priority witness");
        expected.push(SourceRow {
            class,
            value: original.0[body..body + size].to_vec(),
            name: original.u32(row + 4)?,
            priority: original.u32(row + 8)?,
            mode: original.u32(row + 16)?,
            handler: before.u32(witnesses[0] + 28)?,
        });
    }
    ensure!(
        expected.len() == 3,
        "Source Triple Jump group is incomplete"
    );
    ensure!(
        expected
            .iter()
            .find(|row| row.class == 0x808042A5)
            .is_some_and(|row| row.priority == 0),
        "The configured Triple Jump count must exercise ordinary priority zero"
    );
    let prepared = import::prepare(&Request {
        source_packages: &source_path,
        native_packages: &native,
        output: &output.join("prepared"),
        recipe: &recipe,
        choice: selected[0],
        entry: layout::MOVEMENT[0],
    })
    .map_err(anyhow::Error::msg)?;
    ensure!(recipe == baseline, "Preparation mutated its draft");
    let mut changed = baseline.clone();
    changed.flavor = "Concurrent edit".into();
    let preserved = changed.clone();
    ensure!(
        prepared.apply(&mut changed).is_err() && changed == preserved,
        "Stale apply lost an intervening edit"
    );
    prepared.apply(&mut recipe).map_err(anyhow::Error::msg)?;
    let saved = output.join("ability.parhelion.json");
    fs::write(&saved, recipe.to_json_pretty()?)?;
    let reloaded = WeaponRecipe::load_json(&saved)?;
    ensure!(reloaded == recipe, "Portable reload changed the candidate");
    let armor = count::prepare_armor(&source, &native, &output, &catalog)?;
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![reloaded, armor.clone()],
        maps: Vec::new(),
    })
    .map_err(anyhow::Error::msg)?;
    let built = parhelion::build_and_stage_snapshot_with_progress(&snapshot, |_| {})
        .map_err(anyhow::Error::msg)?;
    let names = built
        .artifacts
        .iter()
        .map(|a| a.file_name.clone())
        .collect::<Vec<_>>();
    let staged_path = native_view(&native, &output, &built.run_directory, &names)?;
    let staged_catalog = InvestmentCatalog::load_with_cache_path(
        staged_path.parent().context("Staged root")?,
        &output.join("staged-catalog.json"),
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)?;
    let staged = staged_catalog.subclasses(|_| true);
    let imported = staged
        .iter()
        .find(|s| s.name == recipe.name)
        .context("Staged subclass")?;
    let entry = layout::MOVEMENT[0];
    ensure!(
        imported.entry_rows.get(&entry) == base.entry_rows.get(&entry),
        "Native controller selection changed"
    );
    ensure!(
        imported
            .entry_modifiers
            .get(&entry)
            .is_none_or(Vec::is_empty),
        "Donor properties still activate"
    );
    for other in 0..24u8 {
        if other == entry {
            continue;
        }
        ensure!(
            imported.entry_rows.get(&other) == base.entry_rows.get(&other)
                && imported.entry_modifiers.get(&other) == base.entry_modifiers.get(&other)
                && imported.entry_perks.get(&other) == base.entry_perks.get(&other),
            "Unrelated subclass entry changed"
        );
    }
    let perks = imported
        .entry_perks
        .get(&entry)
        .context("Private movement perk")?;
    ensure!(
        perks.len() == 1,
        "Movement import needs one complete private program"
    );
    let mut emitted = Reader::new(&staged_path, &output.join("readback"), false)?;
    let program = action(&mut emitted, perks[0])?;
    ensure!(
        program.u64(0x88)? == 1 && program.u64(0x90)? == 0,
        "Selected movement lifecycle changed"
    );
    let bank = emitted.tag(0x80BC1D56, Some(0x80809C36))?;
    let rows = bank.array(bank.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let old = before.array(before.pointer(24)? + 0x88, 56, Some(0x8080453B))?;
    let original_keys = old
        .iter()
        .map(|&r| before.u32(r + 16))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        rows.len() == old.len() + expected.len() + 3,
        "Unexpected movement row population"
    );
    let effects = program.array(0x38, 8, Some(0x808040AC))?;
    ensure!(
        effects.len() == 1,
        "A source group gained multiple activation keys"
    );
    let effect = program.pointer(effects[0])?;
    ensure!(
        program.u8(effect)? == 7 && program.u8(effect + 2)? == 3 && program.u8(effect + 8)? == 0,
        "Wrong recipient or operation"
    );
    let key = program.u32(effect + 4)?;
    ensure!(
        !old.iter().any(|&r| before.u32(r + 16).ok() == Some(key)),
        "Private key aliases stock"
    );
    let emitted_rows = rows
        .iter()
        .copied()
        .filter(|&r| bank.u32(r + 16).ok() == Some(key))
        .collect::<Vec<_>>();
    ensure!(
        emitted_rows.len() == expected.len(),
        "Private group is incomplete"
    );
    let mut checked = Vec::new();
    for (row, source_row) in emitted_rows.into_iter().zip(&expected) {
        ensure!(bank.u64(row + 40)? == 0, "Private property gained a gate");
        let body = bank.pointer(row + 48)?;
        let class = bank.u32(body + 4)?;
        let instance = usize::try_from(bank.u64(row + 8)?)?;
        ensure!(
            bank.u64(instance + 8)? == row as u64 && bank.u32(instance + 4)? == 0x8080453B,
            "Private property row has no matching instance"
        );
        let modifier_instance = bank.pointer(instance + 48)?;
        ensure!(
            bank.u32(modifier_instance + 4)? == class + 1
                && bank.u64(body + 8)? == modifier_instance as u64
                && bank.u64(modifier_instance + 8)? == body as u64,
            "Private modifier twins disagree"
        );
        ensure!(
            class == source_row.class,
            "Source group order or recipient changed"
        );
        ensure!(
            bank.0[body + 16..body + 16 + source_row.value.len()] == source_row.value[..],
            "Source movement value changed"
        );
        ensure!(
            bank.u32(row + 20)? == source_row.name
                && bank.u32(row + 24)? == source_row.priority
                && bank.u32(row + 32)? == source_row.mode
                && bank.u32(row + 28)? == source_row.handler,
            "Source movement selection metadata changed"
        );
        checked.push(json!({"key":key,"class":class,"name":source_row.name,
                "priority":source_row.priority,"mode":source_row.mode,
                "handler":source_row.handler,"value":hex::encode(&source_row.value)}));
    }
    ensure!(
        checked.len() == expected.len(),
        "Source group was only partly emitted"
    );
    ensure!(
        stock_rows(&before, &original_keys)? == stock_rows(&bank, &original_keys)?,
        "Stock movement values, selection or gates changed"
    );
    let armor_readback = count::check_armor(&source, &mut emitted, &armor, key, &before)?;
    ensure!(
        package_stamp(&native)? == stamp,
        "Stock package set changed"
    );
    ensure!(
        package_stamp(&source.report().packages)? == source.report().stamp,
        "Source package set changed"
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(
            &json!({"revision":revision,"source_builds":source.report().builds,"source_choice":selected[0],"source_bank_sha256":sha(&original.0),"native_stock_bank_sha256":sha(&before.0),"emitted_bank_sha256":sha(&bank.0),"action_sha256":sha(&program.0),"checked_properties":checked,"armor_count":armor_readback,"recipe_sha256":sha(&fs::read(saved)?),"manifest":built.manifest_path,"preparation":prepared.provenance,"installed":false,"gameplay_verified":false}),
        )?,
    )?;
    stock.finish()?;
    emitted.finish()?;
    Ok(())
}
