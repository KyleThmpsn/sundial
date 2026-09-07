use super::*;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn clean_stock_sunrise_build_enrolls_the_weapon_icon_graph_in_the_native_host() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let spec = bundled_every_end_spec();
    let bundle = build_weapon_project_after_catalog_validation(
        Path::new(&packages),
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .expect("configured clean-stock Sunrise build should succeed");
    let weapon_plan = &bundle.plan.weapons[0];
    let host = bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == HOST_PACKAGE_ID)
        .expect("bundle should contain the host overlay");
    assert_eq!(host.plan.original_entry_count, HOST_EXPECTED_ENTRY_COUNT);
    assert_eq!(host.plan.final_entry_count, HOST_EXPECTED_ENTRY_COUNT + 17);
    assert_eq!(host.plan.appended_tags.len(), 17);
    assert_eq!(host.plan.appended_tags[0].tag, weapon_plan.definition_tag);
    assert_eq!(host.plan.appended_tags[1].tag, weapon_plan.string_tag);
    assert_eq!(
        host.plan
            .appended_tags
            .get(host.plan.appended_tags.len() - 2)
            .map(|entry| entry.tag),
        Some(weapon_plan.icon_definition_tag)
    );
    assert_eq!(
        host.plan.appended_tags.last().map(|entry| entry.tag),
        Some(
            crate::shared_tag_memory::adjacent_companion_tag(weapon_plan.icon_definition_tag)
                .expect("weapon icon should have an adjacent companion tag")
        )
    );
    let assets = bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.plan.chain.identity.package_id == PARHELION_ASSET_PACKAGE_ID)
        .expect("bundle should retain the badge asset package");
    assert_eq!(assets.plan.appended_tags.len(), 7);
    assert_eq!(
        assets.plan.appended_tags[5].tag,
        bundle.plan.sunrise.badge_icon_tag
    );
    if let Some(staging) = std::env::var_os("PARHELION_TEST_STAGE") {
        let paths = bundle
            .write_new(Path::new(&staging))
            .expect("configured Sunrise staging should remain create-new only");
        assert_eq!(paths.len(), 6);
    }
}

