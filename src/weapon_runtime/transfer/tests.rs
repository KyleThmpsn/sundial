use super::*;

fn field(schema: u32) -> WeaponRuntimeField {
    WeaponRuntimeField {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: 1,
            resource_index: 0,
            root: WeaponRuntimeRootKind::ComponentDefinition,
            root_schema: schema,
            path: vec![WeaponRuntimePathElement {
                name_hash: 7,
                type_handle: 9,
                byte_offset: 16,
            }],
            type_handle: 9,
            value_offset: 16,
            byte_size: 4,
        },
        owner_offset: 100,
        name: "Setting".into(),
        path_label: "Setting".into(),
        kind: WeaponRuntimeValueKind::Float32,
        value: WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
        source: WeaponRuntimeFieldSource::NativeMember,
        generated_kind: None,
    }
}

fn registry() -> RuntimeRegistry {
    let records = [(1, 0, true), (2, 1, false), (3, 1, false), (4, 0, true)]
        .into_iter()
        .map(|(handle, base_type, has_member)| {
            (
                handle,
                RegistryRecord {
                    handle,
                    _binding_hash: 0,
                    base_type,
                    struct_size: 32,
                    native_layout: vec![],
                    members: if has_member {
                        vec![RegistryMember {
                            name_hash: 7,
                            type_handle: 9,
                            byte_offset: 16,
                        }]
                    } else {
                        vec![]
                    },
                },
            )
        })
        .collect();
    RuntimeRegistry {
        records,
        names: BTreeMap::new(),
    }
}

#[test]
fn inherited_settings_transfer_between_different_native_schemas() {
    assert!(share_semantics(&field(2), &field(3), &registry()));
    assert!(share_semantics(&field(1), &field(2), &registry()));
}

#[test]
fn matching_names_and_layouts_do_not_prove_shared_meaning() {
    assert!(!share_semantics(&field(2), &field(4), &registry()));
    let mut invalid = field(3);
    invalid.locator.path[0].byte_offset += 4;
    assert!(!share_semantics(&field(2), &invalid, &registry()));
    invalid = field(3);
    invalid.locator.root_schema = 999;
    assert!(!share_semantics(&field(2), &invalid, &registry()));
}

#[test]
fn opaque_data_references_and_ambiguous_inheritance_are_not_portable_settings() {
    let mut source = field(2);
    let mut target = field(3);
    for kind in [
        WeaponRuntimeValueKind::FixedBytes { size: 4 },
        WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
        WeaponRuntimeValueKind::Enum { bits: 32 },
    ] {
        source.kind = kind.clone();
        target.kind = kind;
        assert!(!share_semantics(&source, &target, &registry()));
    }
    let mut registry = registry();
    registry
        .records
        .get_mut(&3)
        .unwrap()
        .members
        .push(RegistryMember {
            name_hash: 7,
            type_handle: 9,
            byte_offset: 16,
        });
    assert!(!share_semantics(&field(2), &field(3), &registry));
    registry.records.get_mut(&1).unwrap().base_type = 3;
    assert!(!share_semantics(&field(2), &field(3), &registry));
}
