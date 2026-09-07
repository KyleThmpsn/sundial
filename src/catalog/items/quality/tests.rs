use super::*;

fn table(values: &[(u32, f32)]) -> Vec<u8> {
    let mut data = vec![0; 48 + values.len() * POWER_CAP_ROW_SIZE];
    let len = data.len() as u64;
    data[..8].copy_from_slice(&len.to_le_bytes());
    data[8..16].copy_from_slice(&(values.len() as u64).to_le_bytes());
    data[16..24].copy_from_slice(&16_i64.to_le_bytes());
    data[32..40].copy_from_slice(&(values.len() as u64).to_le_bytes());
    data[40..44].copy_from_slice(&POWER_CAP_ROW_CLASS.to_le_bytes());
    for (index, (hash, cap)) in values.iter().enumerate() {
        let row = 48 + index * POWER_CAP_ROW_SIZE;
        data[row..row + 4].copy_from_slice(&hash.to_le_bytes());
        data[row + 4..row + 8].copy_from_slice(&cap.to_le_bytes());
    }
    data
}

#[test]
fn power_caps_follow_native_values_and_indices_instead_of_season_numbers() {
    let mut values = (0..21)
        .map(|index| (100 + index, 200.0))
        .collect::<Vec<_>>();
    values[0] = (77, 99_999.0);
    values[3] = (88, 101.0);
    values[11] = (99, 225.0);
    values[20] = (111, 350.0);
    let definitions = decode_power_cap_definitions(&table(&values)).unwrap();
    assert_eq!(definitions.len(), 21);
    assert_eq!(item_power_cap(&[0], &definitions), Some(999_990));
    assert_eq!(item_power_cap(&[3], &definitions), Some(1010));
    assert_eq!(item_power_cap(&[11], &definitions), Some(2250));
    assert_eq!(item_power_cap(&[3, 20], &definitions), Some(3500));
    assert_eq!(definitions[20].hash, 111);
    values.swap(3, 20);
    let changed = decode_power_cap_definitions(&table(&values)).unwrap();
    assert_eq!(item_power_cap(&[3], &changed), Some(3500));
    assert_eq!(changed[3].hash, 111);
}

#[test]
fn duplicate_caps_keep_distinct_indices_and_hashes_through_serialization() {
    let definitions = decode_power_cap_definitions(&table(&[(77, 106.0), (88, 106.0)])).unwrap();
    let encoded = serde_json::to_vec(&definitions).unwrap();
    let restored: Vec<PowerCapDefinition> = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(restored, definitions);
    assert_eq!(restored.len(), 2);
    assert_ne!(restored[0].hash, restored[1].hash);
    assert_eq!(item_power_cap(&[0, 1], &restored), Some(1060));
}

#[test]
fn unresolved_versions_do_not_become_a_partial_or_zero_cap() {
    let definitions = decode_power_cap_definitions(&table(&[(77, 171.0)])).unwrap();
    for groups in [&[][..], &[1], &[u16::MAX], &[0, 1], &[0, u16::MAX]] {
        assert_eq!(item_power_cap(groups, &definitions), None);
    }
}

#[test]
fn malformed_power_cap_tables_are_rejected() {
    let valid = table(&[(77, 171.0)]);
    for end in 0..valid.len() {
        assert!(
            decode_power_cap_definitions(&valid[..end]).is_err(),
            "truncated at {end}"
        );
    }
    let mut extra = valid.clone();
    extra.push(0);
    assert!(decode_power_cap_definitions(&extra).is_err());
    let mut wrong_class = valid.clone();
    wrong_class[40..44].copy_from_slice(&0x8080_5921_u32.to_le_bytes());
    assert!(decode_power_cap_definitions(&wrong_class).is_err());
    let mut wrong_count = valid;
    wrong_count[8..16].copy_from_slice(&2_u64.to_le_bytes());
    assert!(decode_power_cap_definitions(&wrong_count).is_err());
    assert!(decode_power_cap_definitions(&table(&[])).is_err());
    for cap in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -1.0,
        0.0,
        f32::MAX,
        101.125,
    ] {
        assert!(
            decode_power_cap_definitions(&table(&[(77, cap)])).is_err(),
            "{cap}"
        );
    }
}
