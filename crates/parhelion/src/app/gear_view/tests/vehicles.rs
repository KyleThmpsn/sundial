//! Sparrow editor choices saved, staged and independently read through real packages.
use super::*;
use crate::vehicle::{Sparrow, Summon};
use std::collections::BTreeSet;

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn pointer(data: &[u8], at: usize) -> usize {
    (at as i64 + i64::from_le_bytes(data[at..at + 8].try_into().unwrap())) as usize
}

fn array(data: &[u8], at: usize, stride: usize) -> &[u8] {
    let count = u64::from_le_bytes(data[at..at + 8].try_into().unwrap()) as usize;
    if count == 0 {
        return &[];
    }
    let header = pointer(data, at + 8);
    assert_eq!(
        u64::from_le_bytes(data[header..header + 8].try_into().unwrap()) as usize,
        count
    );
    &data[header + 16..header + 16 + count * stride]
}

// This oracle reads the packed program directly, independent of the authoring writer.
fn evaluate(data: &[u8], expression: usize, engine: f32) -> f32 {
    let code = array(data, expression + 16, 1);
    let constants = array(data, expression + 32, 16);
    let mut stack = Vec::new();
    let mut at = 0;
    loop {
        let op = code[at];
        at += 1;
        match op {
            0x34 => {
                let index = usize::from(code[at]);
                at += 1;
                stack.push(f32::from_bits(word(constants, index * 16)));
            }
            0x3C => {
                assert_eq!(code[at], 1);
                at += 1;
                stack.push(engine);
            }
            1 | 3 => {
                let right = stack.pop().unwrap();
                let left = stack.pop().unwrap();
                stack.push(if op == 1 { left + right } else { left * right });
            }
            0x3E => {
                assert_eq!(code[at], 0);
                assert_eq!(at + 1, code.len());
                assert_eq!(stack.len(), 1);
                return stack[0];
            }
            _ => panic!("Unexpected motion instruction {op:02X}"),
        }
    }
}

