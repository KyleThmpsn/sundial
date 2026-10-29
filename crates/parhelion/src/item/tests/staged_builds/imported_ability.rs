//! Failure contract written before imported ability-root support: persisted roots must replace
//! the equipped graph through both ability tables, resolve private component owners per item,
//! preserve nested Specific Ability conditions, leave stock intact and reject changed inputs.
//! A native movement graph is the independent fixture. This does not establish D1 gameplay.
use super::*;
use crate::subclass::{EntryEdits, ImportedAbility, Place, SubclassAbilities, layout};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use sundial::package_authoring::{
    ability_definition, ability_reference,
    sandbox_perk::{
        action,
        program::{
            Action, Asset, NativeAssetPatch, NativeAssetProgram, NativeNode, Position, Program,
            Trigger,
        },
    },
};

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn persisted_imported_abilities_equip_private_roots_and_retarget_child_conditions() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("imported-ability");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let (view, source) = stock_view(&packages, ".parhelion-imported-ability-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    let catalog = crate::test_support::catalog(view.path()).unwrap();
    let subclasses = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let base = subclasses
        .iter()
        .find(|base| base.name == "Stormcaller")
        .unwrap();
    let fixture = subclasses
        .iter()
        .find(|base| base.name == "Gunslinger")
        .unwrap();
    let entry = layout::MOVEMENT[0];
    let source_root = fixture.entry_entities[&entry];
    let donor_root = base.entry_entities[&entry];
    assert_ne!(source_root, donor_root);
    let stock = open_manager(&source).unwrap();
    let graph = stock.read_tag(TagHash(source_root)).unwrap();
    let (count, _, rows, class) = array_at(&graph, 16).unwrap();
    assert_eq!(class, 0x80809C04);
    assert!(count > 0);
    let mut originals = BTreeMap::from([(source_root, graph.clone())]);
    for index in 0..count {
        let owner = read_u32(&graph, rows + index * 12).unwrap();
        assert_eq!(
            stock.get_entry(TagHash(owner)).unwrap().reference,
            0x80809C36
        );
        originals.insert(owner, stock.read_tag(TagHash(owner)).unwrap());
    }
    let bank = sundial::package_authoring::ability_modifier::entity_bank(&graph)
        .unwrap()
        .unwrap();
    assert!(originals.contains_key(&bank));
    let directory = output.join("group");
    fs::create_dir(&directory).unwrap();
    let symbol = |tag| format!("ability-{tag:08X}");
    let mut nodes = Vec::new();
    for (&template, original) in &originals {
        let mut bytes = original.clone();
        let mut patches = Vec::new();
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            let tag = read_u32(&bytes, at).unwrap();
            if originals.contains_key(&tag) {
                bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                patches.push(json!({"offset":at,"symbol":symbol(tag)}));
            }
        }
        let file = format!("{}.bin", symbol(template));
        fs::write(directory.join(&file), bytes).unwrap();
        nodes.push(
            json!({"symbol":symbol(template),"template":template,"file":file,"patches":patches}),
        );
    }
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&json!({
            "attachments":{"installable":true,"roots":[symbol(source_root)],"nodes":nodes}
        }))
        .unwrap(),
    )
    .unwrap();
    let imported = ImportedAbility {
        directory: directory.clone(),
        sha256: parhelion_import::d2_mot::native::attachment::fingerprint(&directory).unwrap(),
        symbol: symbol(source_root),
    };

    // The native wrapper's declared resource points at a separately owned action controller.
    let wrapper = 0x80C191F5;
    let wrapper_bytes = stock.read_tag(TagHash(wrapper)).unwrap();
    let bindings =
        sundial::package_authoring::entity::weapon_component_bindings(&wrapper_bytes, 0x95A60F29)
            .unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one native controller binding")
    };
    let owner = stock.read_tag(TagHash(binding.owner_tag)).unwrap();
    let live = usize::try_from(read_u64(&owner, 16).unwrap()).unwrap() + 16;
    let definition = usize::try_from(read_u64(&owner, 24).unwrap()).unwrap() + 24;
    assert_eq!(read_u32(&owner, definition - 4).unwrap(), 0x80803E52);
    let old_child = read_u32(&owner, definition + 0x50).unwrap();
    let (_, _, source_perk, _) = super::stock::runtime_plug(&stock, 0x02222CBF);
    let mut recipes = Vec::new();
    for ordinal in 0..2 {
        let mut condition = NativeNode::condition(10).unwrap();
        condition.bytes[16..20].copy_from_slice(&donor_root.to_le_bytes());
        let child = Program {
            trigger: Trigger::Native,
            native_trigger: Some(condition),
            actions: vec![Action::Spawn {
                asset: Asset {
                    graph: 0x80BC57DD,
                    ..Default::default()
                },
                position: Position::Event,
            }],
            ..Default::default()
        };
        let mut effect = crate::perk::PerkRecipe::effect(source_perk);
        effect.program = Some(Program {
            trigger: Trigger::Always,
            actions: vec![Action::Spawn {
                asset: Asset {
                    graph: wrapper,
                    ..Default::default()
                },
                position: Position::Owner,
            }],
            native_asset_patches: vec![NativeAssetPatch {
                action_index: 0,
                source_graph: wrapper,
                patches: Vec::new(),
                appends: Vec::new(),
                remove_owners: Vec::new(),
                components: Vec::new(),
                programs: vec![NativeAssetProgram {
                    binding_hash: binding.binding_hash,
                    resource_index: binding.resource_index.try_into().unwrap(),
                    offset: (definition + 0x50 - live).try_into().unwrap(),
                    expected_action: old_child,
                    program: Box::new(child),
                }],
            }],
            ..Default::default()
        });
        let mut perk = crate::perk::PerkRecipe::new();
        perk.name = "Imported Ability Event".into();
        perk.effects.push(effect);
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.imported-ability-{ordinal}"),
            base.hash,
            &base.name,
        )
        .unwrap();
        recipe.kind = crate::ItemKind::Subclass;
        recipe.name = format!("Imported Ability Fixture {ordinal}");
        let mut abilities = SubclassAbilities::default();
        abilities.set_edits(
            base.hash,
            Place::Ability(entry),
            EntryEdits {
                imported_ability: Some(imported.clone()),
                custom_perks: vec![perk],
                removed_perks: base.entry_perks[&entry].clone(),
                removed_modifiers: base.entry_modifiers[&entry]
                    .iter()
                    .map(|&(key, row)| crate::subclass::StockModifier { key, row })
                    .collect(),
                ..Default::default()
            },
        );
        recipe.overrides.subclass_abilities = Some(abilities);
        let path = output.join(format!("fixture-{ordinal}.parhelion.json"));
        recipe.save_json(&path).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(&path).unwrap());
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: source.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: recipes.clone(),
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let (staged_view, staged_path) = stock_view(
        &packages,
        ".parhelion-imported-ability-readback-",
        Oodle::Required,
    );
    fs::write(staged_view.path().join("destiny2.exe"), []).unwrap();
    for artifact in &build.artifacts {
        let target = staged_path.join(&artifact.file_name);
        // Unlink a temporary stock hard link before placing an overlay over its file name.
        if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        fs::copy(build.run_directory.join(&artifact.file_name), target).unwrap();
    }
    let staged = open_manager(&staged_path).unwrap();
    let catalog = crate::test_support::catalog(staged_view.path()).unwrap();
    let built = catalog.subclasses(|_| true);
    let definitions = staged.read_tag(TagHash(0x81613D26)).unwrap();
    let identities = staged.read_tag(TagHash(0x81319130)).unwrap();
    let pairs = ability_definition::definitions(&definitions, &identities).unwrap();
    let stock_pairs = ability_definition::definitions(
        &stock.read_tag(TagHash(0x81613D26)).unwrap(),
        &stock.read_tag(TagHash(0x81319130)).unwrap(),
    )
    .unwrap();
    assert_eq!(&pairs[..stock_pairs.len()], stock_pairs.as_slice());
    assert_eq!(pairs.len(), stock_pairs.len() + recipes.len());
    let globals = staged
        .read_tag(resolve_live_named_tag(&staged, "investment_globals", None).unwrap())
        .unwrap();
    let mut roots = BTreeSet::new();
    let mut owners = BTreeSet::new();
    let mut receipt = Vec::new();
    for recipe in &recipes {
        let subclass = built.iter().find(|item| item.name == recipe.name).unwrap();
        let row = subclass.entry_rows[&entry];
        assert!(usize::from(row) >= stock_pairs.len());
        let root = subclass.entry_entities[&entry];
        assert!(stock.get_entry(TagHash(root)).is_none());
        assert!(roots.insert(root));
        let actual = staged.read_tag(TagHash(root)).unwrap();
        let (count, _, first, _) = array_at(&actual, 16).unwrap();
        let mut remap = BTreeMap::from([(root, source_root)]);
        for index in 0..count {
            let original = read_u32(&graph, rows + index * 12).unwrap();
            let owner = read_u32(&actual, first + index * 12).unwrap();
            assert!(stock.get_entry(TagHash(owner)).is_none());
            if remap.insert(owner, original).is_none() {
                assert!(owners.insert(owner));
            }
        }
        assert_eq!(remap.len(), originals.len());
        for (&tag, &original) in &remap {
            let bytes = staged.read_tag(TagHash(tag)).unwrap();
            let mut normalized = bytes.clone();
            for at in (0..normalized.len().saturating_sub(3)).step_by(4) {
                if let Some(value) = remap.get(&read_u32(&normalized, at).unwrap()) {
                    normalized[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
            }
            assert_eq!(normalized, originals[&original]);
            fs::write(output.join(format!("{tag:08X}.bin")), bytes).unwrap();
        }
        let [perk] = subclass.entry_perks[&entry].as_slice() else {
            panic!("only the private event perk")
        };
        let action =
            load_sandbox_perk_runtime_action(&staged, &globals, usize::from(*perk)).unwrap();
        let decoded = action::decode(&action.action_payload).unwrap();
        let spawn = decoded.effects().find(|effect| effect.kind == 3).unwrap();
        let wrapper = read_u32(&action.action_payload, spawn.offset + 16).unwrap();
        let wrapper = staged.read_tag(TagHash(wrapper)).unwrap();
        let (_, _, first, _) = array_at(&wrapper, 16).unwrap();
        let owner = staged
            .read_tag(TagHash(read_u32(&wrapper, first).unwrap()))
            .unwrap();
        let definition = usize::try_from(read_u64(&owner, 24).unwrap()).unwrap() + 24;
        let child = read_u32(&owner, definition + 0x50).unwrap();
        assert_ne!(child, old_child);
        let child_bytes = staged.read_tag(TagHash(child)).unwrap();
        let references = ability_reference::references(&child_bytes).unwrap();
        assert_eq!(
            references.iter().map(|(_, key)| *key).collect::<Vec<_>>(),
            [root]
        );
        fs::write(output.join(format!("{child:08X}.bin")), &child_bytes).unwrap();
        receipt.push(
            json!({"item":subclass.hash,"row":row,"identity":pairs[usize::from(row)].identity,
            "pattern":pairs[usize::from(row)].pattern,"root":root,"root_sha256":sha(&actual),
            "child_action":child,"child_sha256":sha(&child_bytes),"ability_references":references}),
        );
    }
    for (&tag, bytes) in &originals {
        assert_eq!(&staged.read_tag(TagHash(tag)).unwrap(), bytes);
    }
    assert_eq!(
        staged.read_tag(TagHash(donor_root)).unwrap(),
        stock.read_tag(TagHash(donor_root)).unwrap()
    );
    let path = directory.join(format!("{}.bin", symbol(bank)));
    let original = fs::read(&path).unwrap();
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::write(&path, changed).unwrap();
    let rejected = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {});
    fs::write(&path, original).unwrap();
    assert!(
        rejected
            .err()
            .unwrap()
            .to_string()
            .contains("changed after preparation")
    );
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({
        "revision":revision,"native_build":"86657.20.08.23.1800.d2_rc","source_packages":packages,
        "manifest":build.manifest_path,"fixture_root":source_root,"donor_root":donor_root,"readback":receipt,
        "stock_unchanged":true,"changed_asset_rejected":true,"executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
        "installed":false,"gameplay_verified":false
    })).unwrap()).unwrap();
}
