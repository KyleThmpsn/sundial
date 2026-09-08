use super::*;

const SOURCE_PACKAGE_ID: u16 = 0x058C;
const SOURCE_FILE_NAME: &str = "w64_compression_source_058c_0.pkg";
const OUTPUT_PACKAGE_ID: u16 = 0x0AA0;
const OUTPUT_FILE_NAME: &str = "w64_compression_output_0aa0_0.pkg";
const TAIL_SIZE: usize = 4133;

struct NativeFixture {
    _root: tempfile::TempDir,
    native_packages: PathBuf,
    raw_packages: PathBuf,
    original_payloads: Vec<Vec<u8>>,
    original_bytes: Vec<u8>,
}

impl NativeFixture {
    fn new() -> Self {
        let configured = configured_packages();
        let runtime = configured
            .parent()
            .expect("configured packages must have an install parent")
            .join("bin/x64/oo2core_3_win64.dll");
        assert!(
            runtime.is_file(),
            "missing native runtime: {}",
            runtime.display()
        );

        let root = tempfile::tempdir().unwrap();
        let native_packages = root.path().join("native/packages");
        let raw_packages = root.path().join("raw/packages");
        let native_bin = root.path().join("native/bin/x64");
        fs::create_dir_all(&native_packages).unwrap();
        fs::create_dir_all(&raw_packages).unwrap();
        fs::create_dir_all(&native_bin).unwrap();
        fs::copy(&runtime, native_bin.join("oo2core_3_win64.dll")).unwrap();

        let original_payloads = vec![
            b"An unchanged original entry".to_vec(),
            b"The entry selected for replacement".to_vec(),
            noise(BLOCK_SIZE + 37),
        ];
        let original_bytes = raw_template(&original_payloads);
        for packages in [&native_packages, &raw_packages] {
            fs::write(packages.join(SOURCE_FILE_NAME), &original_bytes).unwrap();
        }
        // Register the configured version-3 decoder before either writer reopens compressed
        // blocks. The fixture contains no stock packages and ignore_caches prevents cache writes.
        let manager = sundial::package_authoring::open_shadowkeep_package_manager(&native_packages)
            .expect("the temporary native package view must initialize the decoder");
        assert_eq!(manager.package_paths.len(), 1);
        drop(manager);

        Self {
            _root: root,
            native_packages,
            raw_packages,
            original_payloads,
            original_bytes,
        }
    }

    fn assert_sources_unchanged(&self) {
        for packages in [&self.native_packages, &self.raw_packages] {
            assert_eq!(
                fs::read(packages.join(SOURCE_FILE_NAME)).unwrap(),
                self.original_bytes,
                "package authoring must not rewrite its source generation"
            );
        }
    }
}

fn configured_packages() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
            .expect("PARHELION_CLEAN_STOCK_PACKAGES must identify a Shadowkeep install"),
    )
    .canonicalize()
    .unwrap()
}

fn raw_template(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut prefix = [0u8; 8];
    prefix[..4].copy_from_slice(&0x8080_4A69u32.to_le_bytes());
    prefix[4..].copy_from_slice(&(8u32 << 9).to_le_bytes());
    let prefixes = vec![prefix; payloads.len()];
    let block_count = payloads
        .iter()
        .map(|bytes| bytes.len().div_ceil(BLOCK_SIZE))
        .sum();
    let mut bytes =
        build_standalone_package_skeleton(SOURCE_PACKAGE_ID, &prefixes, block_count, &[]).unwrap();
    let layout = PackageLayout::parse(&bytes).unwrap();
    let trailer = layout.opaque_trailer(&bytes).unwrap().to_vec();
    bytes.truncate(layout.opaque_trailer_offset);
    let encoder = PackageBlockEncoder::default();
    let mut next_block = 0;
    for (index, payload) in payloads.iter().enumerate() {
        write_payload(
            &mut bytes,
            &layout,
            layout.block_table_offset,
            index,
            payload,
            0,
            &mut next_block,
            payload.len().div_ceil(BLOCK_SIZE),
            &encoder,
        )
        .unwrap();
    }
    assert_eq!(next_block, block_count);
    layout.update_package_tables_hash(&mut bytes).unwrap();
    append_opaque_trailer(&mut bytes, &trailer).unwrap();
    layout.set_file_size(&mut bytes).unwrap();
    PackageLayout::parse(&bytes).unwrap();
    bytes
}