#[test]
#[ignore = "Requires clean PARHELION_DEFAULT_WEAPONS_PACKAGES and fresh PARHELION_VEHICLE_ARTIFACTS on the same volume"]
#[allow(
    clippy::cognitive_complexity,
    reason = "Keep the editor workflow and independent packed readback together"
)]
fn sparrow_controls_stage_private_speed_and_complete_vehicle_graphs() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let output = PathBuf::from(std::env::var_os("PARHELION_VEHICLE_ARTIFACTS").unwrap());
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(output.join("recipes")).unwrap();
    fs::create_dir_all(output.join("motion")).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let source = open_shadowkeep_package_manager(&packages).unwrap();
    let donors = stock.gear_donors(ItemKind::Sparrow.bucket_hashes());
    let ordinary = donors.iter().find(|d| d.name == "Micro Mini").unwrap();
    let fixed = donors.iter().find(|d| d.name == "Always on Time").unwrap();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        ..Default::default()
    };
    app.install_catalog(crate::test_support::catalog(packages.parent().unwrap()).unwrap());
    let ctx = context();
    let mut cases = Vec::new();
    for (name, base, speed, preset, target) in [
        ("Fast Sparrow", ordinary, 200, "Sparrow", 0x8152099A),
        ("Slow Sparrow", ordinary, 50, "Sparrow", 0x8152099A),
        ("Fast Always on Time", fixed, 200, "Sparrow", 0x815209EA),
        ("Summon Pike", ordinary, 100, "Pike", 0x80C0D9FA),
        ("Summon Heavy Pike", ordinary, 100, "Heavy Pike", 0x80C0C1EC),
        (
            "Summon Interceptor",
            ordinary,
            100,
            "Interceptor",
            0x80C0D7A6,
        ),
        (
            "Summon Super Interceptor",
            ordinary,
            100,
            "Super Interceptor",
            0x80BC9A34,
        ),
        ("Summon Tank", ordinary, 100, "Tank", 0x80BFAF2D),
        ("Fast Summoned Pike", ordinary, 200, "Pike", 0x80C0D9FA),
        (
            "Fast Summoned Interceptor",
            ordinary,
            200,
            "Interceptor",
            0x80C0D7A6,
        ),
    ] {
        new_from_menu(&ctx, &mut app, ItemKind::Sparrow);
        app.recipe.set_donor(base.hash, base.name.clone());
        app.edit_weapon_name(name.to_owned());
        let page = settle(&ctx, &mut app);
        assert!(texts(&page).iter().any(|(s, _)| s == "Driving Speed"));
        if preset != "Sparrow" {
            click(
                &ctx,
                &mut app,
                find(&page, "Sparrow", |s, _| s == "Sparrow"),
            );
            let menu = settle(&ctx, &mut app);
            click(&ctx, &mut app, find(&menu, preset, |s, _| s == preset));
        }
        if speed != 100 {
            let label = if speed == 200 { "2×" } else { "0.5×" };
            let page = settle(&ctx, &mut app);
            click(&ctx, &mut app, find(&page, label, |s, _| s == label));
        }
        let settings = app.recipe.overrides.sparrow.as_ref().unwrap();
        assert_eq!(settings.speed_percent, speed);
        let page = settle(&ctx, &mut app);
        capture::write(&ctx, &page, name);
        let path = output
            .join("recipes")
            .join(format!("{}.json", app.recipe.namespace));
        fs::write(&path, app.recipe.to_json_pretty().unwrap()).unwrap();
        let recipe = WeaponRecipe::load_json(path).unwrap();
        assert_eq!(recipe.overrides.sparrow, app.recipe.overrides.sparrow);
        cases.push((recipe, target, speed));
    }
    // A custom entity uses the same whole-graph path as presets, including a vehicle absent
    // from the stock item pattern table.
    let mut custom = cases[3].0.clone();
    custom.rename_authored_item("Summon Custom Pike").unwrap();
    custom.overrides.sparrow = Some(Sparrow {
        summon: Summon::Other {
            entity: crate::HexHash::new(0x80FDFB51),
        },
        speed_percent: 100,
    });
    let encoded = custom.to_json_pretty().unwrap();
    fs::write(output.join("recipes/custom-pike.json"), &encoded).unwrap();
    cases.push((
        WeaponRecipe::from_json_str(&encoded).unwrap(),
        0x80FDFB51,
        100,
    ));
    let mut invalid = cases[0].0.clone();
    invalid.overrides.sparrow.as_mut().unwrap().speed_percent = 0;
    assert!(invalid.to_spec().is_err());
    invalid.overrides.sparrow.as_mut().unwrap().speed_percent = 200;
    invalid.kind = ItemKind::Ship;
    assert!(invalid.to_spec().is_err());
    invalid = cases[7].0.clone();
    invalid.overrides.sparrow.as_mut().unwrap().speed_percent = 200;
    assert!(invalid.to_spec().is_err());

    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: cases.iter().map(|(recipe, ..)| recipe.clone()).collect(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = output.join("view");
    staged_view(&packages, &build, &view);
    let staged =
        InvestmentCatalog::load_with_cache_path(&view, &output.join("catalog.json"), true, |_| {})
            .unwrap();
    let manager = open_shadowkeep_package_manager(&view.join("packages")).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let patterns = manager
        .read_tag(TagHash(word(&globals, 16 + 70 * 16)))
        .unwrap();
    let assignments = manager.read_tag(TagHash(0x80EC3F60)).unwrap();
    let dependencies = crate::shared_tag_dependency_index::dependencies(
        &manager.read_tag(TagHash(0x80EE8CBD)).unwrap(),
        TagHash(0x80EE8CBD),
        TagHash(0x80EC3F62),
    )
    .unwrap();
    let mut private_entities = BTreeSet::new();
    let mut receipt = Vec::new();
    for (recipe, donor, percent) in &cases {
        let hash = recipe.identity.item_hash.parse_u32().unwrap();
        let definition = manager
            .read_tag(TagHash(staged.item_definition_tag(hash).unwrap()))
            .unwrap();
        let translation = pointer(&definition, 0x88);
        let index = u16::from_le_bytes(
            definition[translation + 0x58..translation + 0x5A]
                .try_into()
                .unwrap(),
        );
        let row = &array(&patterns, 8, 48)[usize::from(index) * 48..][..48];
        let global = word(row, 4);
        assert_eq!(
            global,
            recipe
                .identity
                .pattern_global_id_hash
                .as_ref()
                .unwrap()
                .parse_u32()
                .unwrap()
        );
        let assigned = array(&assignments, 8, 8)
            .chunks_exact(8)
            .find(|r| word(r, 0) == global)
            .unwrap();
        let entity_tag = word(assigned, 4);
        assert_ne!(entity_tag, *donor);
        assert!(private_entities.insert(entity_tag));
        assert!(dependencies.contains(&entity_tag));
        let entity = manager.read_tag(TagHash(entity_tag)).unwrap();
        assert_eq!(
            u16::from_le_bytes(entity[0x96..0x98].try_into().unwrap()),
            15
        );
        let original = source.read_tag(TagHash(*donor)).unwrap();
        let graph =
            load_weapon_runtime_graph_for_entity(&manager, hash, global, entity_tag, &entity)
                .unwrap();
        let old_graph =
            load_weapon_runtime_graph_for_entity(&source, 0, 0, *donor, &original).unwrap();
        // Complete donor topology, seats, weapons, provider bindings and event wiring are kept.
        assert_eq!(graph.bindings.len(), old_graph.bindings.len());
        for old in &old_graph.resources {
            let actual = graph
                .resources
                .iter()
                .find(|r| {
                    r.binding_hash == old.binding_hash && r.resource_index == old.resource_index
                })
                .unwrap();
            assert_eq!(actual.concrete_class, old.concrete_class);
            assert_eq!(actual.alias_bindings, old.alias_bindings);
            assert!(
                dependencies.contains(&actual.owner_tag),
                "Owner {:08X} missing from loading index",
                actual.owner_tag
            );
            if *percent == 100 || old.instance.schema != 0x80803CDE {
                assert_eq!(actual.owner_tag, old.owner_tag);
            }
        }
        let mut speeds = Vec::new();
        let mut motion_readback = None;
        if *percent != 100 {
            let old_motion = old_graph
                .resources
                .iter()
                .find(|r| r.instance.schema == 0x80803CDE)
                .unwrap();
            let motion = graph
                .resources
                .iter()
                .find(|r| r.instance.schema == 0x80803CDE)
                .unwrap();
            assert_ne!(motion.owner_tag, old_motion.owner_tag);
            let payload = manager.read_tag(TagHash(motion.owner_tag)).unwrap();
            let old_payload = source.read_tag(TagHash(old_motion.owner_tag)).unwrap();
            assert_eq!(
                u64::from_le_bytes(payload[..8].try_into().unwrap()) as usize,
                payload.len()
            );
            let root = motion.definition.as_ref().unwrap().owner_offset as usize;
            let old_root = old_motion.definition.as_ref().unwrap().owner_offset as usize;
            let path = output.join("motion").join(format!("{hash:08X}.bin"));
            fs::write(&path, &payload).unwrap();
            motion_readback = Some(
                serde_json::json!({"path": path, "owner": motion.owner_tag, "definition_offset": root}),
            );
            for offset in [0x210, 0x270] {
                assert_eq!(
                    array(&payload, root + offset + 0x40, 40),
                    array(&old_payload, old_root + offset + 0x40, 40)
                );
                for engine in [1.0, 2.0, 3.0, 19.0] {
                    let expected = evaluate(&old_payload, old_root + offset, engine)
                        * f32::from(*percent)
                        / 100.0;
                    let value = evaluate(&payload, root + offset, engine);
                    assert_eq!(value, expected);
                    speeds.push(
                        serde_json::json!({"offset": offset, "engine": engine, "value": value}),
                    );
                }
            }
            assert_eq!(
                manager.read_tag(TagHash(old_motion.owner_tag)).unwrap(),
                old_payload
            );
        } else {
            assert_eq!(entity, original);
        }
        receipt.push(serde_json::json!({"name": recipe.name, "item": hash, "pattern": index,
            "global": global, "entity": entity_tag, "donor": donor, "speed_percent": percent,
            "resources": graph.resources.len(), "evaluations": speeds, "motion_readback": motion_readback}));
    }
    invalid = cases[0].0.clone();
    invalid.overrides.sparrow = Some(Sparrow {
        summon: Summon::Other {
            entity: crate::HexHash::new(0x81613D27),
        },
        speed_percent: 100,
    });
    let error = crate::item::build_weapon_project(
        &packages,
        &crate::WeaponProjectSpec {
            weapons: vec![invalid.to_spec().unwrap()],
        },
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("vehicle"), "{error}");
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "build": "Shadowkeep 86657.20.08.23.1800.d2_rc___release", "staged": build.run_directory,
        "manifest": build.manifest_path, "cases": receipt,
        "gameplay_verified": false, "limits": "Summoning, driver attachment, speed, boost, weapons and cleanup need an in-game test."
    })).unwrap()).unwrap();
}
