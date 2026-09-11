use super::*;

fn write_array_descriptor(
    data: &mut [u8],
    descriptor: usize,
    header: usize,
    count: usize,
    class: u32,
) {
    data[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
    data[descriptor + 8..descriptor + 16]
        .copy_from_slice(&(header as i64 - (descriptor + 8) as i64).to_le_bytes());
    data[header..header + 8].copy_from_slice(&(count as u64).to_le_bytes());
    data[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
}

#[test]
fn sandbox_pattern_identity_decodes_runtime_and_gear_art_groups() {
    let mut data = vec![0; 0x70];
    write_array_descriptor(&mut data, 0x08, 0x30, 1, SANDBOX_PATTERN_ROW_CLASS);
    let row = 0x40;
    data[row..row + 4].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    data[row + SANDBOX_PATTERN_GLOBAL_ID_OFFSET..row + SANDBOX_PATTERN_GLOBAL_ID_OFFSET + 4]
        .copy_from_slice(&0x5566_7788_u32.to_le_bytes());
    data[row + SANDBOX_PATTERN_WEAPON_CONTENT_GROUP_HASH_OFFSET
        ..row + SANDBOX_PATTERN_WEAPON_CONTENT_GROUP_HASH_OFFSET + 4]
        .copy_from_slice(&0x99AA_BBCC_u32.to_le_bytes());
    data[row + SANDBOX_PATTERN_WEAPON_TRANSLATION_GROUP_HASH_OFFSET
        ..row + SANDBOX_PATTERN_WEAPON_TRANSLATION_GROUP_HASH_OFFSET + 4]
        .copy_from_slice(&0xDDEE_FF00_u32.to_le_bytes());

    let expected = SandboxPatternIdentity {
        item_hash: 0x1122_3344,
        row_index: 0,
        row_offset: row,
        pattern_global_id_hash: 0x5566_7788,
        weapon_content_group_hash: 0x99AA_BBCC,
        weapon_translation_group_hash: 0xDDEE_FF00,
    };
    assert_eq!(
        sandbox_pattern_identity(&data, expected.item_hash),
        Ok(Some(expected))
    );
    assert_eq!(sandbox_pattern_identity_at(&data, 0), Ok(Some(expected)));
}

#[test]
fn assignment_insert_preserves_sort_order_and_auxiliary_data() {
    let mut data = vec![0; 0x88];
    data[0x28..0x30].copy_from_slice(&[0, 0, 0, 0, 0xBD, 0x9F, 0x80, 0x80]);
    let data_len = data.len() as u64;
    data[0..8].copy_from_slice(&data_len.to_le_bytes());
    write_array_descriptor(
        &mut data,
        0x08,
        0x30,
        3,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_CLASS,
    );
    for (index, (key, tag)) in [(1_u32, 11_u32), (3, 33), (5, 55)].into_iter().enumerate() {
        let row = 0x40 + index * 8;
        data[row..row + 4].copy_from_slice(&key.to_le_bytes());
        data[row + 4..row + 8].copy_from_slice(&tag.to_le_bytes());
    }
    data[0x58..0x60].copy_from_slice(&[0, 0, 0, 0, 0xBD, 0x9F, 0x80, 0x80]);
    write_array_descriptor(&mut data, 0x18, 0x60, 6, 0x8080_000B);

    let authored = append_weapon_entity_assignment(data, 4, 44).unwrap();

    assert_eq!(weapon_entity_assignment(&authored, 4).unwrap(), Some(44));
    assert_eq!(read_u64(&authored, 0).unwrap() as usize, authored.len());
    let primary = assignment_rows(&authored).unwrap();
    assert_eq!(primary.count, 4);
    let keys = (0..4)
        .map(|index| read_u32(&authored, primary.rows + index * 8).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(keys, [1, 3, 4, 5]);
    let secondary = native_array(&authored, 0x18).unwrap();
    assert_eq!(secondary.count, 6);
    assert!((0..6).all(|index| read_u32(&authored, secondary.rows + index * 4).unwrap() == 0));
}

fn two_resource_weapon_entity() -> Vec<u8> {
    let mut data = vec![0; 0x1A0];
    let data_len = data.len() as u64;
    data[0..8].copy_from_slice(&data_len.to_le_bytes());
    write_array_descriptor(
        &mut data,
        ENTITY_COMPONENTS_DESCRIPTOR,
        0xA0,
        2,
        WEAPON_ENTITY_COMPONENT_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_DEFINITION_MAP_DESCRIPTOR,
        0xD0,
        1,
        WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_RESOURCE_MAP_DESCRIPTOR,
        0xF0,
        2,
        WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR,
        0x160,
        2,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
    );

    let owners = [0x8111_0001_u32, 0x8111_0002];
    let classes = [0x8080_1001_u32, 0x8080_1002];
    for index in 0..2 {
        let component = 0xB0 + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
        data[component..component + 4].copy_from_slice(&owners[index].to_le_bytes());
        data[component + 4..component + 8].copy_from_slice(&(index as u32 + 10).to_le_bytes());

        let resource = 0x100 + index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        data[resource + 0x0C..resource + 0x10].copy_from_slice(&classes[index].to_le_bytes());
        data[resource + 0x10..resource + 0x14].copy_from_slice(&owners[index].to_le_bytes());

        let descriptor = 0x170 + index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        data[descriptor..descriptor + 4].copy_from_slice(&owners[index].to_le_bytes());
        data[descriptor + 4..descriptor + 8].copy_from_slice(&classes[index].to_le_bytes());
        data[descriptor + 8..descriptor + 16]
            .copy_from_slice(&(0x80_u64 + index as u64 * 0x20).to_le_bytes());
        data[descriptor + 0x10..descriptor + 0x14].copy_from_slice(&(index as u32).to_le_bytes());
    }
    data[0xE0..0xE4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
    data[0xE4..0xE8].copy_from_slice(&0x0002_0000_u32.to_le_bytes());
    data
}

fn aliased_weapon_entity(definitions: &[(u32, u32)], flavor: u32) -> Vec<u8> {
    let mut data = vec![0; 0x1E0];
    let data_len = data.len() as u64;
    data[0..8].copy_from_slice(&data_len.to_le_bytes());
    write_array_descriptor(
        &mut data,
        ENTITY_COMPONENTS_DESCRIPTOR,
        0xA0,
        2,
        WEAPON_ENTITY_COMPONENT_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_DEFINITION_MAP_DESCRIPTOR,
        0xD0,
        definitions.len(),
        WEAPON_ENTITY_DEFINITION_MAP_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_RESOURCE_MAP_DESCRIPTOR,
        0x110,
        2,
        WEAPON_ENTITY_RESOURCE_MAP_ROW_CLASS,
    );
    write_array_descriptor(
        &mut data,
        ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR,
        0x180,
        2,
        WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_CLASS,
    );

    for (index, (binding_hash, selector)) in definitions.iter().copied().enumerate() {
        let row = 0xE0 + index * SANDBOX_PATTERN_ENTITY_ASSIGNMENT_ROW_SIZE;
        data[row..row + 4].copy_from_slice(&binding_hash.to_le_bytes());
        data[row + 4..row + 8].copy_from_slice(&selector.to_le_bytes());
    }

    for index in 0..2 {
        let owner = 0x8100_0000 | (flavor << 8) | index as u32;
        let class = 0x8080_1000 | (flavor << 4) | index as u32;
        let component = 0xB0 + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
        data[component..component + 4].copy_from_slice(&owner.to_le_bytes());
        data[component + 4..component + 8]
            .copy_from_slice(&(flavor.wrapping_mul(10) + index as u32).to_le_bytes());

        let resource = 0x120 + index * WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
        data[resource..resource + WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE]
            .fill((flavor as u8).wrapping_add(index as u8));
        data[resource + 0x0C..resource + 0x10].copy_from_slice(&class.to_le_bytes());
        data[resource + 0x10..resource + 0x14].copy_from_slice(&owner.to_le_bytes());

        let descriptor = 0x190 + index * WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
        data[descriptor..descriptor + 4].copy_from_slice(&owner.to_le_bytes());
        data[descriptor + 4..descriptor + 8].copy_from_slice(&class.to_le_bytes());
        data[descriptor + 8..descriptor + 16]
            .copy_from_slice(&(u64::from(flavor) * 0x100 + index as u64).to_le_bytes());
        data[descriptor + 0x10..descriptor + 0x14].copy_from_slice(&(index as u32).to_le_bytes());
    }
    data
}

fn alias_second_descriptor_to_first_resource(entity: &mut [u8]) {
    let resources = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR).unwrap();
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR).unwrap();
    let first_resource = resources.rows;
    let second_resource = resources.rows + WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE;
    entity.copy_within(
        first_resource..first_resource + WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE,
        second_resource,
    );
    let first_descriptor = descriptors.rows;
    let second_descriptor = descriptors.rows + WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE;
    entity.copy_within(
        first_descriptor..first_descriptor + WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE,
        second_descriptor,
    );
}

fn couple_descriptors_under_first_component(entity: &mut [u8]) {
    let components = native_array(entity, ENTITY_COMPONENTS_DESCRIPTOR).unwrap();
    let resources = native_array(entity, ENTITY_RESOURCE_MAP_DESCRIPTOR).unwrap();
    let descriptors = native_array(entity, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR).unwrap();
    let first_owner = read_u32(entity, descriptors.rows).unwrap();
    write_u64(entity, ENTITY_COMPONENTS_DESCRIPTOR, 1).unwrap();
    write_u64(entity, components.header, 1).unwrap();
    write_u32(
        entity,
        resources.rows + WEAPON_ENTITY_RESOURCE_MAP_ROW_SIZE + 0x10,
        first_owner,
    )
    .unwrap();
    write_u32(
        entity,
        descriptors.rows + WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE,
        first_owner,
    )
    .unwrap();
    write_u32(
        entity,
        descriptors.rows + WEAPON_ENTITY_RESOURCE_DESCRIPTOR_ROW_SIZE + 0x10,
        0,
    )
    .unwrap();
    validate_weapon_entity(entity).unwrap();
}

#[test]
fn multi_resource_bindings_and_owner_retargets_are_validated() {
    let mut entity = two_resource_weapon_entity();
    let bindings = weapon_component_bindings(&entity, 0x1234_5678).unwrap();
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].resource_index, 0);
    assert_eq!(bindings[1].resource_index, 1);
    assert_eq!(bindings[0].resource_count, 2);
    assert!(weapon_component_binding(&entity, 0x1234_5678).is_err());

    let replacements =
        retarget_weapon_component_owner(&mut entity, 0x8111_0001, 0x8123_0001).unwrap();
    assert_eq!(replacements, 3);
    let bindings = weapon_component_bindings(&entity, 0x1234_5678).unwrap();
    assert_eq!(bindings[0].owner_tag, 0x8123_0001);
    assert_eq!(bindings[1].owner_tag, 0x8111_0002);
    validate_weapon_entity(&entity).unwrap();
    assert_eq!(
        weapon_component_binding_hashes(&entity).unwrap(),
        [0x1234_5678]
    );
}