fn compressible(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index / BLOCK_SIZE) * 53 + index % 11) as u8)
        .collect()
}

fn noise(length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    let mut state = 0x6C98_5F17_1D2A_B043u64;
    for chunk in bytes.chunks_mut(8) {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^= value >> 31;
        chunk.copy_from_slice(&value.to_le_bytes()[..chunk.len()]);
    }
    bytes
}

fn specs(payloads: &[Vec<u8>]) -> Vec<NewTagSpec> {
    payloads
        .iter()
        .map(|payload| NewTagSpec {
            template_tag: TagHash::new(SOURCE_PACKAGE_ID, 0),
            payload: payload.clone(),
            storage: NewTagStorageMode::InheritTemplate,
        })
        .collect()
}

fn reopen_and_check(
    artifact: &ExtendedOverlayArtifact,
    directory: &Path,
    payloads: &[Vec<u8>],
) -> PackageD2PreBL {
    // Parsing validates the metadata SHA-1 and every locally owned physical block SHA-1.
    let layout = PackageLayout::parse(artifact.bytes()).unwrap();
    assert_eq!(layout.entry_count, payloads.len());
    let path = artifact.write_new(directory).unwrap();
    assert_eq!(fs::read(&path).unwrap(), artifact.bytes());
    let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
    for (index, expected) in payloads.iter().enumerate() {
        assert_eq!(
            package
                .read_tag(TagHash::new(layout.package_id, index as u16))
                .unwrap(),
            *expected,
            "entry {index} must survive writing and reopening exactly"
        );
    }
    package
}

