//! Opt-in checks against a fully staged or installed generation, never hardcoded authored offsets.
use super::*;
use crate::plug_classification::PlugClassification;
use sundial::package_authoring::{
    weapon_entity::weapon_component_binding_hashes,
    weapon_runtime::{
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager,
    },
};

#[test]
fn stock_companion_neighbor_is_preserved_without_weakening_array_checks() {
    let mut segment = vec![0; 32 + ITEM_STRING_SANDBOX_PERK_ROW_SIZE];
    write_u64(&mut segment, 0, 16).unwrap(); // Coldheart's neighboring pointer.
    segment[8..16].copy_from_slice(&NESTED_ARRAY_TRAILER);
    write_u64(&mut segment, 16, 1).unwrap();
    write_u32(&mut segment, 24, ITEM_STRING_SANDBOX_PERK_ROW_CLASS).unwrap();
    write_u16(&mut segment, 32, u16::MAX).unwrap();
    write_u32(
        &mut segment,
        36,
        ITEM_STRING_SANDBOX_PERK_EMPTY_EXPRESSION_TAG,
    )
    .unwrap();
    let before = segment.clone();
    validate_item_string_sandbox_perk_segment(&segment).unwrap();
    assert_eq!(segment, before);
    for offset in [8, 24, 28, 32, 36, 40] {
        let mut broken = segment.clone();
        broken[offset] ^= 1;
        assert!(
            validate_item_string_sandbox_perk_segment(&broken).is_err(),
            "byte {offset}"
        );
    }
    assert!(validate_item_string_sandbox_perk_segment(&segment[..segment.len() - 1]).is_err());
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES and optionally PARHELION_PRIVATE_SPEED_RECIPE"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Native generation audit keeps independent byte-level assertions beside each recipe under test"
)]
fn real_default_weapon_generation_preserves_native_chains() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
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
    let dense = read_tag(
        &manager,
        globals_child_tag(&globals, GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT).unwrap(),
        "dense presentation",
    )
    .unwrap();
    let presentations = dense_item_presentation_arrays(&dense).unwrap()[3];
    let collectibles = read_tag(
        &manager,
        root_child_tag(&root, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT).unwrap(),
        "collectibles",
    )
    .unwrap();
    let (_, _, collectible_rows, _) = array_at(&collectibles, 8).unwrap();
    let load = |hash: u32| {
        assert_eq!(by_hash[&hash].len(), 1, "ambiguous item {hash:08X}");
        let index = by_hash[&hash][0];
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
    let dependencies = crate::shared_tag_dependency_index::dependencies(
        &read_tag(&manager, RUNTIME_DEPENDENCY_COMPANION, "loading index").unwrap(),
        RUNTIME_DEPENDENCY_COMPANION,
        RUNTIME_DEPENDENCY_ROOT,
    )
    .unwrap();
    let mut recipes = crate::recipe_library::BUNDLED_RECIPES
        .iter()
        .skip(1)
        .map(|(_, json)| crate::WeaponRecipe::from_json_str(json).unwrap())
        .collect::<Vec<_>>();
    if let Some(path) = std::env::var_os("PARHELION_PRIVATE_SPEED_RECIPE") {
        recipes.push(crate::WeaponRecipe::load_json(path).unwrap());
    }
    let expected_private_actions = recipes
        .iter()
        .flat_map(|recipe| &recipe.overrides.socket_plug_variants)
        .flat_map(|variant| &variant.sandbox_perks)
        .filter(|perk| !perk.runtime_values.is_empty() || !perk.action_float_values.is_empty())
        .count();
    let mut private_actions = BTreeSet::new();
    for recipe in recipes {
        let spec = recipe.to_spec().unwrap();
        eprintln!("Checking {} ({:08X})", recipe.name, spec.identity.item_hash);
        let (definition, item_strings) = load(spec.identity.item_hash);
        assert!(
            item_strings[item_string_watermark_overrides(&item_strings).unwrap()]
                .iter()
                .all(|byte| *byte == 0xFF),
            "{} retains a donor watermark override",
            recipe.name
        );
        assert_eq!(
            weapon_inventory_slot(&definition).unwrap(),
            spec.overrides.inventory_slot.unwrap()
        );
        if let Some(rarity) = spec.overrides.rarity {
            assert_eq!(weapon_rarity(&definition).unwrap(), rarity);
        }
        let item_index = by_hash[&spec.identity.item_hash][0];
        let (collectible_count, _, _, _) = array_at(&collectibles, 8).unwrap();
        let collectible = (0..collectible_count)
            .find(|index| {
                read_u16(
                    &collectibles,
                    collectible_rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
                )
                .unwrap() as usize
                    == item_index
            })
            .unwrap();
        let overrides = collectible_rows
            + collectible * COLLECTIBLE_ROW_SIZE
            + crate::progression::COLLECTIBLE_SOCKET_OVERRIDES_OFFSET;
        assert_eq!(
            &collectibles[overrides..overrides + 16],
            &[0; 16],
            "{} must use authored defaults, not donor Collections socket overrides",
            recipe.name
        );
        let (parent_count, _, parent_rows, _) = array_at(
            &collectibles,
            collectible_rows + collectible * COLLECTIBLE_ROW_SIZE + 0x18,
        )
        .unwrap();
        let parents = (0..parent_count)
            .map(|position| read_u16(&collectibles, parent_rows + position * 2).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            parents.len(),
            4,
            "weapon page plus three Sunrise class pages"
        );
        for badge in crate::badge::sunrise_badge_collectible_parents(parents[0])
            .into_iter()
            .skip(1)
        {
            assert!(parents.contains(&badge));
        }
        if let Some(damage) = spec.overrides.modern_damage_type {
            if damage != ModernDamageType::Kinetic {
                assert_eq!(
                    weapon_damage_descriptor(&definition).unwrap(),
                    WeaponDamageDescriptor::Elemental(damage)
                );
            }
        }
        let ammo = spec.overrides.ammo_type.unwrap();
        assert_eq!(item_string_ammo_type(&item_strings).unwrap(), Some(ammo));
        let authored =
            load_weapon_runtime_entity_with_manager(&manager, spec.identity.item_hash).unwrap();
        assert!(
            crate::weapon_ammo::patches(&manager, &authored.payload, ammo)
                .unwrap()
                .is_empty(),
            "{} default and all selectable variants must use the requested ammo",
            recipe.name
        );
        // Some stock inventory items share a pattern whose identity is another item hash.
        // Follow the item's authoritative pattern index, just as the compiler does.
        let (source_definition, _) = load(spec.donor_item_hash);
        let source = load_weapon_runtime_entity_at_pattern_index_with_manager(
            &manager,
            weapon_pattern_index(&source_definition).unwrap().unwrap(),
        )
        .unwrap();
        let source_ammo = weapon_component_bindings(&source.payload, 0x5F0D_D954).unwrap()[0];
        let authored_ammo = weapon_component_bindings(&authored.payload, 0x5F0D_D954).unwrap()[0];
        let mut normalized = authored.payload.clone();
        if source_ammo.owner_tag != authored_ammo.owner_tag {
            assert!(dependencies.contains(&authored_ammo.owner_tag));
            assert!(dependencies.contains(&authored.entity_tag));
            retarget_weapon_component_owner(
                &mut normalized,
                authored_ammo.owner_tag,
                source_ammo.owner_tag,
            )
            .unwrap();
        }
        assert_eq!(
            weapon_component_binding_hashes(&normalized).unwrap(),
            weapon_component_binding_hashes(&source.payload).unwrap()
        );
        assert_eq!(
            normalized, source.payload,
            "{} must keep its gameplay graph, including translator",
            recipe.name
        );

        let defaults = weapon_default_plug_indices(&definition).unwrap();
        let socket_resource =
            relative_target(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET).unwrap();
        let (_, _, socket_rows, _) = array_at(&definition, socket_resource).unwrap();
        for (socket, column) in spec.overrides.socket_columns.iter().enumerate() {
            let Some(column) = column else { continue };
            let plug_hash =
                read_u32(&items, rows + usize::from(defaults[socket]) * ITEM_ROW_SIZE).unwrap();
            let (plug, plug_strings) = load(plug_hash);
            let source_hash = column.choices[0];
            let (source_plug, _) = load(source_hash);
            let source_perks = match weapon_sandbox_perks(&source_plug) {
                Ok(perks) => perks,
                Err(_) => {
                    // Stat-only barrel/magazine plugs have no investment-perk resource.
                    assert_eq!(plug_hash, source_hash);
                    assert_eq!(plug, source_plug);
                    assert!(
                        !spec
                            .overrides
                            .socket_plug_variants
                            .iter()
                            .any(|variant| usize::from(variant.socket_index) == socket
                                && variant.choice_index == 0)
                    );
                    continue;
                }
            };
            let perks = weapon_sandbox_perks(&plug).unwrap();
            if let Some(socket_type) = column.socket_type {
                assert_eq!(
                    read_u16(
                        &definition,
                        socket_rows + socket * ITEM_ORDINARY_SOCKET_ROW_SIZE
                    )
                    .unwrap(),
                    socket_type
                );
            }
            let variant = spec.overrides.socket_plug_variants.iter().find(|variant| {
                usize::from(variant.socket_index) == socket && variant.choice_index == 0
            });
            if let Some(variant) = variant {
                assert_ne!(plug_hash, source_hash);
                assert_eq!(
                    perks.len(),
                    source_perks.len() + variant.additional_sandbox_perks.len()
                );
                for (&authored, &source) in perks[source_perks.len()..]
                    .iter()
                    .zip(&variant.additional_sandbox_perks)
                {
                    assert_ne!(
                        authored, source,
                        "Additional effects need private display metadata"
                    );
                    let installed =
                        load_sandbox_perk_runtime_action(&manager, &globals, usize::from(authored))
                            .unwrap();
                    let stock =
                        load_sandbox_perk_runtime_action(&manager, &globals, usize::from(source))
                            .unwrap();
                    assert_eq!(installed.action_tag, stock.action_tag);
                    assert_eq!(installed.action_payload, stock.action_payload);
                    let hidden = load_sandbox_perk_runtime_action(&manager, &globals, 416).unwrap();
                    assert_eq!(installed.finished_perk.detail, hidden.finished_perk.detail);
                }
                if variant.description.is_some() {
                    let (_, source_strings) = load(source_hash);
                    assert_ne!(
                        &plug_strings[ITEM_DESCRIPTION_REFERENCE_OFFSET
                            ..ITEM_DESCRIPTION_REFERENCE_OFFSET + 8],
                        &source_strings[ITEM_DESCRIPTION_REFERENCE_OFFSET
                            ..ITEM_DESCRIPTION_REFERENCE_OFFSET + 8]
                    );
                    assert_eq!(
                        read_u32(&plug_strings, ITEM_DESCRIPTION_REFERENCE_OFFSET).unwrap(),
                        LOCALIZATION_DONOR_TABLE_INDEX as u32
                    );
                }
                if let Some(classification_hash) = variant.classification_donor_hash {
                    let (template, template_strings) = load(classification_hash);
                    let field = |hash| {
                        presentations.rows
                            + by_hash[&hash][0] * ITEM_DENSE_PRESENTATION_ROW_SIZE
                            + ITEM_DENSE_PRESENTATION_TYPE_OFFSET
                    };
                    assert_eq!(
                        read_u32(&dense, field(plug_hash)).unwrap(),
                        read_u32(&dense, field(classification_hash)).unwrap(),
                        "{} private type must reach dense presentation",
                        recipe.name
                    );
                    assert_eq!(
                        PlugClassification::from_template(&plug, &plug_strings).unwrap(),
                        PlugClassification::from_template(&template, &template_strings).unwrap()
                    );
                }
                for (index, &source_perk) in source_perks.iter().enumerate() {
                    let Some(edit) = variant
                        .sandbox_perks
                        .iter()
                        .find(|perk| perk.source_perk_index == source_perk)
                    else {
                        assert_eq!(perks[index], source_perk);
                        continue;
                    };
                    assert_ne!(perks[index], source_perk);
                    let stock = load_sandbox_perk_runtime_action(
                        &manager,
                        &globals,
                        usize::from(source_perk),
                    )
                    .unwrap();
                    let installed = load_sandbox_perk_runtime_action(
                        &manager,
                        &globals,
                        usize::from(perks[index]),
                    )
                    .unwrap();
                    if variant.description.is_some() {
                        assert_eq!(
                            &installed.finished_perk.detail.unwrap()[8..16],
                            &plug_strings[ITEM_DESCRIPTION_REFERENCE_OFFSET
                                ..ITEM_DESCRIPTION_REFERENCE_OFFSET + 8],
                            "The weapon tooltip must use the same private description as the plug"
                        );
                    }
                    if let Some(classification) = variant.classification_donor_hash {
                        let (definition, _) = load(classification);
                        let visible = weapon_sandbox_perks(&definition)
                            .unwrap()
                            .into_iter()
                            .filter_map(|perk| {
                                load_sandbox_perk_runtime_action(
                                    &manager,
                                    &globals,
                                    usize::from(perk),
                                )
                                .ok()
                            })
                            .find(|perk| {
                                perk.finished_perk
                                    .detail
                                    .is_some_and(|detail| read_u16(&detail, 0).unwrap() != u16::MAX)
                            })
                            .unwrap();
                        assert_eq!(
                            &installed.finished_perk.detail.unwrap()[20..24],
                            &visible.finished_perk.detail.unwrap()[20..24]
                        );
                    }
                    if edit.runtime_values.is_empty() && edit.action_float_values.is_empty() {
                        assert_eq!(installed.action_tag, stock.action_tag);
                    } else {
                        // Both current edited plugs use the independently verified Micro-Missile
                        // owner/graph/action layout. Infer its allocation from the installed action.
                        assert_eq!(source_perk, 1178);
                        assert!(private_actions.insert(installed.action_tag));
                        assert_eq!(
                            installed.action_tag.pkg_id(),
                            PRIVATE_PERK_RUNTIME_PACKAGE_ID
                        );
                        let allocator = AppendedTagAllocator::new(
                            PRIVATE_PERK_RUNTIME_PACKAGE_ID,
                            installed.action_tag.entry_index() as usize - 2,
                        );
                        let mut tags = Vec::new();
                        let expected = clone_private_sandbox_perk_runtime(
                            &manager,
                            &stock,
                            &edit.runtime_values,
                            &edit.action_float_values,
                            edit.activation,
                            allocator,
                            &mut tags,
                        )
                        .unwrap();
                        assert_eq!(expected, installed.action_tag);
                        assert_eq!(tags.len(), 7);
                        for (index, tag) in tags.iter().enumerate() {
                            let assigned = allocator
                                .assigned_tag(index, "audit", "private runtime")
                                .unwrap();
                            assert!(dependencies.contains(&assigned.0));
                            assert_eq!(
                                read_tag(&manager, assigned, "private runtime").unwrap(),
                                tag.payload
                            );
                        }
                        assert_eq!(
                            read_u32(&tags[0].payload, 0x1818).unwrap(),
                            62.5_f32.to_bits()
                        );
                        assert_eq!(
                            read_u32(&tags[0].payload, 0x274).unwrap(),
                            62.5_f32.to_bits()
                        );
                    }
                }
            } else {
                assert_eq!(plug_hash, source_hash);
                assert_eq!(perks, source_perks);
            }
            // Every active action must resolve AND be enrolled. Stock plugs may also
            // contain a display-only finished perk using the native empty expression.
            for perk in perks {
                let action =
                    match load_sandbox_perk_runtime_action(&manager, &globals, usize::from(perk)) {
                        Ok(action) => action,
                        Err(error) => {
                            assert!(
                                source_perks.contains(&perk),
                                "a private action cannot be missing: {error}"
                            );
                            assert!(
                                error.ends_with("runtime key 0x811C9DC5 is not assigned"),
                                "unexpected missing action: {error}"
                            );
                            eprintln!("  retained stock empty-expression perk {perk}");
                            continue;
                        }
                    };
                assert!(
                    dependencies.contains(&action.action_tag.0),
                    "{} socket {socket}: action {} not enrolled",
                    recipe.name,
                    action.action_tag
                );
                for graph in action.graphs {
                    for binding in weapon_component_binding_hashes(&graph.payload).unwrap() {
                        for component in weapon_component_bindings(&graph.payload, binding).unwrap()
                        {
                            read_tag(&manager, TagHash(component.owner_tag), "perk component")
                                .unwrap();
                        }
                    }
                }
            }
        }
    }
    assert_eq!(private_actions.len(), expected_private_actions);
    let (stock_missile, stock_strings) = load(0xDD5C_B37A);
    assert_eq!(
        weapon_sandbox_perks(&stock_missile).unwrap(),
        vec![1178, 416]
    );
    assert_eq!(
        PlugClassification::from_template(&stock_missile, &stock_strings)
            .unwrap()
            .category,
        0x0078_A617
    );
    let stock_owner = read_tag(&manager, TagHash(0x8152_82E7), "stock projectile").unwrap();
    assert_eq!(read_u32(&stock_owner, 0x1818).unwrap(), 1.0_f32.to_bits());
    assert_eq!(read_u32(&stock_owner, 0x274).unwrap(), 1.0_f32.to_bits());
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only native collection routing checks"]
fn real_collection_routing_uses_rarity_and_target_slot() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
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
    let collectibles = read_tag(
        &manager,
        root_child_tag(&root, ROOT_COLLECTIBLE_DEFINITION_TABLE_SLOT).unwrap(),
        "collectibles",
    )
    .unwrap();
    let (count, _, rows, _) = array_at(&items, 8).unwrap();
    let (_, _, string_rows, _) = array_at(&strings, 8).unwrap();
    let (collectible_count, _, collectible_rows, _) = array_at(&collectibles, 8).unwrap();
    let load = |index: usize, table: &[u8], table_rows: usize| {
        read_tag(
            &manager,
            TagHash(read_u32(table, table_rows + index * ITEM_ROW_SIZE + 16).unwrap()),
            "item",
        )
        .unwrap()
    };
    for (hash, rarity, slot) in [
        (
            0x4CE3_CE93,
            AuthoredWeaponRarity::Exotic,
            WeaponInventorySlot::Kinetic,
        ),
        (
            0x4CE3_CE93,
            AuthoredWeaponRarity::Exotic,
            WeaponInventorySlot::Energy,
        ),
        (
            0x4CE3_CE93,
            AuthoredWeaponRarity::Exotic,
            WeaponInventorySlot::Power,
        ),
        (
            0xAD47_46D5,
            AuthoredWeaponRarity::Legendary,
            WeaponInventorySlot::Energy,
        ),
    ] {
        let item = find_u32_row_key(&items, rows, count, ITEM_ROW_SIZE, hash)
            .unwrap()
            .unwrap();
        let donor = (0..collectible_count)
            .find(|index| {
                read_u16(
                    &collectibles,
                    collectible_rows + index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
                )
                .unwrap() as usize
                    == item
            })
            .unwrap();
        let definition = load(item, &items, rows);
        let item_strings = load(item, &strings, string_rows);
        let selected = resolve_weapon_collection_donor(
            &manager,
            &items,
            rows,
            count,
            &strings,
            string_rows,
            &collectibles,
            collectible_rows,
            collectible_count,
            donor,
            &definition,
            &item_strings,
            rarity,
            slot,
        )
        .unwrap()[0];
        let selected_item = read_u16(
            &collectibles,
            collectible_rows + selected * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )
        .unwrap() as usize;
        let candidate = load(selected_item, &items, rows);
        assert_eq!(
            weapon_rarity(&candidate).unwrap() == AuthoredWeaponRarity::Exotic,
            rarity == AuthoredWeaponRarity::Exotic
        );
        if rarity == AuthoredWeaponRarity::Exotic {
            assert_eq!(weapon_inventory_slot(&candidate).unwrap(), slot);
        } else {
            let candidate_strings = load(selected_item, &strings, string_rows);
            assert_eq!(
                read_u32(&candidate_strings, ITEM_TYPE_REFERENCE_OFFSET + 4).unwrap(),
                read_u32(&item_strings, ITEM_TYPE_REFERENCE_OFFSET + 4).unwrap()
            );
        }
        assert_ne!(selected, donor);
    }
}