#[test]
fn component_owner_payload_retarget_only_rewrites_structural_owner_fields() {
    const OLD_OWNER: u32 = 0x8111_0001;
    const NEW_OWNER: u32 = 0x8123_0001;
    const RESOURCE: usize = 0x80;
    const CONCRETE: usize = 0x100;

    let entity = two_resource_weapon_entity();
    let mut owner = vec![0_u8; 0x140];
    write_u64(&mut owner, 0, 0x140).unwrap();
    write_u32(&mut owner, 0x40, OLD_OWNER).unwrap();
    write_u32(&mut owner, RESOURCE, OLD_OWNER).unwrap();
    write_u32(&mut owner, RESOURCE + 4, 0x8080_2001).unwrap();
    write_u64(&mut owner, RESOURCE + 8, CONCRETE as u64).unwrap();
    write_u32(&mut owner, CONCRETE, OLD_OWNER).unwrap();
    write_u32(&mut owner, CONCRETE + 4, 0x8080_1001).unwrap();

    assert_eq!(
        retarget_weapon_component_owner_payload(&mut owner, &entity, OLD_OWNER, NEW_OWNER),
        Ok(2)
    );
    assert_eq!(read_u32(&owner, RESOURCE).unwrap(), NEW_OWNER);
    assert_eq!(read_u32(&owner, CONCRETE).unwrap(), NEW_OWNER);
    assert_eq!(
        read_u32(&owner, 0x40).unwrap(),
        OLD_OWNER,
        "an unrelated integer collision must not be rewritten"
    );
}

