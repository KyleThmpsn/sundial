use super::*;

fn runtime_binding(resource_offset: u64) -> WeaponRuntimeBinding {
    WeaponRuntimeBinding {
        binding_hash: 0x1111_2222,
        binding_label: "Test".to_owned(),
        resource_index: 0,
        resource_count: 1,
        owner_tag: 0x8111_0001,
        concrete_class: 0x8080_1001,
        resource_offset,
    }
}

#[test]
fn runtime_locator_buildability_matches_compiler_shape_limits() {
    let mut locator = WeaponRuntimeFieldLocator {
        graph_tag: None,
        binding_hash: 0x1111_2222,
        resource_index: 0,
        root: WeaponRuntimeRootKind::ComponentDefinition,
        root_schema: 0x8080_1001,
        path: Vec::new(),
        type_handle: 0x8080_1002,
        value_offset: 0,
        byte_size: 4,
    };
    assert!(locator.is_buildable());
    locator.graph_tag = Some(0x8152_82E1);
    assert!(locator.for_graph(0x8152_82E2).is_err());
    assert!(locator.for_graph(0x8152_82E1).unwrap().graph_tag.is_none());
    locator.type_handle = 0;
    assert!(!locator.is_buildable());
    locator.type_handle = 0x8080_1002;
    locator.path = (0..=MAX_RUNTIME_SCHEMA_DEPTH)
        .map(|index| WeaponRuntimePathElement {
            name_hash: index as u32 + 1,
            type_handle: 0x8080_1002,
            byte_offset: 0,
        })
        .collect();
    assert!(!locator.is_buildable());
}

#[test]
fn component_definition_prefix_rejects_invalid_absolute_targets() {
    let binding = runtime_binding(0x20);
    let mut owner = vec![0_u8; 0xC0];
    owner[0x20..0x24].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0x24..0x28].copy_from_slice(&0x8080_2001_u32.to_le_bytes());

    for invalid_target in [0_u64, u64::MAX, 0x21, 0x20, 0xC0] {
        owner[0x28..0x30].copy_from_slice(&invalid_target.to_le_bytes());
        assert_eq!(
            native_component_definition_reference(&owner, &binding),
            Ok(None)
        );
    }

    owner[0x28..0x30].copy_from_slice(&0x80_u64.to_le_bytes());
    owner[0x80..0x84].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
    owner[0x84..0x88].copy_from_slice(&binding.concrete_class.to_le_bytes());
    assert_eq!(
        native_component_definition_reference(&owner, &binding),
        Ok(None)
    );

    owner[0x80..0x84].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0x84..0x88].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
    assert_eq!(
        native_component_definition_reference(&owner, &binding),
        Ok(None)
    );
}

#[test]
fn embedded_runtime_registry_resolves_every_base_and_member_type() {
    let registry = runtime_registry().expect("embedded runtime registry");
    assert!(!registry.records.is_empty());
    for record in registry.records.values() {
        if !matches!(record.base_type, 0 | u32::MAX) {
            assert!(registry.records.contains_key(&record.base_type));
        }
        for member in &record.members {
            assert!(registry.records.contains_key(&member.type_handle));
        }
    }
}

#[test]
fn secondary_projectile_components_decode_reflected_fields_with_checked_bounds() {
    let registry = runtime_registry().expect("runtime registry");
    for (schema, size) in [(0x8080_37A1, 512), (0x8080_3769, 728), (0x8080_377D, 776)] {
        assert_eq!(registry.records[&schema].struct_size as usize, size);
        let root = OwnerRootDescriptor {
            kind: WeaponRuntimeRootKind::ComponentDefinition,
            target: 0x20,
            schema,
            limit: 0x20 + size,
        };
        let owner = vec![0_u8; root.limit];
        let fields = decode_registry_root_fields(&owner, root, size, 0x1111_2222, 0, registry)
            .expect("secondary component fields");
        assert!(fields.iter().any(|field| {
            field.source == WeaponRuntimeFieldSource::NativeMember && !field.locator.path.is_empty()
        }));
        for field in fields {
            assert_eq!(field.locator.root_schema, schema);
            assert!(field.locator.value_offset + field.locator.byte_size <= size as u32);
            assert_eq!(
                field.owner_offset,
                root.target as u32 + field.locator.value_offset
            );
        }
        assert!(
            decode_registry_root_fields(
                &owner[..owner.len() - 1],
                root,
                size,
                0x1111_2222,
                0,
                registry,
            )
            .is_err()
        );
    }
}

