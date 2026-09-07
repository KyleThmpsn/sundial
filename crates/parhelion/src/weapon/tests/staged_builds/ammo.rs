use super::*;

#[test]
#[ignore = "requires PARHELION_AMMO_TEST_PACKAGES pointing to Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Byte-level integration audit checks every native variant without sharing production validation"
)]
fn real_native_ammo_clone_preserves_every_other_field() {
    use sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager;
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_AMMO_TEST_PACKAGES").expect("set PARHELION_AMMO_TEST_PACKAGES"),
    );
    let manager = open_manager(&packages).unwrap();
    let source = load_weapon_runtime_entity_with_manager(&manager, 0x4CE3_CE93).unwrap();
    let binding = weapon_component_bindings(&source.payload, 0x5F0D_D954).unwrap()[0];
    assert_eq!(binding.owner_tag, 0x8152_AEA5);
    assert_eq!(binding.resource_offset, 0x80);
    let stock = read_tag(&manager, TagHash(binding.owner_tag), "Breachlight content").unwrap();
    assert!(
        crate::weapon_ammo::patches(&manager, &source.payload, WeaponAmmoType::Primary)
            .unwrap()
            .is_empty()
    );
    let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, HOST_EXPECTED_ENTRY_COUNT + 256);
    for ammo in [WeaponAmmoType::Special, WeaponAmmoType::Heavy] {
        let patches = crate::weapon_ammo::patches(&manager, &source.payload, ammo).unwrap();
        assert_eq!(
            patches.len(),
            25,
            "default plus all 24 perk-selected variants"
        );
        assert_eq!(patches[0].offset, 0x4FC - 0x80);
        let mut entity = source.payload.clone();
        let mut tags = Vec::new();
        append_patched_runtime_resource_owners(
            &manager,
            &mut entity,
            &[],
            &patches,
            allocator,
            &mut tags,
        )
        .unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].template_tag.0, binding.owner_tag);
        let authored_owner = allocator.assigned_tag(0, "test", "ammo owner").unwrap();
        let mut normalized = tags[0].payload.clone();
        for patch in &patches {
            let start = binding.resource_offset as usize + patch.offset as usize;
            assert_eq!(patch.bytes, vec![1, ammo as u8 - 1]);
            assert_eq!(&normalized[start..start + 2], patch.bytes.as_slice());
            normalized[start..start + 2].copy_from_slice(&stock[start..start + 2]);
        }
        retarget_weapon_component_owner_payload(
            &mut normalized,
            &entity,
            authored_owner.0,
            binding.owner_tag,
        )
        .unwrap();
        assert_eq!(
            normalized, stock,
            "only ammo bytes and typed owner references may change"
        );
        retarget_weapon_component_owner(&mut entity, authored_owner.0, binding.owner_tag).unwrap();
        assert_eq!(
            entity, source.payload,
            "all graph bindings, including translator, must survive"
        );
    }
    assert_eq!(
        read_tag(&manager, TagHash(binding.owner_tag), "unchanged stock").unwrap(),
        stock
    );
    if std::env::var_os("PARHELION_VERIFY_INSTALLED_NATIVE_AMMO").is_some() {
        let installed = load_weapon_runtime_entity_with_manager(&manager, 0x72F7_F442).unwrap();
        let installed_binding =
            weapon_component_bindings(&installed.payload, 0x5F0D_D954).unwrap()[0];
        assert_ne!(installed_binding.owner_tag, binding.owner_tag);
        assert!(
            crate::weapon_ammo::patches(&manager, &installed.payload, WeaponAmmoType::Special)
                .unwrap()
                .is_empty(),
            "installed default and every selectable variant must already be Special"
        );
        assert_eq!(
            crate::weapon_ammo::patches(&manager, &installed.payload, WeaponAmmoType::Primary)
                .unwrap()
                .len(),
            25
        );
    }
}
