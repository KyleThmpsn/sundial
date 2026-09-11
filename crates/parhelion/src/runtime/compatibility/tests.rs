use super::*;
use sundial::{
    investment::{
        InvestmentCatalog, WeaponAmmoType, WeaponDamageProfile, WeaponInventorySlot, WeaponRarity,
    },
    package_authoring::weapon_entity::{
        WEAPON_ENTITY_COMPONENT_ROW_CLASS, WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS, WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
    },
};

fn summary(hash: u32) -> WeaponDonorSummary {
    WeaponDonorSummary {
        hash,
        name: format!("Test {hash}"),
        type_name: "Sidearm".into(),
        bucket_hash: 1,
        collection_backed: true,
        power_cap: None,
        damage_type: None,
        inventory_slot: Some(WeaponInventorySlot::Kinetic),
        ammo_type: Some(WeaponAmmoType::Primary),
        weapon_pattern_index: Some(hash as u16),
        weapon_translation_group: Some(0x1234_5678),
        stat_group_index: None,
        damage_profile: WeaponDamageProfile::Unknown,
        rarity: WeaponRarity::Legendary,
    }
}

#[test]
fn lower_risk_requires_known_matching_metadata_and_schemas() {
    let baseline = summary(1);
    let candidate = summary(2);
    let shape = WeaponRuntimeResourceShape {
        instance_schema: 0x8080_1234,
        definition_schema: Some(0x8080_5678),
    };
    let mut reasons = Vec::new();
    metadata_reasons(Some(&baseline), Some(&candidate), &mut reasons);
    shape_reasons(Ok(shape), Ok(shape), &mut reasons);
    assert_eq!(
        assessed(reasons, &[1]).status,
        DonorCompatibility::LowerRisk
    );

    let mut reasons = Vec::new();
    metadata_reasons(None, Some(&candidate), &mut reasons);
    assert_eq!(
        assessed(reasons, &[1]).status,
        DonorCompatibility::Experimental
    );
    let mut reasons = Vec::new();
    shape_reasons(Ok(shape), Err("Unknown native class".into()), &mut reasons);
    assert_eq!(
        assessed(reasons, &[1]).status,
        DonorCompatibility::Experimental
    );
}

#[test]
fn semantic_mismatches_remain_experimental_and_not_incompatible() {
    let baseline = summary(1);
    let mut candidate = summary(2);
    candidate.type_name = "Grenade Launcher".into();
    candidate.ammo_type = Some(WeaponAmmoType::Special);
    candidate.weapon_translation_group = Some(0x8765_4321);
    let mut reasons = Vec::new();
    metadata_reasons(Some(&baseline), Some(&candidate), &mut reasons);
    shape_reasons(
        Ok(WeaponRuntimeResourceShape {
            instance_schema: 1,
            definition_schema: Some(2),
        }),
        Ok(WeaponRuntimeResourceShape {
            instance_schema: 1,
            definition_schema: Some(3),
        }),
        &mut reasons,
    );
    let result = assessed(reasons, &[WEAPON_TRIGGER_COMPONENT_KEY]);
    assert_eq!(result.status, DonorCompatibility::Experimental);
    assert_eq!(result.reasons.len(), 4);
}

#[test]
fn unknown_metadata_cannot_become_a_lower_risk_match() {
    let baseline = summary(1);
    for mut candidate in [summary(2), summary(3), summary(4), summary(5)] {
        match candidate.hash {
            2 => candidate.type_name.clear(),
            3 => candidate.ammo_type = None,
            4 => candidate.weapon_translation_group = None,
            _ => candidate.weapon_translation_group = Some(0),
        }
        let mut reasons = Vec::new();
        metadata_reasons(Some(&baseline), Some(&candidate), &mut reasons);
        assert_eq!(
            assessed(reasons, &[]).status,
            DonorCompatibility::Experimental
        );
    }
}

