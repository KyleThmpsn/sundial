use super::*;

fn regression_weapons(group: &str, include_dummies: bool) -> Vec<WeaponCloneSpec> {
    // Native-verified cases from the 776 selectable stock weapons in the Shadowkeep catalog.
    // Hashes distinguish collectible-free variants from same-name Collections weapons.
    // Dummy classification follows Sundial's shared src/dummy_items.rs list.
    let cases: serde_json::Value = serde_json::from_str(include_str!("stock_cases.json")).unwrap();
    cases[group]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| include_dummies || case["dummy"].as_bool() != Some(true))
        .map(|case| {
            let hash =
                u32::from_str_radix(case["hash"].as_str().unwrap().trim_start_matches("0x"), 16)
                    .unwrap();
            let name = case["name"].as_str().unwrap().to_owned();
            let namespace = format!("parhelion.collections.{group}.{hash:08x}");
            WeaponCloneSpec {
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
                    source: "Source: Collections regression".to_owned(),
                    ..Default::default()
                },
                overrides: WeaponCloneOverrides::default(),
            }
        })
        .collect()
}

fn collectible_indices(sources: &sources::ProjectSources, hash: u32) -> Vec<usize> {
    let item = sources.stock_item_rows_by_hash[&hash][0];
    (0..sources.stock_collectible_count)
        .filter(|index| {
            usize::from(
                read_u16(
                    &sources.stock_collectibles,
                    sources.collectible_rows
                        + index * COLLECTIBLE_ROW_SIZE
                        + COLLECTIBLE_ITEM_INDEX_OFFSET,
                )
                .unwrap(),
            ) == item
        })
        .collect()
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_collectible_free_weapons_build_as_bases_and_geometry_donors() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let mut weapons = regression_weapons("missing_collectible", false);
    assert_eq!(weapons.len(), 9);
    let unsupported = verify_native_base_requirements(&sources, &weapons);
    assert_eq!(
        unsupported,
        BTreeSet::from([0x67F5_15B2, 0xEFD9_F21F, 0x654C_35C4])
    );
    let mut appearances = Vec::new();
    let mut without_peer = Vec::new();
    for base in &weapons {
        assert!(
            collectible_indices(&sources, base.donor_item_hash).is_empty(),
            "{}",
            base.text.name
        );
        let reference = WeaponPresentationDonorReference {
            item_hash: base.donor_item_hash,
            expected_name: base.expected_donor_name.clone(),
        };
        crate::weapon::donors::resolve_presentation_donor(&sources, &reference).unwrap();
        let Some(mut geometry) = compatible_geometry_project(&sources, base) else {
            without_peer.push(base.donor_item_hash);
            continue;
        };
        geometry.namespace = format!("{}.geometry", base.namespace);
        geometry.identity = WeaponCloneIdentity::from_namespace(&geometry.namespace).unwrap();
        geometry.presentation_donor = Some(reference);
        appearances.push(geometry);
    }
    assert!(without_peer.is_empty(), "{without_peer:X?}");
    assert_eq!(appearances.len(), 9);
    weapons.retain(|weapon| !unsupported.contains(&weapon.donor_item_hash));
    assert_eq!(weapons.len(), 6);
    weapons.extend(appearances);
    for (hash, rarity) in [
        (0x032B_2570, AuthoredWeaponRarity::Legendary),
        (0x032B_2570, AuthoredWeaponRarity::Exotic),
    ] {
        let mut weapon = weapons
            .iter()
            .find(|weapon| weapon.donor_item_hash == hash)
            .unwrap()
            .clone();
        weapon.namespace = format!("{}.rarity.{}", weapon.namespace, rarity.package_value());
        weapon.identity = WeaponCloneIdentity::from_namespace(&weapon.namespace).unwrap();
        weapon.overrides.rarity = Some(rarity);
        weapons.push(weapon);
    }
    let project = WeaponProjectSpec { weapons };
    let bundle = build_weapon_project(&packages, &project)
        .expect("compatible collectible-free variants should build as bases and geometry donors");
    assert_eq!(bundle.plan.weapons.len(), 17);
    verify_staged_collections(&packages, &sources, &project, &bundle);
}

