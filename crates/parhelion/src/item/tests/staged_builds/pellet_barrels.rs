//! Persisted recipes through compilation, packaging and independent native readback.
//! Failure cases written before the compiler change:
//! - a copied spread pointer reaches stock or unrelated data, or loses its 20-byte ring rows
//! - instance and definition pool counts disagree, or a trajectory loses its reciprocal handle
//! - capacity follows the old barrel, misses an unchanged firing graph, or shrinks a larger pool
//! - pool growth drops explicit projectile values or changes a stock donor
//! - clearing spread is ignored, or a malformed final spread is treated as a single projectile
//! - a component donor sharing another item's pattern is rejected or supplies the wrong spread
use super::*;
use sundial::package_authoring::{
    entity::WEAPON_BARREL_COMPONENT_KEY as BARREL,
    runtime::{
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager, load_weapon_runtime_graph_for_entity,
    },
    sandbox_perk::entity::projectile::parameters::{self, Kind},
};

const MOVEMENT: u32 = 0x0437_756D;

mod controls;

fn relative(bytes: &[u8], at: usize) -> usize {
    let delta = i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    assert_ne!(delta, 0, "a required native pointer is present");
    at.checked_add_signed(isize::try_from(delta).unwrap())
        .unwrap()
}

/// Decode the native pattern and complete ring rows without the compiler's spread reader.
fn spread(manager: &PackageManager, entity: &[u8]) -> (u32, Vec<u8>) {
    let bindings = weapon_component_bindings(entity, BARREL).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one barrel")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    assert_eq!(binding.concrete_class, 0x8080_3889);
    assert_eq!(read_u32(&owner, instance + 4).unwrap(), 0x8080_3865);
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    let slot = definition + 0xE60;
    if read_u64(&owner, slot).unwrap() == 0 {
        return (1, Vec::new());
    }
    let pattern = relative(&owner, slot);
    assert_eq!(read_u32(&owner, pattern - 4).unwrap(), 0x8080_888D);
    for ordinal in 0..3 {
        let at = pattern + ordinal * 0x18;
        assert_eq!(read_u32(&owner, at).unwrap(), 0x8080_888D);
        assert_eq!(read_u32(&owner, at + 4).unwrap(), ordinal as u32);
        assert_eq!(read_u32(&owner, at + 8).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, at + 12).unwrap(), 0x8080_888D);
        assert_eq!(read_u64(&owner, at + 16).unwrap(), pattern as u64);
    }
    let count = read_u64(&owner, pattern + 0x48).unwrap() as usize;
    let header = relative(&owner, pattern + 0x50);
    assert_eq!(read_u32(&owner, header - 4).unwrap(), 0x8080_9FBD);
    assert_eq!(read_u64(&owner, header).unwrap(), count as u64);
    assert_eq!(read_u32(&owner, header + 8).unwrap(), 0x8080_888F);
    let rows = owner[header + 16..header + 16 + count * 20].to_vec();
    let pellets = read_u32(&owner, pattern + 0x60).unwrap();
    assert_eq!(
        rows.chunks_exact(20)
            .map(|row| read_u32(row, 12).unwrap())
            .sum::<u32>(),
        pellets
    );
    (pellets, rows)
}

fn fired(manager: &PackageManager, entity: &[u8], group: u32) -> u32 {
    let binding = weapon_component_bindings(entity, 0x5F0D_D954).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    let blocks = crate::weapon::ammo::property_offsets(&owner, definition).unwrap();
    let block = blocks
        .iter()
        .copied()
        .find(|at| read_u32(&owner, at + 0x10).unwrap() == group)
        .unwrap_or(blocks[0]);
    read_u32(&owner, block + 0xF0).unwrap()
}

