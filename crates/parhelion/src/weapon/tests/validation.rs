use super::*;

#[test]
fn runtime_value_shape_accepts_native_package_schema_handles() {
    let runtime = WeaponRuntimeValueOverride {
        locator: WeaponRuntimeFieldLocator {
            binding_hash: 0x39AF_D7D3,
            resource_index: 0,
            root: WeaponRuntimeRootKind::ComponentInstance,
            root_schema: 0x80BF_DFEA,
            path: vec![WeaponRuntimePathElement {
                name_hash: 0x1234_5678,
                type_handle: 0x8080_000F,
                byte_offset: 0xBD4,
            }],
            type_handle: 0x8080_000F,
            value_offset: 0xBD4,
            byte_size: 4,
        },
        value: WeaponRuntimeValue::Float32Bits(500.0_f32.to_bits()),
    };

    validate_runtime_value_override_shapes(&[runtime])
        .expect("native package schema handles are valid runtime locators");
}

#[test]
fn project_rejects_empty_but_accepts_batches_above_thirty_two_weapons() {
    assert!(canonical_project_weapons(&WeaponProjectSpec { weapons: vec![] }).is_err());
    let weapons = (0..64)
        .map(|index| project_weapon(&format!("parhelion.weapon-{index}"), index + 1))
        .collect();
    let canonical = canonical_project_weapons(&WeaponProjectSpec { weapons }).unwrap();
    assert_eq!(canonical.len(), 64);
    let mut collision = canonical.clone();
    collision[63].identity.item_hash = collision[0].identity.item_hash;
    assert!(canonical_project_weapons(&WeaponProjectSpec { weapons: collision }).is_err());
}

#[test]
fn project_rejects_duplicate_namespaces_and_identity_domains_independently() {
    let first = project_weapon("parhelion.first", 1);
    let mut second = project_weapon("parhelion.second", 2);
    second.namespace = first.namespace.clone();
    assert!(
        canonical_project_weapons(&WeaponProjectSpec {
            weapons: vec![first.clone(), second]
        })
        .is_err()
    );

    for collision in 0..3 {
        let mut second = project_weapon("parhelion.second", 2);
        match collision {
            0 => second.identity.item_hash = first.identity.item_hash,
            1 => second.identity.collectible_hash = first.identity.collectible_hash,
            2 => second.identity.unlock_hash = first.identity.unlock_hash,
            _ => unreachable!(),
        }
        assert!(
            canonical_project_weapons(&WeaponProjectSpec {
                weapons: vec![first.clone(), second]
            })
            .is_err()
        );
    }
}

#[test]
fn project_canonical_order_is_input_permutation_independent() {
    let first = project_weapon("parhelion.alpha", 1);
    let second = project_weapon("parhelion.beta", 2);
    let forward = canonical_project_weapons(&WeaponProjectSpec {
        weapons: vec![first.clone(), second.clone()],
    })
    .unwrap();
    let reverse = canonical_project_weapons(&WeaponProjectSpec {
        weapons: vec![second, first],
    })
    .unwrap();
    assert_eq!(forward, reverse);
}

#[test]
fn namespace_identity_is_stable_distinct_and_uses_fnv_localization_keys() {
    let namespace = "parhelion.example-weapon";
    let first = WeaponCloneIdentity::from_namespace(namespace).expect("namespace should allocate");
    let second = WeaponCloneIdentity::from_namespace(namespace)
        .expect("namespace should allocate deterministically");

    assert_eq!(first, second);
    first
        .validate_for_donor(0x6212_9AF7)
        .expect("allocated identity should validate");
    for (role, hash) in [
        ("name", first.name_hash),
        ("flavor", first.flavor_hash),
        ("source", first.source_hash),
    ] {
        assert!(LOCALIZATION_DONOR_STRING_HASHES[1] < hash);
        assert!((0..=u16::MAX).any(|nonce| {
            let key = if nonce == 0 {
                format!("parhelion/{namespace}/{role}")
            } else {
                format!("parhelion/{namespace}/{role}/{nonce}")
            };
            fnv1_name_hash(&key) == hash
        }));
    }
}

#[test]
fn generic_clone_defaults_to_exact_donor_inheritance() {
    let namespace = "parhelion.generic-donor";
    let spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: 0x6212_9AF7,
        expected_donor_name: Some("Tranquility".to_owned()),
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("namespace should allocate"),
        text: WeaponCloneText {
            name: "Quiet Reflection".to_owned(),
            flavor: "An exact donor clone.".to_owned(),
            source: "Source: unit test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides::default(),
    };

    spec.validate().expect("generic recipe should validate");
    assert!(spec.overrides.investment_stats.is_empty());
    assert!(spec.overrides.modern_damage_type.is_none());
    assert!(spec.overrides.power_cap_group.is_none());
    assert!(spec.overrides.socket_columns.is_empty());
}

#[test]
fn catalog_compatibility_validation_is_lazy_for_exact_donor_inheritance() {
    let namespace = "parhelion.catalog-free-inheritance";
    let spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: 0x6212_9AF7,
        expected_donor_name: None,
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("namespace should allocate"),
        text: WeaponCloneText {
            name: "Catalog-free inheritance".to_owned(),
            flavor: "Exact donor bytes need no compatibility lookup.".to_owned(),
            source: "Source: unit test".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides::default(),
    };

    validate_weapon_clone_specs_against_catalog(
        Path::new("this-install-deliberately-does-not-exist"),
        [&spec],
    )
    .expect("an exact donor clone should skip loading the installed catalog");
}

#[test]
fn generic_clone_rejects_ambiguous_overrides_and_donor_collision() {
    let namespace = "parhelion.invalid-generic";
    let mut spec = WeaponCloneSpec {
        namespace: namespace.to_owned(),
        donor_item_hash: 0x6212_9AF7,
        expected_donor_name: None,
        presentation_donor: None,
        icon_donor: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        identity: WeaponCloneIdentity::from_namespace(namespace)
            .expect("namespace should allocate"),
        text: WeaponCloneText {
            name: "Invalid".to_owned(),
            flavor: "Invalid".to_owned(),
            source: "Invalid".to_owned(),
            ..WeaponCloneText::default()
        },
        overrides: WeaponCloneOverrides {
            investment_stats: vec![(15, 50), (15, 60)],
            ..WeaponCloneOverrides::default()
        },
    };
    assert!(spec.validate().is_err());

    spec.overrides.investment_stats.clear();
    spec.donor_item_hash = spec.identity.item_hash;
    assert!(spec.validate().is_err());
}

#[test]
fn native_power_cap_index_zero_is_authorable() {
    let mut overrides = WeaponCloneOverrides {
        power_cap_group: Some(0),
        ..Default::default()
    };
    validate_native_scalar_overrides(&overrides).unwrap();
    overrides.power_cap_group = None;
    overrides.power_cap_groups = Some(vec![0, 15]);
    validate_native_scalar_overrides(&overrides).unwrap();
    overrides.power_cap_groups = Some(Vec::new());
    assert!(validate_native_scalar_overrides(&overrides).is_err());
}
