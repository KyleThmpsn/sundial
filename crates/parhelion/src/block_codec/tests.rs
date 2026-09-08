use super::*;

#[test]
fn raw_blocks_store_only_the_plaintext_range() {
    let block = PackageBlockEncoder::default()
        .encode(0x058C, b"Every End")
        .unwrap();
    assert_eq!(block.stored, b"Every End");
    assert_eq!(block.flags, FULL_RAW_FLAGS);
    assert_eq!(block.gcm_tag, [0; 16]);
}

#[test]
fn full_raw_blocks_keep_their_complete_logical_span() {
    let plaintext = vec![0xA5; BLOCK_SIZE];
    let block = PackageBlockEncoder::default()
        .encode(0x058C, &plaintext)
        .unwrap();
    assert_eq!(block.stored, plaintext);
}

#[test]
fn empty_and_oversized_plaintext_blocks_are_rejected() {
    let encoder = PackageBlockEncoder::default();
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
}

#[cfg(all(windows, target_pointer_width = "64"))]
#[test]
fn a_present_but_invalid_install_codec_does_not_silently_disable_compression() {
    let root = tempfile::tempdir().unwrap();
    let packages = root.path().join("packages");
    let runtime = root.path().join("bin/x64");
    std::fs::create_dir(&packages).unwrap();
    std::fs::create_dir_all(&runtime).unwrap();
    std::fs::write(runtime.join("oo2core_3_win64.dll"), b"not a native library").unwrap();
    assert!(PackageBlockEncoder::open_for_packages(&packages).is_err());
}