/// Follow every trajectory and handle in the packed owner, including both reset counters.
fn pool(manager: &PackageManager, graph: u32) -> (usize, [u32; 2]) {
    let entity = manager.read_tag(TagHash(graph)).unwrap();
    let bindings = weapon_component_bindings(&entity, MOVEMENT).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one moving projectile")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    let count = read_u64(&owner, instance + 0x1A8).unwrap() as usize;
    let header = relative(&owner, instance + 0x1B0);
    assert_eq!(read_u32(&owner, header - 4).unwrap(), 0x8080_9FBD);
    assert_eq!(read_u64(&owner, header).unwrap(), count as u64);
    assert_eq!(read_u32(&owner, header + 8).unwrap(), 0x8080_37BA);
    assert_eq!(read_u32(&owner, instance + 0x1C0).unwrap() as usize, count);
    assert_eq!(read_u64(&owner, definition + 0x80).unwrap() as usize, count);
    let mut handles = BTreeSet::new();
    for i in 0..count {
        let row = header + 16 + i * 0x210;
        assert_eq!(read_u32(&owner, row).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, row + 4).unwrap(), 0x8080_37BB);
        let handle = read_u64(&owner, row + 8).unwrap() as usize;
        assert!(handles.insert(handle));
        assert_eq!(relative(&owner, row + 0x10), instance);
        assert_eq!(read_u32(&owner, handle).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, handle + 4).unwrap(), 0x8080_37BA);
        assert_eq!(read_u64(&owner, handle + 8).unwrap(), row as u64);
    }
    (
        count,
        [
            read_u32(&owner, instance + 0x144).unwrap(),
            read_u32(&owner, definition + 0x88).unwrap(),
        ],
    )
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "The staged workflow keeps its independent native readback and source-preservation checks together"
)]
fn composed_pellet_barrels_and_final_projectiles_survive_packaging() {
    let packages = crate::test_support::stock_packages();
    let output = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_TEST_ARTIFACTS").expect("artifact directory"),
    )
    .join("pellet-barrels");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    let changes = std::process::Command::new("git")
        .args(["diff", "--binary", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(changes.status.success());
    fs::write(output.join("source.diff"), changes.stdout).unwrap();
    // Preserve new source files too, which a working-tree diff omits until they are tracked.
    for (name, source) in [
        (
            "spread.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../src/entity/spread.rs"
            )),
        ),
        ("firing.rs", include_str!("../../custom_runtime/firing.rs")),
        ("pellet_barrels.rs", include_str!("pellet_barrels.rs")),
    ] {
        fs::write(output.join(format!("source-{name}")), source).unwrap();
    }
    let stock = open_manager(&packages).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    let named = |name: &str| {
        donors
            .iter()
            .find(|donor| {
                donor.name.replace('\u{2019}', "'") == name && donor.weapon_pattern_index.is_some()
            })
            .unwrap_or_else(|| panic!("required donor {name}"))
    };
    let hand = named("IKELOS_HC_v1.0.2");
    let shotgun = named("Felwinter's Lie");
    let shared = named("Baligant XU7743");
    let thorn = named("Thorn");
    let wolves = named("Lord of Wolves");
    let runtime = |donor: &sundial::investment::WeaponDonorSummary| {
        load_weapon_runtime_entity_at_pattern_index_with_manager(
            &stock,
            donor.weapon_pattern_index.unwrap(),
        )
        .unwrap()
    };
    let hand_runtime = runtime(hand);
    let shotgun_runtime = runtime(shotgun);
    let shared_runtime = runtime(shared);
    assert_ne!(
        shared.hash, shared_runtime.item_hash,
        "this donor must exercise an item whose selector names another pattern identity"
    );
    let thorn_runtime = runtime(thorn);
    let wolves_runtime = runtime(wolves);
    let expected_spread = spread(&stock, &shotgun_runtime.payload);
    let shared_spread = spread(&stock, &shared_runtime.payload);
    assert!(expected_spread.0 > 1 && !expected_spread.1.is_empty());
    assert!(shared_spread.0 > 1 && !shared_spread.1.is_empty());
    let thorn_graph = fired(
        &stock,
        &thorn_runtime.payload,
        thorn_runtime.weapon_content_group_hash,
    );
    let wolves_graph = fired(
        &stock,
        &wolves_runtime.payload,
        wolves_runtime.weapon_content_group_hash,
    );
    assert!(pool(&stock, thorn_graph).0 < expected_spread.0 as usize);
    let wolves_capacity = pool(&stock, wolves_graph).0;
    assert!(wolves_capacity >= expected_spread.0 as usize);
    let graph_payload = stock.read_tag(TagHash(thorn_graph)).unwrap();
    let graph =
        load_weapon_runtime_graph_for_entity(&stock, 0, 0, thorn_graph, &graph_payload).unwrap();
    let mut speed = Vec::new();
    parameters::discover(&graph)
        .iter()
        .find(|parameter| parameter.kind == Kind::Speed)
        .unwrap()
        .set(&mut speed, 3.25)
        .unwrap();
    let make = |label: &str,
                host: &sundial::investment::WeaponDonorSummary,
                barrel: &sundial::investment::WeaponDonorSummary,
                behavior: Option<&str>| {
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.pellets.{label}"),
            host.hash,
            &host.name,
        )
        .unwrap();
        recipe.set_component_splice(
            BARREL,
            Some(crate::WeaponDonorReference {
                item_hash: barrel.hash.into(),
                expected_name: Some(barrel.name.clone()),
            }),
        );
        recipe.overrides.skip_behavior_perks = true;
        recipe.overrides.additional_behaviors = behavior
            .into_iter()
            .map(|behavior| crate::recipe::AdditionalBehaviorRecipe {
                behavior: behavior.into(),
            })
            .collect();
        recipe
    };
    let mut edited = make("typed", hand, shotgun, Some("thorn-graph"));
    edited.overrides.projectile = Some(crate::weapon::projectile::Edits {
        graph: thorn_graph,
        values: speed,
    });
    let mut cleared = make("cleared", hand, shotgun, Some("thorn-graph"));
    let binding = weapon_component_bindings(&hand_runtime.payload, BARREL).unwrap()[0];
    let owner = stock.read_tag(TagHash(binding.owner_tag)).unwrap();
    let slot = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize + 0xE60;
    cleared.overrides.runtime_resource_patches.push(
        crate::recipe::WeaponRuntimeResourcePatchRecipe {
            binding_hash: BARREL.into(),
            resource_index: 0,
            offset: (slot - binding.resource_offset as usize) as u32,
            bytes: "00 00 00 00 00 00 00 00".into(),
            graph_values: Vec::new(),
        },
    );
    let recipes = vec![
        edited,
        make("sufficient", hand, shotgun, Some("lord-of-wolves-graph")),
        make("base", thorn, shotgun, None),
        make("removed", shotgun, hand, Some("thorn-graph")),
        cleared,
        make("shared-pattern", hand, shared, Some("thorn-graph")),
    ];
    let recipes = recipes
        .into_iter()
        .enumerate()
        .map(|(i, recipe)| {
            let path = output.join(format!("recipe-{i}.json"));
            recipe.save_json(&path).unwrap();
            crate::WeaponRecipe::load_json(path).unwrap()
        })
        .collect::<Vec<_>>();
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
    for (i, recipe) in recipes.iter().enumerate() {
        let item = recipe.identity.item_hash.parse_u32().unwrap();
        let authored = load_weapon_runtime_entity_with_manager(&manager, item).unwrap();
        let actual = spread(&manager, &authored.payload);
        assert_eq!(
            actual,
            if i < 3 {
                expected_spread.clone()
            } else if i == 5 {
                shared_spread.clone()
            } else {
                (1, Vec::new())
            }
        );
        let graph = fired(
            &manager,
            &authored.payload,
            authored.weapon_content_group_hash,
        );
        let (capacity, speeds) = pool(&manager, graph);
        let expected_capacity = if i == 1 {
            wolves_capacity
        } else if i < 3 {
            expected_spread.0 as usize
        } else if i == 5 {
            (shared_spread.0 as usize).max(pool(&stock, thorn_graph).0)
        } else {
            pool(&stock, thorn_graph).0
        };
        assert_eq!(capacity, expected_capacity);
        if i == 0 {
            assert_eq!(speeds, [3.25_f32.to_bits(); 2]);
        }
        if i == 0 || i == 2 || i == 5 {
            assert!(
                stock.get_entry(TagHash(graph)).is_none(),
                "growth uses a private graph"
            );
        }
        readback.push(serde_json::json!({"item": item, "entity": authored.entity_tag, "graph": graph, "pellets": actual.0, "rings": actual.1, "capacity": capacity, "speed_bits": speeds}));
    }
    let mut inputs = Vec::new();
    for (label, source) in [
        ("hand", &hand_runtime),
        ("shotgun", &shotgun_runtime),
        ("shared-pattern", &shared_runtime),
        ("thorn", &thorn_runtime),
        ("wolves", &wolves_runtime),
    ] {
        let mut tags = BTreeSet::from([source.entity_tag]);
        for key in [BARREL, 0x5F0D_D954] {
            tags.extend(
                weapon_component_bindings(&source.payload, key)
                    .unwrap()
                    .iter()
                    .map(|binding| binding.owner_tag),
            );
        }
        let graph = fired(&stock, &source.payload, source.weapon_content_group_hash);
        tags.insert(graph);
        let payload = stock.read_tag(TagHash(graph)).unwrap();
        tags.extend(
            weapon_component_bindings(&payload, MOVEMENT)
                .unwrap()
                .iter()
                .map(|binding| binding.owner_tag),
        );
        for tag in tags {
            let payload = stock.read_tag(TagHash(tag)).unwrap();
            assert_eq!(
                manager.read_tag(TagHash(tag)).unwrap(),
                payload,
                "stock {label} 0x{tag:08X}"
            );
            let path = output.join(format!("{label}-{tag:08X}.bin"));
            fs::write(&path, payload).unwrap();
            inputs.push(serde_json::json!({"file": path.file_name().unwrap().to_string_lossy(), "sha256": crate::artifact::digest_file(&path).unwrap().sha256}));
        }
    }
    let mut malformed = recipes[4].clone();
    malformed.overrides.runtime_resource_patches[0].bytes = "FF FF FF FF FF FF FF 7F".into();
    malformed.save_json(output.join("malformed.json")).unwrap();
    let bad = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("rejected"),
        ignore_installed_authored_overlays: true,
        recipes: vec![malformed],
    })
    .unwrap();
    let rejection = crate::build_and_stage_snapshot_with_progress(&bad, |_| {})
        .expect_err("an unreadable final spread must fail")
        .to_string();
    let executable = std::env::current_exe().unwrap();
    let artifacts = build.artifacts.iter().map(|artifact| {
        let path = build.run_directory.join(&artifact.file_name);
        serde_json::json!({"file": path, "sha256": crate::artifact::digest_file(&path).unwrap().sha256})
    }).collect::<Vec<_>>();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "native_build": "86657.20.08.23.1800.d2_rc", "packages": packages, "executable": executable,
        "revision_at_run": String::from_utf8_lossy(&revision.stdout).trim(), "working_changes": "source.diff",
        "repeat_command": "cargo test --release -p parhelion composed_pellet_barrels_and_final_projectiles_survive_packaging -- --ignored --nocapture",
        "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
        "inputs": inputs, "artifacts": artifacts, "readback": readback, "malformed_rejection": rejection,
        "shared_pattern_donor": {"item": shared.hash, "pattern_index": shared.weapon_pattern_index, "pattern_item": shared_runtime.item_hash, "entity": shared_runtime.entity_tag},
        "limits": "Package readback only. Pellet damage, rendered rounds and repeated firing require gameplay verification."
    })).unwrap()).unwrap();
}