#[test]
fn component_owner_payload_retarget_rejects_malformed_absolute_target() {
    const OLD_OWNER: u32 = 0x8111_0001;
    const RESOURCE: usize = 0x80;

    let entity = two_resource_weapon_entity();
    let mut owner = vec![0_u8; 0x140];
    write_u64(&mut owner, 0, 0x140).unwrap();
    write_u32(&mut owner, RESOURCE, OLD_OWNER).unwrap();
    write_u32(&mut owner, RESOURCE + 4, 0x8080_2001).unwrap();
    write_u64(&mut owner, RESOURCE + 8, 0x13C).unwrap();
    let original = owner.clone();

    assert!(
        retarget_weapon_component_owner_payload(&mut owner, &entity, OLD_OWNER, 0x8123_0001)
            .is_err()
    );
    assert_eq!(owner, original, "validation must complete before mutation");
}

#[test]
fn owner_retarget_includes_both_event_endpoints_and_is_atomic() {
    const OLD: u32 = 0x8111_0001;
    const NEW: u32 = 0x8123_0001;
    let mut entity = two_resource_weapon_entity();
    entity.resize(0x1F8, 0);
    write_u64(&mut entity, 0, 0x1F8).unwrap();
    write_array_descriptor(&mut entity, 0x20, 0x1A0, 1, 0x8080_9BC9);
    for offset in [0x1B8, 0x1D8] {
        write_u32(&mut entity, offset, OLD).unwrap();
        write_u32(&mut entity, offset + 4, 0x8080_9789).unwrap();
        write_u64(&mut entity, offset + 8, 0x2280).unwrap();
    }
    write_u32(&mut entity, 0x1F0, OLD).unwrap();
    let original = entity.clone();
    assert_eq!(
        retarget_weapon_component_owner(&mut entity, OLD, NEW),
        Ok(5)
    );
    for offset in [0x1B8, 0x1D8] {
        assert_eq!(read_u32(&entity, offset), Ok(NEW));
        assert_eq!(read_u64(&entity, offset + 8), Ok(0x2280));
    }
    assert_eq!(read_u32(&entity, 0x1F0), Ok(OLD));
    for (offset, value) in [(0x1A8, 0x8080_000B), (0x1DC, 0)] {
        let mut malformed = original.clone();
        write_u32(&mut malformed, offset, value).unwrap();
        let before = malformed.clone();
        assert!(retarget_weapon_component_owner(&mut malformed, OLD, NEW).is_err());
        assert_eq!(malformed, before);
    }
    let mut truncated = original;
    truncated.truncate(0x1F0);
    write_u64(&mut truncated, 0, 0x1F0).unwrap();
    let before = truncated.clone();
    assert!(retarget_weapon_component_owner(&mut truncated, OLD, NEW).is_err());
    assert_eq!(truncated, before);
}