#[test]
#[ignore = "requires PARHELION_COLLECTION_STOCK_PACKAGES; builds in memory, never installs"]
fn real_rarity_switch_builds_both_collection_directions() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_COLLECTION_STOCK_PACKAGES").unwrap());
    let mut exotic = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../recipes/holdover.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    exotic.overrides.rarity = Some(AuthoredWeaponRarity::Exotic);
    let mut legendary = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../recipes/periapsis.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    legendary.donor_item_hash = 0xAD47_46D5; // Sunshot, deliberately Exotic gameplay.
    legendary.expected_donor_name = Some("Sunshot".to_owned());
    legendary.presentation_donor = None;
    legendary.overrides.socket_columns.clear();
    legendary.overrides.socket_plug_variants.clear();
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![exotic, legendary],
        },
    )
    .unwrap();
    assert_eq!(bundle.plan.weapons.len(), 2);
    assert!(!bundle.artifacts.is_empty());
}

#[test]
#[ignore = "requires PARHELION_COLLECTION_STOCK_PACKAGES; builds in memory, never installs"]
fn real_sparse_metadata_donors_build_with_valid_collection_counts() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_COLLECTION_STOCK_PACKAGES").unwrap());
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![
                project_weapon("parhelion.sparse.crooked-fang", 0xCBCC_1483),
                project_weapon("parhelion.sparse.hawthorne", 0x6BB9_DF01),
            ],
        },
    )
    .expect("Native sparse metadata and alternate collection count exemplars should build");
    assert_eq!(bundle.plan.weapons.len(), 2);
    assert!(!bundle.artifacts.is_empty());
}