fn array(data: &mut [u8], descriptor: usize, header: usize, count: usize, class: u32) {
    data[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
    data[descriptor + 8..descriptor + 16]
        .copy_from_slice(&(header as i64 - (descriptor + 8) as i64).to_le_bytes());
    data[header..header + 8].copy_from_slice(&(count as u64).to_le_bytes());
    data[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
}

/// Two independent resources deliberately share one owner partition.
fn shared_owner_source(flavor: u32) -> Arc<WeaponRuntimeEntitySource> {
    let mut data = vec![0_u8; 0x1E0];
    data[0..8].copy_from_slice(&0x1E0_u64.to_le_bytes());
    array(&mut data, 0x10, 0xA0, 1, WEAPON_ENTITY_COMPONENT_ROW_CLASS);
    array(
        &mut data,
        0x48,
        0xD0,
        2,
        WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
    );
    array(
        &mut data,
        0x58,
        0x110,
        2,
        WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
    );
    array(
        &mut data,
        0x68,
        0x180,
        2,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
    );
    let owner = 0x8100_0000 | (flavor << 8);
    data[0xB0..0xB4].copy_from_slice(&owner.to_le_bytes());
    let mut hashes = [WEAPON_TRIGGER_COMPONENT_KEY, WEAPON_RELOAD_COMPONENT_KEY];
    hashes.sort();
    for (index, hash) in hashes.into_iter().enumerate() {
        let row = 0xE0 + index * 8;
        data[row..row + 4].copy_from_slice(&hash.to_le_bytes());
        data[row + 4..row + 8].copy_from_slice(&(0x0001_0000_u32 | index as u32).to_le_bytes());
        let class = 0x8080_1000 | (flavor << 4) | index as u32;
        let resource = 0x120 + index * 0x28;
        data[resource + 0x0C..resource + 0x10].copy_from_slice(&class.to_le_bytes());
        data[resource + 0x10..resource + 0x14].copy_from_slice(&owner.to_le_bytes());
        let descriptor = 0x190 + index * 0x18;
        data[descriptor..descriptor + 4].copy_from_slice(&owner.to_le_bytes());
        data[descriptor + 4..descriptor + 8].copy_from_slice(&class.to_le_bytes());
        data[descriptor + 8..descriptor + 16]
            .copy_from_slice(&(0x80_u64 + index as u64 * 0x20).to_le_bytes());
    }
    Arc::new(WeaponRuntimeEntitySource {
        item_hash: flavor,
        pattern_global_id_hash: flavor + 0x1000,
        weapon_content_group_hash: flavor + 0x2000,
        entity_tag: flavor + 0x3000,
        payload: data,
    })
}

#[test]
fn compiler_preflight_keeps_other_selected_donors_and_detects_conflicts() {
    let baseline = shared_owner_source(1);
    let selected = RequestedDonor {
        binding_hash: WEAPON_RELOAD_COMPONENT_KEY,
        item_hash: 2,
        source: shared_owner_source(2),
    };
    let candidate = RequestedDonor {
        binding_hash: WEAPON_TRIGGER_COMPONENT_KEY,
        item_hash: 3,
        source: shared_owner_source(3),
    };
    assert!(compose(&baseline, std::slice::from_ref(&candidate)).is_ok());
    let error = compose(&baseline, &[selected, candidate]).unwrap_err();
    assert!(error.contains("Runtime component donor conflict"));
}

#[test]
fn shared_owner_preview_and_effective_provenance_include_implicit_changes() {
    let baseline = shared_owner_source(1);
    let selected = RequestedDonor {
        binding_hash: WEAPON_RELOAD_COMPONENT_KEY,
        item_hash: 2,
        source: shared_owner_source(2),
    };
    // Choosing the baseline for Trigger clears its explicit override. Reload still controls
    // their shared owner, so Trigger must not claim it effectively follows the baseline.
    let requested = vec![
        selected,
        RequestedDonor {
            binding_hash: WEAPON_TRIGGER_COMPONENT_KEY,
            item_hash: 1,
            source: Arc::clone(&baseline),
        },
    ];
    compose(&baseline, &requested).unwrap();
    let bindings = collect_bindings(&baseline.payload).unwrap();
    let affected = affected_bindings(&bindings, WEAPON_TRIGGER_COMPONENT_KEY).unwrap();
    assert!(affected.contains(&WEAPON_TRIGGER_COMPONENT_KEY));
    assert!(affected.contains(&WEAPON_RELOAD_COMPONENT_KEY));
    let sources = effective_sources(&bindings, 1, &requested, baseline.item_hash).unwrap();
    for binding in affected {
        assert_eq!(
            sources[&binding],
            vec![EffectiveComponentSource {
                donor_item_hash: 2,
                via_binding_hash: Some(WEAPON_RELOAD_COMPONENT_KEY),
            }]
        );
    }
}

#[test]
fn missing_selected_binding_is_not_silently_assessed_as_compatible() {
    let source = shared_owner_source(1);
    let bindings = collect_bindings(&source.payload).unwrap();
    assert!(affected_bindings(&bindings, 0x1234_5678).is_err());
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn native_candidate_scan_preserves_stock_and_rejects_structural_conflicts() {
    let packages = std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap();
    let packages = Path::new(&packages);
    let cache = tempfile::tempdir().unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &cache.path().join("catalog.json"),
        false,
        |_| {},
    )
    .unwrap();
    let all = catalog.weapon_donors();
    let hashes = [0x4CE3_CE93, 0xEE06_B019]; // Breachlight and The Mountaintop.
    let donors = all
        .into_iter()
        .filter(|donor| hashes.contains(&donor.hash))
        .collect::<Vec<_>>();
    assert_eq!(donors.len(), 2);
    let baseline = donors.iter().find(|donor| donor.hash == hashes[0]).unwrap();
    let key = RuntimeGraphKey::new(baseline.weapon_pattern_index, baseline.hash, []);
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let mut cache = ScanCache::new(&manager);
    let source = cache
        .entity(key.pattern_index, key.fallback_item_hash)
        .unwrap();
    let bindings = collect_bindings(&source.payload).unwrap();
    let selected = bindings
        .keys()
        .copied()
        .find(|&binding| {
            affected_bindings(&bindings, binding)
                .unwrap()
                .iter()
                .all(|affected| {
                    bindings[affected]
                        .iter()
                        .all(|resource| cache.shape(resource).is_ok())
                })
        })
        .expect("Breachlight should expose a partition with verified native schemas");
    let report = assess_component_donors(packages, &key, selected, &donors).unwrap();
    assert_eq!(
        report.candidates[&baseline.hash].status,
        DonorCompatibility::LowerRisk
    );
    assert!(
        report
            .affected_bindings
            .iter()
            .any(|(hash, _)| *hash == selected)
    );
    assert!(report.current_error.is_none());

    // A saved additional binding absent from the native baseline must remain in every
    // candidate preflight. It must not disappear merely because another picker is open.
    let missing_binding = 0x1234_5678;
    assert!(!bindings.contains_key(&missing_binding));
    let other = donors.iter().find(|donor| donor.hash == hashes[1]).unwrap();
    let invalid_key = RuntimeGraphKey::new(
        key.pattern_index,
        key.fallback_item_hash,
        [(missing_binding, other.weapon_pattern_index, other.hash)],
    );
    let invalid = assess_component_donors(packages, &invalid_key, selected, &donors).unwrap();
    assert!(invalid.current_error.is_some());
    assert!(
        invalid
            .candidates
            .values()
            .all(|candidate| candidate.status == DonorCompatibility::Incompatible)
    );
}
