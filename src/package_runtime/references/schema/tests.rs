use super::*;

#[test]
fn repeat_parameters_are_not_misread_as_fields() {
    let record = decode(
        0x88,
        &[(0, 10), (4, 0), (4, 0), (1, 0), (0, 4), (16, 4), (32, 3)],
    )
    .unwrap();
    assert_eq!(
        &*record.fields,
        &[(0, 4), (4, 4), (8, 4), (12, 4), (16, 4), (32, 3)]
    );
    assert!(decode(8, &[(0, 10), (100, 0), (4, 0), (1, 0), (0, 4)]).is_err());
    assert!(decode(8, &[(0, 10), (2, 0)]).is_err());
    assert!(decode(8, &[(0, 11)]).is_err());
    assert!(decode(8, &[(0, 13)]).is_err());
}

#[test]
fn generated_layouts_use_their_own_checked_size_and_program() {
    let mut data = vec![0; 0x88];
    data[0..8].copy_from_slice(&0x88u64.to_le_bytes());
    data[0x14..0x18].copy_from_slice(&0x60u32.to_le_bytes());
    data[0x38..0x40].copy_from_slice(&0x30u64.to_le_bytes());
    data[0x64..0x68].copy_from_slice(&0x8080_00E1u32.to_le_bytes());
    data[0x68..0x70].copy_from_slice(&1u64.to_le_bytes());
    data[0x70..0x74].copy_from_slice(&0x30u32.to_le_bytes());
    data[0x74..0x78].copy_from_slice(&4u32.to_le_bytes());
    let record = generated(&data).unwrap();
    assert_eq!(record.size, 0x60);
    assert_eq!(&*record.fields, &[(0x30, 4)]);
    data[0x70..0x74].copy_from_slice(&0x60u32.to_le_bytes());
    assert!(generated(&data).is_err());
    data[0x68..0x70].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(generated(&data).is_err());
}