#[test]
fn runtime_values_encode_and_decode_native_little_endian_bytes() {
    let cases = [
        (
            WeaponRuntimeValueKind::Boolean,
            WeaponRuntimeValue::Boolean(true),
            vec![1],
        ),
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 16 },
            WeaponRuntimeValue::Signed(-1234),
            vec![0x2E, 0xFB],
        ),
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 32 },
            WeaponRuntimeValue::Unsigned(0xDEAD_BEEF),
            vec![0xEF, 0xBE, 0xAD, 0xDE],
        ),
        (
            WeaponRuntimeValueKind::Float32,
            WeaponRuntimeValue::Float32Bits((-0.0f32).to_bits()),
            vec![0, 0, 0, 0x80],
        ),
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits([
                1.0f32.to_bits(),
                2.0f32.to_bits(),
                3.0f32.to_bits(),
                4.0f32.to_bits(),
            ]),
            vec![
                0, 0, 0x80, 0x3F, 0, 0, 0, 0x40, 0, 0, 0x40, 0x40, 0, 0, 0x80, 0x40,
            ],
        ),
    ];
    for (kind, value, expected) in cases {
        let bytes = encode_weapon_runtime_value(&kind, &value).expect("encode runtime value");
        assert_eq!(bytes, expected);
        assert_eq!(decode_runtime_value(&expected, 0, &kind).unwrap(), value);
    }
}

#[test]
fn technical_ranges_cover_every_unreflected_owner_byte_without_crossing_components() {
    let owner_payload = (0_u8..64).collect::<Vec<_>>();
    let root = OwnerRootDescriptor {
        kind: WeaponRuntimeRootKind::Instance,
        target: 8,
        schema: 0x8080_1234,
        limit: 40,
    };
    let known = WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            graph_tag: None,
            binding_hash: WEAPON_BARREL_COMPONENT_KEY,
            resource_index: 0,
            root: root.kind,
            root_schema: root.schema,
            path: vec![WeaponRuntimePathElement {
                name_hash: 1,
                type_handle: 2,
                byte_offset: 4,
            }],
            type_handle: 2,
            value_offset: 4,
            byte_size: 4,
        },
        owner_offset: 12,
        name: "Known".to_owned(),
        path_label: "Known".to_owned(),
        kind: WeaponRuntimeValueKind::UnsignedInteger { bits: 32 },
        value: WeaponRuntimeValue::Unsigned(u64::from_le_bytes([12, 13, 14, 15, 0, 0, 0, 0])),
        source: WeaponRuntimeFieldSource::NativeMember,
        generated_kind: None,
        name_inferred: false,
    };
    let mut fields = vec![known];
    append_uncovered_runtime_ranges(
        &owner_payload,
        root,
        &mut fields,
        WEAPON_BARREL_COMPONENT_KEY,
        0,
        &[(24, 28)],
    )
    .expect("append technical ranges");

    for relative in 0..32_usize {
        let coverage = fields
            .iter()
            .filter(|field| {
                let start = field.locator.value_offset as usize;
                let end = start + field.locator.byte_size as usize;
                start <= relative && relative < end
            })
            .count();
        if (16..20).contains(&relative) {
            assert_eq!(coverage, 0, "component-owned byte {relative}");
        } else {
            assert_eq!(coverage, 1, "shared owner byte {relative}");
        }
    }
    for field in fields
        .iter()
        .filter(|field| field.source == WeaponRuntimeFieldSource::OpaqueNativeType)
    {
        assert_eq!(
            field.owner_offset,
            8 + field.locator.value_offset,
            "technical locators remain root-relative"
        );
    }
}

