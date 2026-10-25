//! Persisted subclass perks must resolve their imported roots without an item runtime entity.
//! Failure boundaries: missing assets, the previous item's symbol map leaking into a subclass,
//! native envelope fallback, shared mutable owners, altered stock graphs and stale fingerprints.
use super::*;
use crate::subclass::{EntryEdits, Place, SubclassAbilities};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::sandbox_perk::program::{
    Action, Asset, ImportedAsset, Position, Program, Trigger,
};

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn persisted_subclass_attachments_follow_private_pool_actions() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("imported-subclass");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory for each run");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let catalog_path = output.join("catalog.json");
    let catalog =
        InvestmentCatalog::load_with_cache_path(install, &catalog_path, true, |_| {}).unwrap();
    let bases = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let base = bases
        .iter()
        .find(|base| base.name == "Stormcaller")
        .unwrap();
    let stock = open_manager(view.path()).unwrap();
    let source_root = 0x80BC_57DD;
    let graph = stock.read_tag(TagHash(source_root)).unwrap();
    let (_, _, rows, class) = array_at(&graph, 16).unwrap();
    assert_eq!(class, 0x8080_9C04);
    let source_owner = read_u32(&graph, rows).unwrap();
    let owner = stock.read_tag(TagHash(source_owner)).unwrap();
    let directory = output.join("group");
    fs::create_dir_all(&directory).unwrap();
    let mut nodes = Vec::new();
    for (symbol, template, original) in [
        ("subclass-fixture-root", source_root, &graph),
        ("subclass-fixture-owner", source_owner, &owner),
    ] {
        let mut bytes = original.clone();
        let mut patches = Vec::new();
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            if read_u32(&bytes, at).unwrap() == source_owner {
                bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                patches.push(json!({"offset":at,"symbol":"subclass-fixture-owner"}));
            }
        }
        fs::write(directory.join(format!("{symbol}.bin")), bytes).unwrap();
        nodes.push(json!({"symbol":symbol,"template":template,
            "file":format!("{symbol}.bin"),"patches":patches}));
    }
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&json!({
            "attachments":{"installable":true,"roots":["subclass-fixture-root"],"nodes":nodes}
        }))
        .unwrap(),
    )
    .unwrap();
    let (_, _, source_perk, _) = super::stock::runtime_plug(&stock, 0x0222_2CBF);
    let mut recipes = Vec::new();
    for index in 0..2 {
        let mut effect = crate::perk::PerkRecipe::effect(source_perk);
        effect.program = Some(Program {
            name: "Subclass Attachment".into(),
            trigger: Trigger::MeleeKill,
            actions: vec![Action::Spawn {
                asset: Asset {
                    graph: source_root,
                    ..Default::default()
                },
                position: Position::Event,
            }],
            imported_assets: vec![ImportedAsset {
                action_index: 0,
                directory: directory.clone(),
                sha256: parhelion_import::d2_mot::native::attachment::fingerprint(&directory)
                    .unwrap(),
                symbol: "subclass-fixture-root".into(),
            }],
            ..Default::default()
        });
        let mut perk = crate::perk::PerkRecipe::new();
        perk.name = "Subclass Attachment".into();
        perk.effects.push(effect);
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.subclass-attachment-{index}"),
            base.hash,
            &base.name,
        )
        .unwrap();
        recipe.kind = crate::ItemKind::Subclass;
        recipe.name = format!("Imported Subclass Fixture {index}");
        let mut abilities = SubclassAbilities::default();
        abilities.set_edits(
            base.hash,
            Place::Ability(19),
            EntryEdits {
                custom_perks: vec![perk],
                ..Default::default()
            },
        );
        recipe.overrides.subclass_abilities = Some(abilities);
        let path = output.join(format!("fixture-{index}.parhelion.json"));
        fs::write(&path, recipe.to_json_pretty().unwrap()).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(&path).unwrap());
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: recipes.clone(),
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let staged_view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let staged_install = staged_view.path().parent().unwrap();
    fs::write(staged_install.join("destiny2.exe"), []).unwrap();
    for artifact in &build.artifacts {
        staged_view
            .add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(staged_view.path()).unwrap();
    let globals_tag = resolve_live_named_tag(&staged, "investment_globals", None).unwrap();
    let globals = staged.read_tag(globals_tag).unwrap();
    let catalog =
        InvestmentCatalog::load_with_cache_path(staged_install, &catalog_path, true, |_| {})
            .unwrap();
    let subclasses = catalog.subclasses(|_| true);
    let mut roots = BTreeSet::new();
    let mut owners = BTreeSet::new();
    let mut receipt = Vec::new();
    for recipe in &recipes {
        let subclass = subclasses
            .iter()
            .find(|item| item.name == recipe.name)
            .unwrap();
        let perks = subclass.entry_perks[&19]
            .iter()
            .filter(|perk| !base.entry_perks[&19].contains(perk))
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(perks.len(), 1);
        let action =
            load_sandbox_perk_runtime_action(&staged, &globals, usize::from(perks[0])).unwrap();
        let decoded =
            sundial::package_authoring::sandbox_perk::action::decode(&action.action_payload)
                .unwrap();
        let spawn = decoded.effects().find(|effect| effect.kind == 3).unwrap();
        let root = read_u32(&action.action_payload, spawn.offset + 16).unwrap();
        assert_ne!(root, source_root, "the native envelope must be replaced");
        assert!(
            roots.insert(root),
            "each item resolves its own imported root"
        );
        let bytes = staged.read_tag(TagHash(root)).unwrap();
        let (_, _, at, _) = array_at(&bytes, 16).unwrap();
        let owner_tag = read_u32(&bytes, at).unwrap();
        assert_ne!(owner_tag, source_owner);
        assert!(owners.insert(owner_tag));
        let owner_bytes = staged.read_tag(TagHash(owner_tag)).unwrap();
        for (actual, expected) in [(&bytes, &graph), (&owner_bytes, &owner)] {
            let mut normalized = actual.clone();
            for at in (0..normalized.len().saturating_sub(3)).step_by(4) {
                if read_u32(&normalized, at).unwrap() == owner_tag {
                    normalized[at..at + 4].copy_from_slice(&source_owner.to_le_bytes());
                }
            }
            assert_eq!(&normalized, expected);
        }
        fs::write(output.join(format!("{root:08X}.bin")), &bytes).unwrap();
        fs::write(output.join(format!("{owner_tag:08X}.bin")), &owner_bytes).unwrap();
        fs::write(
            output.join(format!("{}.bin", action.action_tag)),
            &action.action_payload,
        )
        .unwrap();
        receipt.push(json!({"item":subclass.hash,"perk":perks[0],"root":root,
            "owner":owner_tag,"action_sha256":sha(&action.action_payload),
            "graph_sha256":sha(&bytes),"owner_sha256":sha(&owner_bytes)}));
    }
    assert_eq!(staged.read_tag(TagHash(source_root)).unwrap(), graph);
    assert_eq!(staged.read_tag(TagHash(source_owner)).unwrap(), owner);
    let payload = directory.join("subclass-fixture-owner.bin");
    let original = fs::read(&payload).unwrap();
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::write(&payload, changed).unwrap();
    let result = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {});
    fs::write(&payload, original).unwrap();
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("changed after preparation")
    );
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "revision":revision,"source_packages":packages,"manifest":build.manifest_path,
            "source_graph_sha256":sha(&graph),"source_owner_sha256":sha(&owner),
            "readback":receipt,"stock_unchanged":true,"changed_asset_rejected":true,
            "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
            "installed":false,"gameplay_verified":false
        }))
        .unwrap(),
    )
    .unwrap();
}
