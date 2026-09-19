use super::*;

fn fixture() -> (Vec<u8>, Record) {
    let mut data = vec![0; 64];
    data[..8].copy_from_slice(&2_u64.to_le_bytes());
    data[8..16].copy_from_slice(&24_i64.to_le_bytes());
    data[28..32].copy_from_slice(&0x8080_9FBD_u32.to_le_bytes());
    data[32..40].copy_from_slice(&2_u64.to_le_bytes());
    data[40..44].copy_from_slice(&0x8080_000F_u32.to_le_bytes());
    (
        data,
        Record {
            size: 16,
            fields: vec![(8, 3)].into(),
        },
    )
}

#[test]
fn container_requires_an_independent_typed_array_and_matching_count() {
    let (mut data, declaration) = fixture();
    let decoded = read(&data, 0, 0, 16, &declaration).unwrap().unwrap();
    assert!(
        read(&data, 0, 0, 8, &declaration)
            .unwrap_err()
            .contains("declaring element")
    );
    assert_eq!(
        decoded,
        (
            "Array Element Count".into(),
            "2 entries of 0x8080000F".into()
        )
    );
    data[..8].copy_from_slice(&3_u64.to_le_bytes());
    assert!(
        read(&data, 0, 0, 16, &declaration)
            .unwrap_err()
            .contains("disagrees")
    );
    data[8..16].fill(0);
    assert!(
        read(&data, 0, 0, 16, &declaration)
            .unwrap_err()
            .contains("no typed array")
    );
    data[..8].fill(0);
    assert_eq!(
        read(&data, 0, 0, 16, &declaration).unwrap().unwrap().1,
        "0 (empty typed array)"
    );
}

#[test]
fn wire_noop_does_not_prove_an_array_or_allow_unchecked_pointers() {
    let (mut data, mut declaration) = fixture();
    declaration.fields = vec![].into();
    assert_eq!(read(&data, 0, 0, 16, &declaration).unwrap(), None);
    declaration.fields = vec![(8, 3)].into();
    data[28..32].copy_from_slice(&0x8080_000F_u32.to_le_bytes());
    assert_eq!(read(&data, 0, 0, 16, &declaration).unwrap(), None);
    data[8..16].copy_from_slice(&i64::MAX.to_le_bytes());
    assert!(read(&data, 0, 0, 16, &declaration).is_err());
}
