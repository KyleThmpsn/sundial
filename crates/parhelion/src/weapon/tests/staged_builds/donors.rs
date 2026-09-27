use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_cross_slot_sword_builds_without_presentation_override() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let namespace = "parhelion.cross-slot-without-presentation.integration";
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
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
        kind: crate::ItemKind::Weapon,
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
            source: "Source: Guardians Made Their Own Fate".to_owned(),
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
        &item_string_sandbox_perk_segment(&solar_string_exemplar)
            .unwrap()
            .unwrap()[8..],
        &sandbox_string_template[8..],
        "the array marker, header and row must match independently of preceding context"
    );
    assert_eq!(item_string_sandbox_perk_count(&donor_strings).unwrap(), 0);
    assert_eq!(
        item_string_sandbox_perk_count(&authored_strings).unwrap(),
        1
    );
    let companion_start = donor_strings.len().next_multiple_of(8);
    assert_eq!(authored_strings.len(), companion_start + 0x40);
    assert!(
        authored_strings[donor_strings.len()..companion_start]
            .iter()
            .all(|byte| *byte == 0)
    );
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
        &item_string_sandbox_perk_segment(&authored_strings)
            .unwrap()
            .unwrap()[8..],
        &sandbox_string_template[8..]
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

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_donor_roles_use_first_indexed_rows_and_reject_invalid_inputs() {
    use crate::weapon::donors::{
        resolve_donor_item, resolve_icon_donor, resolve_presentation_donor,
        resolve_render_gear_donor, resolve_runtime_component_donor,
    };

    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let mut sources = crate::weapon::sources::load_project_sources(Path::new(&packages))
        .expect("clean stock sources should load");
    let gameplay_hash = 0xEE06_B019;
    let appearance_hash = 0x7405_1969;

    let gameplay = resolve_donor_item(&sources, gameplay_hash, "Donor")
        .expect("the gameplay donor should resolve");
    assert_eq!(
        gameplay.item_index, sources.stock_item_rows_by_hash[&gameplay_hash][0],
        "donor resolution must retain the first matching native row"
    );
    assert_eq!(
        resolve_item_name(&sources.manager, gameplay.string_tag).unwrap(),
        "The Mountaintop"
    );
    resolve_presentation_donor(
        &sources,
        &WeaponPresentationDonorReference {
            item_hash: appearance_hash,
            expected_name: Some("Truthteller".to_owned()),
        },
    )
    .expect("the geometry donor should resolve");
    resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: appearance_hash,
            expected_name: Some("Truthteller".to_owned()),
        },
    )
    .expect("the icon donor should resolve");
    resolve_render_gear_donor(
        &sources,
        &WeaponRenderGearDonorReference {
            item_hash: appearance_hash,
            expected_name: Some("Truthteller".to_owned()),
        },
    )
    .expect("the render-gear donor should resolve");
    resolve_runtime_component_donor(
        &sources,
        &WeaponRuntimeComponentDonorReference {
            binding_hash: 0x1234_5678,
            item_hash: gameplay_hash,
            expected_name: Some("The Mountaintop".to_owned()),
        },
    )
    .expect("the runtime donor should resolve");

    let error = resolve_icon_donor(
        &sources,
        &WeaponIconDonorReference {
            item_hash: appearance_hash,
            expected_name: Some("Wrong Name".to_owned()),
        },
    )
    .err()
    .expect("a mismatched icon donor name should be rejected")
    .to_string();
    assert!(
        error.contains("Icon donor item resolves to \"Truthteller\""),
        "{error}"
    );

    let error = resolve_render_gear_donor(
        &sources,
        &WeaponRenderGearDonorReference {
            item_hash: 0xFFFF_FFFE,
            expected_name: None,
        },
    )
    .err()
    .expect("a missing render-gear donor should be rejected")
    .to_string();
    assert!(
        error.contains("Render-gear donor item 0xFFFFFFFE is missing"),
        "{error}"
    );

    let string_row = sources.string_rows + gameplay.item_index * ITEM_ROW_SIZE;
    write_u32(&mut sources.stock_item_strings, string_row, appearance_hash).unwrap();
    let error = resolve_donor_item(&sources, gameplay_hash, "Donor")
        .err()
        .expect("a misaligned donor item and string row should be rejected")
        .to_string();
    assert!(
        error.contains("Donor item and item-string rows are not aligned"),
        "{error}"
    );
}