fn assert_entry_flags(
    artifact: &ExtendedOverlayArtifact,
    package: &PackageD2PreBL,
    entry_index: usize,
    expected_flags: &[u16],
) {
    let layout = PackageLayout::parse(artifact.bytes()).unwrap();
    let entry = &package.entries()[entry_index];
    assert_eq!(entry.starting_block_offset, 0);
    assert_eq!(
        (entry.file_size as usize).div_ceil(BLOCK_SIZE),
        expected_flags.len()
    );
    for (ordinal, expected_flag) in expected_flags.iter().enumerate() {
        let row = layout.block_table_offset
            + (entry.starting_block as usize + ordinal) * BLOCK_HEADER_SIZE;
        let bytes = artifact.bytes();
        let flags = u16::from_le_bytes(bytes[row + 10..row + 12].try_into().unwrap());
        let patch = u16::from_le_bytes(bytes[row + 8..row + 10].try_into().unwrap());
        let stored_size = read_u32(bytes, row + 4).unwrap() as usize;
        let logical_size = (entry.file_size as usize - ordinal * BLOCK_SIZE).min(BLOCK_SIZE);
        assert_eq!(
            flags, *expected_flag,
            "entry {entry_index}, block {ordinal}"
        );
        assert_eq!(patch, layout.patch_id);
        assert_eq!(&bytes[row + 32..row + BLOCK_HEADER_SIZE], &[0; 16]);
        if flags == 0 {
            assert_eq!(
                stored_size, logical_size,
                "raw fallback must not acquire block padding"
            );
        } else {
            assert!(
                stored_size < logical_size,
                "compression must beat the unpadded raw size"
            );
        }
    }
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and its native Oodle3 DLL, writes temporary packages only"]
fn native_standalone_compression_round_trips_mixed_blocks_and_reduces_size() {
    let fixture = NativeFixture::new();
    let payloads = vec![
        compressible(BLOCK_SIZE * 2 + TAIL_SIZE),
        noise(BLOCK_SIZE),
        vec![0xA7],
    ];
    let new_tags = specs(&payloads);
    let native = build_standalone_package_with_references(
        &fixture.native_packages,
        OUTPUT_PACKAGE_ID,
        OUTPUT_FILE_NAME,
        &new_tags,
        &[],
    )
    .unwrap();
    let raw = build_standalone_package_with_references(
        &fixture.raw_packages,
        OUTPUT_PACKAGE_ID,
        OUTPUT_FILE_NAME,
        &new_tags,
        &[],
    )
    .unwrap();
    let native_package = reopen_and_check(&native, &fixture.native_packages, &payloads);
    let raw_package = reopen_and_check(&raw, &fixture.raw_packages, &payloads);
    assert_eq!(native.plan.appended_tags, raw.plan.appended_tags);
    assert_entry_flags(&native, &native_package, 0, &[1, 1, 1]);
    assert_entry_flags(&native, &native_package, 1, &[0]);
    assert_entry_flags(&native, &native_package, 2, &[0]);
    for (index, payload) in payloads.iter().enumerate() {
        assert_entry_flags(
            &raw,
            &raw_package,
            index,
            &vec![0; payload.len().div_ceil(BLOCK_SIZE)],
        );
    }
    assert!(native.bytes().len() + BLOCK_SIZE < raw.bytes().len());
    eprintln!(
        "Standalone package bytes: raw={}, compressed={}",
        raw.bytes().len(),
        native.bytes().len()
    );
    fixture.assert_sources_unchanged();
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES and its native Oodle3 DLL, writes temporary packages only"]
fn native_overlay_compression_preserves_originals_and_round_trips_new_blocks() {
    let fixture = NativeFixture::new();
    let replacement = compressible(BLOCK_SIZE * 2 + TAIL_SIZE);
    let appended_payloads = vec![
        compressible(BLOCK_SIZE + TAIL_SIZE),
        noise(BLOCK_SIZE),
        vec![0x6D],
    ];
    let replacements = [ReplacementSpec {
        tag: TagHash::new(SOURCE_PACKAGE_ID, 1),
        payload: replacement.clone(),
    }];
    let new_tags = specs(&appended_payloads);
    let native = build_extended_overlay(
        &fixture.native_packages,
        SOURCE_PACKAGE_ID,
        &replacements,
        &new_tags,
    )
    .unwrap();
    let raw = build_extended_overlay(
        &fixture.raw_packages,
        SOURCE_PACKAGE_ID,
        &replacements,
        &new_tags,
    )
    .unwrap();
    let mut expected = fixture.original_payloads.clone();
    expected[1] = replacement;
    expected.extend(appended_payloads);
    let native_package = reopen_and_check(&native, &fixture.native_packages, &expected);
    let raw_package = reopen_and_check(&raw, &fixture.raw_packages, &expected);
    assert_eq!(native.plan.appended_tags, raw.plan.appended_tags);
    assert_entry_flags(&native, &native_package, 1, &[1, 1, 1]);
    assert_entry_flags(&native, &native_package, 3, &[1, 1]);
    assert_entry_flags(&native, &native_package, 4, &[0]);
    assert_entry_flags(&native, &native_package, 5, &[0]);
    for index in [1, 3, 4, 5] {
        assert_entry_flags(
            &raw,
            &raw_package,
            index,
            &vec![0; expected[index].len().div_ceil(BLOCK_SIZE)],
        );
    }

    let original = PackageLayout::parse(&fixture.original_bytes).unwrap();
    let layout = PackageLayout::parse(native.bytes()).unwrap();
    assert_eq!(layout.patch_id, 1);
    for index in [0, 2] {
        let before = original.entry_table_offset + index * ENTRY_HEADER_SIZE;
        let after = layout.entry_table_offset + index * ENTRY_HEADER_SIZE;
        assert_eq!(
            &fixture.original_bytes[before..before + ENTRY_HEADER_SIZE],
            &native.bytes()[after..after + ENTRY_HEADER_SIZE],
            "unchanged entry {index} must retain its complete source header"
        );
    }
    let block_bytes = original.block_count * BLOCK_HEADER_SIZE;
    assert_eq!(
        &fixture.original_bytes
            [original.block_table_offset..original.block_table_offset + block_bytes],
        &native.bytes()[layout.block_table_offset..layout.block_table_offset + block_bytes],
        "all inherited block records must still refer to the unchanged source generation"
    );
    assert!(native.bytes().len() + BLOCK_SIZE < raw.bytes().len());
    eprintln!(
        "Overlay package bytes: raw={}, compressed={}",
        raw.bytes().len(),
        native.bytes().len()
    );
    fixture.assert_sources_unchanged();
}

fn rebuild_asset_copy(
    before: &[u8],
    layout: &PackageLayout,
    payloads: &[Vec<u8>],
    encoder: &PackageBlockEncoder,
) -> Vec<u8> {
    assert_eq!(
        layout.patch_id, 0,
        "the authored asset package must own every block"
    );
    // This checked prefix retains every metadata region and rejects unclassified data before
    // discarding old physical payloads. Shared-tag tables and entry prefixes stay in place.
    let prefix = layout.sparse_overlay_metadata_prefix(before).unwrap();
    let mut bytes = prefix.to_vec();
    let mut next_block = 0usize;
    for (index, payload) in payloads.iter().enumerate() {
        write_payload(
            &mut bytes,
            layout,
            layout.block_table_offset,
            index,
            payload,
            0,
            &mut next_block,
            payload.len().div_ceil(BLOCK_SIZE),
            encoder,
        )
        .unwrap();
    }
    assert_eq!(
        next_block, layout.block_count,
        "this regression preserves the existing block directory"
    );
    let hash_range = layout.update_package_tables_hash(&mut bytes).unwrap();
    append_opaque_trailer(&mut bytes, layout.opaque_trailer(before).unwrap()).unwrap();
    layout.set_file_size(&mut bytes).unwrap();
    let candidate = PackageLayout::parse(&bytes).unwrap();
    assert_eq!(
        candidate.opaque_trailer(&bytes).unwrap(),
        layout.opaque_trailer(before).unwrap()
    );

    // Normalize only the fields compression is allowed to change, then compare the complete
    // header and metadata prefix, including opaque tables not interpreted by the package reader.
    let mut normalized = bytes[..prefix.len()].to_vec();
    normalized[hash_range.clone()].copy_from_slice(&before[hash_range]);
    normalized[0x160..0x168].copy_from_slice(&before[0x160..0x168]);
    for index in 0..layout.entry_count {
        let location = layout.entry_table_offset + index * ENTRY_HEADER_SIZE + 8;
        normalized[location..location + 8].copy_from_slice(&before[location..location + 8]);
    }
    let blocks = layout.block_table_offset
        ..layout.block_table_offset + layout.block_count * BLOCK_HEADER_SIZE;
    normalized[blocks.clone()].copy_from_slice(&before[blocks]);
    assert_eq!(
        normalized, prefix,
        "compression must preserve all other metadata bytes"
    );
    bytes
}

fn stored_payload_size(bytes: &[u8], layout: &PackageLayout) -> usize {
    (0..layout.block_count)
        .map(|index| {
            read_u32(
                bytes,
                layout.block_table_offset + index * BLOCK_HEADER_SIZE + 4,
            )
            .unwrap() as usize
        })
        .sum()
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with an installed asset package, writes a temporary copy only"]
fn native_installed_asset_compressed_copy_round_trips_without_source_changes() {
    let fixture = NativeFixture::new();
    let packages = configured_packages();
    let path = packages.join("w64_parhelion_assets_0aa0_0.pkg");
    let before =
        fs::read(&path).expect("the configured view must contain the authored asset package");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let layout = PackageLayout::parse(&before).unwrap();
    let source = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
    let encoder = PackageBlockEncoder::open_for_packages(&fixture.native_packages).unwrap();
    let payloads = (0..layout.entry_count)
        .map(|index| {
            source
                .read_tag(TagHash::new(layout.package_id, index as u16))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let after = rebuild_asset_copy(&before, &layout, &payloads, &encoder);
    let copy_path = fixture.native_packages.join(path.file_name().unwrap());
    let mut copy_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&copy_path)
        .unwrap();
    copy_file.write_all(&after).unwrap();
    copy_file.sync_all().unwrap();
    drop(copy_file);
    assert_eq!(fs::read(&copy_path).unwrap(), after);
    let reopened = PackageD2PreBL::open(copy_path.to_str().unwrap()).unwrap();
    for (index, expected) in payloads.iter().enumerate() {
        assert_eq!(
            reopened
                .read_tag(TagHash::new(layout.package_id, index as u16))
                .unwrap(),
            *expected
        );
    }
    let candidate = PackageLayout::parse(&after).unwrap();
    assert!(after.len() <= before.len());
    eprintln!(
        "Verified temporary asset copy: original file={}, compressed file={}, original block storage={}, \
         compressed block storage={}, entries={}, blocks={}. Every entry round-trips and all \
         non-storage metadata and opaque trailer bytes are unchanged. The installed file is untouched.",
        before.len(),
        after.len(),
        stored_payload_size(&before, &layout),
        stored_payload_size(&after, &candidate),
        layout.entry_count,
        layout.block_count,
    );
    drop(reopened);
    drop(source);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fixture.assert_sources_unchanged();
}
