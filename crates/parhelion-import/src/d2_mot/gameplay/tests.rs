use super::*;

fn array_fixture(at: usize, class: u32, stride: usize, count: usize) -> Payload {
    let mut bytes = vec![0; 0x80 + stride * count];
    bytes[at..at + 8].copy_from_slice(&(count as u64).to_le_bytes());
    bytes[at + 8..at + 16].copy_from_slice(&(0x70i64 - (at + 8) as i64).to_le_bytes());
    bytes[0x70..0x78].copy_from_slice(&(count as u64).to_le_bytes());
    bytes[0x78..0x7c].copy_from_slice(&class.to_le_bytes());
    Payload(bytes)
}

#[test]
fn source_rows_use_modern_strides_and_resolve_table_identities() {
    let mut table = array_fixture(8, 0x808076AE, 12, 2);
    table.0[0x80..0x84].copy_from_slice(&123u32.to_le_bytes());
    table.0[0x8c..0x90].copy_from_slice(&456u32.to_le_bytes());
    let mut item = array_fixture(0x30, 0x80807387, 0x20, 2);
    item.0[0xa0..0xa4].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(perk_hashes(&item, 0x20, &table).unwrap(), [123, 456]);
    item.0[0xa0..0xa4].copy_from_slice(&2u32.to_le_bytes());
    assert!(perk_hashes(&item, 0x20, &table).is_err());

    let mut table = array_fixture(8, 0x8080586F, 0x24, 2);
    table.0[0x80..0x84].copy_from_slice(&123u32.to_le_bytes());
    table.0[0xa4..0xa8].copy_from_slice(&456u32.to_le_bytes());
    let mut item = array_fixture(0x20, 0x80807386, 0x40, 2);
    item.0[0x84..0x88].copy_from_slice(&(-7i32).to_le_bytes());
    item.0[0xc0..0xc4].copy_from_slice(&1u32.to_le_bytes());
    item.0[0xc8..0xd0].copy_from_slice(&1u64.to_le_bytes());
    let stats = stat_values(&item, 0x20, &table).unwrap();
    assert_eq!(stats[0], json!({"hash":123,"value":-7,"literal":true}));
    assert_eq!(stats[1]["hash"], 456);
    assert_eq!(stats[1]["literal"], false);
}
