use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_cross_slot_sword_builds_without_presentation_override() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let namespace = "parhelion.cross-slot-without-presentation.integration";
    let spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: 0x4659_8066,
        expected_donor_name: Some("Crown-Splitter".to_owned()),
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: "Energy Sword".to_owned(),
            flavor: "Retains the native sword model and animation family.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            inventory_slot: Some(WeaponInventorySlot::Energy),
            ammo_type: Some(WeaponAmmoType::Special),
            rarity: Some(AuthoredWeaponRarity::Exotic),
            ..WeaponCloneOverrides::default()
        },
    };
    let result = build_weapon_project(
        Path::new(&packages),
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    );
    result.expect("Energy/Special sword should build without a presentation override");
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
#[allow(clippy::cognitive_complexity)]
fn real_mountaintop_energy_solar_clone_preserves_socket_topology_when_configured() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let packages = PathBuf::from(packages);
    let namespace = "parhelion.second-sun.integration";
    let spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: 0xEE06_B019,
        expected_donor_name: Some("The Mountaintop".to_owned()),
        presentation_donor: Some(WeaponPresentationDonorReference {
            item_hash: 0x7405_1969,
            expected_name: Some("Truthteller".to_owned()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("Second Sun namespace should allocate"),
        text: WeaponCloneText {
            name: "Second Sun".to_owned(),
            flavor: "A second dawn, made by our own hands.".to_owned(),
            source: "Source: Guardians Make Their Own Fate".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            inventory_slot: Some(WeaponInventorySlot::Energy),
            modern_damage_type: Some(ModernDamageType::Solar),
            power_cap_group: Some(11),
            ..WeaponCloneOverrides::default()
        },
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("configured clean-stock Second Sun build should succeed");
    let weapon_plan = &bundle.plan.weapons[0];

    let source_root = packages
        .parent()
        .expect("configured package directory should have a parent");
    let view = tempfile::Builder::new()
        .prefix(".parhelion-second-sun-test-")
        .tempdir_in(source_root)
        .expect("temporary package view should be created on the package volume");
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).expect("temporary packages directory should be created");
    for entry in fs::read_dir(&packages).expect("clean-stock packages should be listable") {
        let entry = entry.expect("clean-stock package entry should be readable");
        let source = entry.path();
        if source.extension().and_then(|value| value.to_str()) != Some("pkg") {
            continue;
        }
        fs::hard_link(&source, view_packages.join(entry.file_name()))
            .expect("clean-stock package should hard-link into the temporary view");
    }
    let source_oodle = source_root
        .join("bin")
        .join("x64")
        .join("oo2core_3_win64.dll");
    if source_oodle.is_file() {
        let target_bin = view.path().join("bin").join("x64");
        fs::create_dir_all(&target_bin).expect("temporary Oodle directory should be created");
        fs::hard_link(&source_oodle, target_bin.join("oo2core_3_win64.dll"))
            .expect("Oodle runtime should hard-link into the temporary view");
    }
    let staged = bundle
        .write_new(&view_packages)
        .expect("Second Sun overlays should stage create-new in the temporary view");
    assert_eq!(staged.len(), bundle.artifacts.len());

    let source_manager = open_manager(&packages).expect("clean-stock manager should open");
    let donor_definition = read_tag(
        &source_manager,
        weapon_plan.template_definition_tag,
        "Mountaintop definition",
    )
    .expect("Mountaintop definition should load");
    assert_eq!(
        weapon_inventory_slot(&donor_definition).unwrap(),
        WeaponInventorySlot::Kinetic
    );
    assert_eq!(
        weapon_damage_descriptor(&donor_definition).unwrap(),
        WeaponDamageDescriptor::Empty
    );

    let authored_manager = open_manager(&view_packages).expect("staged package view should open");
    let authored_definition = read_tag(
        &authored_manager,
        weapon_plan.definition_tag,
        "Second Sun definition",
    )
    .expect("Second Sun definition should load from the staged package");
    assert_eq!(
        read_u64(&authored_definition, 0).unwrap() as usize,
        authored_definition.len()
    );
    assert_eq!(
        weapon_inventory_slot(&authored_definition).unwrap(),
        WeaponInventorySlot::Energy
    );
    assert_eq!(
        weapon_damage_descriptor(&authored_definition).unwrap(),
        WeaponDamageDescriptor::Elemental(ModernDamageType::Solar)
    );
    let version = weapon_version_array(&authored_definition).unwrap().unwrap();
    assert!(!version.groups.is_empty());
    assert!(version.groups.iter().all(|group| *group == 11));
    let donor_sockets = weapon_default_plug_indices(&donor_definition).unwrap();
    let authored_sockets = weapon_default_plug_indices(&authored_definition).unwrap();
    assert_eq!(donor_sockets.len(), 10);
    assert_eq!(authored_sockets, donor_sockets);

    let source_globals = resolve_live_named_tag(&source_manager, "investment_globals", None)
        .expect("investment globals should be named");
    let source_globals = read_tag(&source_manager, source_globals, "investment globals")
        .expect("investment globals should load");
    let item_strings = read_tag(
        &source_manager,
        globals_child_tag(&source_globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap(),
        "item strings",
    )
    .expect("item strings should load");
    let (item_count, _, item_rows, _) = array_at(&item_strings, 8).unwrap();
    let truthteller_index = find_u32_row_key(
        &item_strings,
        item_rows,
        item_count,
        ITEM_ROW_SIZE,
        0x7405_1969,
    )
    .unwrap()
    .expect("Truthteller should exist in the clean-stock item table");
    let truthteller_string_tag = TagHash(
        read_u32(
            &item_strings,
            item_rows + truthteller_index * ITEM_ROW_SIZE + 16,
        )
        .unwrap(),
    );
    let donor_strings = read_tag(
        &source_manager,
        weapon_plan.template_string_tag,
        "Mountaintop item strings",
    )
    .expect("Mountaintop item strings should load");
    let truthteller_strings = read_tag(
        &source_manager,
        truthteller_string_tag,
        "Truthteller item strings",
    )
    .expect("Truthteller item strings should load");
    let authored_strings = read_tag(
        &authored_manager,
        weapon_plan.string_tag,
        "Second Sun item strings",
    )
    .expect("Second Sun item strings should load");
    let sandbox_string_template =
        canonical_item_sandbox_perk_string_template(&source_manager, &item_strings)
            .expect("stock elemental item-string template should resolve");
    let solar_string_exemplar = read_tag(
        &source_manager,
        TagHash(0x8133_741E),
        "Martyr's Retribution item strings",
    )
    .expect("stock Solar item-string template should load");
    assert_eq!(
        item_string_sandbox_perk_segment(&solar_string_exemplar)
            .unwrap()
            .unwrap(),
        sandbox_string_template
    );
    assert_eq!(item_string_sandbox_perk_count(&donor_strings).unwrap(), 0);
    assert_eq!(
        item_string_sandbox_perk_count(&authored_strings).unwrap(),
        1
    );
    assert_eq!(authored_strings.len(), donor_strings.len() + 0x40);
    assert_eq!(
        read_u64(&authored_strings, 0).unwrap() as usize,
        authored_strings.len()
    );
    assert_eq!(
        item_string_client_classification(&authored_strings, WeaponInventorySlot::Energy).unwrap(),
        item_string_client_classification(&truthteller_strings, WeaponInventorySlot::Energy)
            .unwrap(),
        "Second Sun must inherit the complete Energy grenade-launcher client tuple"
    );
    assert_ne!(
        item_string_client_classification(&authored_strings, WeaponInventorySlot::Energy).unwrap(),
        item_string_client_classification(&donor_strings, WeaponInventorySlot::Kinetic).unwrap(),
        "Second Sun must not retain Mountaintop's Kinetic client tuple"
    );
    assert_eq!(
        item_string_sandbox_perk_segment(&authored_strings)
            .unwrap()
            .unwrap(),
        sandbox_string_template
    );
    assert_eq!(
        relative_target(
            &authored_strings,
            ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET,
        )
        .unwrap(),
        relative_target(
            &donor_strings,
            ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET,
        )
        .unwrap()
    );
    assert_eq!(
        relative_target(&authored_strings, 0x70).unwrap(),
        relative_target(&donor_strings, 0x70).unwrap(),
        "out-of-line sandbox companion authoring must not relocate unrelated donor arrays"
    );
    validate_weapon_sandbox_perk_parallelism(
        &authored_definition,
        &authored_strings,
        &sandbox_string_template,
    )
    .expect("Second Sun definition and item-string sandbox rows should remain parallel");
    let socket_names = donor_sockets
        .iter()
        .copied()
        .filter(|index| usize::from(*index) < item_count && *index != u16::MAX)
        .map(|index| {
            let string_tag = TagHash(
                read_u32(
                    &item_strings,
                    item_rows + usize::from(index) * ITEM_ROW_SIZE + 16,
                )
                .unwrap(),
            );
            resolve_item_name(&source_manager, string_tag)
                .expect("Mountaintop socket plug should resolve")
        })
        .collect::<Vec<_>>();
    assert!(socket_names.iter().any(|name| name == "Micro-Missile"));
}
