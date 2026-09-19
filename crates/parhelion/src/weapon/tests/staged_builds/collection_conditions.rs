use super::*;

mod donor_variants;

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_weapons_with_non_single_flag_acquisition_build() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let mut weapons = affected_weapons(&sources);
    assert_eq!(weapons.len(), 86);
    assert_eq!(
        weapons
            .iter()
            .map(|weapon| &weapon.text.name)
            .collect::<BTreeSet<_>>()
            .len(),
        85
    );
    let khvostov = weapons
        .iter()
        .find(|weapon| weapon.donor_item_hash == 0x6080_3CD7)
        .unwrap()
        .clone();
    for rarity in [
        AuthoredWeaponRarity::Legendary,
        AuthoredWeaponRarity::Exotic,
    ] {
        let mut weapon = khvostov.clone();
        weapon.namespace = format!("{}.{}", weapon.namespace, rarity.package_value());
        weapon.identity = WeaponCloneIdentity::from_namespace(&weapon.namespace).unwrap();
        weapon.overrides.rarity = Some(rarity);
        weapon.overrides.collection_destination =
            Some(crate::collection::Family::AutoRifles.template());
        weapons.push(weapon);
    }
    let project = WeaponProjectSpec { weapons };
    let bundle = build_weapon_project(&packages, &project)
        .expect("weapons with alternative stock acquisition conditions should build");
    assert_eq!(bundle.plan.weapons.len(), 88);
    verify_staged_collections(&packages, &sources, &project, &bundle);
}

