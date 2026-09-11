use super::*;
use crate::presentation::Badge;

#[test]
#[ignore = "requires PARHELION_PRESENTATION_TEST_PACKAGES and PARHELION_PRESENTATION_STAGE_ROOT"]
fn real_personalization_stages_distinct_badges_corner_icons_and_lore() {
    let packages =
        PathBuf::from(std::env::var_os("PARHELION_PRESENTATION_TEST_PACKAGES").expect("packages"));
    let staging_root =
        PathBuf::from(std::env::var_os("PARHELION_PRESENTATION_STAGE_ROOT").expect("stage root"));
    let mut recipes = recipes();
    let build = |recipes| {
        let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: packages.clone(),
            staging_root: staging_root.clone(),
            ignore_installed_authored_overlays: true,
            recipes,
        })
        .unwrap();
        crate::build_and_stage_snapshot_with_progress(&snapshot, |p| {
            eprintln!(
                "Presentation: {} {}/{}",
                p.phase.label(),
                p.completed,
                p.total
            )
        })
        .unwrap()
    };
    let first = build(recipes.clone());
    let first_bytes = first
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.file_name.clone(),
                fs::read(first.run_directory.join(&artifact.file_name)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    recipes.reverse();
    let repeated = build(recipes.clone());
    for artifact in &first.artifacts {
        assert_eq!(
            first_bytes[&artifact.file_name],
            fs::read(repeated.run_directory.join(&artifact.file_name)).unwrap(),
            "{} depends on recipe order",
            artifact.file_name
        );
    }
    let view = tempfile::tempdir_in(packages.parent().unwrap()).unwrap();
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).unwrap();
    for entry in fs::read_dir(&packages).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "pkg")
            && !crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
                .iter()
                .any(|name| entry.file_name() == *name)
        {
            fs::hard_link(entry.path(), view_packages.join(entry.file_name())).unwrap();
        }
    }
    for artifact in &first.artifacts {
        fs::hard_link(
            repeated.run_directory.join(&artifact.file_name),
            view_packages.join(&artifact.file_name),
        )
        .unwrap();
    }
    fs::create_dir_all(view.path().join("bin/x64")).unwrap();
    fs::hard_link(
        packages
            .parent()
            .unwrap()
            .join("bin/x64/oo2core_3_win64.dll"),
        view.path().join("bin/x64/oo2core_3_win64.dll"),
    )
    .unwrap();
    let manager = open_manager(&view_packages).unwrap();
    verify_generation(&manager, recipes);
    eprintln!(
        "Personalization packages staged at {}",
        repeated.run_directory.display()
    );
}

fn recipes() -> Vec<crate::WeaponRecipe> {
    (0..3)
        .map(|i| {
            let mut recipe = crate::WeaponRecipe::new_named_weapon_for_donor(
                format!("Personal Story {i}"),
                0xA25B8F8F,
                "Arc Logic",
            )
            .unwrap();
            recipe.overrides.collection_destination = Some(crate::collection::Destination {
                ammo: crate::collection::Ammo::Special,
                family: crate::collection::Family::Sidearms,
            });
            recipe.overrides.badge = Some(Badge {
                name: if i < 2 {
                    "The Wanderers".into()
                } else {
                    "The Stargazers".into()
                },
                description: "A personal collection.".into(),
                icon: Some(crate::weapon::tests::personalization::artwork(if i < 2 {
                    40
                } else {
                    90
                })),
            });
            recipe.overrides.exclude_from_sunrise_badge =
                i == 1 || std::env::var_os("PARHELION_BADGE_EXCLUDE_ALL").is_some();
            recipe.overrides.corner_icon =
                (i != 2).then(|| crate::weapon::tests::personalization::artwork(40 + i * 10));
            recipe.overrides.lore = (i != 2)
                .then(|| format!("A private story for weapon {i}.\n\nSecond paragraph: 星."));
            if i == 2 {
                recipe.overrides.rarity = Some(crate::RecipeRarity::Exotic);
            }
            recipe
        })
        .collect::<Vec<_>>()
}