/// One shared setup for both cross-family outcomes: build the weapon, stage it into a
/// throwaway package view, and hand back the managers needed to read what the build wrote.
fn staged_cross_family_build(
    namespace: &str,
    name: &str,
    gameplay: (u32, &str),
    appearance: (u32, &str),
) -> (
    crate::weapon::NewWeaponProjectBundle,
    tempfile::TempDir,
    PathBuf,
) {
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages"),
    );
    let spec = WeaponCloneSpec {
        kind: crate::ItemKind::Weapon,
        namespace: namespace.to_owned(),
        donor_item_hash: gameplay.0,
        expected_donor_name: Some(gameplay.1.to_owned()),
        presentation_donor: Some(WeaponPresentationDonorReference {
            item_hash: appearance.0,
            expected_name: Some(appearance.1.to_owned()),
        }),
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("test namespace should allocate"),
        text: WeaponCloneText {
            name: name.to_owned(),
            flavor: "Cross-family appearance integration test.".to_owned(),
            source: "Source: integration test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides::default(),
    };
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("cross-family appearance should build");
    let source_root = packages.parent().unwrap();
    let view = tempfile::Builder::new()
        .prefix(".parhelion-cross-family-test-")
        .tempdir_in(source_root)
        .unwrap();
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
    let staged = bundle.write_new(&view_packages).unwrap();
    assert_eq!(staged.len(), bundle.artifacts.len());
    (bundle, view, packages)
}

/// The translation group the authored weapon's own sandbox-pattern row names, which is the
/// family whose rig and animations the entity is expected to carry.
fn authored_translation_group(manager: &PackageManager, definition: &[u8]) -> u32 {
    let globals = manager
        .read_tag(
            sundial::package_authoring::resolve_live_named_tag(manager, "investment_globals", None)
                .unwrap(),
        )
        .unwrap();
    let patterns = manager
        .read_tag(TagHash(read_u32(&globals, 16 + 70 * 16).unwrap()))
        .unwrap();
    sandbox_pattern_identity_at(
        &patterns,
        usize::from(weapon_pattern_index(definition).unwrap().unwrap()),
    )
    .unwrap()
    .unwrap()
    .weapon_translation_group_hash
}

const AUTO_RIFLE_GROUP: u32 = 0xCCC7_37C4;
const HAND_CANNON_GROUP: u32 = 0xC8CC_993A;

/// A hand cannon's model on an auto rifle's gameplay. The families interchange their rig and
/// animation components, so the build moves the appearance's presentation onto the private
/// runtime rather than pinning anything: the model keeps its own bones, and the authored row
/// keeps naming the appearance's family so the client resolves that family's animations.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_cross_family_appearance_moves_the_rig_when_the_families_interchange() {
    let (bundle, view, packages) = staged_cross_family_build(
        "parhelion.cross-family-rig.integration",
        "Old Devils",
        (0x23DB_942F, "Age-Old Bond"),
        (0x092D_8A05, "Better Devils"),
    );
    let plan = &bundle.plan.weapons[0];
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let definition = read_tag(&manager, plan.definition_tag, "authored definition").unwrap();
    let donor = read_tag(&stock, plan.template_definition_tag, "donor definition").unwrap();
    assert_eq!(authored_translation_group(&stock, &donor), AUTO_RIFLE_GROUP);
    assert_eq!(
        authored_translation_group(&manager, &definition),
        HAND_CANNON_GROUP,
        "the authored row should name the appearance's family once its rig is carried"
    );
    // Nothing is pinned, so the gear art is the appearance's own stock arrangement and the
    // build allocates no private geometry.
    let rows = weapon_art_arrangements(&definition).unwrap();
    assert_eq!(rows.len(), 1);
    let globals = manager
        .read_tag(
            sundial::package_authoring::resolve_live_named_tag(
                &manager,
                "investment_globals",
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let metadata = manager
        .read_tag(TagHash(read_u32(&globals, 0x430).unwrap()))
        .unwrap();
    let (count, _, meta_rows, _) =
        sundial::package_authoring::native_payload::native_array_at(&metadata, 8).unwrap();
    let row = usize::from(rows[0].arrangement);
    assert!(row < count);
    // The row is still the appearance's own stock arrangement, not a private clone owned by
    // the authored item, and the authored definition selects exactly the row the appearance
    // selects. A moved rig needs no private geometry at all.
    assert_ne!(
        read_u32(&metadata, meta_rows + row * 32).unwrap(),
        plan.item_hash,
        "a moved rig needs no private gear-art row"
    );
    assert_ne!(
        rows[0].arrangement,
        weapon_art_arrangements(&donor).unwrap()[0].arrangement,
        "the geometry should be the appearance's, not the gameplay donor's"
    );
}

/// A shotgun's model on an auto rifle's gameplay. The shotgun family places its skeleton's
/// event receiver at a different offset, so the rig cannot move. The build falls back to
/// private gear parts pinned to the gameplay rig's root bone, and the authored row names the
/// gameplay family because that is the rig those parts now ride.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_cross_family_appearance_pins_the_parts_when_the_rig_cannot_move() {
    let (bundle, view, packages) = staged_cross_family_build(
        "parhelion.cross-family-pin.integration",
        "Sudden Bond",
        (0x23DB_942F, "Age-Old Bond"),
        (0x7002_8208, "A Sudden Death"),
    );
    assert_parts_pinned(&bundle, &view, &packages);
}

