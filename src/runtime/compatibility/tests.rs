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
            inferred: BTreeMap::new(),
        },
    )
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