fn verify_generation(manager: &PackageManager, recipes: Vec<crate::WeaponRecipe>) {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(globals_child_tag(&globals, 0).unwrap())
        .unwrap();
    let items = manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let collectibles = manager
        .read_tag(root_child_tag(&root, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let nodes = manager
        .read_tag(root_child_tag(&root, ROOT_PRESENTATION_NODE_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    assert_eq!(array_at(&nodes, 8).unwrap().0, 937);
    verify_sunrise_record_capacity(manager, &root);
    let lore = manager
        .read_tag(root_child_tag(&root, 52).unwrap())
        .unwrap();
    let lore_strings = manager
        .read_tag(globals_child_tag(&globals, 34).unwrap())
        .unwrap();
    assert_eq!(array_at(&lore, 8).unwrap().0, 1427);
    let icons = manager
        .read_tag(globals_child_tag(&globals, GLOBALS_ITEM_ICON_TABLE_SLOT).unwrap())
        .unwrap();
    let item_strings = manager
        .read_tag(globals_child_tag(&globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap())
        .unwrap();
    let mut layers = BTreeSet::new();
    let expected_sunrise_count = recipes
        .iter()
        .filter(|recipe| !recipe.overrides.exclude_from_sunrise_badge)
        .count();
    let objectives = manager
        .read_tag(root_child_tag(&root, ROOT_OBJECTIVE_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let (_, _, objective_rows, _) = array_at(&objectives, 8).unwrap();
    let (_, _, node_rows, _) = array_at(&nodes, 8).unwrap();
    let destination = crate::collection::Destination {
        ammo: crate::collection::Ammo::Special,
        family: crate::collection::Family::Sidearms,
    };
    let added = node_rows + 936 * crate::progression::PRESENTATION_NODE_ROW_SIZE;
    assert_eq!(
        read_u32(
            &nodes,
            added + crate::progression::PRESENTATION_NODE_HASH_OFFSET
        )
        .unwrap(),
        destination.hash("node")
    );
    let page_objective = read_u16(
        &nodes,
        added + crate::progression::PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
    )
    .unwrap();
    let page_objective_row =
        objective_rows + usize::from(page_objective) * crate::progression::OBJECTIVE_ROW_SIZE;
    assert_eq!(
        read_i32(
            &objectives,
            page_objective_row + crate::progression::OBJECTIVE_COMPLETION_VALUE_OFFSET
        )
        .unwrap(),
        2
    );
    assert_eq!(
        array_at(
            &nodes,
            added + crate::progression::PRESENTATION_NODE_COLLECTIBLES_OFFSET
        )
        .unwrap()
        .0,
        2
    );
    let page_program =
        crate::progression::numeric_program_layout(&objectives, page_objective_row + 8).unwrap();
    assert_eq!(
        page_program
            .tokens
            .iter()
            .filter(|(opcode, _)| *opcode == 1)
            .count(),
        2
    );
    verify_discovery_counts(
        &nodes,
        &objectives,
        page_objective_row,
        &page_program.tokens,
    );
    let objective = objective_rows + 8483 * crate::progression::OBJECTIVE_ROW_SIZE;
    assert_eq!(
        read_i32(
            &objectives,
            objective + crate::progression::OBJECTIVE_COMPLETION_VALUE_OFFSET
        )
        .unwrap() as usize,
        expected_sunrise_count
    );
    let program =
        crate::progression::numeric_program_layout(&objectives, objective + 0x08).unwrap();
    assert_eq!(
        program
            .tokens
            .iter()
            .filter(|(opcode, _)| *opcode == crate::progression::NUMERIC_FLAG_INSTRUCTION)
            .count(),
        expected_sunrise_count
    );
    if expected_sunrise_count == 0 {
        assert_eq!(program.tokens, vec![(11, 0)]);
    }
    for leaf in 925..=927 {
        let descriptor = node_rows
            + leaf * crate::progression::PRESENTATION_NODE_ROW_SIZE
            + crate::progression::PRESENTATION_NODE_COLLECTIBLES_OFFSET;
        assert_eq!(
            array_at(&nodes, descriptor).unwrap().0,
            expected_sunrise_count
        );
    }
    let tables = PresentationTables {
        items: &items,
        collectibles: &collectibles,
        nodes: &nodes,
        objectives: &objectives,
        icons: &icons,
        item_strings: &item_strings,
        lore_strings: &lore_strings,
    };
    for recipe in recipes {
        layers.insert(verify_recipe(manager, &tables, recipe));
    }
    assert_eq!(
        layers.len(),
        3,
        "Distinct corner choices collapsed onto one watermark"
    );
}

fn verify_discovery_counts(
    nodes: &[u8],
    objectives: &[u8],
    page_objective_row: usize,
    tokens: &[(u8, u16)],
) {
    let (_, _, node_rows, _) = array_at(nodes, 8).unwrap();
    let (_, _, objective_rows, _) = array_at(objectives, 8).unwrap();
    let excluded =
        crate::progression::numeric_program_layout(objectives, page_objective_row + 0x38).unwrap();
    assert_eq!(excluded.tokens, vec![(11, 0)]);
    let flags = tokens
        .iter()
        .filter_map(|&(opcode, value)| {
            (opcode == crate::progression::NUMERIC_FLAG_INSTRUCTION).then_some(value)
        })
        .collect::<Vec<_>>();
    for acquired in 0..=flags.len() {
        let mut stack = Vec::new();
        for &(opcode, value) in tokens {
            match opcode {
                11 => stack.push(i32::from(value)),
                1 => stack.push(i32::from(flags[..acquired].contains(&value))),
                17 => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(left + right);
                }
                _ => panic!("Unexpected page counter instruction {opcode}"),
            }
        }
        assert_eq!(stack, vec![acquired as i32]);
        assert_eq!(
            read_i32(
                objectives,
                page_objective_row + crate::progression::OBJECTIVE_COMPLETION_VALUE_OFFSET
            )
            .unwrap(),
            2
        );
    }
    for (node, total, excluded_pool) in [(0, 5090, 2544), (655, 728, 5158)] {
        let index = read_u16(
            nodes,
            node_rows
                + node * crate::progression::PRESENTATION_NODE_ROW_SIZE
                + crate::progression::PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )
        .unwrap();
        let row = objective_rows + usize::from(index) * crate::progression::OBJECTIVE_ROW_SIZE;
        assert_eq!(
            read_i32(
                objectives,
                row + crate::progression::OBJECTIVE_COMPLETION_VALUE_OFFSET
            )
            .unwrap(),
            total,
            "node {node} has the wrong total"
        );
        assert_eq!(
            crate::progression::numeric_program_layout(objectives, row + 0x38)
                .unwrap()
                .tokens,
            vec![(12, excluded_pool)],
            "node {node} must preserve its stock exclusion expression"
        );
    }
}

// Sunrise upstream 169fd296: records/definition.h, nodes/definition.h,
// unlocks/definition.h and package_record_build.cpp. Record values advance
// by the number of objectives, reserving two values for a row without any.
fn verify_sunrise_record_capacity(manager: &PackageManager, root: &[u8]) {
    let records = manager.read_tag(root_child_tag(root, 72).unwrap()).unwrap();
    let (count, _, rows, _) = array_at(&records, 8).unwrap();
    assert!(count <= 4096);
    let mut objective_count = 0;
    let mut value_end = 2746;
    for index in 0..count {
        let count = read_u64(&records, rows + index * 216 + 48).unwrap() as usize;
        assert!(count <= u8::MAX as usize);
        objective_count += count;
        value_end += if count == 0 { 2 } else { count };
    }
    assert!(objective_count <= 4096);
    assert!(value_end <= 6200);
    eprintln!(
        "Sunrise record budgets: {count}/4096 definitions, {objective_count}/4096 objectives, values end at {value_end}/6200"
    );
}

fn verify_ancestor_objectives(nodes: &[u8], objectives: &[u8], flag: u16) {
    let (_, _, node_rows, _) = array_at(nodes, 8).unwrap();
    let (_, _, objective_rows, _) = array_at(objectives, 8).unwrap();
    for ancestor in crate::progression::presentation_ancestor_nodes(nodes, &[936]).unwrap() {
        let objective = read_u16(
            nodes,
            node_rows
                + ancestor * crate::progression::PRESENTATION_NODE_ROW_SIZE
                + crate::progression::PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )
        .unwrap();
        if objective == u16::MAX {
            continue;
        }
        let objective_row =
            objective_rows + usize::from(objective) * crate::progression::OBJECTIVE_ROW_SIZE;
        for field in [8, 0x38] {
            if read_u64(objectives, objective_row + field).unwrap() == 0 {
                continue;
            }
            let program =
                crate::progression::numeric_program_layout(objectives, objective_row + field)
                    .unwrap();
            assert_eq!(
                program
                    .tokens
                    .iter()
                    .filter(|token| **token == (1, flag))
                    .count(),
                usize::from(field == 8),
                "node {ancestor} field {field:X} counts acquisition in the wrong lane"
            );
            if ancestor == 936 && field == 0x38 {
                assert_eq!(
                    program.tokens,
                    vec![(11, 0)],
                    "Added page must have no excluded entries"
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
struct PresentationTables<'a> {
    items: &'a [u8],
    collectibles: &'a [u8],
    nodes: &'a [u8],
    objectives: &'a [u8],
    icons: &'a [u8],
    item_strings: &'a [u8],
    lore_strings: &'a [u8],
}

fn verify_recipe(
    manager: &PackageManager,
    tables: &PresentationTables<'_>,
    recipe: crate::WeaponRecipe,
) -> u32 {
    let PresentationTables {
        items,
        collectibles,
        nodes,
        objectives,
        icons,
        item_strings,
        lore_strings,
    } = *tables;
    let (_, _, item_rows, _) = array_at(items, 8).unwrap();
    let (_, _, collectible_rows, _) = array_at(collectibles, 8).unwrap();
    let (_, _, icon_rows, _) = array_at(icons, 8).unwrap();
    let (_, _, string_rows, _) = array_at(item_strings, 8).unwrap();
    let (_, _, lore_rows, _) = array_at(lore_strings, 8).unwrap();
    let spec = recipe.to_spec().unwrap();
    let index = (0..array_at(items, 8).unwrap().0)
        .find(|&i| read_u32(items, item_rows + i * 24).unwrap() == spec.identity.item_hash)
        .unwrap();
    let definition = manager
        .read_tag(TagHash(
            read_u32(items, item_rows + index * 24 + 16).unwrap(),
        ))
        .unwrap();
    let strings = manager
        .read_tag(TagHash(
            read_u32(item_strings, string_rows + index * 24 + 16).unwrap(),
        ))
        .unwrap();
    let icon = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap() as usize;
    let container = manager
        .read_tag(TagHash(
            read_u32(icons, icon_rows + icon * 24 + ITEM_ICON_CONTAINER_OFFSET).unwrap(),
        ))
        .unwrap();
    let layer = read_u32(&container, ICON_WATERMARK_LAYER_OFFSET).unwrap();
    let collectible = (0..array_at(collectibles, 8).unwrap().0)
        .find(|&i| {
            read_u32(
                collectibles,
                collectible_rows + i * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_HASH_OFFSET,
            )
            .unwrap()
                == spec.identity.collectible_hash
        })
        .unwrap();
    let row = collectible_rows + collectible * COLLECTIBLE_ROW_SIZE;
    let (count, _, parents, _) = array_at(
        collectibles,
        row + COLLECTIBLE_PRESENTATION_NODE_PARENTS_OFFSET,
    )
    .unwrap();
    assert_eq!(
        count,
        if spec.overrides.exclude_from_sunrise_badge {
            4
        } else {
            7
        }
    );
    let parents = (0..count)
        .map(|i| read_u16(collectibles, parents + i * 2).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        parents.contains(&936),
        spec.overrides.rarity != Some(AuthoredWeaponRarity::Exotic)
    );
    if parents.contains(&936) {
        let flag = crate::progression::collection_unlock_index(collectibles, row).unwrap() as u16;
        verify_ancestor_objectives(nodes, objectives, flag);
    }
    for parent in [925, 926, 927] {
        assert_eq!(
            parents.contains(&parent),
            !spec.overrides.exclude_from_sunrise_badge
        );
    }
    assert_eq!(
        crate::progression::template_presentation_parents(nodes, collectibles, collectible)
            .unwrap(),
        parents
    );
    if spec.overrides.rarity == Some(AuthoredWeaponRarity::Exotic) {
        assert_eq!(
            read_u16(collectibles, row + COLLECTIBLE_MATERIAL_SET_OFFSET).unwrap(),
            COLLECTIBLE_EXOTIC_WEAPON_MATERIAL_SET
        );
    }
    if spec.overrides.lore.is_some() {
        let block = relative_target(&definition, 0x28).unwrap();
        let lore_index = read_u16(&definition, block).unwrap();
        assert!(lore_index >= 1425);
        assert_eq!(read_u16(collectibles, row + 0x2C).unwrap(), lore_index);
        assert_eq!(
            read_u32(lore_strings, lore_rows + usize::from(lore_index) * 40 + 32).unwrap(),
            crate::presentation::text_hash(&spec.namespace, "lore")
        );
    }
    layer
}