/// A bow's model on a shotgun's gameplay. Bow limbs and strings blend several bones per vertex
/// instead of naming one, so pinning has to rewrite their blend weights as well.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_bow_appearance_pins_its_blended_parts() {
    let (bundle, view, packages) = staged_cross_family_build(
        "parhelion.cross-family-bow-pin.integration",
        "Spiteful Death",
        (0x7002_8208, "A Sudden Death"),
        (0x1920_B488, "The Spiteful Fang"),
    );
    assert!(
        assert_parts_pinned(&bundle, &view, &packages) > 0,
        "the bow should carry blended vertices"
    );
}

/// A legendary bow's model on an exotic bow's gameplay. The exotic has a translation group of
/// its own, so the appearance counts as another family, and the bow rig moves across. The
/// appearance's row selects the legendary's content block, which keeps firing the exotic's own
/// graph. Neither block carries a behavior record, so the graph is all that differs.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_bow_appearance_builds_on_an_exotic_bow() {
    let (bundle, view, packages) = staged_cross_family_build(
        "parhelion.cross-family-bow.integration",
        "Spiteful Ghoul",
        (0x3092_080D, "Trinity Ghoul"),
        (0x1920_B488, "The Spiteful Fang"),
    );
    let plan = &bundle.plan.weapons[0];
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let definition = read_tag(&manager, plan.definition_tag, "authored definition").unwrap();
    let donor = read_tag(&stock, plan.template_definition_tag, "donor definition").unwrap();
    assert_ne!(
        authored_translation_group(&manager, &definition),
        authored_translation_group(&stock, &donor),
        "the authored row should name the appearance's family once its rig is carried"
    );
    assert_ne!(
        weapon_art_arrangements(&definition).unwrap()[0].arrangement,
        weapon_art_arrangements(&donor).unwrap()[0].arrangement,
        "the geometry should be the appearance's, not the gameplay donor's"
    );

    assert_keeps_own_behavior(&bundle, &view, &packages, 0x3092_080D, 0x1920_B488, false);
}

/// An exotic bow's model on another exotic bow's gameplay, both with behavior records. The
/// appearance's row selects Wish-Ender's block, which fires Le Monarque's own graph and reads its
/// own state array and behavior record.
#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_bow_appearance_keeps_the_base_behavior_record() {
    let (bundle, view, packages) = staged_cross_family_build(
        "parhelion.cross-family-bow-record.integration",
        "Wishful Monarch",
        (0xD5EA_CCB7, "Le Monarque"),
        (0x3092_080C, "Wish-Ender"),
    );
    assert_keeps_own_behavior(&bundle, &view, &packages, 0xD5EA_CCB7, 0x3092_080C, true);
}

