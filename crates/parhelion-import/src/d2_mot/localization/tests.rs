use super::*;

fn u64_at(p: &mut Payload, at: usize, value: u64) {
    p.0[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn array(p: &mut Payload, at: usize, header: usize, count: u64, class: u32) {
    u64_at(p, at, count);
    u64_at(p, at + 8, (header - at - 8) as u64);
    u64_at(p, header, count);
    p.0[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
}
fn fixture() -> Payload {
    let mut p = Payload(vec![0; 256]);
    array(&mut p, 8, 0x50, 2, 0x808099F7);
    array(&mut p, 0x28, 0xA0, 5, 0x80800001);
    array(&mut p, 0x38, 0xC0, 1, 0x808099F5);
    u64_at(&mut p, 0x68, 0xB0 - 0x68);
    p.0[0x74..0x76].copy_from_slice(&4u16.to_le_bytes());
    u64_at(&mut p, 0x88, 0xB4 - 0x88);
    p.0[0x94..0x96].copy_from_slice(&1u16.to_le_bytes());
    p.0[0x98..0x9A].copy_from_slice(&0xE142u16.to_le_bytes());
    p.0[0xB0..0xB5].copy_from_slice("é! \u{1}".as_bytes());
    u64_at(&mut p, 0xD0, (-0x70i64) as u64);
    u64_at(&mut p, 0xD8, 2);
    p
}
#[test]
fn joins_utf8_parts_and_preserves_full_symbol_shift() {
    assert_eq!(decode(&fixture(), 0xD0).unwrap(), "é! \u{E143}");
}
#[test]
fn rejects_misaligned_parts_and_outside_characters() {
    let mut p = fixture();
    u64_at(&mut p, 0xD0, (-0x6Fi64) as u64);
    assert!(decode(&p, 0xD0).is_err());
    let mut p = fixture();
    p.0[0x74..0x76].copy_from_slice(&6u16.to_le_bytes());
    assert!(decode(&p, 0xD0).is_err());
    let mut p = fixture();
    u64_at(&mut p, 0xD8, u64::MAX);
    assert!(decode(&p, 0xD0).is_err());
}
