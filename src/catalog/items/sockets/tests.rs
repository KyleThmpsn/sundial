use super::*;

fn plug_member_array(row_class: u32, item_index: u16, reserved: [u8; 6]) -> Vec<u8> {
    let mut data = vec![0_u8; 72];
    data[0..8].copy_from_slice(&1_u64.to_le_bytes());
    data[8..16].copy_from_slice(&16_i64.to_le_bytes());
    data[24..32].copy_from_slice(&1_u64.to_le_bytes());
    data[32..36].copy_from_slice(&row_class.to_le_bytes());
    data[40..42].copy_from_slice(&item_index.to_le_bytes());
    data[42..48].copy_from_slice(&reserved);
    data
}

#[test]
fn plug_member_indices_are_u16_and_require_the_native_row_class() {
    let item_hashes = [0x1111_u64, 0x2222];
    let data = plug_member_array(0x8080_2E03, 1, [0xA5; 6]);
    let decoded = plug_member_hashes(&data, 0, &item_hashes).unwrap();
    assert!(decoded.complete);
    assert_eq!(decoded.values, vec![0x2222]);

    let wrong_class = plug_member_array(0xDEAD_BEEF, 1, [0; 6]);
    let decoded = plug_member_hashes(&wrong_class, 0, &item_hashes).unwrap();
    assert!(!decoded.complete);
    assert!(decoded.values.is_empty());
}

#[test]
fn package_sources_keep_embedded_reusable_and_randomized_members_separate() {
    let mut item = vec![0_u8; 240];
    write_u16(&mut item, 12, 0);
    write_u16(&mut item, 32, 1);
    write_plug_member_array(&mut item, 64, 2, 128);
    write_u32(&mut item, 144, 3);
    write_u32(&mut item, 176, 1);

    let mut plug_sets = vec![0_u8; 320];
    write_array_descriptor(&mut plug_sets, 8, 2, 64);
    write_plug_member_array(&mut plug_sets, 88, 1, 160);
    write_u32(&mut plug_sets, 176, 2);
    write_plug_member_array(&mut plug_sets, 112, 2, 224);
    write_u32(&mut plug_sets, 240, 4);
    write_u32(&mut plug_sets, 272, 5);

    let sources = socket_package_sources(&item, 0, &[100, 101, 102, 103, 104, 105], &plug_sets);

    assert_eq!(sources.len(), 3);
    assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
    assert_eq!(sources[0].allowed, vec![103, 101]);
    assert_eq!(sources[0].ordered_members, vec![103, 101]);
    assert!(sources[0].valid);
    assert_eq!(
        sources[1].kind,
        SocketOptionSourceKind::ReusableSet { index: 0 }
    );
    assert_eq!(sources[1].allowed, vec![102]);
    assert_eq!(sources[1].ordered_members, vec![102]);
    assert!(sources[1].valid);
    assert_eq!(
        sources[2].kind,
        SocketOptionSourceKind::RandomizedSet { index: 1 }
    );
    assert_eq!(sources[2].allowed, vec![104, 105]);
    assert_eq!(sources[2].ordered_members, vec![104, 105]);
    assert!(sources[2].valid);
}

#[test]
fn invalid_shared_set_reference_is_retained_without_unsafe_members() {
    let mut item = vec![0_u8; 96];
    write_u16(&mut item, 12, 7);
    write_u16(&mut item, 32, u16::MAX);
    let mut plug_sets = vec![0_u8; 128];
    write_array_descriptor(&mut plug_sets, 8, 1, 64);

    let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

    assert_eq!(sources.len(), 1);
    assert_eq!(
        sources[0].kind,
        SocketOptionSourceKind::ReusableSet { index: 7 }
    );
    assert!(!sources[0].valid);
    assert!(sources[0].allowed.is_empty());
}

#[test]
fn referenced_shared_set_decodes_when_unreferenced_tail_rows_are_missing() {
    let mut item = vec![0_u8; 96];
    write_u16(&mut item, 12, 0);
    write_u16(&mut item, 32, u16::MAX);
    let mut plug_sets = vec![0_u8; 160];
    write_array_descriptor(&mut plug_sets, 8, 10, 48);
    write_plug_member_array(&mut plug_sets, 72, 1, 112);
    write_u32(&mut plug_sets, 128, 0);

    let sources = socket_package_sources(&item, 0, &[100], &plug_sets);

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].allowed, vec![100]);
    assert!(sources[0].valid);
}

#[test]
fn partially_invalid_list_keeps_members_that_decode_safely() {
    let mut item = vec![0_u8; 208];
    write_u16(&mut item, 12, u16::MAX);
    write_u16(&mut item, 32, u16::MAX);
    write_plug_member_array(&mut item, 64, 2, 128);
    write_u32(&mut item, 144, 0);
    write_u32(&mut item, 176, 7);

    let sources = socket_package_sources(&item, 0, &[100], &[]);

    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].kind, SocketOptionSourceKind::Embedded);
    assert_eq!(sources[0].allowed, vec![100]);
    assert_eq!(sources[0].ordered_members, vec![100]);
    assert!(!sources[0].valid);
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_i64(data: &mut [u8], offset: usize, value: i64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_array_descriptor(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
    write_u64(data, descriptor, count);
    let pointer = descriptor + 8;
    write_i64(data, pointer, i64::try_from(header - pointer).unwrap());
    write_u64(data, header, count);
}

fn write_plug_member_array(data: &mut [u8], descriptor: usize, count: u64, header: usize) {
    write_array_descriptor(data, descriptor, count, header);
    write_u32(data, header + 8, 0x8080_2E03);
}
