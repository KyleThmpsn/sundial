//! Shared-pattern behavior sources through saved recipes, packaging and native label readback.
//! Failure cases recorded before the reader change: the graft silently drops source labels,
//! replaces the host's type label, reads another content group, or modifies a stock owner.
use super::*;
use sundial::package_authoring::runtime::{
    WeaponRuntimeEntitySource, load_weapon_runtime_entity_at_pattern_index_with_manager,
    load_weapon_runtime_entity_with_manager,
};

/// Follow the selected content block and its native label array without the graft compiler.
fn labels(manager: &PackageManager, source: &WeaponRuntimeEntitySource) -> (u32, Vec<[u8; 24]>) {
    let bindings = weapon_component_bindings(&source.payload, 0x5F0D_D954).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one weapon-content component")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    let mut blocks = vec![definition + 0x80];
    if read_u64(&owner, definition + 0x240).unwrap() != 0 {
        let (count, _, rows, class) = array_at(&owner, definition + 0x240).unwrap();
        assert_eq!(class, 0x8080_3ACF);
        blocks.extend((0..count).map(|index| rows + index * 0x1C0));
    }
    let block = blocks
        .iter()
        .copied()
        .find(|at| read_u32(&owner, at + 0x10).unwrap() == source.weapon_content_group_hash)
        .unwrap_or(blocks[0]);
    let end = block + read_u32(&owner, block + 8).unwrap() as usize;
    let array = |slot: usize, expected: u32| {
        let header = relative_target(&owner, slot).ok()?;
        (header >= 4
            && read_u32(&owner, header - 4).ok() == Some(0x8080_9FBD)
            && read_u32(&owner, header + 8).ok() == Some(expected))
        .then_some(header)
    };
    for slot in (block + 0x20..=end - 0x30).step_by(8) {
        if let Some(header) = array(slot, 0x8080_94B3)
            && array(slot + 0x10, 0x8080_94B0).is_some()
        {
            let count = read_u64(&owner, slot + 8).unwrap() as usize;
            assert_eq!(read_u64(&owner, header).unwrap(), count as u64);
            let rows = owner[header + 16..header + 16 + count * 24]
                .chunks_exact(24)
                .map(|row| row.try_into().unwrap())
                .collect();
            return (binding.owner_tag, rows);
        }
    }
    panic!("required native label array")
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
fn shared_pattern_behavior_labels_survive_packaging() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("shared-pattern-labels");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let stock = open_manager(&packages).unwrap();
    let source = |hash| {
        let index = catalog
            .weapon_donor(hash)
            .unwrap()
            .summary
            .weapon_pattern_index
            .unwrap();
        load_weapon_runtime_entity_at_pattern_index_with_manager(&stock, index).unwrap()
    };
    let base = source(0xA25B_8F8F);
    let (base_owner, base_labels) = labels(&stock, &base);
    let mut recipes = Vec::new();
    let mut donors = Vec::new();
    for (behavior, item, required) in [
        ("arbalest-graph", 0x7EF6_3891, 0x2E65_81A4),
        ("legend-of-acrius-graph", 0x67F5_15B2, 0x2E65_81A5),
    ] {
        let donor = source(item);
        assert_ne!(donor.item_hash, item, "shared-pattern source fixture");
        let (owner, rows) = labels(&stock, &donor);
        assert!(
            rows.iter()
                .skip(1)
                .any(|row| read_u32(row, 0).unwrap() == required)
        );
        assert!(
            !base_labels
                .iter()
                .any(|row| read_u32(row, 0).unwrap() == required)
        );
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.shared-pattern-labels.{behavior}"),
            0xA25B_8F8F,
            "Arc Logic",
        )
        .unwrap();
        recipe.overrides.skip_behavior_perks = true;
        recipe.overrides.additional_behaviors = vec![crate::recipe::AdditionalBehaviorRecipe {
            behavior: behavior.into(),
        }];
        let path = output.join(format!("{behavior}.json"));
        recipe.save_json(&path).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(path).unwrap());
        donors.push((item, donor.item_hash, owner, rows));
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staged"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = crate::workflow::FilteredPackageView::create(&packages, &[]).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let manager = open_manager(view.path()).unwrap();
    let mut readback = Vec::new();
    for (recipe, (item, pattern_item, donor_owner, source_labels)) in recipes.iter().zip(donors) {
        let authored_item = recipe.identity.item_hash.parse_u32().unwrap();
        let authored = load_weapon_runtime_entity_with_manager(&manager, authored_item).unwrap();
        let (owner, actual) = labels(&manager, &authored);
        assert_eq!(
            actual.first(),
            base_labels.first(),
            "the host keeps its type"
        );
        for expected in base_labels.iter().chain(source_labels.iter().skip(1)) {
            assert!(
                actual.contains(expected),
                "the host and source labels survive packaging"
            );
        }
        for tag in [base_owner, donor_owner] {
            let original = stock.read_tag(TagHash(tag)).unwrap();
            assert_eq!(
                manager.read_tag(TagHash(tag)).unwrap(),
                original,
                "stock owner"
            );
            fs::write(output.join(format!("stock-{tag:08X}.bin")), original).unwrap();
        }
        let payload = manager.read_tag(TagHash(owner)).unwrap();
        fs::write(output.join(format!("authored-{owner:08X}.bin")), payload).unwrap();
        readback.push(serde_json::json!({
            "donor_item": item, "pattern_item": pattern_item, "source_owner": donor_owner,
            "authored_item": authored_item, "authored_entity": authored.entity_tag,
            "authored_owner": owner, "labels": actual, "stock_owners_unchanged": true,
        }));
    }
    let artifacts = build.artifacts.iter().map(|artifact| {
        let path = build.run_directory.join(&artifact.file_name);
        serde_json::json!({"file": path, "sha256": crate::artifact::digest_file(&path).unwrap().sha256})
    }).collect::<Vec<_>>();
    let executable = std::env::current_exe().unwrap();
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "native_build": "86657.20.08.23.1800.d2_rc", "packages": packages,
        "revision": String::from_utf8_lossy(&revision.stdout).trim(),
        "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
        "recipes": recipes, "artifacts": artifacts, "readback": readback,
        "repeat_command": "cargo test --release -p parhelion shared_pattern_behavior_labels_survive_packaging -- --ignored --nocapture",
        "limits": "Package readback only. Borrowed perk activation and damage require gameplay verification."
    })).unwrap()).unwrap();
}
