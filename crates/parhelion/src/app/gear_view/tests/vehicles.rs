//! Sparrow editor choices saved, staged and independently read through real packages.
use super::*;
use crate::vehicle::{Sparrow, Summon};
use std::collections::BTreeSet;
use sundial::package_authoring::sandbox_perk::action::{self, FactValue};

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

// Native F3F8F0 chooses the boxed value before the fallback. Read independently of the writer.
fn numeric(data: &[u8], config: usize) -> f32 {
    if u64::from_le_bytes(data[config..config + 8].try_into().unwrap()) == 0 {
        return f32::from_bits(word(data, config + 12));
    }
    let value = pointer(data, config);
    match data[value] {
        0 => f32::from_bits(word(data, value + 4)),
        1 => word(data, value + 4) as i32 as f32,
        2 => data[value + 1] as i8 as f32,
        kind => panic!("Unexpected numeric kind {kind}"),
    }
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

/// Picks `label` from the overflow menu of the Vehicle card titled `card`.
fn preset(ctx: &egui::Context, app: &mut PackageAuthoringApp, card: &str, label: &str) {
    let page = settle(ctx, app);
    let menu = format!("More {card} Options");
    let button = crate::test_support::driver::accessible(&page, &menu)
        .unwrap_or_else(|| panic!("the {card} card has its menu"));
    click(ctx, app, button.center());
    let opened = settle(ctx, app);
    click(ctx, app, find(&opened, label, |text, _| text == label));
}

/// Search and use an asset in the existing native picker after discovery completes.
fn pick_asset(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    search_hint: &str,
    query: &str,
    use_label: &str,
) {
    let start = Instant::now();
    while !app.perk_workbench.discovery_settled() {
        settle(ctx, app);
        assert!(
            start.elapsed() < Duration::from_secs(900),
            "Asset discovery timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let page = settle(ctx, app);
    click(
        ctx,
        app,
        find(&page, search_hint, |text, _| text == search_hint),
    );
    frame(ctx, app, vec![egui::Event::Text(query.into())]);
    let page = settle(ctx, app);
    capture::write(ctx, &page, use_label);
    click(
        ctx,
        app,
        find(&page, use_label, |text, _| text == use_label),
    );
}

#[test]
#[ignore = "Requires clean SUNDIAL_INSTALL and fresh SUNDIAL_TEST_ARTIFACTS on the same volume"]
#[allow(
    clippy::cognitive_complexity,
    reason = "Keep the editor workflow and independent packed readback together"
)]
fn sparrow_controls_stage_private_speed_and_complete_vehicle_graphs() {
    let packages = crate::test_support::install().join("packages");
    let output = crate::test_support::artifact_dir("vehicles");
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(output.join("recipes")).unwrap();
    fs::create_dir_all(output.join("motion")).unwrap();
    fs::create_dir_all(output.join("handling")).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let source = open_shadowkeep_package_manager(&packages).unwrap();
    let source_globals = source
        .read_tag(resolve_live_named_tag(&source, "investment_globals", None).unwrap())
        .unwrap();
    let donors = stock.gear_donors(ItemKind::Sparrow.bucket_hashes());
    let ordinary = donors.iter().find(|d| d.name == "Micro Mini").unwrap();
    let fixed = donors.iter().find(|d| d.name == "Always on Time").unwrap();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        perk_workbench: Workbench::offline(),
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
        ("Tuned Sparrow", ordinary, 200, "Sparrow", 0x8152099A),
        ("Tuned Pike", ordinary, 200, "Pike", 0x80C0D9FA),
        (
            "Tuned Interceptor",
            ordinary,
            200,
            "Interceptor",
            0x80C0D7A6,
        ),
        ("Tuned Tank", ordinary, 100, "Tank", 0x80BFAF2D),
    ] {
        new_from_menu(&ctx, &mut app, ItemKind::Sparrow);
        assert!(
            app.recipe.overrides.sparrow.is_none(),
            "{name} starts without the previous draft's vehicle settings"
        );
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
            let label = if speed == 200 {
                "2× Speed"
            } else {
                "0.5× Speed"
            };
            self::preset(&ctx, &mut app, "Driving", label);
        }
        if name.starts_with("Tuned ") {
            if preset != "Tank" {
                self::preset(&ctx, &mut app, "Driving", "Match Speed");
            }
            self::preset(&ctx, &mut app, "Durability", "Resilient");
            if preset == "Sparrow" {
                for label in ["Improved Side Dodges", "Air Control", "Roll Tricks"] {
                    let page = settle(&ctx, &mut app);
                    click(&ctx, &mut app, find(&page, label, |s, _| s == label));
                }
            } else {
                for label in ["Rapid Fire", "Double Damage"] {
                    self::preset(&ctx, &mut app, "Weapons", label);
                }
            }
            let page = settle(&ctx, &mut app);
            click(
                &ctx,
                &mut app,
                find(&page, "Faster Summoning", |s, _| s == "Faster Summoning"),
            );
            if preset == "Interceptor" {
                let page = settle(&ctx, &mut app);
                click(
                    &ctx,
                    &mut app,
                    find(&page, "Original Projectiles", |s, _| {
                        s == "Original Projectiles"
                    }),
                );
                let menu = settle(&ctx, &mut app);
                click(
                    &ctx,
                    &mut app,
                    find(&menu, "Tank Shells", |s, _| s == "Tank Shells"),
                );
                // Keep the named donor shortcut, then take the same firing graph through
                // the native projectile picker. Its identity is checked after staging.
                let page = settle(&ctx, &mut app);
                click(
                    &ctx,
                    &mut app,
                    find(&page, "Tank Shells", |s, _| s == "Tank Shells"),
                );
                let menu = settle(&ctx, &mut app);
                click(
                    &ctx,
                    &mut app,
                    find(&menu, "Choose Other Projectile…", |s, _| {
                        s == "Choose Other Projectile…"
                    }),
                );
                let before = app.recipe.overrides.sparrow.clone();
                pick_asset(
                    &ctx,
                    &mut app,
                    "Search Projectiles or Weapons",
                    "80BFAB5F",
                    "Use Projectile",
                );
                assert_eq!(
                    app.recipe
                        .overrides
                        .sparrow
                        .as_ref()
                        .unwrap()
                        .weapons
                        .projectile,
                    crate::vehicle::Projectile::Other {
                        entity: crate::HexHash::new(0x80BFAB5F)
                    }
                );
                assert_eq!(
                    before.as_ref().unwrap().durability,
                    app.recipe.overrides.sparrow.as_ref().unwrap().durability
                );
            }
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
    // Browse and cancel without changing a preset, then choose a vehicle absent from the
    // stock item pattern table. Save and stage the picker's actual selection.
    app.recipe = cases[3].0.clone();
    app.recipe
        .rename_authored_item("Summon Custom Pike")
        .unwrap();
    let before = app.recipe.overrides.sparrow.clone();
    let page = settle(&ctx, &mut app);
    click(&ctx, &mut app, find(&page, "Pike", |s, _| s == "Pike"));
    let menu = settle(&ctx, &mut app);
    click(
        &ctx,
        &mut app,
        find(&menu, "Choose Other Vehicle…", |s, _| {
            s == "Choose Other Vehicle…"
        }),
    );
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(app.recipe.overrides.sparrow, before);
    let page = settle(&ctx, &mut app);
    click(&ctx, &mut app, find(&page, "Pike", |s, _| s == "Pike"));
    let menu = settle(&ctx, &mut app);
    click(
        &ctx,
        &mut app,
        find(&menu, "Choose Other Vehicle…", |s, _| {
            s == "Choose Other Vehicle…"
        }),
    );
    pick_asset(
        &ctx,
        &mut app,
        "Search Vehicles or Tags",
        "80FDFB51",
        "Use Vehicle",
    );
    assert_eq!(
        app.recipe.overrides.sparrow.as_ref().unwrap().summon,
        Summon::Other {
            entity: crate::HexHash::new(0x80FDFB51)
        }
    );
    let page = settle(&ctx, &mut app);
    capture::write(&ctx, &page, "Custom Vehicle Selected");
    let custom = app.recipe.clone();
    let encoded = custom.to_json_pretty().unwrap();
    fs::write(output.join("recipes/custom-pike.json"), &encoded).unwrap();
    cases.push((
        WeaponRecipe::from_json_str(&encoded).unwrap(),
        0x80FDFB51,
        100,
    ));
    // A tank chosen through the catalog clears a prior hover-speed edit and keeps
    // driving disabled, just as the named Tank shortcut does.
    app.recipe = cases[9].0.clone();
    app.recipe
        .rename_authored_item("Summon Custom Tank")
        .unwrap();
    let page = settle(&ctx, &mut app);
    click(
        &ctx,
        &mut app,
        find(&page, "Interceptor", |s, _| s == "Interceptor"),
    );
    let menu = settle(&ctx, &mut app);
    click(
        &ctx,
        &mut app,
        find(&menu, "Choose Other Vehicle…", |s, _| {
            s == "Choose Other Vehicle…"
        }),
    );
    pick_asset(
        &ctx,
        &mut app,
        "Search Vehicles or Tags",
        "80BFAF2D",
        "Use Vehicle",
    );
    assert_eq!(
        app.recipe.overrides.sparrow.as_ref().unwrap().speed_percent,
        100
    );
    assert_eq!(
        app.recipe.overrides.sparrow.as_ref().unwrap().driving,
        crate::vehicle::Driving::default()
    );
    // Its Driving card offers no speed to change.
    let page = settle(&ctx, &mut app);
    assert!(
        texts(&page)
            .iter()
            .any(|(s, _)| s == "Fixed for this vehicle")
    );
    assert!(
        crate::test_support::driver::accessible(&page, "More Driving Options").is_none()
            && !texts(&page).iter().any(|(s, _)| s == "Driving Speed"),
        "a tank's Driving card has no speed controls"
    );
    capture::write(&ctx, &page, "Custom Tank Selected");
    let encoded = app.recipe.to_json_pretty().unwrap();
    fs::write(output.join("recipes/custom-tank.json"), &encoded).unwrap();
    cases.push((
        WeaponRecipe::from_json_str(&encoded).unwrap(),
        0x80BFAF2D,
        100,
    ));
    let mut invalid = cases[0].0.clone();
    invalid.overrides.sparrow.as_mut().unwrap().speed_percent = 0;
    assert!(invalid.to_spec().is_err());
    // Old speed-only recipes retain their meaning, while unsupported tuning fails validation.
    let mut old = serde_json::to_value(&cases[0].0).unwrap();
    old["overrides"]["sparrow"] = serde_json::json!({"summon": "sparrow", "speed_percent": 200});
    let old = WeaponRecipe::from_json_str(&serde_json::to_string(&old).unwrap()).unwrap();
    let old = old.overrides.sparrow.unwrap();
    assert_eq!(old.speed_percent, 200);
    assert_eq!(old.driving, crate::vehicle::Driving::default());
    assert_eq!(old.durability, crate::vehicle::Durability::default());
    invalid = cases[7].0.clone();
    invalid
        .overrides
        .sparrow
        .as_mut()
        .unwrap()
        .driving
        .boost_percent = 200;
    assert!(invalid.to_spec().is_err());
    invalid = cases[0].0.clone();
    invalid
        .overrides
        .sparrow
        .as_mut()
        .unwrap()
        .weapons
        .damage_percent = 200;
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
        let tuned = recipe.name.starts_with("Tuned ");
        let edited_owners = old_graph
            .resources
            .iter()
            .filter(|r| {
                (r.instance.schema == 0x80803CDE
                    && (*percent != 100 || tuned && *donor != 0x80BFAF2D))
                    || (tuned && matches!(r.instance.schema, 0x80804BEE | 0x80803889))
            })
            .map(|r| r.owner_tag)
            .collect::<BTreeSet<_>>();
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
            if !edited_owners.contains(&old.owner_tag) {
                assert_eq!(actual.owner_tag, old.owner_tag);
            } else {
                assert_ne!(actual.owner_tag, old.owner_tag);
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
                let channels = array(&payload, root + offset + 0x40, 40);
                let old_channels = array(&old_payload, old_root + offset + 0x40, 40);
                assert_eq!(channels.len(), old_channels.len());
                for (channel, old) in channels.chunks_exact(40).zip(old_channels.chunks_exact(40)) {
                    let old_owner = u32::from_le_bytes(old[..4].try_into().unwrap());
                    let expected = if old_owner == old_motion.owner_tag {
                        motion.owner_tag
                    } else {
                        old_owner
                    };
                    assert_eq!(
                        u32::from_le_bytes(channel[..4].try_into().unwrap()),
                        expected
                    );
                    assert_eq!(&channel[4..], &old[4..]);
                }
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
        } else if !recipe.name.starts_with("Tuned ") {
            assert_eq!(entity, original);
        }
        let mut tuning = Vec::new();
        let mut handling = Vec::new();
        if recipe.name.starts_with("Tuned ") {
            let settings = recipe.overrides.sparrow.as_ref().unwrap();
            assert_eq!(settings.durability.health_percent, 200);
            assert_eq!(settings.durability.repair_delay_percent, 50);
            assert_eq!(settings.durability.repair_rate_percent, 200);
            assert!(settings.handling.fast_summon);
            for old in old_graph
                .resources
                .iter()
                .filter(|r| matches!(r.instance.schema, 0x80804BEE | 0x80803889 | 0x80803CDE))
            {
                let actual = graph
                    .resources
                    .iter()
                    .find(|r| {
                        r.binding_hash == old.binding_hash && r.resource_index == old.resource_index
                    })
                    .unwrap();
                if old.instance.schema == 0x80803CDE && *donor == 0x80BFAF2D {
                    continue;
                }
                assert_ne!(actual.owner_tag, old.owner_tag);
                let bytes = manager.read_tag(TagHash(actual.owner_tag)).unwrap();
                let original_bytes = source.read_tag(TagHash(old.owner_tag)).unwrap();
                assert_eq!(
                    manager.read_tag(TagHash(old.owner_tag)).unwrap(),
                    original_bytes
                );
                let root = actual.definition.as_ref().unwrap().owner_offset as usize;
                let old_root = old.definition.as_ref().unwrap().owner_offset as usize;
                let float = |data: &[u8], at| f32::from_bits(word(data, at));
                let mut regions = Vec::new();
                if old.instance.schema == 0x80804BEE {
                    for (at, factor) in [
                        (0x48, 2.0),
                        (0xA8, 2.0),
                        (0x68, 0.5),
                        (0xC8, 0.5),
                        (0x88, 0.5),
                        (0xE8, 0.5),
                    ] {
                        assert_eq!(
                            numeric(&bytes, root + at),
                            numeric(&original_bytes, old_root + at) * factor
                        );
                    }
                    for (at, factor) in [
                        (0x54, 2.0),
                        (0xB4, 2.0),
                        (0x74, 0.5),
                        (0xD4, 0.5),
                        (0x94, 0.5),
                        (0xF4, 0.5),
                    ] {
                        assert_eq!(
                            float(&bytes, root + at),
                            float(&original_bytes, old_root + at) * factor
                        );
                    }
                    let rows = array(&bytes, root + 0x368, 80);
                    let previous = array(&original_bytes, old_root + 0x368, 80);
                    assert_eq!(rows.len(), previous.len());
                    for (row, old_row) in rows.chunks_exact(80).zip(previous.chunks_exact(80)) {
                        assert_eq!(row[0x1C], old_row[0x1C]);
                        for (at, factor) in [(0x14, 2.0), (0x38, 0.5), (0x3C, 0.5), (0x40, 0.5)] {
                            assert_eq!(float(row, at), float(old_row, at) * factor);
                        }
                        regions.push(serde_json::json!({"capacity": float(row, 0x14), "delay": float(row, 0x38), "duration": float(row, 0x40)}));
                    }
                } else if old.instance.schema == 0x80803CDE {
                    for at in [0x2D0, 0x2D4] {
                        assert_eq!(
                            float(&bytes, root + at),
                            float(&original_bytes, old_root + at) * 2.0
                        );
                    }
                } else if old.instance.schema == 0x80803889 {
                    for (input, factor) in [
                        (0, 2.0),
                        (1, 2.0),
                        (2, 2.0),
                        (3, 2.0),
                        (4, 0.5),
                        (5, 0.5),
                        (32, 2.0),
                    ] {
                        assert_eq!(
                            numeric(&bytes, root + 0x10 + input * 0x20),
                            numeric(&original_bytes, old_root + 0x10 + input * 0x20) * factor
                        );
                    }
                    for at in [0xB80, 0xB90] {
                        let expected = if *donor == 0x80C0D7A6 {
                            0x80BFAB5F
                        } else {
                            word(&original_bytes, old_root + at)
                        };
                        assert_eq!(word(&bytes, root + at), expected);
                        assert!(dependencies.contains(&expected));
                    }
                    if *donor == 0x80C0D7A6 {
                        let projectile = manager.read_tag(TagHash(0x80BFAB5F)).unwrap();
                        let loaded = load_weapon_runtime_graph_for_entity(
                            &manager,
                            0,
                            0,
                            0x80BFAB5F,
                            &projectile,
                        )
                        .unwrap();
                        for resource in loaded.resources {
                            assert!(dependencies.contains(&resource.owner_tag));
                        }
                    }
                }
                let path = output
                    .join("motion")
                    .join(format!("{hash:08X}-{:08X}.bin", actual.instance.schema));
                fs::write(&path, &bytes).unwrap();
                tuning.push(serde_json::json!({"schema": actual.instance.schema, "owner": actual.owner_tag, "source_owner": old.owner_tag, "definition_offset": root, "path": path, "regions": regions}));
            }
            // Handling and cooldown come from an equipped private plug, never a stock mutation.
            let authored = staged.gear_donor(hash).unwrap();
            let engine_lane = crate::item::weapon_socket_types(&definition)
                .unwrap()
                .iter()
                .position(|role| *role == 61)
                .unwrap();
            let engine_hash = authored.sockets[engine_lane].native_default.unwrap();
            assert!(stock.item_definition_tag(engine_hash).is_none());
            let perks = staged.item_sandbox_perk_indices(engine_hash);
            for (source_row, key) in [
                (325, 0x254EF1D6),
                (327, 0x12DAFD65),
                (328, 0xD9EA66FF),
                (1042, 0xC45B9C11),
            ] {
                if source_row != 325 && *donor != 0x8152099A {
                    continue;
                }
                let writes_key = |effect: &action::DecodedEffect| {
                    effect.kind == 10
                        && effect.facts.iter().any(|fact| {
                            fact.label == "Property Key" && fact.value == FactValue::Key(key)
                        })
                };
                let (index, runtime, decoded) = perks
                    .iter()
                    .find_map(|&index| {
                        let runtime = load_sandbox_perk_runtime_action(
                            &manager,
                            &globals,
                            usize::from(index),
                        )
                        .ok()?;
                        let decoded = action::decode(&runtime.action_payload).ok()?;
                        let carries = decoded.effects().any(writes_key);
                        carries.then_some((index, runtime, decoded))
                    })
                    .unwrap_or_else(|| panic!("Equipped engine lacks trait property 0x{key:08X}"));
                let stock_runtime =
                    load_sandbox_perk_runtime_action(&source, &source_globals, source_row).unwrap();
                let stock_action = action::decode(&stock_runtime.action_payload).unwrap();
                let effect = decoded.effects().find(|effect| writes_key(effect)).unwrap();
                assert!(effect.retained);
                assert_eq!(
                    effect.facts,
                    stock_action
                        .effects()
                        .find(|effect| writes_key(effect))
                        .unwrap()
                        .facts
                );
                let group = decoded
                    .groups
                    .iter()
                    .find(|group| group.effects.iter().any(writes_key))
                    .unwrap();
                assert_eq!(
                    group
                        .activation
                        .iter()
                        .map(|condition| condition.kind)
                        .collect::<Vec<_>>(),
                    vec![0]
                );
                assert_ne!(runtime.action_tag, stock_runtime.action_tag);
                assert!(dependencies.contains(&runtime.action_tag.0));
                assert_eq!(
                    manager.read_tag(stock_runtime.action_tag).unwrap(),
                    stock_runtime.action_payload
                );
                let path = output
                    .join("handling")
                    .join(format!("{hash:08X}-{key:08X}.bin"));
                fs::write(&path, &runtime.action_payload).unwrap();
                handling.push(serde_json::json!({"engine": engine_hash, "perk": index, "source_row": source_row, "property": key, "action": runtime.action_tag.0, "path": path, "facts": effect.facts.iter().map(|fact| fact.render()).collect::<Vec<_>>()}));
            }
        }
        receipt.push(serde_json::json!({"name": recipe.name, "item": hash, "pattern": index,
            "global": global, "entity": entity_tag, "donor": donor, "speed_percent": percent,
            "resources": graph.resources.len(), "evaluations": speeds, "motion_readback": motion_readback, "tuning": tuning, "handling": handling}));
    }
    invalid = cases[0].0.clone();
    invalid.overrides.sparrow = Some(Sparrow {
        summon: Summon::Other {
            entity: crate::HexHash::new(0x81613D27),
        },
        speed_percent: 100,
        ..Default::default()
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
