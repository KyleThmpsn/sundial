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
fn component_definition_prefix_uses_an_absolute_owner_offset() {
    let binding = runtime_binding(0x20);
    let mut owner = vec![0_u8; 0xC0];
    owner[0x20..0x24].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0x24..0x28].copy_from_slice(&0x8080_2001_u32.to_le_bytes());
    owner[0x28..0x30].copy_from_slice(&0x80_u64.to_le_bytes());
    owner[0x80..0x84].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0x84..0x88].copy_from_slice(&binding.concrete_class.to_le_bytes());

    assert_eq!(
        native_component_definition_reference(&owner, &binding),
        Ok(Some((0x80, 0x8080_2001)))
    );
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
fn embedded_runtime_registry_is_closed_and_anchored() {
    let registry = runtime_registry().expect("embedded runtime registry");
    assert!(registry.records.len() >= 900);
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
fn runtime_values_round_trip_exact_native_bits() {
    let cases = [
        (
            WeaponRuntimeValueKind::Boolean,
            WeaponRuntimeValue::Boolean(true),
        ),
        (
            WeaponRuntimeValueKind::SignedInteger { bits: 16 },
            WeaponRuntimeValue::Signed(-1234),
        ),
        (
            WeaponRuntimeValueKind::UnsignedInteger { bits: 32 },
            WeaponRuntimeValue::Unsigned(0xDEAD_BEEF),
        ),
        (
            WeaponRuntimeValueKind::Float32,
            WeaponRuntimeValue::Float32Bits((-0.0f32).to_bits()),
        ),
        (
            WeaponRuntimeValueKind::Vector4Float32,
            WeaponRuntimeValue::Vector4Float32Bits([
                1.0f32.to_bits(),
                2.0f32.to_bits(),
                3.0f32.to_bits(),
                4.0f32.to_bits(),
            ]),
        ),
    ];
    for (kind, value) in cases {
        let bytes = encode_weapon_runtime_value(&kind, &value).expect("encode runtime value");
        assert_eq!(decode_runtime_value(&bytes, 0, &kind).unwrap(), value);
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
        let bytes = encode_weapon_runtime_value(&field.kind, &field.value).unwrap();
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
            let displayed_bytes = encode_weapon_runtime_value(&field.kind, &field.value).unwrap();
            let resolved = resolve_weapon_runtime_field(&manager, &source.payload, &field.locator)
                .unwrap_or_else(|error| {
                    panic!(
                        "Pattern {pattern_index} displayed field {} failed to resolve: {error}",
                        field.path_label
                    )
                });
            assert_eq!(resolved.field.kind, field.kind, "{}", field.path_label);
            let compiled_bytes =
                encode_weapon_runtime_value(&resolved.field.kind, &field.value).unwrap();
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
