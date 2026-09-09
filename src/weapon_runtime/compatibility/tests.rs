use super::*;

const INSTANCE: u32 = 0x8080_1001;
const DEFINITION: u32 = 0x8080_2001;

fn fixture() -> (Vec<u8>, WeaponRuntimeBinding, RuntimeRegistry) {
    let binding = WeaponRuntimeBinding {
        binding_hash: WEAPON_TRIGGER_COMPONENT_KEY,
        binding_label: "Trigger".into(),
        resource_index: 0,
        resource_count: 1,
        owner_tag: 0x8111_0001,
        concrete_class: INSTANCE,
        resource_offset: 0x40,
    };
    let mut owner = vec![0_u8; 0x140];
    owner[0..8].copy_from_slice(&0x140_u64.to_le_bytes());
    owner[0x10..0x18].copy_from_slice(&0x30_i64.to_le_bytes());
    owner[0x18..0x20].copy_from_slice(&0xA8_i64.to_le_bytes());
    owner[0x40..0x44].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0x44..0x48].copy_from_slice(&DEFINITION.to_le_bytes());
    owner[0x48..0x50].copy_from_slice(&0xC0_u64.to_le_bytes());
    owner[0xC0..0xC4].copy_from_slice(&binding.owner_tag.to_le_bytes());
    owner[0xC4..0xC8].copy_from_slice(&INSTANCE.to_le_bytes());
    let records = [(INSTANCE, 0x20), (DEFINITION, 0x18)]
        .into_iter()
        .map(|(handle, struct_size)| {
            (
                handle,
                RegistryRecord {
                    handle,
                    _binding_hash: 0,
                    base_type: 0,
                    struct_size,
                    members: Vec::new(),
                    native_layout: Vec::new(),
                },
            )
        })
        .collect();
    (
        owner,
        binding,
        RuntimeRegistry {
            records,
            names: BTreeMap::new(),
        },
    )
}

#[test]
fn known_native_shape_requires_no_reflected_fields() {
    let (owner, binding, registry) = fixture();
    assert_eq!(
        native_resource_shape(&owner, &binding, &registry),
        Ok(WeaponRuntimeResourceShape {
            instance_schema: INSTANCE,
            definition_schema: Some(DEFINITION),
        })
    );
    assert!(
        registry
            .records
            .values()
            .all(|record| record.members.is_empty())
    );
}

#[test]
fn absent_native_definition_is_distinct_from_an_unknown_declared_schema() {
    let (owner, binding, mut registry) = fixture();
    registry.records.remove(&DEFINITION);
    let error = native_resource_shape(&owner, &binding, &registry).unwrap_err();
    assert!(error.contains("definition schema") && error.contains("not verified"));
    for absent in [0_u32, u32::MAX] {
        let mut without_definition = owner.clone();
        without_definition[0x44..0x48].copy_from_slice(&absent.to_le_bytes());
        assert_eq!(
            native_resource_shape(&without_definition, &binding, &registry),
            Ok(WeaponRuntimeResourceShape {
                instance_schema: INSTANCE,
                definition_schema: None,
            })
        );
    }
}

#[test]
fn unknown_zero_size_and_out_of_bounds_native_roots_are_not_verified() {
    let (owner, binding, registry) = fixture();
    for schema in [INSTANCE, DEFINITION] {
        for size in [0, 0x200] {
            let (_, _, mut changed_registry) = fixture();
            changed_registry
                .records
                .get_mut(&schema)
                .unwrap()
                .struct_size = size;
            assert!(native_resource_shape(&owner, &binding, &changed_registry).is_err());
        }
    }
    let mut unknown = binding.clone();
    unknown.concrete_class = 0x8080_FFFF;
    assert!(
        native_resource_shape(&owner, &unknown, &registry)
            .unwrap_err()
            .contains("not verified")
    );
    for offset in [owner.len() as u64, u64::MAX] {
        let mut outside = binding.clone();
        outside.resource_offset = offset;
        assert!(native_resource_shape(&owner, &outside, &registry).is_err());
    }
}

