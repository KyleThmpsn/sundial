use super::*;

fn put32(p: &mut [u8], at: usize, value: u32) {
    p[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(p: &mut [u8], at: usize, value: u64) {
    p[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> (Vec<u8>, u32, u32) {
    let mut p = vec![0; 0xD2];
    let owner = TagHash::new(0x234, 3).0;
    let companion = TagHash::new(0x234, 4).0;
    put64(&mut p, 0, 0xD2);
    put32(&mut p, 8, companion);
    put32(&mut p, 12, owner);
    for (descriptor, header, class) in [
        (0x10, 0x40, 0x8080_9EFB),
        (0x58, 0x90, 0x8080_000B),
        (0x68, 0xC0, 0x8080_000A),
    ] {
        put64(&mut p, descriptor, 1);
        put64(&mut p, descriptor + 8, (header - descriptor - 8) as u64);
        put32(&mut p, header - 4, 0x8080_9FBD);
        put64(&mut p, header, 1);
        put64(&mut p, header + 8, class);
    }
    put64(&mut p, 0x50, 0x234);
    put32(&mut p, 0xA0, (1 << 3) | (1 << 4) | (1 << 5));
    p[0xD0..].copy_from_slice(&7u16.to_le_bytes());
    (p, companion, owner)
}

#[test]
fn reads_both_dependency_encodings() {
    let (p, companion, owner) = fixture();
    assert_eq!(
        dependencies(&p, companion, owner).unwrap(),
        [3, 4, 5, 7].map(|i| TagHash::new(0x234, i).0).into()
    );
}

#[test]
fn decoded_package_ids_are_not_narrowed_to_authored_tag_ids() {
    let (mut payload, companion, owner) = fixture();
    for package in [0xE06, 0xEC0, 0x1FFF] {
        put64(&mut payload, 0x50, package);
        let groups = index::decode(&payload, companion, owner).unwrap();
        assert_eq!(
            index::entries(&groups),
            [3, 4, 5, 7].map(|entry| (package as u16, entry)).into()
        );
    }
    put64(&mut payload, 0x50, 0x2000);
    assert!(index::decode(&payload, companion, owner).is_err());
}

#[test]
fn loading_classes_use_the_complete_native_word() {
    let (mut payload, companion, owner) = fixture();
    put64(&mut payload, 0x98, 0x0001_8080_000B);
    assert!(index::decode(&payload, companion, owner).is_err());
}

#[test]
fn malformed_or_incomplete_indexes_never_become_empty_successes() {
    let (p, companion, owner) = fixture();
    for (at, value) in [
        (0, 1),
        (8, 2),
        (0x3C, 0),
        (0x98, 0),
        (0xA0, 1 << 5),
        (0x60, u32::MAX),
    ] {
        let mut changed = p.clone();
        put32(&mut changed, at, value);
        assert!(
            dependencies(&changed, companion, owner).is_err(),
            "offset {at:X}"
        );
    }
    let mut duplicate = p.clone();
    duplicate[0xD0..].copy_from_slice(&5u16.to_le_bytes());
    assert!(dependencies(&duplicate, companion, owner).is_err());
    let mut outside = p;
    outside[0xD0..].copy_from_slice(&8192u16.to_le_bytes());
    assert!(dependencies(&outside, companion, owner).is_err());
    assert!(dependencies(&[], companion, owner).is_err());
}
