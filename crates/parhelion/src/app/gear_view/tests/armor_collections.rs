//! Class-specific armor destinations and completion, from saved recipes to staged packages.
use super::*;
use std::collections::BTreeSet;

fn word(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}

fn dword(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn array(data: &[u8], at: usize, stride: usize) -> Vec<usize> {
    let count = u64::from_le_bytes(data[at..at + 8].try_into().unwrap()) as usize;
    if count == 0 {
        return vec![];
    }
    let header =
        (at as i64 + 8 + i64::from_le_bytes(data[at + 8..at + 16].try_into().unwrap())) as usize;
    (0..count).map(|i| header + 16 + i * stride).collect()
}

#[test]
#[ignore = "Requires SUNDIAL_INSTALL and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent readback together"
)]
fn armor_pages_and_badge_completion_follow_supported_classes() {
    let packages = crate::test_support::install().join("packages");
    let output = crate::test_support::artifact_dir("gear");
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(output.join("recipes")).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        ..Default::default()
    };
    // The app takes its own catalog, a second read of the test cache, and `stock` stays for
    // the checks below.
    app.install_catalog(crate::test_support::catalog(packages.parent().unwrap()).unwrap());
    let ctx = context();
    let donors = stock.gear_donors(ItemKind::Armor.bucket_hashes());
    let mut recipes = Vec::new();
    let mut classes = Vec::new();
    for class in 0..3 {
        for exotic in [false, true] {
            let donor = donors
                .iter()
                .find(|donor| {
                    donor.collection_backed
                        && stock.item_class_type(donor.hash) == Some(class)
                        && (donor.rarity == WeaponRarity::Exotic) == exotic
                        && matches!(donor.rarity, WeaponRarity::Legendary | WeaponRarity::Exotic)
                })
                .unwrap();
            new_from_menu(&ctx, &mut app, ItemKind::Armor);
            app.recipe.set_donor(donor.hash, donor.name.clone());
            app.edit_weapon_name(format!("Armor Class {class} Exotic {exotic}"));
            let page = settle(&ctx, &mut app);
            capture::write(&ctx, &page, &format!("armor-class-{class}-{exotic}"));
            recipes.push(app.recipe.clone());
            classes.push(Some(class));
        }
    }
    // Exclusion removes badge membership without removing the armor's class page.
    let mut excluded = recipes[0].clone();
    excluded
        .rename_authored_item("Armor Excluded from Badge")
        .unwrap();
    excluded.overrides.exclude_from_sunrise_badge = true;
    recipes.push(excluded);
    classes.push(Some(0));
    // Native Festival masks provide an unrestricted armor fixture without raw payload edits.
    let neutral = donors
        .iter()
        .find(|donor| donor.collection_backed && stock.item_class_type(donor.hash) == Some(3))
        .unwrap();
    new_from_menu(&ctx, &mut app, ItemKind::Armor);
    app.recipe.set_donor(neutral.hash, neutral.name.clone());
    app.edit_weapon_name("Every Class Armor".into());
    recipes.push(app.recipe.clone());
    classes.push(None);
    // Cross the native five-item row limit while retaining class-neutral and excluded pieces.
    for ordinal in 0..8 {
        let mut extra = recipes[0].clone();
        extra
            .rename_authored_item(format!("Titan Overflow {ordinal}"))
            .unwrap();
        recipes.push(extra);
        classes.push(Some(0));
    }
    new_from_menu(&ctx, &mut app, ItemKind::Ship);
    app.edit_weapon_name("Shared Badge Ship".into());
    recipes.push(app.recipe.clone());
    classes.push(None);
    for recipe in &recipes {
        let json = recipe.to_json_pretty().unwrap();
        fs::write(
            output
                .join("recipes")
                .join(format!("{}.json", recipe.namespace)),
            &json,
        )
        .unwrap();
    }
    let recipes = recipes
        .into_iter()
        .map(|r| {
            WeaponRecipe::load_json(output.join("recipes").join(format!("{}.json", r.namespace)))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = output.join("view");
    staged_view(&packages, &build, &view);
    let staged =
        InvestmentCatalog::load_with_cache_path(&view, &output.join("catalog.json"), true, |_| {})
            .unwrap();
    let brand = crate::branding::Branding::detect(packages.parent().unwrap()).name();
    let manager = open_shadowkeep_package_manager(&view.join("packages")).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager.read_tag(TagHash(dword(&globals, 16))).unwrap();
    let table = |slot: usize| {
        manager
            .read_tag(TagHash(dword(&root, 8 + slot * 16)))
            .unwrap()
    };
    let nodes = table(63);
    let records = table(72);
    let objectives = table(58);
    let collectibles = table(19);
    let node_rows = array(&nodes, 8, 0xA8);
    let record_rows = array(&records, 8, 0xD8);
    let objective_rows = array(&objectives, 8, 0xA0);
    let collectible_rows = array(&collectibles, 8, 0xB8);
    let mut readback = Vec::new();
    for (ordinal, recipe) in recipes.iter().enumerate() {
        let hash = recipe.identity.item_hash.parse_u32().unwrap();
        let parents = staged.item_collection_parents(hash);
        let paths = staged.item_collection_paths(hash);
        if recipe.kind == ItemKind::Armor {
            let exotic = ordinal < 6 && ordinal % 2 == 1;
            for class in (0..3).filter(|&c| classes[ordinal].is_none_or(|v| v == c)) {
                let class_name = class_label(class).unwrap();
                if exotic {
                    assert!(
                        paths.iter().any(|p| p
                            .iter()
                            .map(String::as_str)
                            .eq([class_name, "Armor", "Exotic", "Items"])),
                        "{paths:?}"
                    );
                    assert!(!paths.iter().any(|p| {
                        p.iter()
                            .map(String::as_str)
                            .eq([brand, class_name, "Armor", "Items"])
                    }));
                    assert!(!paths.iter().any(|p| p.len() == 5 && p[1] == brand));
                } else {
                    assert!(
                        paths.iter().any(|p| p.len() == 5
                            && p[0].starts_with(&format!("{brand} Armor Set "))
                            && p[1..]
                                .iter()
                                .map(String::as_str)
                                .eq([brand, class_name, "Armor", "Items"])),
                        "{paths:?}"
                    );
                }
            }
        }
        for (class, badge) in [0x5355_4E54, 0x5355_4E48, 0x5355_4E57]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                parents.contains(&badge),
                !recipe.overrides.exclude_from_sunrise_badge
                    && classes[ordinal].is_none_or(|c| usize::from(c) == class)
            );
        }
        readback.push(serde_json::json!({"item":hash,"class":classes[ordinal],"paths":paths,"parents":parents}));
    }
    // Read the badge's backing record and evaluate its private flag sum independently.
    // Acquiring this class's two pieces, the neutral mask and the shared ship must complete this class alone.
    for (class, badge) in [0x5355_4E54, 0x5355_4E48, 0x5355_4E57]
        .into_iter()
        .enumerate()
    {
        let node = node_rows
            .iter()
            .copied()
            .find(|&n| dword(&nodes, n + 0x28) == badge)
            .unwrap();
        let member_indices = array(&nodes, node + 0x78, 4)
            .into_iter()
            .map(|r| word(&nodes, r))
            .collect::<Vec<_>>();
        let expected_count = if class == 0 { 12 } else { 4 };
        assert_eq!(member_indices.len(), expected_count);
        let expected_flags = member_indices
            .iter()
            .map(|&index| {
                let row = collectible_rows[usize::from(index)];
                let acquisition = array(&collectibles, row + 0x70, 8);
                assert_eq!(acquisition.len(), 1);
                assert_eq!(dword(&collectibles, acquisition[0]), 1);
                word(&collectibles, acquisition[0] + 4)
            })
            .collect::<BTreeSet<_>>();
        let record = record_rows[usize::from(word(&nodes, node + 0x52))];
        let link = array(&records, record + 0x30, 2);
        assert_eq!(link.len(), 1);
        let objective = objective_rows[usize::from(word(&records, link[0]))];
        let target = dword(&objectives, objective + 0x30);
        assert_eq!(target as usize, expected_count);
        let tokens = array(&objectives, objective + 8, 8);
        let flags = tokens
            .iter()
            .filter(|&&r| objectives[r] == 1)
            .map(|&r| word(&objectives, r + 4))
            .collect::<BTreeSet<_>>();
        assert_eq!(flags, expected_flags);
        for missing in expected_flags.iter().map(Some).chain([None]) {
            let mut stack = Vec::<u32>::new();
            for &token in &tokens {
                match objectives[token] {
                    1 => stack.push(u32::from(missing != Some(&word(&objectives, token + 4)))),
                    11 => stack.push(u32::from(word(&objectives, token + 4))),
                    17 => {
                        let b = stack.pop().unwrap();
                        let a = stack.pop().unwrap();
                        stack.push(a + b);
                    }
                    opcode => panic!("Unexpected badge opcode {opcode}"),
                }
            }
            assert_eq!(stack.len(), 1);
            assert_eq!(stack[0] == target, missing.is_none());
        }
        readback.push(serde_json::json!({"badge_class":class,"target":target,"flags":flags,"members":member_indices}));
    }
    // Read the actual category-to-set links. Each set must be selectable, named and bounded.
    for (class, parent_hash, sizes) in [
        (0, 0x305A_5226, vec![5, 5, 1]),
        (1, 0xDF3B_D502, vec![2]),
        (2, 0x4BB1_6895, vec![2]),
    ] {
        let parent = node_rows
            .iter()
            .position(|&n| dword(&nodes, n + 0x28) == parent_hash)
            .unwrap();
        let category = node_rows
            .iter()
            .copied()
            .find(|&n| {
                dword(&nodes, n + 0x28) == crate::collection::GearPage::armor(class).hash("node")
            })
            .unwrap();
        assert_eq!(
            array(&nodes, category + 0x18, 2)
                .iter()
                .map(|&r| usize::from(word(&nodes, r)))
                .collect::<Vec<_>>(),
            [parent]
        );
        assert!(array(&nodes, category + 0x78, 4).is_empty());
        let sets = array(&nodes, category + 0x68, 24);
        assert_eq!(sets.len(), sizes.len());
        let mut union = BTreeSet::new();
        for (ordinal, (&link, &size)) in sets.iter().zip(&sizes).enumerate() {
            let set_index = usize::from(word(&nodes, link));
            let set = node_rows[set_index];
            assert_eq!(
                array(&nodes, set + 0x18, 2)
                    .iter()
                    .map(|&r| word(&nodes, r))
                    .collect::<Vec<_>>(),
                [node_rows.iter().position(|&n| n == category).unwrap() as u16]
            );
            assert!(array(&nodes, set + 0x68, 24).is_empty());
            let members = array(&nodes, set + 0x78, 4);
            assert_eq!(members.len(), size);
            let name = format!("{brand} Armor Set {}", ["I", "II", "III"][ordinal]);
            for member in members {
                let collectible_index = word(&nodes, member);
                assert!(union.insert(collectible_index));
            }
            let objective = objective_rows[usize::from(word(&nodes, set + 0x50))];
            assert_eq!(dword(&objectives, objective + 0x30) as usize, size);
            assert!(readback.iter().any(|r| {
                r["paths"]
                    .as_array()
                    .is_some_and(|paths| paths.iter().any(|p| p[0] == name))
            }));
        }
        let objective = objective_rows[usize::from(word(&nodes, category + 0x50))];
        assert_eq!(
            dword(&objectives, objective + 0x30) as usize,
            sizes.iter().sum::<usize>()
        );
        readback.push(serde_json::json!({"armor_class":class,"set_sizes":sizes,"members":union}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&readback).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(view).unwrap();
}