#[test]
fn invalid_declared_definition_targets_cannot_look_absent() {
    let (owner, binding, registry) = fixture();
    for target in [0, 0x40, 0x41, 0x140, u64::MAX] {
        let mut invalid = owner.clone();
        invalid[0x48..0x50].copy_from_slice(&target.to_le_bytes());
        assert!(
            native_resource_shape(&invalid, &binding, &registry)
                .unwrap_err()
                .contains("invalid target or identity")
        );
    }
    for range in [0xC0..0xC4, 0xC4..0xC8] {
        let mut invalid = owner.clone();
        invalid[range].copy_from_slice(&0xDEAD_BEEF_u32.to_le_bytes());
        assert!(native_resource_shape(&invalid, &binding, &registry).is_err());
    }
}

#[test]
fn native_definition_prefixes_and_owner_descriptors_must_fit() {
    let (owner, binding, _) = fixture();
    for (schema, size) in [(INSTANCE, 8), (DEFINITION, 4)] {
        let (_, _, mut registry) = fixture();
        registry.records.get_mut(&schema).unwrap().struct_size = size;
        assert!(native_resource_shape(&owner, &binding, &registry).is_err());
    }
    let (mut owner, binding, registry) = fixture();
    owner[0x18..0x20].copy_from_slice(&i64::MAX.to_le_bytes());
    assert!(native_resource_shape(&owner, &binding, &registry).is_err());
}

#[test]
fn native_components_without_a_definition_prefix_remain_distinct() {
    let (mut owner, binding, registry) = fixture();
    owner[0x40..0x44].copy_from_slice(&0x1234_u32.to_le_bytes());
    assert_eq!(
        native_resource_shape(&owner, &binding, &registry)
            .unwrap()
            .definition_schema,
        None
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn native_resource_shapes_match_full_decoding_for_stock_and_projectile_resources() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must point to clean Shadowkeep packages");
    let manager = open_shadowkeep_packages(Path::new(&packages).parent().unwrap()).unwrap();
    let source = load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, 370).unwrap();
    // This existing private-projectile fixture exercises native instance 0x80803B73 and
    // definition 0x8080388F even when a weapon's selected resource classes are generated-only.
    let projectile = manager.read_tag(TagHash(0x8152_82E1)).unwrap();
    let registry = runtime_registry().unwrap();
    let mut checked = 0;
    let mut with_definition = 0;
    let mut without_definition = 0;
    for entity in [&source.payload, &projectile] {
        for binding_hash in weapon_component_binding_hashes(entity).unwrap() {
            for binding in weapon_component_bindings(entity, binding_hash).unwrap() {
                let shape = match load_weapon_runtime_resource_shape(&manager, &binding) {
                    Ok(shape) => shape,
                    Err(error) => {
                        eprintln!("Unverified binding 0x{binding_hash:08X}: {error}");
                        continue;
                    }
                };
                let runtime_binding = runtime_shape_binding(&binding).unwrap();
                let owner = read_component_owner(&manager, binding.owner_tag).unwrap();
                let full = decode_component_resource(&manager, &owner, &runtime_binding, registry)
                    .unwrap_or_else(|error| {
                        panic!(
                            "Verified binding 0x{binding_hash:08X} failed full decoding: {error}"
                        )
                    });
                assert_eq!(shape.instance_schema, full.instance.schema);
                assert_eq!(
                    shape.definition_schema,
                    full.definition.as_ref().map(|root| root.schema)
                );
                checked += 1;
                if shape.definition_schema.is_some() {
                    with_definition += 1;
                } else {
                    without_definition += 1;
                }
            }
        }
    }
    eprintln!(
        "Checked {checked} resources, {with_definition} with native definitions, {without_definition} without"
    );
    assert!(
        checked > 0,
        "Stock fixture must exercise verified native resource shapes"
    );
    assert!(
        with_definition > 0,
        "Stock fixture must exercise native definitions"
    );
}