/// Checks that the content block the authored row selects fires the base weapon's own graph and,
/// when `records` is set, reaches the base weapon's own state array and behavior record.
fn assert_keeps_own_behavior(
    bundle: &crate::weapon::NewWeaponProjectBundle,
    view: &tempfile::TempDir,
    packages: &Path,
    base: u32,
    appearance: u32,
    records: bool,
) {
    use crate::weapon_behavior::{block_for_group, content, first_triple};
    use sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager;
    let plan = &bundle.plan.weapons[0];
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(packages).unwrap();
    let authored = load_weapon_runtime_entity_with_manager(&manager, plan.item_hash).unwrap();
    let base = load_weapon_runtime_entity_with_manager(&stock, base).unwrap();
    let appearance = load_weapon_runtime_entity_with_manager(&stock, appearance).unwrap();
    assert_eq!(
        authored.weapon_content_group_hash, appearance.weapon_content_group_hash,
        "the authored row should select the appearance's content block"
    );
    let built = content(&manager, &authored.payload).unwrap();
    let original = content(&stock, &base.payload).unwrap();
    let selected = block_for_group(&built, authored.weapon_content_group_hash).unwrap();
    let own = block_for_group(&original, base.weapon_content_group_hash).unwrap();
    let theirs = block_for_group(&original, appearance.weapon_content_group_hash).unwrap();
    let graph = |owner: &[u8], block: usize| read_u32(owner, block + 0xF0).unwrap();
    assert_ne!(
        graph(&original.owner, theirs),
        graph(&original.owner, own),
        "the two weapons should fire different graphs in stock"
    );
    assert_eq!(
        graph(&built.owner, selected),
        graph(&original.owner, own),
        "the weapon should fire its base weapon's own graph"
    );
    if !records {
        return;
    }
    // The state array and behavior record each slot of a block's first triple reaches.
    let reached = |owner: &[u8], block: usize| {
        let triple = first_triple(owner, block).unwrap();
        [0x10_usize, 0x20].map(|slot| {
            let at = triple + slot;
            let relative = i64::from_le_bytes(owner[at..at + 8].try_into().unwrap());
            let target = at
                .checked_add_signed(isize::try_from(relative).unwrap())
                .unwrap();
            owner[target - 4..target + 44].to_vec()
        })
    };
    assert_ne!(
        reached(&original.owner, theirs),
        reached(&original.owner, own),
        "the two weapons should carry different behavior records in stock"
    );
    assert_eq!(
        reached(&built.owner, selected),
        reached(&original.owner, own),
        "the weapon should read its base weapon's own behavior record"
    );
}