#[test]
fn owner_retarget_follows_nested_reciprocal_objects_without_rewriting_collisions() {
    const OLD: u32 = 0x8111_0001;
    const NEW: u32 = 0x8123_0001;
    let entity = two_resource_weapon_entity();
    let mut owner = vec![0; 0x200];
    write_u64(&mut owner, 0, 0x200).unwrap();
    for (offset, class, target) in [
        (0x80, 0x8080_2001, 0x100),
        (0x100, 0x8080_1001, 0x80),
        (0x120, 0x8080_9789, 0x160),
        (0x160, 0x8080_9788, 0x120),
        (0x180, 0x8080_9789, 0xFFFF_FFF8),
        (0x1A0, 0x8080_9789, 0x100),
        (0x1C0, 0x1111_1111, 0x1E0),
        (0x1E0, 0x8080_9788, 0x1C0),
    ] {
        write_u32(&mut owner, offset, OLD).unwrap();
        write_u32(&mut owner, offset + 4, class).unwrap();
        write_u64(&mut owner, offset + 8, target).unwrap();
    }
    let original = owner.clone();
    assert_eq!(
        retarget_weapon_component_owner_payload(&mut owner, &entity, OLD, NEW),
        Ok(4)
    );
    for offset in [0x80, 0x100, 0x120, 0x160] {
        assert_eq!(read_u32(&owner, offset), Ok(NEW));
        owner[offset..offset + 4].copy_from_slice(&original[offset..offset + 4]);
    }
    assert_eq!(owner, original, "only proven object references may change");
}

