use super::*;

#[test]
fn raw_blocks_enforce_length_bounds_and_store_only_plaintext() {
    let encoder = PackageBlockEncoder::default();
    for plaintext in [b"Every End".to_vec(), vec![0xA5; BLOCK_SIZE]] {
        let block = encoder.encode(0x058C, &plaintext).unwrap();
        assert_eq!(block.stored, plaintext);
        assert_eq!(block.flags, FULL_RAW_FLAGS);
        assert_eq!(block.gcm_tag, [0; 16]);
    }
    assert!(encoder.encode(0x058C, &[]).is_err());
    assert!(encoder.encode(0x058C, &vec![0; BLOCK_SIZE + 1]).is_err());
}

#[test]
fn compressed_storage_is_used_only_when_strictly_smaller() {
    let raw = [7_u8; 64];
    for len in [64, 65, BLOCK_SIZE + 1] {
        let block = select_verified_storage(&raw, Some(vec![1; len])).unwrap();
        assert_eq!(block.stored, raw);
        assert_eq!(block.flags, FULL_RAW_FLAGS);
    }
    let block = select_verified_storage(&raw, Some(vec![1; 12])).unwrap();
    assert_eq!(block.stored, [1; 12]);
    assert_eq!(block.flags, COMPRESSED_FLAGS);
    assert_eq!(block.gcm_tag, [0; 16]);
    assert!(select_verified_storage(&raw, Some(Vec::new())).is_err());
}

#[test]
fn missing_package_directory_is_rejected_and_codec_free_views_remain_supported() {
    let root = tempfile::tempdir().unwrap();
    assert!(PackageBlockEncoder::open_for_packages(&root.path().join("missing")).is_err());
    let packages = root.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    let encoder = PackageBlockEncoder::open_for_packages(&packages).unwrap();
    let block = encoder.encode(0x058C, &[7; 500]).unwrap();
    assert_eq!(block.flags, FULL_RAW_FLAGS);
    assert_eq!(block.stored, [7; 500]);
    let runtime = root.path().join("bin/x64");
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(runtime.join("oo2core_3_win64.dll"), b"not a native library").unwrap();
    assert!(PackageBlockEncoder::open_for_packages(&packages).is_err());
}