/// Walks one pinned build from its art row down to every vertex row, and returns how many rows
/// blended bones before pinning.
#[expect(
    clippy::cognitive_complexity,
    reason = "One staged build is walked from the art row down to its vertex rows in sequence"
)]
fn assert_parts_pinned(
    bundle: &crate::weapon::NewWeaponProjectBundle,
    view: &tempfile::TempDir,
    packages: &Path,
) -> usize {
    let plan = &bundle.plan.weapons[0];
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(packages).unwrap();
    let definition = read_tag(&manager, plan.definition_tag, "authored definition").unwrap();
    let donor = read_tag(&stock, plan.template_definition_tag, "donor definition").unwrap();
    assert_eq!(
        authored_translation_group(&manager, &definition),
        authored_translation_group(&stock, &donor),
        "pinned parts ride the gameplay rig, so the row must name that family"
    );
    let rows = weapon_art_arrangements(&definition).unwrap();
    assert_eq!(rows.len(), 1);
    let globals = manager
        .read_tag(
            sundial::package_authoring::resolve_live_named_tag(
                &manager,
                "investment_globals",
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let metadata = manager
        .read_tag(TagHash(read_u32(&globals, 0x430).unwrap()))
        .unwrap();
    let (count, _, meta_rows, _) =
        sundial::package_authoring::native_payload::native_array_at(&metadata, 8).unwrap();
    let row = usize::from(rows[0].arrangement);
    assert!(row < count);
    assert_eq!(
        read_u32(&metadata, meta_rows + row * 32).unwrap(),
        plan.item_hash,
        "pinned parts need a private gear-art row owned by the authored item"
    );
    // Every private part selects bone zero, so no vertex names a bone the gameplay rig lacks.
    let mut keys = vec![
        read_u32(&metadata, meta_rows + row * 32 + 8).unwrap(),
        read_u32(&metadata, meta_rows + row * 32 + 12).unwrap(),
    ];
    if crate::tag_payload::read_u64(&metadata, meta_rows + row * 32 + 16).unwrap() != 0 {
        let (slots, _, entries, _) = sundial::package_authoring::native_payload::native_array_at(
            &metadata,
            meta_rows + row * 32 + 16,
        )
        .unwrap();
        for slot in 0..slots {
            let resource =
                crate::tag_payload::relative_target(&metadata, entries + slot * 8).unwrap();
            let (n, _, assignments, _) =
                sundial::package_authoring::native_payload::native_array_at(
                    &metadata,
                    resource + 8,
                )
                .unwrap();
            for index in 0..n {
                keys.push(read_u32(&metadata, assignments + index * 4).unwrap());
            }
        }
    }
    keys.retain(|key| !matches!(*key, 0 | u32::MAX | 0x811C_9DC5));
    keys.sort_unstable();
    keys.dedup();
    assert!(!keys.is_empty());
    let map = manager.read_tag(TagHash(0x80EC_3F61)).unwrap();
    let (map_count, _, map_rows, _) =
        sundial::package_authoring::native_payload::native_array_at(&map, 8).unwrap();
    let mut parts = 0;
    let mut blended = 0;
    for key in keys {
        let relation = (0..map_count)
            .map(|index| map_rows + index * 8)
            .find(|&offset| read_u32(&map, offset).unwrap() == key)
            .map(|offset| read_u32(&map, offset + 4).unwrap())
            .unwrap_or_else(|| panic!("key {key:08X} is unmapped"));
        assert!(stock.get_entry(TagHash(relation)).is_none());
        let entity_tag = read_u32(&manager.read_tag(TagHash(relation)).unwrap(), 0x10).unwrap();
        let entity = manager.read_tag(TagHash(entity_tag)).unwrap();
        let (components, _, component_rows, _) =
            sundial::package_authoring::native_payload::native_array_at(&entity, 0x10).unwrap();
        for index in 0..components {
            let tag = read_u32(&entity, component_rows + index * 12).unwrap();
            let Ok(bytes) = manager.read_tag(TagHash(tag)) else {
                continue;
            };
            let header = crate::tag_payload::relative_target(&bytes, 0x10).unwrap();
            if read_u32(&bytes, header - 4).unwrap() != 0x8080_72B8 {
                continue;
            }
            let data = crate::tag_payload::relative_target(&bytes, 0x18).unwrap();
            let model = manager
                .read_tag(TagHash(read_u32(&bytes, data + 0x1DC).unwrap()))
                .unwrap();
            assert_eq!(read_u32(&model, 0x40).unwrap(), 1, "bone palette");
            let (meshes, _, mesh_rows, _) =
                sundial::package_authoring::native_payload::native_array_at(&model, 0x10).unwrap();
            for mesh in 0..meshes {
                let header_tag = read_u32(&model, mesh_rows + mesh * 0x88).unwrap();
                let entry = manager.get_entry(TagHash(header_tag)).unwrap();
                let header = manager.read_tag(TagHash(header_tag)).unwrap();
                let stride = usize::from(crate::tag_payload::read_u16(&header, 4).unwrap());
                let positions = manager.read_tag(TagHash(entry.reference)).unwrap();
                for row in positions.chunks_exact(stride) {
                    // A blended row keeps its 0x7FFF selector and weighs bone 0 alone, with
                    // every other slot naming bone 254 as native one-bone rows do.
                    let pinned = match (&row[6..8], stride) {
                        ([0, 0], _) => true,
                        ([0xFF, 0x7F], 12) => row[8..12] == [0, 0xFE, 0xFF, 0],
                        ([0xFF, 0x7F], 16) => row[8..16] == [0xFF, 0, 0, 0, 0, 0xFE, 0xFE, 0xFE],
                        _ => false,
                    };
                    assert!(
                        pinned,
                        "a pinned {stride}-byte row still selects another bone"
                    );
                    blended += usize::from(row[6..8] != [0, 0]);
                }
            }
        }
        parts += 1;
    }
    assert!(parts > 0);
    blended
}
