use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
#[allow(clippy::cognitive_complexity)]
fn real_two_weapon_project_is_permutation_identical_when_configured() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let clean_packages = PathBuf::from(packages);
    let clean_root = clean_packages
        .parent()
        .expect("clean packages directory needs a parent");
    let install_view = tempfile::Builder::new()
        .prefix(".parhelion-clean-install-")
        .tempdir_in(
            clean_root
                .parent()
                .expect("clean package view needs a parent directory"),
        )
        .expect("temporary install-shaped view should be created on the package volume");
    let packages = install_view.path().join("packages");
    fs::create_dir(&packages).unwrap();
    for entry in fs::read_dir(&clean_packages).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|value| value.to_str()) == Some("pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name())).unwrap();
        }
    }
    fs::write(install_view.path().join("destiny2.exe"), []).unwrap();
    let install_bin = install_view.path().join("bin").join("x64");
    fs::create_dir_all(&install_bin).unwrap();
    fs::hard_link(
        clean_root
            .join("bin")
            .join("x64")
            .join("oo2core_3_win64.dll"),
        install_bin.join("oo2core_3_win64.dll"),
    )
    .unwrap();
    let every_end_spec = bundled_every_end_spec();
    let every_end_item_hash = every_end_spec.identity.item_hash;
    let every_end = every_end_spec;
    let second_sun_spec = legacy_second_sun_gl_spec();
    let second_sun_item_hash = second_sun_spec.identity.item_hash;
    let second_sun_donor_hash = second_sun_spec.donor_item_hash;
    let second_sun_pattern_global_id = second_sun_spec.identity.pattern_global_id_hash;
    let second_sun = second_sun_spec;
    let forward = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![every_end.clone(), second_sun.clone()],
        },
    )
    .expect("two-weapon project should build");
    let reverse = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![second_sun, every_end],
        },
    )
    .expect("reversed two-weapon project should build");
    assert_eq!(forward.plan.weapons.len(), 2);
    let expected_packages = crate::package_profile::authored_packages_for_file_names(
        forward
            .artifacts
            .iter()
            .map(|artifact| artifact.plan.output_file_name.as_str()),
    )
    .expect("project should emit a complete recipe-selected package set");
    assert_eq!(forward.artifacts.len(), expected_packages.len());
    assert_eq!(reverse.artifacts.len(), forward.artifacts.len());
    let mut forward_artifacts = forward.artifacts.iter().collect::<Vec<_>>();
    let mut reverse_artifacts = reverse.artifacts.iter().collect::<Vec<_>>();
    forward_artifacts.sort_by_key(|artifact| artifact.plan.chain.identity.package_id);
    reverse_artifacts.sort_by_key(|artifact| artifact.plan.chain.identity.package_id);
    for (left, right) in forward_artifacts.into_iter().zip(reverse_artifacts) {
        assert_eq!(left.plan.chain.identity, right.plan.chain.identity);
        assert_eq!(left.bytes(), right.bytes());
    }
    let host = forward
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == HOST_PACKAGE_ID)
        .unwrap();
    assert_eq!(host.plan.original_entry_count, HOST_EXPECTED_ENTRY_COUNT);
    assert_eq!(
        host.plan.final_entry_count,
        HOST_EXPECTED_ENTRY_COUNT + host.plan.appended_tags.len()
    );
    let assets = forward
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == PARHELION_ASSET_PACKAGE_ID)
        .unwrap();
    assert_eq!(assets.plan.original_entry_count, 0);
    assert_eq!(
        assets.plan.final_entry_count,
        assets.plan.appended_tags.len()
    );
    assert_eq!(forward.plan.sunrise.watermarked_icon_containers.len(), 2);

    let source_root = packages
        .parent()
        .expect("packages directory needs a parent");
    let view = tempfile::Builder::new()
        .prefix(".parhelion-project-test-")
        .tempdir_in(source_root)
        .expect("temporary package view should be created on the package volume");
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).unwrap();
    for entry in fs::read_dir(&packages).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|value| value.to_str()) == Some("pkg") {
            fs::hard_link(entry.path(), view_packages.join(entry.file_name())).unwrap();
        }
    }
    let source_oodle = source_root
        .join("bin")
        .join("x64")
        .join("oo2core_3_win64.dll");
    if source_oodle.is_file() {
        let target_bin = view.path().join("bin").join("x64");
        fs::create_dir_all(&target_bin).unwrap();
        fs::hard_link(&source_oodle, target_bin.join("oo2core_3_win64.dll")).unwrap();
    }
    assert_eq!(
        forward.write_new(&view_packages).unwrap().len(),
        forward.artifacts.len()
    );
    let manager = open_manager(&view_packages).expect("all authored packages should reopen");
    for (ordinal, plan) in forward.plan.weapons.iter().enumerate() {
        let definition = read_tag(&manager, plan.definition_tag, "project definition").unwrap();
        let strings = read_tag(&manager, plan.string_tag, "project strings").unwrap();
        assert_eq!(
            read_u32(&definition, ITEM_DEFINITION_HASH_OFFSET).unwrap(),
            plan.item_hash
        );
        let expected_icon = u16::try_from(STOCK_ITEM_ICON_COUNT + 1 + ordinal).unwrap();
        assert_eq!(
            read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap(),
            expected_icon
        );
        assert!(
            usize::from(read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap())
                > STOCK_ITEM_ICON_COUNT
        );
        assert_eq!(
            read_u32(&strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET).unwrap(),
            BLANK_LOCALIZED_REFERENCE_TABLE_INDEX
        );
        assert_eq!(
            read_u32(&strings, ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET + 4).unwrap(),
            BLANK_LOCALIZED_REFERENCE_HASH
        );
        assert!(
            matching_u32_offsets(&strings, RANDOM_PERKS_NOT_REACQUIRABLE_SOURCE_HASH).is_empty()
        );
    }
    let second = forward
        .plan
        .weapons
        .iter()
        .find(|plan| plan.item_hash == second_sun_item_hash)
        .unwrap();
    let second_definition = read_tag(&manager, second.definition_tag, "Second Sun").unwrap();
    let second_strings = read_tag(&manager, second.string_tag, "Second Sun strings").unwrap();
    assert_eq!(
        weapon_inventory_slot(&second_definition).unwrap(),
        WeaponInventorySlot::Energy
    );
    assert_eq!(
        weapon_damage_descriptor(&second_definition).unwrap(),
        WeaponDamageDescriptor::Elemental(ModernDamageType::Solar)
    );
    assert!(
        weapon_version_array(&second_definition)
            .unwrap()
            .unwrap()
            .groups
            .iter()
            .all(|group| *group == 11)
    );
    assert_eq!(
        weapon_default_plug_indices(&second_definition)
            .unwrap()
            .len(),
        10
    );
    let topology = weapon_translation_topology(&second_definition).unwrap();
    let presentation = weapon_presentation_tuple(&second_definition, &topology).unwrap();
    assert!(
        presentation.weapon_pattern_index.is_some(),
        "the authored definition must select its private sandbox-pattern row"
    );
    assert_eq!(
        read_u16(&presentation.art_rows, TRANSLATION_ART_VARIANT_OFFSET).unwrap(),
        0x03A9
    );
    assert!(presentation.dye_arrays[0].is_empty());
    assert_eq!(
        (0..3)
            .map(|position| {
                read_u32(
                    &presentation.dye_arrays[1],
                    position * TRANSLATION_DYE_ROW_SIZE,
                )
                .unwrap()
            })
            .collect::<Vec<_>>(),
        vec![0x06C4_0004, 0x06C5_0005, 0x06C6_0006]
    );
    assert!(presentation.dye_arrays[2].is_empty());
    let second_icon_index = read_u16(&second_strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap();
    assert!(usize::from(second_icon_index) > STOCK_ITEM_ICON_COUNT);
    let truthteller_strings = read_tag(
        &manager,
        TagHash(0x8133_76BC),
        "stock Truthteller item strings",
    )
    .expect("stock Truthteller item strings should load");
    assert_eq!(
        item_string_client_classification(&second_strings, WeaponInventorySlot::Energy).unwrap(),
        item_string_client_classification(&truthteller_strings, WeaponInventorySlot::Energy)
            .unwrap(),
        "the aggregate workflow must inherit Truthteller's complete Energy GL client tuple"
    );

    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None).unwrap();
    let globals = read_tag(&manager, globals_tag, "project globals").unwrap();
    let root = read_tag(
        &manager,
        TagHash(read_u32(&globals, 16).unwrap()),
        "project investment root",
    )
    .unwrap();
    let item_icons = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_ITEM_ICON_TABLE_SLOT).unwrap(),
        "project item icons",
    )
    .unwrap();
    let (icon_count, _, icon_rows, icon_class) = array_at(&item_icons, 8).unwrap();
    assert_eq!(icon_class, ITEM_ICON_ROW_CLASS);
    assert_eq!(
        icon_count,
        STOCK_ITEM_ICON_COUNT + 1 + forward.plan.weapons.len()
    );
    let watermark_layer = read_tag(
        &manager,
        forward.plan.sunrise.watermark_layer_tag,
        "project weapon watermark layer",
    )
    .unwrap();
    assert!(!watermark_layer.is_empty());
    for (ordinal, plan) in forward.plan.weapons.iter().enumerate() {
        let icon_index = STOCK_ITEM_ICON_COUNT + 1 + ordinal;
        let icon_row = icon_rows + icon_index * ITEM_ICON_ROW_SIZE;
        assert_eq!(read_u32(&item_icons, icon_row).unwrap(), plan.item_hash);
        let container =
            TagHash(read_u32(&item_icons, icon_row + ITEM_ICON_CONTAINER_OFFSET).unwrap());
        assert!(
            forward
                .plan
                .sunrise
                .watermarked_icon_containers
                .contains(&container)
        );
        let container_payload =
            read_tag(&manager, container, "project watermarked icon container").unwrap();
        let container_entry = manager
            .get_entry(container)
            .expect("project watermarked icon container should have an entry");
        assert_eq!(container_entry.file_type, 0x10);
        assert_eq!(container_entry.file_subtype, 0x00);
        assert_eq!(container_entry.reference, 0x8080_4A53);
        assert_eq!(
            read_u32(&container_payload, ICON_WATERMARK_LAYER_OFFSET,).unwrap(),
            u32::from(forward.plan.sunrise.watermark_layer_tag)
        );
        let companion = crate::shared_tag_memory::adjacent_companion_tag(container)
            .expect("project icon definition should have an adjacent companion tag");
        let companion_entry = manager
            .get_entry(companion)
            .expect("project icon companion should have an entry");
        assert_eq!(companion_entry.file_type, 0x08);
        assert_eq!(companion_entry.file_subtype, 0x00);
        assert_eq!(companion_entry.reference, 0x8080_9EF9);
        let companion =
            crate::shared_tag_memory::read_and_validate_icon_companion(&manager, container)
                .expect("project icon companion should be canonical");
        assert!(companion.dependencies.contains(&u32::from(container)));
        assert!(companion.dependencies.contains(&u32::from(companion.tag)));
    }
    let badge_icon_entry = manager
        .get_entry(forward.plan.sunrise.badge_icon_tag)
        .expect("project badge icon container should have an entry");
    assert_eq!(badge_icon_entry.file_type, 0x10);
    assert_eq!(badge_icon_entry.file_subtype, 0x00);
    assert_eq!(badge_icon_entry.reference, 0x8080_4A53);
    let badge_companion = crate::shared_tag_memory::read_and_validate_icon_companion(
        &manager,
        forward.plan.sunrise.badge_icon_tag,
    )
    .expect("project badge icon companion should be canonical");
    assert!(
        badge_companion
            .dependencies
            .contains(&u32::from(forward.plan.sunrise.badge_icon_tag))
    );
    let dense = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT).unwrap(),
        "project dense item presentation",
    )
    .unwrap();
    let (_, _, dense_rows, dense_class) = array_at(&dense, ITEM_DENSE_PRESENTATION_DESCRIPTOR)
        .expect("project dense item-presentation rows should parse");
    assert_eq!(dense_class, ITEM_DENSE_PRESENTATION_ROW_CLASS);
    let second_dense =
        dense_rows + usize::from(second.item_index) * ITEM_DENSE_PRESENTATION_ROW_SIZE;
    let second_selector_index = usize::try_from(
        read_u32(
            &dense,
            second_dense + ITEM_DENSE_PRESENTATION_SELECTOR_OFFSET,
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(second_selector_index, 0x0000_BA99);
    let project_items = read_tag(
        &manager,
        root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        "project item table",
    )
    .unwrap();
    let (project_item_count, _, project_item_rows, _) = array_at(&project_items, 8).unwrap();
    let truthteller_index = find_u32_row_key(
        &project_items,
        project_item_rows,
        project_item_count,
        ITEM_ROW_SIZE,
        0x7405_1969,
    )
    .unwrap()
    .expect("Truthteller presentation donor should remain present");
    let mountaintop_index = find_u32_row_key(
        &project_items,
        project_item_rows,
        project_item_count,
        ITEM_ROW_SIZE,
        second_sun_donor_hash,
    )
    .unwrap()
    .expect("Mountaintop gameplay donor should remain present");
    let truthteller_definition = read_tag(
        &manager,
        TagHash(
            read_u32(
                &project_items,
                project_item_rows + truthteller_index * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        ),
        "Truthteller presentation donor",
    )
    .unwrap();
    let mountaintop_definition = read_tag(
        &manager,
        TagHash(
            read_u32(
                &project_items,
                project_item_rows + mountaintop_index * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        ),
        "Mountaintop gameplay donor",
    )
    .unwrap();
    let sandbox_patterns = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_SANDBOX_PATTERN_TABLE_SLOT).unwrap(),
        "project sandbox patterns",
    )
    .unwrap();
    let authored_pattern = sandbox_pattern_identity_at(
        &sandbox_patterns,
        usize::from(weapon_pattern_index(&second_definition).unwrap().unwrap()),
    )
    .unwrap()
    .unwrap();
    let appearance_pattern = sandbox_pattern_identity_at(
        &sandbox_patterns,
        usize::from(
            weapon_pattern_index(&truthteller_definition)
                .unwrap()
                .unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    let runtime_pattern = sandbox_pattern_identity_at(
        &sandbox_patterns,
        usize::from(
            weapon_pattern_index(&mountaintop_definition)
                .unwrap()
                .unwrap(),
        ),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        authored_pattern.pattern_global_id_hash, second_sun_pattern_global_id,
        "the authored sandbox row must use its private runtime identity"
    );
    let entity_assignments = read_tag(
        &manager,
        TagHash(SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG),
        "project sandbox-pattern entity assignments",
    )
    .unwrap();
    assert_ne!(
        weapon_entity_assignment(&entity_assignments, authored_pattern.pattern_global_id_hash)
            .unwrap(),
        weapon_entity_assignment(&entity_assignments, runtime_pattern.pattern_global_id_hash)
            .unwrap(),
        "a separate appearance donor needs private content to inherit its HUD icon"
    );
    assert_eq!(
        (
            authored_pattern.weapon_content_group_hash,
            authored_pattern.weapon_translation_group_hash,
        ),
        (
            appearance_pattern.weapon_content_group_hash,
            appearance_pattern.weapon_translation_group_hash,
        ),
        "the authored sandbox row must retain the appearance donor's gear-art identity"
    );
    let truthteller_dense = dense_rows + truthteller_index * ITEM_DENSE_PRESENTATION_ROW_SIZE;
    assert_eq!(
        read_u32(
            &dense,
            second_dense + ITEM_DENSE_PRESENTATION_CLASSIFICATION_OFFSET,
        )
        .unwrap(),
        read_u32(
            &dense,
            truthteller_dense + ITEM_DENSE_PRESENTATION_CLASSIFICATION_OFFSET,
        )
        .unwrap(),
        "the authored dense row must preserve its presentation donor's classification"
    );
    let (selector_count, _, selector_rows, selector_class) =
        array_at(&dense, ITEM_DENSE_ICON_SELECTOR_DESCRIPTOR).unwrap();
    assert_eq!(selector_class, ITEM_DENSE_ICON_SELECTOR_ROW_CLASS);
    assert!(second_selector_index < selector_count);
    let second_selector = selector_rows + second_selector_index * ITEM_DENSE_ICON_SELECTOR_ROW_SIZE;
    let second_icon_tag_index = usize::try_from(read_u32(&dense, second_selector).unwrap())
        .expect("private dense icon tag index should fit");
    let (icon_tag_count, _, icon_tag_rows, icon_tag_class) =
        array_at(&dense, ITEM_DENSE_ICON_TAG_DESCRIPTOR).unwrap();
    assert_eq!(icon_tag_class, ITEM_DENSE_ICON_TAG_ROW_CLASS);
    assert!(second_icon_tag_index < icon_tag_count);
    let registered_icon_container = TagHash(
        read_u32(
            &dense,
            icon_tag_rows + second_icon_tag_index * ITEM_DENSE_ICON_TAG_ROW_SIZE,
        )
        .unwrap(),
    );
    let second_icon_row = icon_rows + usize::from(second_icon_index) * ITEM_ICON_ROW_SIZE;
    assert_eq!(
        registered_icon_container.0,
        read_u32(&item_icons, second_icon_row + ITEM_ICON_CONTAINER_OFFSET,).unwrap()
    );
    let registered_icon = read_tag(
        &manager,
        registered_icon_container,
        "Second Sun private dense icon container",
    )
    .unwrap();
    assert_eq!(
        read_u32(&registered_icon, ICON_WATERMARK_LAYER_OFFSET,).unwrap(),
        u32::from(forward.plan.sunrise.watermark_layer_tag)
    );
    let displays = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_COLLECTIBLE_DISPLAY_TABLE_SLOT).unwrap(),
        "project collectible displays",
    )
    .unwrap();
    let (_, _, display_rows, _) = array_at(&displays, 8).unwrap();
    let collectibles = read_tag(
        &manager,
        root_child_tag(&root, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT).unwrap(),
        "project collectibles",
    )
    .unwrap();
    let (_, _, collectible_rows, _) = array_at(&collectibles, 8).unwrap();
    let every = forward
        .plan
        .weapons
        .iter()
        .find(|plan| plan.item_hash == every_end_item_hash)
        .unwrap();
    let every_display =
        display_rows + usize::from(every.collectible_index) * COLLECTIBLE_DISPLAY_ROW_SIZE;
    let every_collectible =
        collectible_rows + usize::from(every.collectible_index) * COLLECTIBLE_ROW_SIZE;
    assert_eq!(
        collectibles[every_collectible + COLLECTIBLE_CURATED_ACQUISITION_FLAG_OFFSET],
        COLLECTIBLE_CURATED_ACQUISITION_FLAG
    );
    assert_eq!(
        read_u16(
            &collectibles,
            every_collectible + COLLECTIBLE_REACQUISITION_STATE_OFFSET,
        )
        .unwrap(),
        COLLECTIBLE_REACQUISITION_ENABLED
    );
    assert_eq!(
        read_u16(
            &collectibles,
            every_collectible + COLLECTIBLE_MATERIAL_SET_OFFSET,
        )
        .unwrap(),
        COLLECTIBLE_CURATED_WEAPON_MATERIAL_SET
    );
    assert_eq!(
        read_u32(
            &displays,
            every_display + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
        )
        .unwrap(),
        BLANK_LOCALIZED_REFERENCE_TABLE_INDEX
    );
    assert_eq!(
        read_u32(
            &displays,
            every_display + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET + 4,
        )
        .unwrap(),
        BLANK_LOCALIZED_REFERENCE_HASH
    );
    assert_eq!(
        read_u32(
            &displays,
            display_rows
                + usize::from(second.collectible_index) * COLLECTIBLE_DISPLAY_ROW_SIZE
                + COLLECTIBLE_DISPLAY_ICON_INDEX_OFFSET,
        )
        .unwrap(),
        u32::from(second_icon_index)
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn project_builder_allows_forced_plugs_but_rejects_catalog_incompatible_stats() {
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must be configured"),
    );
    let mut spec = bundled_every_end_spec();
    spec.namespace = "parhelion.public-incompatible-socket-integration".to_owned();
    spec.identity = WeaponCloneIdentity::from_namespace(&spec.namespace)
        .expect("integration namespace should allocate");
    spec.text.name = "Incompatible Socket Integration".to_owned();
    spec.overrides.socket_columns[0] = Some(WeaponSocketColumnOverride {
        choices: vec![spec.donor_item_hash],
        ..WeaponSocketColumnOverride::default()
    });

    let forced = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("the public project builder must allow an explicitly forced plug");
    assert_eq!(forced.plan.weapons.len(), 1);

    let mut stat_spec = bundled_every_end_spec();
    stat_spec.overrides.socket_columns.clear();
    stat_spec.overrides.investment_stats.push((u16::MAX, 50));
    let stat_error = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![stat_spec],
        },
    )
    .expect_err("the project builder must reject an unsupported stat definition");
    assert!(
        stat_error.to_string().contains("is not present in"),
        "unexpected stat error: {stat_error}"
    );
}