fn verify_native_base_requirements(
    sources: &sources::ProjectSources,
    weapons: &[WeaponCloneSpec],
) -> BTreeSet<u32> {
    let socketless = BTreeSet::from([
        0xB180_EBE6,
        0xC07A_C8FB,
        0xACA4_9190,
        0x1B76_17AC,
        0x2090_FACD,
        0x7FF3_47D1,
    ]);
    let mut rejected = BTreeSet::new();
    for weapon in weapons {
        let project = WeaponProjectSpec {
            weapons: vec![weapon.clone()],
        };
        let placement = placements::Plan::new(sources, &project.weapons).unwrap();
        let mut report = |_: build::Phase, _: &str, _: usize, _: usize| {};
        let mut progress = build::Progress::new(1, &mut report);
        let result = resolve::resolve_project_weapons_with_progress(
            sources,
            &project.weapons,
            &placement,
            &mut progress,
        );
        let expected = if socketless.contains(&weapon.donor_item_hash) {
            let donor = crate::weapon::donors::resolve_donor_item(
                sources,
                weapon.donor_item_hash,
                "socketless fixture",
            )
            .unwrap();
            let definition =
                read_tag(&sources.manager, donor.definition_tag, "socketless fixture").unwrap();
            assert_eq!(
                read_i64(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap(),
                0
            );
            Some("no socket block for gameplay authoring")
        } else if weapon.donor_item_hash == 0x67F5_15B2 {
            Some("Donor embeds its item identity at unsupported offsets")
        } else if matches!(weapon.donor_item_hash, 0xEFD9_F21F | 0x654C_35C4) {
            Some("Weapon equipment-slot value")
        } else {
            None
        };
        if let Some(expected) = expected {
            let error = result
                .err()
                .expect("an independently unsupported base must remain rejected")
                .to_string();
            assert!(error.contains(expected), "{error}");
            rejected.insert(weapon.donor_item_hash);
        } else {
            result.unwrap_or_else(|error| panic!("Unexpected base rejection: {error}"));
        }
    }
    rejected
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn collectible_free_appearance_resolution_preserves_gameplay_structure_guards() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let mut weapons = regression_weapons("missing_collectible", true);
    assert_eq!(weapons.len(), 45);
    for weapon in &weapons {
        let reference = WeaponPresentationDonorReference {
            item_hash: weapon.donor_item_hash,
            expected_name: weapon.expected_donor_name.clone(),
        };
        crate::weapon::donors::resolve_presentation_donor(&sources, &reference).unwrap();
    }
    weapons.retain(|weapon| {
        matches!(
            weapon.donor_item_hash,
            0xB180_EBE6
                | 0xC07A_C8FB
                | 0xACA4_9190
                | 0x1B76_17AC
                | 0x2090_FACD
                | 0x7FF3_47D1
                | 0x67F5_15B2
                | 0xEFD9_F21F
                | 0x654C_35C4
        )
    });
    assert_eq!(verify_native_base_requirements(&sources, &weapons).len(), 9);
}

fn compatible_geometry_project(
    sources: &sources::ProjectSources,
    geometry: &WeaponCloneSpec,
) -> Option<WeaponCloneSpec> {
    let item = crate::weapon::donors::resolve_donor_item(
        sources,
        geometry.donor_item_hash,
        "test geometry",
    )
    .unwrap();
    let definition = read_tag(&sources.manager, item.definition_tag, "geometry").unwrap();
    let strings = read_tag(&sources.manager, item.string_tag, "geometry strings").unwrap();
    let family = read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4).unwrap();
    let slot = weapon_inventory_slot(&definition).unwrap();
    let group = sandbox_pattern_source_at(
        &sources.stock_sandbox_patterns,
        weapon_pattern_index(&definition).unwrap().unwrap(),
    )
    .unwrap()
    .weapon_translation_group_hash;
    for candidate in 0..sources.stock_item_count {
        if candidate == item.item_index {
            continue;
        }
        let item = candidate;
        let candidate_hash = read_u32(
            &sources.stock_item_table,
            sources.item_rows + item * ITEM_ROW_SIZE,
        )
        .unwrap();
        let definition = read_tag(
            &sources.manager,
            TagHash(
                read_u32(
                    &sources.stock_item_table,
                    sources.item_rows + item * ITEM_ROW_SIZE + 16,
                )
                .unwrap(),
            ),
            "compatible base",
        )
        .unwrap();
        if read_i64(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).ok() == Some(0)
            || matching_u32_offsets(&definition, candidate_hash) != [ITEM_DEFINITION_HASH_OFFSET]
        {
            continue;
        }
        let candidate_slot = weapon_inventory_slot(&definition).ok();
        if weapon_equipment_slot(&definition).ok() != candidate_slot {
            continue;
        }
        if (candidate_slot != Some(slot)
            && !matches!(
                (slot, candidate_slot),
                (
                    WeaponInventorySlot::Kinetic,
                    Some(WeaponInventorySlot::Energy)
                ) | (
                    WeaponInventorySlot::Energy,
                    Some(WeaponInventorySlot::Kinetic)
                )
            ))
            || weapon_rarity(&definition).is_err()
        {
            continue;
        }
        let string_tag = TagHash(
            read_u32(
                &sources.stock_item_strings,
                sources.string_rows + item * ITEM_ROW_SIZE + 16,
            )
            .unwrap(),
        );
        let strings = read_tag(&sources.manager, string_tag, "base strings").unwrap();
        if read_u32(&strings, ITEM_TYPE_REFERENCE_OFFSET + 4).unwrap() != family {
            continue;
        }
        let Some(pattern) = weapon_pattern_index(&definition).unwrap() else {
            continue;
        };
        if sandbox_pattern_source_at(&sources.stock_sandbox_patterns, pattern)
            .unwrap()
            .weapon_translation_group_hash
            != group
        {
            continue;
        }
        let mut base = geometry.clone();
        base.donor_item_hash = read_u32(
            &sources.stock_item_table,
            sources.item_rows + item * ITEM_ROW_SIZE,
        )
        .unwrap();
        base.expected_donor_name = Some(resolve_item_name(&sources.manager, string_tag).unwrap());
        assert_ne!(base.donor_item_hash, geometry.donor_item_hash);
        return Some(base);
    }
    None
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn real_bases_with_unrelated_stock_conditions_build_independent_collections() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let sources = sources::load_project_sources(&packages).unwrap();
    let weapons = regression_weapons("unrelated_condition_flags", false);
    assert_eq!(weapons.len(), 58);
    for weapon in &weapons {
        let indices = collectible_indices(&sources, weapon.donor_item_hash);
        assert_eq!(indices.len(), 1);
        let row = sources.collectible_rows + indices[0] * COLLECTIBLE_ROW_SIZE;
        let acquired = collection_unlock_index(&sources.stock_collectibles, row).unwrap() as u16;
        assert!(
            crate::progression::COLLECTIBLE_SECONDARY_CONDITION_OFFSETS
                .into_iter()
                .any(|field| {
                    let descriptor = row + field;
                    read_u64(&sources.stock_collectibles, descriptor).unwrap() != 0
                        && numeric_program_layout(&sources.stock_collectibles, descriptor)
                            .unwrap()
                            .tokens
                            .iter()
                            .any(|(opcode, operand)| {
                                *opcode == crate::progression::NUMERIC_FLAG_INSTRUCTION
                                    && *operand != acquired
                            })
                }),
            "{}",
            weapon.text.name
        );
    }
    let project = WeaponProjectSpec { weapons };
    let bundle = build_weapon_project(&packages, &project).expect(
        "bases with unrelated stock conditions should build independent Collections entries",
    );
    assert_eq!(bundle.plan.weapons.len(), 58);
    verify_staged_collections(&packages, &sources, &project, &bundle);
}