fn affected_weapons(sources: &sources::ProjectSources) -> Vec<WeaponCloneSpec> {
    let mut weapons = Vec::new();
    let mut conditions = [0usize; 3];
    for index in 0..sources.stock_collectible_count {
        let row = sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE;
        let item = usize::from(
            read_u16(
                &sources.stock_collectibles,
                row + COLLECTIBLE_ITEM_INDEX_OFFSET,
            )
            .unwrap(),
        );
        if item >= sources.stock_item_count {
            continue;
        }
        let item_row = sources.item_rows + item * ITEM_ROW_SIZE;
        let definition = read_tag(
            &sources.manager,
            TagHash(read_u32(&sources.stock_item_table, item_row + 16).unwrap()),
            "weapon acquisition regression donor",
        )
        .unwrap();
        if weapon_inventory_slot(&definition).is_err() || weapon_rarity(&definition).is_err() {
            continue;
        }
        if collection_unlock_index(&sources.stock_collectibles, row).is_ok() {
            continue;
        }
        let hash = read_u32(&sources.stock_item_table, item_row).unwrap();
        let string_tag = TagHash(
            read_u32(
                &sources.stock_item_strings,
                sources.string_rows + item * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        );
        let name = resolve_item_name(&sources.manager, string_tag).unwrap();
        let descriptor = row + crate::progression::COLLECTIBLE_CONDITION_OFFSET;
        if read_u64(&sources.stock_collectibles, descriptor).unwrap() == 0 {
            conditions[0] += 1;
        } else {
            let tokens = numeric_program_layout(&sources.stock_collectibles, descriptor)
                .unwrap()
                .tokens;
            conditions[if tokens == [(11, 1)] { 1 } else { 2 }] += 1;
        }
        eprintln!("Affected: {name} 0x{hash:08X}");
        let namespace = format!("parhelion.acquisition-regression.{hash:08x}");
        weapons.push(WeaponCloneSpec {
            identity: WeaponCloneIdentity::from_namespace(&namespace).unwrap(),
            namespace,
            donor_item_hash: hash,
            expected_donor_name: Some(name.clone()),
            presentation_donor: None,
            icon_donor: None,
            render_gear_donor: None,
            runtime_component_donors: Vec::new(),
            text: WeaponCloneText {
                name,
                source: "Source: acquisition regression".to_owned(),
                ..Default::default()
            },
            overrides: WeaponCloneOverrides::default(),
        });
    }
    assert_eq!(
        conditions,
        [11, 73, 2],
        "empty, always acquired, multiple flags"
    );
    weapons
}

fn verify_staged_collections(
    packages: &Path,
    sources: &sources::ProjectSources,
    project: &WeaponProjectSpec,
    bundle: &NewWeaponProjectBundle,
) {
    let view = staged_view(packages, ".parhelion-acquisition-test-", bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let collectibles = read_tag(
        &manager,
        sources.collectible_table_tag,
        "staged collectibles",
    )
    .unwrap();
    let nodes = read_tag(
        &manager,
        sources.presentation_node_table_tag,
        "staged Collections nodes",
    )
    .unwrap();
    let mut unlocks = BTreeSet::new();
    let mut khvostov_pages = BTreeMap::new();
    for plan in &bundle.plan.weapons {
        let spec = project
            .weapons
            .iter()
            .find(|weapon| weapon.identity.item_hash == plan.item_hash)
            .unwrap();
        let (rarity, page) =
            verify_staged_weapon(&manager, sources, &collectibles, &nodes, spec, plan);
        assert!(unlocks.insert(plan.unlock_definition_index));
        if spec.donor_item_hash == 0x6080_3CD7 {
            khvostov_pages.insert(rarity.package_value(), page);
        }
    }
    if khvostov_pages.len() == 3 {
        assert_eq!(
            khvostov_pages[&AuthoredWeaponRarity::Common.package_value()],
            khvostov_pages[&AuthoredWeaponRarity::Legendary.package_value()]
        );
        assert_ne!(
            khvostov_pages[&AuthoredWeaponRarity::Common.package_value()],
            khvostov_pages[&AuthoredWeaponRarity::Exotic.package_value()]
        );
    }
    verify_stock_conditions(sources, &collectibles);
}

fn verify_staged_weapon(
    manager: &PackageManager,
    sources: &sources::ProjectSources,
    collectibles: &[u8],
    nodes: &[u8],
    spec: &WeaponCloneSpec,
    plan: &NewWeaponPlan,
) -> (AuthoredWeaponRarity, u16) {
    let definition = read_tag(manager, plan.definition_tag, "authored definition").unwrap();
    let source = read_tag(
        &sources.manager,
        plan.template_definition_tag,
        "stock donor",
    )
    .unwrap();
    assert_eq!(
        read_tag(
            manager,
            plan.template_definition_tag,
            "unchanged stock donor"
        )
        .unwrap(),
        source
    );
    assert_eq!(plan.template_item_hash, spec.donor_item_hash);
    verify_staged_appearance(manager, sources, spec, &definition, &source);
    assert_eq!(
        resolve_item_name(manager, plan.string_tag).unwrap(),
        spec.text.name
    );
    assert_eq!(
        weapon_default_plug_indices(&definition).unwrap(),
        weapon_default_plug_indices(&source).unwrap()
    );
    let (_, _, rows, _) = array_at(collectibles, 8).unwrap();
    let row = rows + usize::from(plan.collectible_index) * COLLECTIBLE_ROW_SIZE;
    for field in crate::progression::COLLECTIBLE_SECONDARY_CONDITION_OFFSETS {
        assert_eq!(&collectibles[row + field..row + field + 16], &[0; 16]);
    }
    assert_eq!(
        read_u16(collectibles, row + COLLECTIBLE_ITEM_INDEX_OFFSET).unwrap(),
        plan.item_index
    );
    assert_eq!(
        numeric_program_layout(
            collectibles,
            row + crate::progression::COLLECTIBLE_CONDITION_OFFSET
        )
        .unwrap()
        .tokens,
        [(
            crate::progression::NUMERIC_FLAG_INSTRUCTION,
            plan.unlock_definition_index
        )]
    );
    assert!(usize::from(plan.unlock_definition_index) >= sources.stock_unlock_count);
    let parents =
        template_presentation_parents(nodes, collectibles, usize::from(plan.collectible_index))
            .unwrap();
    assert_eq!(
        parents.len(),
        4,
        "Collections page and three Sunrise badge classes"
    );
    for badge in [925, 926, 927] {
        assert!(parents.contains(&badge));
    }
    let rarity = weapon_rarity(&definition).unwrap();
    assert_eq!(
        rarity,
        spec.overrides
            .rarity
            .unwrap_or(weapon_rarity(&source).unwrap())
    );
    let page = parents
        .iter()
        .copied()
        .find(|parent| ![925, 926, 927].contains(parent))
        .unwrap();
    (rarity, page)
}

fn verify_staged_appearance(
    manager: &PackageManager,
    sources: &sources::ProjectSources,
    spec: &WeaponCloneSpec,
    definition: &[u8],
    source: &[u8],
) {
    let appearance = spec.presentation_donor.as_ref().map(|reference| {
        let donor = crate::weapon::donors::resolve_donor_item(
            sources,
            reference.item_hash,
            "geometry verification",
        )
        .unwrap();
        let expected = read_tag(&sources.manager, donor.definition_tag, "stock geometry").unwrap();
        assert_eq!(
            read_tag(manager, donor.definition_tag, "unchanged stock geometry").unwrap(),
            expected
        );
        expected
    });
    let appearance = appearance.as_deref().unwrap_or(source);
    assert_eq!(
        weapon_art_arrangements(definition).unwrap(),
        weapon_art_arrangements(appearance).unwrap()
    );
    assert_eq!(
        weapon_render_dye_rows(definition).unwrap(),
        weapon_render_dye_rows(appearance).unwrap()
    );
}

fn verify_stock_conditions(sources: &sources::ProjectSources, collectibles: &[u8]) {
    let (_, _, rows, _) = array_at(collectibles, 8).unwrap();
    for index in 0..sources.stock_collectible_count {
        for field in [
            0x30,
            0x40,
            0x50,
            0x60,
            crate::progression::COLLECTIBLE_CONDITION_OFFSET,
        ] {
            let before = sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE + field;
            let after = rows + index * COLLECTIBLE_ROW_SIZE + field;
            if read_u64(&sources.stock_collectibles, before).unwrap() == 0 {
                assert_eq!(
                    &collectibles[after..after + 16],
                    &sources.stock_collectibles[before..before + 16]
                );
            } else {
                // Compare stock instruction bytes without imposing the weapon author's
                // canonical trailer padding on unrelated stock collectible programs.
                let (old_count, old_header, old_rows, old_class) =
                    array_at(&sources.stock_collectibles, before).unwrap();
                let (new_count, new_header, new_rows, new_class) =
                    array_at(collectibles, after).unwrap();
                assert_eq!((new_count, new_class), (old_count, old_class));
                assert_eq!(
                    &collectibles[new_header..new_rows + new_count * NUMERIC_INSTRUCTION_ROW_SIZE],
                    &sources.stock_collectibles
                        [old_header..old_rows + old_count * NUMERIC_INSTRUCTION_ROW_SIZE],
                    "stock collectible {index} condition 0x{field:X} changed"
                );
            }
        }
    }
}