#[test]
#[ignore = "requires PARHELION_PROJECTILE_TEST_PACKAGES pointing to Shadowkeep packages"]
fn real_private_intrinsic_classification_preserves_perks_and_native_socket() {
    use crate::plug_classification::PlugClassification;
    use sundial::package_authoring::investment_schema::ITEM_STRING_UI_TEMPLATE_HASH_OFFSET;
    let packages = PathBuf::from(std::env::var_os("PARHELION_PROJECTILE_TEST_PACKAGES").unwrap());
    let manager = open_manager(&packages).unwrap();
    let globals = read_tag(
        &manager,
        resolve_live_named_tag(&manager, "investment_globals", None).unwrap(),
        "globals",
    )
    .unwrap();
    let root = read_tag(&manager, TagHash(read_u32(&globals, 16).unwrap()), "root").unwrap();
    let items = read_tag(
        &manager,
        root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        "items",
    )
    .unwrap();
    let strings = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap(),
        "strings",
    )
    .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let (_, _, string_rows, _) = array_at(&strings, 8).unwrap();
    let by_hash = index_item_rows_by_hash(&items, rows, count).unwrap();
    let load = |hash: u32| {
        let index = by_hash[&hash][0];
        assert_eq!(by_hash[&hash].len(), 1);
        assert_eq!(
            read_u32(&strings, string_rows + index * ITEM_ROW_SIZE).unwrap(),
            hash
        );
        (
            read_tag(
                &manager,
                TagHash(read_u32(&items, rows + index * ITEM_ROW_SIZE + 16).unwrap()),
                "definition",
            )
            .unwrap(),
            read_tag(
                &manager,
                TagHash(read_u32(&strings, string_rows + index * ITEM_ROW_SIZE + 16).unwrap()),
                "item strings",
            )
            .unwrap(),
        )
    };
    let (frame, frame_strings) = load(0xC684_24BC);
    let classification = PlugClassification::from_template(&frame, &frame_strings).unwrap();
    assert_eq!(classification.category, 0x67FB_A961);
    let (stock, stock_strings) = load(0xDD5C_B37A);
    assert_eq!(
        PlugClassification::from_template(&stock, &stock_strings)
            .unwrap()
            .category,
        0x0078_A617
    );
    let (mut edited, mut edited_strings) = (stock.clone(), stock_strings.clone());
    classification
        .apply(&mut edited, &mut edited_strings)
        .unwrap();
    assert_eq!(weapon_sandbox_perks(&edited).unwrap(), vec![1178, 416]);
    assert_eq!(
        PlugClassification::from_template(&edited, &edited_strings).unwrap(),
        classification
    );
    assert_eq!(edited[ITEM_RARITY_OFFSET], frame[ITEM_RARITY_OFFSET]);
    // Restore only native classification; every runtime byte must match.
    edited[0x188..0x18C].copy_from_slice(&stock[0x188..0x18C]);
    edited[ITEM_RARITY_OFFSET] = stock[ITEM_RARITY_OFFSET];
    edited_strings[ITEM_TYPE_REFERENCE_OFFSET..ITEM_TYPE_REFERENCE_OFFSET + 8].copy_from_slice(
        &stock_strings[ITEM_TYPE_REFERENCE_OFFSET..ITEM_TYPE_REFERENCE_OFFSET + 8],
    );
    edited_strings[ITEM_STRING_UI_TEMPLATE_HASH_OFFSET..ITEM_STRING_UI_TEMPLATE_HASH_OFFSET + 4]
        .copy_from_slice(
            &stock_strings
                [ITEM_STRING_UI_TEMPLATE_HASH_OFFSET..ITEM_STRING_UI_TEMPLATE_HASH_OFFSET + 4],
        );
    assert_eq!((edited, edited_strings), (stock, stock_strings));

    if std::env::var_os("PARHELION_VERIFY_INSTALLED_INTRINSIC").is_some() {
        let (weapon, _) = load(0x72F7_F442);
        let defaults = weapon_default_plug_indices(&weapon).unwrap();
        let intrinsic_hash =
            read_u32(&items, rows + usize::from(defaults[0]) * ITEM_ROW_SIZE).unwrap();
        assert_eq!(intrinsic_hash, 0x9730_5699);
        assert_ne!(defaults[3], defaults[0]);
        assert_ne!(defaults[4], defaults[0]);
        let resource = relative_target(&weapon, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
        let (_, _, socket_rows, _) = array_at(&weapon, resource).unwrap();
        assert_eq!(read_u16(&weapon, socket_rows).unwrap(), 176);
        let (private, private_strings) = load(intrinsic_hash);
        assert_eq!(
            PlugClassification::from_template(&private, &private_strings).unwrap(),
            classification
        );
        assert_eq!(weapon_sandbox_perks(&private).unwrap(), vec![2481, 416]);
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn stock_weapon_icon_rows_are_keyed_by_their_item_hash_when_configured() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let packages = PathBuf::from(packages);
    let manager = open_manager(&packages).expect("clean stock packages should open");
    let globals =
        resolve_live_named_tag(&manager, "investment_globals", None).expect("globals tag");
    let globals_data = read_tag(&manager, globals, "investment globals").unwrap();
    let root_tag = TagHash(read_u32(&globals_data, 16).unwrap());
    let root = read_tag(&manager, root_tag, "investment root").unwrap();
    let item_table = read_tag(
        &manager,
        root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap(),
        "item table",
    )
    .unwrap();
    let item_strings = read_tag(
        &manager,
        globals_child_tag(&globals_data, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap(),
        "item strings",
    )
    .unwrap();
    let icons = read_tag(
        &manager,
        globals_child_tag(&globals_data, GLOBALS_ITEM_ICON_TABLE_SLOT).unwrap(),
        "item icons",
    )
    .unwrap();
    let (item_count, _, item_rows, _) = array_at(&item_table, 8).unwrap();
    let (_, _, string_rows, _) = array_at(&item_strings, 8).unwrap();
    let (_, _, icon_rows, _) = array_at(&icons, 8).unwrap();

    for item_hash in [0xA25B_8F8F, 0x7405_1969] {
        let item_index =
            find_u32_row_key(&item_table, item_rows, item_count, ITEM_ROW_SIZE, item_hash)
                .unwrap()
                .expect("display donor should exist");
        let string_tag = TagHash(
            read_u32(&item_strings, string_rows + item_index * ITEM_ROW_SIZE + 16).unwrap(),
        );
        let string_payload = read_tag(&manager, string_tag, "display donor strings").unwrap();
        let icon_index =
            usize::from(read_u16(&string_payload, ITEM_STRING_ICON_INDEX_OFFSET).unwrap());
        assert_eq!(
            read_u32(&icons, icon_rows + icon_index * ITEM_ICON_ROW_SIZE).unwrap(),
            item_hash,
            "item icon row {icon_index} should be keyed by its owning item"
        );
    }
}