#[test]
fn component_graft_transplants_complete_donor_alias_closure() {
    const SELECTED: u32 = 0xAAAA_0001;
    const ALIAS: u32 = 0xBBBB_0002;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    const ONE_AT_ONE: u32 = 0x0001_0001;
    let mut donor = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x11);
    alias_second_descriptor_to_first_resource(&mut donor);
    couple_descriptors_under_first_component(&mut donor);
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x22);
    alias_second_descriptor_to_first_resource(&mut target);
    couple_descriptors_under_first_component(&mut target);

    graft_weapon_component_binding(&mut target, &donor, SELECTED).unwrap();

    let selected = weapon_component_binding(&target, SELECTED).unwrap();
    let alias = weapon_component_binding(&target, ALIAS).unwrap();
    let donor_selected = weapon_component_binding(&donor, SELECTED).unwrap();
    assert_ne!(selected.descriptor_index, alias.descriptor_index);
    assert_eq!(selected.owner_tag, donor_selected.owner_tag);
    assert_eq!(selected.concrete_class, donor_selected.concrete_class);
    assert_eq!(selected.resource_offset, donor_selected.resource_offset);
    assert_eq!(alias.owner_tag, donor_selected.owner_tag);
    assert_eq!(alias.concrete_class, donor_selected.concrete_class);
    assert_eq!(alias.resource_offset, donor_selected.resource_offset);
    assert_eq!(alias.component_index, selected.component_index);
    let target_components = native_array(&target, ENTITY_COMPONENTS_DESCRIPTOR).unwrap();
    let donor_components = native_array(&donor, ENTITY_COMPONENTS_DESCRIPTOR).unwrap();
    let target_component =
        target_components.rows + selected.component_index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
    let donor_component =
        donor_components.rows + donor_selected.component_index * WEAPON_ENTITY_COMPONENT_ROW_SIZE;
    assert_eq!(
        &target[target_component..target_component + WEAPON_ENTITY_COMPONENT_ROW_SIZE],
        &donor[donor_component..donor_component + WEAPON_ENTITY_COMPONENT_ROW_SIZE]
    );
    validate_weapon_entity(&target).unwrap();
}

#[test]
fn component_graft_rejects_missing_donor_alias_identity_without_mutation() {
    const SELECTED: u32 = 0xAAAA_0001;
    const ALIAS: u32 = 0xBBBB_0002;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    let donor = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ZERO)], 0x11);
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO)], 0x22);
    let original = target.clone();

    let error = graft_weapon_component_binding(&mut target, &donor, SELECTED).unwrap_err();

    assert!(error.contains("incompatible owner topology"));
    assert!(error.contains("0xBBBB0002 resource 0"));
    assert_eq!(target, original);
}

#[test]
fn component_graft_rejects_conflicting_alias_topology_without_mutation() {
    const SELECTED: u32 = 0xAAAA_0001;
    const FIRST_ALIAS: u32 = 0xBBBB_0002;
    const SECOND_ALIAS: u32 = 0xCCCC_0003;
    const TWO_AT_ZERO: u32 = 0x0002_0000;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    const ONE_AT_ONE: u32 = 0x0001_0001;
    let donor = aliased_weapon_entity(
        &[
            (SELECTED, TWO_AT_ZERO),
            (FIRST_ALIAS, ONE_AT_ZERO),
            (SECOND_ALIAS, ONE_AT_ONE),
        ],
        0x11,
    );
    let mut target = aliased_weapon_entity(
        &[
            (SELECTED, TWO_AT_ZERO),
            (FIRST_ALIAS, ONE_AT_ZERO),
            (SECOND_ALIAS, ONE_AT_ZERO),
        ],
        0x22,
    );
    let original = target.clone();

    let error = graft_weapon_component_binding(&mut target, &donor, SELECTED).unwrap_err();

    assert!(
        error.contains("conflicting alias topology")
            || error.contains("incompatible owner topology")
            || error.contains("Runtime component donor conflict")
    );
    assert_eq!(target, original);
}

