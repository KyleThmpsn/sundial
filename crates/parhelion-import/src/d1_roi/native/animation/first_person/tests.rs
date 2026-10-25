use super::*;

// Build the native bank envelope independently of the production array writer.
fn bank(indices: &[u16]) -> (Payload, usize) {
    let mut bytes = vec![0; 0xC0 + indices.len() * 32];
    for (field, header, count, class) in [
        (8, 0x80, 2, 0x80808F48u32),
        (0x68, 0xB0, indices.len(), 0x80809002),
    ] {
        bytes[field..field + 8].copy_from_slice(&(count as u64).to_le_bytes());
        bytes[field + 8..field + 16]
            .copy_from_slice(&(header as i64 - field as i64 - 8).to_le_bytes());
        bytes[header..header + 8].copy_from_slice(&(count as u64).to_le_bytes());
        bytes[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
    }
    bytes[0x90..0x94].copy_from_slice(&0x80800011u32.to_le_bytes());
    bytes[0x94..0x98].copy_from_slice(&0x80800022u32.to_le_bytes());
    for (i, index) in indices.iter().enumerate() {
        let row = 0xC0 + i * 32;
        bytes[row + 16..row + 20].copy_from_slice(&(i as u32 + 10).to_le_bytes());
        bytes[row + 20..row + 24].copy_from_slice(
            &if *index == u16::MAX {
                0xFF7FFFFFu32
            } else {
                3f32.to_bits()
            }
            .to_le_bytes(),
        );
        bytes[row + 24..row + 26].copy_from_slice(&index.to_le_bytes());
    }
    (Payload(bytes), 0xC0)
}

#[test]
fn converted_bank_preserves_unbound_and_unreplaced_descriptors() {
    let (original, row) = bank(&[0, u16::MAX, 1, 0]);
    let output = patch_bank(&original, &[json!({"native":0x80800011u32,"frames":61})]).unwrap();
    let mut expected = original.0.clone();
    for at in [row + 20, row + 3 * 32 + 20] {
        expected[at..at + 4].copy_from_slice(&2f32.to_le_bytes());
    }
    assert_eq!(output.0, expected);
    if let Some(artifacts) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
        let folder = PathBuf::from(artifacts).join("d1-first-person");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("original.bin"), &original.0).unwrap();
        fs::write(folder.join("converted.bin"), &output.0).unwrap();
    }
    let (unbound, _) = bank(&[u16::MAX, u16::MAX]);
    assert_eq!(patch_bank(&unbound, &[]).unwrap().0, unbound.0);
}

#[test]
fn malformed_descriptors_do_not_become_unbound_fallbacks() {
    for (offset, value) in [(0, 1), (20, 0), (26, 1), (31, 1)] {
        let (mut malformed, row) = bank(&[u16::MAX]);
        malformed.0[row + offset] = value;
        assert!(patch_bank(&malformed, &[]).is_err());
    }
    let (bad_index, _) = bank(&[2]);
    assert!(patch_bank(&bad_index, &[]).is_err());
}
