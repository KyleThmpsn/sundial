//! Source perk preparation, persisted authoring and private package readback.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, perk::import};
use parhelion_import::d2_mot::{gameplay::perks, payload::Payload, reader::Reader};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use sundial::{
    investment::InvestmentCatalog,
    package_authoring::{
        PackageManager,
        entity::weapon_component_bindings,
        resolve_live_named_tag,
        sandbox_perk::{action, load_sandbox_perk_runtime_action},
    },
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn manager(reader: &Reader) -> Result<PackageManager> {
    PackageManager::new(
        &reader.manager.package_dir,
        reader.manager.version,
        Some(reader.manager.platform),
    )
    .map_err(anyhow::Error::msg)
}

fn recipe(name: &str, hash: u32) -> Result<WeaponRecipe> {
    let saved: serde_json::Value =
        serde_json::from_str(include_str!("../recipes/every-end.parhelion.json"))?;
    let mut recipe = WeaponRecipe::from_json_str(&serde_json::to_string(&json!({
        "schema":1,"namespace":saved["namespace"],"collection_placement":"sunrise_badge",
        "donor":{"item_hash":format!("0x{hash:08X}"),"expected_name":null},
        "identity":saved["identity"],"name":saved["name"],"flavor":"Private import verification.","source":"Parhelion"
    }))?)?;
    recipe.rename_authored_item(name)?;
    Ok(recipe)
}

fn catalog(packages: &Path, output: &Path) -> Result<InvestmentCatalog> {
    InvestmentCatalog::load_with_cache_path(
        packages.parent().context("Package parent")?,
        output,
        true,
        |_| {},
    )
    .map_err(anyhow::Error::msg)
}

fn view(native: &Path, output: &Path, built: &parhelion::BuildReport) -> Result<PathBuf> {
    let root = output.join("view");
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    let game = native.parent().context("Native root")?;
    fs::hard_link(game.join("destiny2.exe"), root.join("destiny2.exe"))?;
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    fs::copy(
        game.join("bin/x64/oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for artifact in &built.artifacts {
        let destination = packages.join(&artifact.file_name);
        ensure!(!destination.exists(), "Never overwrite a stock hard link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    Ok(packages)
}

#[test]
#[ignore = "Requires clean SUNDIAL_STOCK_PACKAGES, PARHELION_IMPORT_MODERN_PACKAGES and fresh PARHELION_EQUIPPED_OUTPUT"]
fn equipped_modifiers_survive_private_staging_and_preserve_stock() -> Result<()> {
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let output = configured("PARHELION_EQUIPPED_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output)?;
    let stock = catalog(&native, &output.join("stock-catalog.json"))?;
    let mut reader = Reader::new(&native, &output.join("stock"), false)?;
    let stock_manager = manager(&reader)?;
    let globals_tag = resolve_live_named_tag(&stock_manager, "investment_globals", None)
        .map_err(anyhow::Error::msg)?;
    let globals = reader.tag(globals_tag.0, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = items.array(8, 24, Some(0x80807BE8))?;
    ensure!(!rows.is_empty(), "Required stock catalog is empty");
    for row in rows {
        let hash = items.u32(row)?;
        ensure!(
            stock.item_display_name(hash) != Some("Dual Loader"),
            "Configure a stock catalog without imported Dual Loader"
        );
    }
    let donor = stock
        .weapon_donors()
        .into_iter()
        .find(|d| d.name == "Perfect Paradox" && d.type_name == "Shotgun")
        .and_then(|d| stock.weapon_donor(d.hash))
        .context("Stock shotgun donor")?;
    let (socket, plug, perk) = donor
        .sockets
        .iter()
        .filter(|s| s.index >= 3 && s.max_authored_choices > 0)
        .find_map(|s| {
            let plug = s.native_default?;
            stock
                .item_sandbox_perk_indices(plug)
                .into_iter()
                .find(|&p| {
                    load_sandbox_perk_runtime_action(&stock_manager, &globals.0, usize::from(p))
                        .is_ok()
                })
                .map(|p| (s.index as u16, plug, p))
        })
        .context("Usable native trait socket")?;
    let stock_action =
        load_sandbox_perk_runtime_action(&stock_manager, &globals.0, usize::from(perk))
            .map_err(anyhow::Error::msg)?;
    let oracle = reader.tag(0x80BBC73C, Some(0x808040B5))?;
    let oracle_decoded = action::decode(&oracle.0).map_err(anyhow::Error::msg)?;
    let stock_envelope = reader.tag(0x8162C91A, Some(0x80809C0F))?;
    let stock_binding =
        weapon_component_bindings(&stock_envelope.0, 0x7330E39F).map_err(anyhow::Error::msg)?[0];
    let stock_owner = reader.tag(stock_binding.owner_tag, None)?;
    let mut source = Reader::new(&modern, &output.join("source-oracle"), true)?;
    let mut recipes = Vec::new();
    let mut amounts = Vec::new();
    let cases = [
        (25606670, "Dual Loader", 1.0f32),
        (3143051906, "Enhanced Dual Loader", 2.0f32),
    ];
    for (i, (hash, name, expected)) in cases.into_iter().enumerate() {
        // Read the shipped source record independently of the new translator.
        let assigned = perks::assigned(&mut source, hash)?;
        ensure!(assigned.len() == 1, "Source corpus changed");
        let controller = perks::controller::read(&assigned[0].controller)?;
        let p = &assigned[0].controller;
        let effects = p.array(controller.states[1].offset + 8, 24, Some(0x808037AB))?;
        ensure!(effects.len() == 1, "Source effect corpus changed");
        let effect = p.pointer(effects[0])?;
        let graph_tag = source.ref64(p, effect + 16)?;
        let graph = source.tag(graph_tag, Some(0x80809AD8))?;
        let components = graph.array(8, 12, Some(0x80809ACD))?;
        ensure!(components.len() == 1, "Source component corpus changed");
        let owner = source.tag(graph.u32(components[0])?, Some(0x80809B06))?;
        let settings = owner.pointer(24)?;
        let modifiers = owner.array(settings + 88, 112, Some(0x80802D33))?;
        ensure!(modifiers.len() == 1, "Source modifier corpus changed");
        let row = modifiers[0];
        let amount = owner.f32(row + 16)?;
        ensure!(
            amount == expected
                && owner.u8(row + 20)? == 0
                && owner.u8(row + 96)? == 1
                && owner.u16(row + 90)? == 3,
            "Source refill behavior changed"
        );
        amounts.push(amount);
        let mut recipe = recipe(&format!("Equipped Import {i}"), donor.summary.hash)?;
        let prepared = import::prepare(&import::Request {
            modern_packages: &modern,
            native_packages: &native,
            output: &output.join(format!("prepare-{i}")),
            recipe: &recipe,
            plug_hash: hash,
            socket_index: socket,
            choice_index: 0,
            source_plug_hash: plug,
            source_perk_index: perk,
            profile_key: 0,
            name,
        })
        .map_err(anyhow::Error::msg)?;
        let mut concurrent = recipe.clone();
        concurrent.name.push_str(" Edited");
        let before = concurrent.clone();
        ensure!(
            prepared.apply(&mut concurrent).is_err() && concurrent == before,
            "Preparation overwrote a concurrent edit"
        );
        prepared.apply(&mut recipe).map_err(anyhow::Error::msg)?;
        let path = output.join(format!("recipe-{i}.parhelion.json"));
        fs::write(&path, recipe.to_json_pretty()?)?;
        recipes.push(WeaponRecipe::load_json(&path)?);
    }
    let unsupported = recipe("Unsupported Equipped Import", donor.summary.hash)?;
    let unchanged = unsupported.clone();
    ensure!(
        import::prepare(&import::Request {
            modern_packages: &modern,
            native_packages: &native,
            output: &output.join("unsupported"),
            recipe: &unsupported,
            plug_hash: 0xCCD44161,
            socket_index: socket,
            choice_index: 0,
            source_plug_hash: plug,
            source_perk_index: perk,
            profile_key: 0,
            name: "Surplus",
        })
        .is_err()
            && unsupported == unchanged,
        "Unsupported multi-input behavior was partially imported"
    );
    source.finish()?;
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    let packages = view(&native, &output, &built)?;
    let installed = catalog(&packages, &output.join("staged-catalog.json"))?;
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let staged_manager = manager(&staged)?;
    let globals_tag = resolve_live_named_tag(&staged_manager, "investment_globals", None)
        .map_err(anyhow::Error::msg)?;
    let globals = staged.tag(globals_tag.0, None)?;
    let mut evidence = Vec::new();
    let mut private_graphs = std::collections::BTreeSet::new();
    for (i, recipe) in recipes.iter().enumerate() {
        let item = installed
            .weapon_donor(recipe.identity.item_hash.parse_u32()?)
            .context("Staged weapon")?;
        let private_plug = item.sockets[usize::from(socket)]
            .native_default
            .context("Staged default plug")?;
        ensure!(private_plug != plug, "Stock plug was reused");
        let indices = installed.item_sandbox_perk_indices(private_plug);
        ensure!(
            indices.len() == 1 && indices[0] != perk,
            "Private perk assignment is missing"
        );
        let runtime =
            load_sandbox_perk_runtime_action(&staged_manager, &globals.0, usize::from(indices[0]))
                .map_err(anyhow::Error::msg)?;
        ensure!(
            runtime.action_tag != stock_action.action_tag,
            "Stock action was reused"
        );
        let decoded = action::decode(&runtime.action_payload).map_err(anyhow::Error::msg)?;
        ensure!(
            decoded.groups.len() == 1 && decoded.auxiliary.is_empty(),
            "Unexpected native controller topology"
        );
        let group = &decoded.groups[0];
        ensure!(
            group.activation.len() == 1 && group.removal.len() == 1 && group.effects.len() == 1,
            "Native lifecycle was lost"
        );
        for (condition, kind) in [(&group.activation[0], 14), (&group.removal[0], 15)] {
            ensure!(condition.kind == kind, "Native lifecycle kind differs");
            let shipped = oracle_decoded
                .conditions()
                .into_iter()
                .find(|c| c.kind == kind)
                .context("Shipped lifecycle oracle")?;
            let actual = &runtime.action_payload[condition.offset..condition.offset + 112];
            let expected = &oracle.0[shipped.offset..shipped.offset + 112];
            ensure!(
                actual[..7] == expected[..7]
                    && actual[8..80] == expected[8..80]
                    && actual[88..112] == expected[88..112],
                "Native lifecycle differs from the independently shipped controller"
            );
        }
        let effect = &group.effects[0];
        let p = Payload(runtime.action_payload.clone());
        ensure!(
            effect.kind == 1 && p.u8(effect.offset + 2)? == 0,
            "Modifier is not attached to its weapon"
        );
        ensure!(
            p.u32(effect.offset + 24)? == 0x811C9DC5 && p.u32(effect.offset + 28)? == 0x811C9DC5,
            "Attachment suppresses cleanup"
        );
        let graph_tag = p.u32(effect.offset + 16)?;
        ensure!(
            graph_tag != 0x8162C91A && private_graphs.insert(graph_tag),
            "Modifier graphs are not private"
        );
        let graph = staged.tag(graph_tag, Some(0x80809C0F))?;
        ensure!(
            graph.u64(16)? == 1 && graph.u64(32)? == 0 && graph.u64(48)? == 0,
            "Unrelated donor behavior remains in the private modifier"
        );
        let bindings =
            weapon_component_bindings(&graph.0, 0x7330E39F).map_err(anyhow::Error::msg)?;
        ensure!(bindings.len() == 1, "Staged modifier binding");
        let binding = bindings[0];
        ensure!(
            binding.owner_tag != stock_binding.owner_tag,
            "Stock modifier owner was reused"
        );
        let owner = staged.tag(binding.owner_tag, None)?;
        let at = usize::try_from(owner.u64(binding.resource_offset as usize + 8)?)?;
        ensure!(
            owner.f32(at + 40)? == amounts[i]
                && owner.u8(at + 44)? == 0
                && owner.u16(at + 74)? == 3
                && owner.u8(at + 76)? == 1,
            "Staged refill modifier differs from the source"
        );
        fs::write(
            output.join(format!("private-action-{i}.bin")),
            &runtime.action_payload,
        )?;
        fs::write(output.join(format!("private-owner-{i}.bin")), &owner.0)?;
        evidence.push(json!({"item":recipe.identity.item_hash,"plug":private_plug,"perk":indices[0],"action":runtime.action_tag.0,"graph":graph_tag,"owner":binding.owner_tag,"row":at,"amount":amounts[i],"action_sha256":digest(&runtime.action_payload),"owner_sha256":digest(&owner.0)}));
    }
    ensure!(
        staged.tag(stock_action.action_tag.0, None)?.0 == stock_action.action_payload,
        "Stock action changed"
    );
    ensure!(
        staged.tag(stock_binding.owner_tag, None)?.0 == stock_owner.0,
        "Stock modifier changed"
    );
    staged.finish()?;
    reader.finish()?;
    fs::write(
        output.join("verified.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":evidence,"gameplay_verified":false,"native_oracle":"80BBC73C","package_artifacts":built.artifacts}),
        )?,
    )?;
    Ok(())
}