#[test]
fn technical_ranges_are_chunked_without_losing_bytes() {
    let owner_payload = vec![0xA5; 640];
    let root = OwnerRootDescriptor {
        kind: WeaponRuntimeRootKind::ComponentDefinition,
        target: 16,
        schema: 0x8080_5678,
        limit: 616,
    };
    let mut fields = Vec::new();
    append_uncovered_runtime_ranges(
        &owner_payload,
        root,
        &mut fields,
        WEAPON_TRIGGER_COMPONENT_KEY,
        0,
        &[],
    )
    .expect("append technical ranges");
    assert_eq!(fields.len(), 3);
    assert!(
        fields
            .iter()
            .all(|field| field.locator.byte_size as usize <= MAX_TECHNICAL_RUNTIME_FIELD_BYTES)
    );
    assert_eq!(
        fields
            .iter()
            .map(|field| field.locator.byte_size as usize)
            .sum::<usize>(),
        600
    );
}

#[test]
fn shared_owner_ranges_reconstruct_identical_saved_locators() {
    let payload = (0_u8..64).collect::<Vec<_>>();
    let root = WeaponRuntimeRoot {
        kind: WeaponRuntimeRootKind::Definition,
        schema: 0x8080_5678,
        owner_offset: 8,
        byte_size: 32,
        generated_schema: false,
        structure: Default::default(),
        fields: Vec::new(),
    };
    let mut displayed = vec![root.clone()];
    prepare_shared_owner_roots(
        &payload,
        &mut displayed,
        WEAPON_TRIGGER_COMPONENT_KEY,
        1,
        &[(24, 28)],
    )
    .unwrap();
    assert_eq!(displayed[0].fields.len(), 2);
    let mut resolved = vec![root.clone()];
    prepare_shared_owner_roots(
        &payload,
        &mut resolved,
        WEAPON_TRIGGER_COMPONENT_KEY,
        1,
        &[(24, 28)],
    )
    .unwrap();
    assert_eq!(displayed, resolved);
    for field in &resolved[0].fields {
        let start = field.owner_offset as usize;
        let bytes = encode_weapon_runtime_field_value(field, &field.value).unwrap();
        assert_eq!(bytes, payload[start..start + bytes.len()]);
        assert!(start + bytes.len() <= 24 || start >= 28);
        assert!(field.locator.is_buildable());
    }

    // A component boundary change must invalidate a formerly exact technical range.
    let saved = &displayed[0].fields[0].locator;
    let mut changed = vec![root];
    prepare_shared_owner_roots(
        &payload,
        &mut changed,
        WEAPON_TRIGGER_COMPONENT_KEY,
        1,
        &[(20, 28)],
    )
    .unwrap();
    assert!(
        changed[0]
            .fields
            .iter()
            .all(|field| field.locator != *saved)
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn displayed_runtime_fields_resolve_to_exact_source_bytes() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let manager = open_shadowkeep_packages(Path::new(&packages).parent().unwrap()).unwrap();
    // These two verified stock rows exercise different weapon families. Neither is required
    // to contain unreflected shared-owner ranges. The synthetic boundary tests above cover
    // reconstruction of that optional field category explicitly.
    for pattern_index in [370, 285] {
        let source =
            load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern_index)
                .unwrap();
        let graph = load_weapon_runtime_graph_for_entity(
            &manager,
            source.item_hash,
            source.pattern_global_id_hash,
            source.entity_tag,
            &source.payload,
        )
        .unwrap();
        let mut checked_count = 0;
        for field in graph.fields() {
            if !field.locator.is_buildable() {
                continue;
            }
            let displayed_bytes = encode_weapon_runtime_field_value(field, &field.value).unwrap();
            let resolved = resolve_weapon_runtime_field(&manager, &source.payload, &field.locator)
                .unwrap_or_else(|error| {
                    panic!(
                        "Pattern {pattern_index} displayed field {} failed to resolve: {error}",
                        field.path_label
                    )
                });
            assert_eq!(resolved.field.kind, field.kind, "{}", field.path_label);
            let compiled_bytes =
                encode_weapon_runtime_field_value(&resolved.field, &field.value).unwrap();
            assert_eq!(displayed_bytes, compiled_bytes, "{}", field.path_label);
            let payload = read_component_owner(&manager, resolved.owner_tag).unwrap();
            assert_eq!(
                compiled_bytes,
                payload[resolved.owner_offset..resolved.owner_offset + compiled_bytes.len()],
                "{}",
                field.path_label
            );
            checked_count += 1;
        }
        assert!(
            checked_count > 0,
            "Pattern {pattern_index} must expose runtime fields"
        );
    }
}