#[test]
fn component_graft_promotes_the_complete_owner_partition_without_adding_components() {
    const SELECTED: u32 = 0xAAAA_0001;
    const COUPLED: u32 = 0xBBBB_0002;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    const ONE_AT_ONE: u32 = 0x0001_0001;
    let mut donor = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (COUPLED, ONE_AT_ONE)], 0x11);
    couple_descriptors_under_first_component(&mut donor);
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (COUPLED, ONE_AT_ONE)], 0x22);
    couple_descriptors_under_first_component(&mut target);
    let old_owner = weapon_component_binding(&target, SELECTED)
        .unwrap()
        .owner_tag;
    let donor_selected = weapon_component_binding(&donor, SELECTED).unwrap();
    let donor_coupled = weapon_component_binding(&donor, COUPLED).unwrap();
    let component_count = native_array(&target, ENTITY_COMPONENTS_DESCRIPTOR)
        .unwrap()
        .count;

    graft_weapon_component_binding(&mut target, &donor, SELECTED).unwrap();

    let selected = weapon_component_binding(&target, SELECTED).unwrap();
    let coupled = weapon_component_binding(&target, COUPLED).unwrap();
    assert_eq!(selected.owner_tag, donor_selected.owner_tag);
    assert_eq!(selected.concrete_class, donor_selected.concrete_class);
    assert_eq!(selected.resource_offset, donor_selected.resource_offset);
    assert_eq!(coupled.owner_tag, donor_coupled.owner_tag);
    assert_eq!(coupled.concrete_class, donor_coupled.concrete_class);
    assert_eq!(coupled.resource_offset, donor_coupled.resource_offset);
    assert_eq!(
        native_array(&target, ENTITY_COMPONENTS_DESCRIPTOR)
            .unwrap()
            .count,
        component_count
    );
    assert!(
        weapon_component_aliases(&target)
            .unwrap()
            .iter()
            .all(|alias| alias.owner_tag != old_owner)
    );
    validate_weapon_entity(&target).unwrap();
}

#[test]
fn component_graft_batch_rejects_conflicting_overlapping_donors_atomically() {
    const SELECTED: u32 = 0xAAAA_0001;
    const ALIAS: u32 = 0xBBBB_0002;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    const ONE_AT_ONE: u32 = 0x0001_0001;
    let mut first_donor =
        aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x11);
    alias_second_descriptor_to_first_resource(&mut first_donor);
    couple_descriptors_under_first_component(&mut first_donor);
    let mut second_donor =
        aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x22);
    alias_second_descriptor_to_first_resource(&mut second_donor);
    couple_descriptors_under_first_component(&mut second_donor);
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x33);
    alias_second_descriptor_to_first_resource(&mut target);
    couple_descriptors_under_first_component(&mut target);
    let original = target.clone();

    let error = graft_weapon_component_bindings(
        &mut target,
        &[(SELECTED, &first_donor), (ALIAS, &second_donor)],
    )
    .unwrap_err();

    assert!(error.contains("Runtime component donor conflict"));
    assert!(error.contains("0xAAAA0001"));
    assert!(error.contains("0xBBBB0002"));
    assert_eq!(target, original);
}

#[test]
fn component_graft_batch_allows_identical_overlapping_donors() {
    const SELECTED: u32 = 0xAAAA_0001;
    const ALIAS: u32 = 0xBBBB_0002;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    const ONE_AT_ONE: u32 = 0x0001_0001;
    let mut donor = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x11);
    alias_second_descriptor_to_first_resource(&mut donor);
    couple_descriptors_under_first_component(&mut donor);
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO), (ALIAS, ONE_AT_ONE)], 0x22);
    alias_second_descriptor_to_first_resource(&mut target);
    couple_descriptors_under_first_component(&mut target);

    graft_weapon_component_bindings(&mut target, &[(SELECTED, &donor), (ALIAS, &donor)]).unwrap();

    let selected = weapon_component_binding(&target, SELECTED).unwrap();
    let alias = weapon_component_binding(&target, ALIAS).unwrap();
    assert_eq!(selected.owner_tag, alias.owner_tag);
    assert_eq!(selected.concrete_class, alias.concrete_class);
    assert_eq!(selected.resource_offset, alias.resource_offset);
    validate_weapon_entity(&target).unwrap();
}

#[test]
fn component_graft_single_is_atomic_on_late_component_metadata_failure() {
    const SELECTED: u32 = 0xAAAA_0001;
    const ONE_AT_ZERO: u32 = 0x0001_0000;
    let mut target = aliased_weapon_entity(&[(SELECTED, ONE_AT_ZERO)], 0x11);
    let mut donor = target.clone();
    let descriptors = native_array(&donor, ENTITY_RESOURCE_DESCRIPTORS_DESCRIPTOR).unwrap();
    write_u32(&mut donor, descriptors.rows + 0x10, u32::MAX).unwrap();
    let original = target.clone();

    let error = graft_weapon_component_binding(&mut target, &donor, SELECTED).unwrap_err();

    assert!(error.contains("outside the component list"));
    assert_eq!(target, original);
}
