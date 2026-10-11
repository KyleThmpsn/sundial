//! Saved Barrel controls through fresh packages and independent geometry/pool readback.
//! Written before implementation. Failure model: stale donor defaults, accumulated scaling,
//! missing native headers, inconsistent ring totals, insufficient final capacity, stock mutation,
//! invalid numeric settings accepted, or reset retaining a private pattern.
use super::*;
use crate::weapon::barrel::{Edits, Ring};

fn geometry(manager: &PackageManager, entity: &[u8]) -> (u32, Vec<u8>, [f32; 2]) {
    let (pellets, rows) = spread(manager, entity);
    if rows.is_empty() {
        return (pellets, rows, [0.0; 2]);
    }
    let binding = weapon_component_bindings(entity, BARREL).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    let pattern = relative(&owner, definition + 0xE60);
    (
        pellets,
        rows,
        [0x58, 0x5C].map(|offset| f32::from_bits(read_u32(&owner, pattern + offset).unwrap())),
    )
}

fn ring(pellets: u16, inner: f32, outer: f32, rotation: f32, randomness: f32) -> Ring {
    Ring {
        pellets,
        inner_radius_bits: inner.to_bits(),
        outer_radius_bits: outer.to_bits(),
        rotation_bits: rotation.to_bits(),
        randomness_bits: randomness.to_bits(),
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "The persisted package workflow retains independent geometry, capacity and stock readback together"
)]
fn saved_barrel_controls_emit_geometry_and_capacity_without_changing_stock() {
    let packages = crate::test_support::stock_packages();
    let output = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_ARTIFACTS").unwrap())
        .join("barrel-controls");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("fresh artifacts");
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(result.status.success());
        result.stdout
    };
    let revision = git(&["rev-parse", "HEAD"]);
    fs::write(
        output.join("source.diff"),
        git(&["diff", "--binary", "HEAD"]),
    )
    .unwrap();
    for (name, source) in [
        (
            "native-spread.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../src/entity/spread.rs"
            )),
        ),
        ("barrel.rs", include_str!("../../../../weapon/barrel.rs")),
        (
            "compiler-barrel.rs",
            include_str!("../../../custom_runtime/barrel.rs"),
        ),
        ("controls.rs", include_str!("controls.rs")),
    ] {
        fs::write(output.join(name), source).unwrap();
    }
    let stock = open_manager(&packages).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    let named = |name: &str| {
        donors
            .iter()
            .find(|d| d.name.replace('\u{2019}', "'") == name)
            .unwrap_or_else(|| panic!("required donor {name}"))
    };
    let thorn = named("Thorn");
    let shotgun = named("Felwinter's Lie");
    let tractor = named("Tractor Cannon");
    let source = |donor: &sundial::investment::WeaponDonorSummary| {
        load_weapon_runtime_entity_at_pattern_index_with_manager(
            &stock,
            donor.weapon_pattern_index.unwrap(),
        )
        .unwrap()
    };
    let shotgun_source = source(shotgun);
    let original = geometry(&stock, &shotgun_source.payload);
    assert_eq!(original.0, 12);
    assert_eq!(
        original
            .1
            .chunks_exact(20)
            .map(|r| read_u32(r, 12).unwrap())
            .collect::<Vec<_>>(),
        [1, 4, 7]
    );
    let thorn_source = source(thorn);
    assert!(geometry(&stock, &thorn_source.payload).1.is_empty());
    let make = |label: &str,
                donor: Option<&sundial::investment::WeaponDonorSummary>,
                edits: Option<Edits>| {
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.barrel-controls.{label}"),
            thorn.hash,
            &thorn.name,
        )
        .unwrap();
        recipe.set_component_splice(
            BARREL,
            donor.map(|donor| crate::WeaponDonorReference {
                item_hash: donor.hash.into(),
                expected_name: Some(donor.name.clone()),
            }),
        );
        recipe.overrides.barrel = edits;
        recipe
    };
    let custom = (0..4)
        .map(|i| ring(1, 1.0, 1.0, i as f32 * std::f32::consts::FRAC_PI_2, 0.0))
        .collect::<Vec<_>>();
    let recipes = [
        make(
            "grow",
            Some(shotgun),
            Some(Edits {
                pellets: Some(18),
                spread_scale_bits: Some(0.5_f32.to_bits()),
                ..Default::default()
            }),
        ),
        make(
            "shrink",
            Some(shotgun),
            Some(Edits {
                pellets: Some(6),
                ..Default::default()
            }),
        ),
        make(
            "new",
            None,
            Some(Edits {
                pellets: Some(8),
                ..Default::default()
            }),
        ),
        make(
            "zero",
            Some(shotgun),
            Some(Edits {
                spread_scale_bits: Some(0.0_f32.to_bits()),
                ..Default::default()
            }),
        ),
        make(
            "shape",
            None,
            Some(Edits {
                rings: Some(custom.clone()),
                ..Default::default()
            }),
        ),
        make("reset", None, None),
        make(
            "changed-donor",
            Some(tractor),
            Some(Edits {
                pellets: Some(7),
                spread_scale_bits: Some(2.0_f32.to_bits()),
                ..Default::default()
            }),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, recipe)| {
        let path = output.join(format!("recipe-{i}.json"));
        recipe.save_json(&path).unwrap();
        crate::WeaponRecipe::load_json(path).unwrap()
    })
    .collect::<Vec<_>>();
    let mut stock_payloads = BTreeMap::new();
    for donor in [thorn, shotgun, tractor] {
        let runtime = source(donor);
        stock_payloads.insert(runtime.entity_tag, runtime.payload.clone());
        let barrel = weapon_component_bindings(&runtime.payload, BARREL).unwrap()[0];
        stock_payloads.insert(
            barrel.owner_tag,
            stock.read_tag(TagHash(barrel.owner_tag)).unwrap(),
        );
        let graph = fired(&stock, &runtime.payload, runtime.weapon_content_group_hash);
        let payload = stock.read_tag(TagHash(graph)).unwrap();
        for binding in weapon_component_bindings(&payload, MOVEMENT).unwrap() {
            stock_payloads.insert(
                binding.owner_tag,
                stock.read_tag(TagHash(binding.owner_tag)).unwrap(),
            );
        }
        stock_payloads.insert(graph, payload);
    }
    let mut runs = Vec::new();
    for run in 0..2 {
        let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: packages.clone(),
            staging_root: output.join(format!("staged-{run}")),
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
        let mut readbacks = Vec::new();
        for (i, recipe) in recipes.iter().enumerate() {
            let authored = load_weapon_runtime_entity_with_manager(
                &manager,
                recipe.identity.item_hash.parse_u32().unwrap(),
            )
            .unwrap();
            let (total, rows, extent) = geometry(&manager, &authored.payload);
            assert_eq!(total, [18, 6, 8, 12, 4, 1, 7][i]);
            if i < 2 {
                let expected_counts = if i == 0 { [1, 6, 11] } else { [1, 2, 3] };
                for (j, (row, source)) in rows
                    .chunks_exact(20)
                    .zip(original.1.chunks_exact(20))
                    .enumerate()
                {
                    assert_eq!(read_u32(row, 12).unwrap(), expected_counts[j]);
                    assert_eq!(&row[..12], &source[..12]);
                    assert_eq!(&row[16..20], &source[16..20]);
                }
                let factor = if i == 0 { 0.5 } else { 1.0 };
                assert_eq!(extent, original.2.map(|value| value * factor));
            }
            if i == 2 {
                assert_eq!(rows.len(), 20);
                assert_eq!(read_u32(&rows, 0).unwrap(), 0.0_f32.to_bits());
                assert_eq!(read_u32(&rows, 4).unwrap(), 1.0_f32.to_bits());
            }
            if i == 3 {
                assert_eq!(extent, [0.0; 2]);
            }
            if i == 4 {
                assert_eq!(rows.len(), 80);
                for (row, expected) in rows.chunks_exact(20).zip(&custom) {
                    assert_eq!(
                        [0, 4, 8, 12, 16].map(|at| read_u32(row, at).unwrap()),
                        [
                            expected.inner_radius_bits,
                            expected.outer_radius_bits,
                            expected.rotation_bits,
                            1,
                            expected.randomness_bits
                        ]
                    );
                }
                assert_eq!(extent[0], extent[1]);
            }
            if i == 5 {
                assert!(rows.is_empty());
            }
            if i == 6 {
                assert_eq!(
                    extent[0],
                    geometry(&stock, &source(tractor).payload).2[0] * 2.0
                );
            }
            // Follow every selectable content block, not just the initial firing slot.
            let content = weapon_component_bindings(&authored.payload, 0x5F0D_D954).unwrap()[0];
            let owner = manager.read_tag(TagHash(content.owner_tag)).unwrap();
            let definition =
                read_u64(&owner, content.resource_offset as usize + 8).unwrap() as usize;
            let mut capacities = BTreeMap::new();
            let mut without_pool = Vec::new();
            for block in crate::weapon::ammo::property_offsets(&owner, definition).unwrap() {
                let graph = read_u32(&owner, block + 0xF0).unwrap();
                if graph != 0 && graph != u32::MAX {
                    let payload = manager.read_tag(TagHash(graph)).unwrap();
                    if weapon_component_bindings(&payload, MOVEMENT)
                        .unwrap()
                        .is_empty()
                    {
                        without_pool.push(graph);
                        continue;
                    }
                    let (capacity, ..) = pool(&manager, graph);
                    assert!(capacity >= total as usize);
                    capacities.insert(graph, capacity);
                }
            }
            assert!(!capacities.is_empty());
            readbacks.push(serde_json::json!({"item": authored.item_hash, "entity": authored.entity_tag, "pellets": total, "rows": rows, "scale_and_extent": extent, "capacities": capacities, "graphs_without_trajectory_pools": without_pool}));
        }
        for (&tag, bytes) in &stock_payloads {
            assert_eq!(
                manager.read_tag(TagHash(tag)).unwrap(),
                *bytes,
                "stock tag {tag:08X}"
            );
        }
        let artifacts = build.artifacts.iter().map(|artifact| {
            let path = build.run_directory.join(&artifact.file_name);
            serde_json::json!({"file": path, "sha256": crate::artifact::digest_file(&path).unwrap().sha256})
        }).collect::<Vec<_>>();
        runs.push(serde_json::json!({"run": run, "artifacts": artifacts, "readback": readbacks}));
    }
    let mut rejected = Vec::new();
    for edits in [
        Edits {
            pellets: Some(0),
            ..Default::default()
        },
        Edits {
            spread_scale_bits: Some(f32::NAN.to_bits()),
            ..Default::default()
        },
        Edits {
            rings: Some(Vec::new()),
            ..Default::default()
        },
        Edits {
            rings: Some(vec![ring(1, 2.0, 1.0, 0.0, 0.0)]),
            ..Default::default()
        },
        Edits {
            rings: Some(vec![ring(1, 0.0, 1.0, 0.0, 1.1)]),
            ..Default::default()
        },
        Edits {
            pellets: Some(2),
            rings: Some(vec![ring(1, 0.0, 1.0, 0.0, 0.0)]),
            ..Default::default()
        },
    ] {
        let recipe = make(&format!("invalid-{}", rejected.len()), None, Some(edits));
        fs::write(
            output.join(format!("invalid-{}.json", rejected.len())),
            serde_json::to_vec_pretty(&recipe).unwrap(),
        )
        .unwrap();
        let result = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: packages.clone(),
            staging_root: output.join(format!("rejected-{}", rejected.len())),
            ignore_installed_authored_overlays: true,
            recipes: vec![recipe],
        })
        .and_then(|snapshot| crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}));
        rejected.push(
            result
                .expect_err("invalid Barrel geometry cannot be staged")
                .to_string(),
        );
    }
    let mut inputs = Vec::new();
    for (tag, payload) in stock_payloads {
        let path = output.join(format!("input-{tag:08X}.bin"));
        fs::write(&path, payload).unwrap();
        inputs.push(serde_json::json!({"tag": tag, "file": path, "sha256": crate::artifact::digest_file(&path).unwrap().sha256}));
    }
    let executable = std::env::current_exe().unwrap();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "native_build": "86657.20.08.23.1800.d2_rc___release", "inputs": inputs,
        "revision": String::from_utf8_lossy(&revision).trim(), "working_changes": "source.diff",
        "executable": executable, "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
        "repeat_command": "cargo test --release -p parhelion saved_barrel_controls_emit_geometry_and_capacity_without_changing_stock -- --ignored --nocapture",
        "runs": runs, "rejected": rejected,
        "limits": "Native package readback. In-game activation, repeated firing, orientation, damage and ammo cost remain unverified."
    })).unwrap()).unwrap();
}