#[test]
fn inferred_member_names_are_consistent_and_never_shadow_a_verified_name() {
    let registry = runtime_registry().expect("runtime registry");
    assert!(!registry.inferred.is_empty());
    for (hash, name) in &registry.inferred {
        assert_eq!(
            fnv1_name_hash(name),
            *hash,
            "inferred name {name} does not hash to 0x{hash:08X}"
        );
        assert!(
            !registry.names.contains_key(hash),
            "inferred name 0x{hash:08X} shadows a verified name"
        );
    }
    // A verified name always wins and is never reported as inferred.
    for hash in registry.names.keys() {
        assert!(!runtime_member_name(*hash, registry).1);
    }
    let (label, inferred) = runtime_member_name(0x2570_6232, registry);
    assert_eq!(label, "Magnetism Distance");
    assert!(!inferred);
    let unknown = runtime_member_name(0x0000_0001, registry);
    assert_eq!(unknown, ("Member 0x00000001".to_owned(), false));
}
#[cfg(test)]
mod gameplay_names {
    /// The Advanced Gameplay tab lists a weapon's components and every value inside them.
    /// A row named only by a hash tells a reader nothing, so this holds what that tab can
    /// name: every value, with a component-class fallback for unresolved bindings.
    ///
    /// The bindings that remain select components whose members are the shared falloff
    /// member and unreflected bytes, which say nothing about their role. They fall back to
    /// the component class they select, which is what the binding addresses.
    #[test]
    #[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES"]
    fn the_advanced_gameplay_tab_names_values_and_falls_back_to_component_classes() {
        use std::collections::BTreeMap;
        let packages = std::path::PathBuf::from(
            std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES")
                .expect("PARHELION_DEFAULT_WEAPONS_PACKAGES must name an installed package set"),
        );
        let install = packages.parent().expect("packages need an install root");
        let catalog =
            crate::investment::InvestmentCatalog::load(install, false, |_| {}).expect("catalog");
        let manager = crate::package_runtime::open_shadowkeep_packages(install).expect("packages");
        let mut bindings = BTreeMap::<u32, String>::new();
        let (mut named_values, mut values) = (0usize, 0usize);
        let mut weapons = 0usize;
        for summary in catalog.weapon_donors().iter().take(120) {
            let Ok(graph) = crate::weapon_runtime::load_weapon_runtime_graph_with_manager(
                &manager,
                summary.hash,
            ) else {
                continue;
            };
            weapons += 1;
            for binding in &graph.bindings {
                bindings
                    .entry(binding.binding_hash)
                    .or_insert_with(|| binding.binding_label.clone());
            }
            for field in graph.fields() {
                values += 1;
                if !field.name.trim().is_empty() && !field.name.starts_with("Native Value") {
                    named_values += 1;
                }
            }
        }
        assert!(weapons > 0, "no weapons loaded");
        assert!(values > 0, "no runtime values were checked");
        assert_eq!(named_values, values, "a runtime value row carries no name");
        let unnamed = bindings
            .values()
            .filter(|label| label.starts_with("Binding 0x") || label.starts_with("Component 0x"))
            .count();
        // A binding that falls back names the component class it selects, never a bare hash.
        assert!(
            !bindings
                .values()
                .any(|label| label.starts_with("Binding 0x")),
            "a binding fell back past its component class to its own hash"
        );
        println!(
            "{weapons} weapons: {values} values all named, {} of {} bindings named",
            bindings.len() - unnamed,
            bindings.len()
        );
    }
}
