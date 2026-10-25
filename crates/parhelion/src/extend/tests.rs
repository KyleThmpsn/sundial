#[test]
fn invalid_field_offsets_leave_package_bytes_unchanged() {
    for offset in [7, 9, usize::MAX - 1, usize::MAX] {
        let mut bytes = [42; 8];
        assert!(super::read_u32(&bytes, offset).is_err());
        assert!(super::write_u16(&mut bytes, offset, 0).is_err());
        assert!(super::write_u32(&mut bytes, offset, 0).is_err());
        assert!(super::write_u64(&mut bytes, offset, 0).is_err());
        assert_eq!(bytes, [42; 8]);
    }
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL; read-only donor layout regression"]
fn real_spare_shared_tag_capacity_is_valid_for_read_only_templates() {
    let packages = crate::test_support::install().join("packages");
    let chain = super::discover_patch_chain(&packages, 0x01DC).unwrap();
    let path = &chain.latest().path;
    let native = super::PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
    let before = std::fs::metadata(path).unwrap().modified().unwrap();
    for index in [0, native.entries().len() - 1] {
        let prefix =
            super::template_entry_prefix(&packages, tiger_pkg::TagHash::new(0x01DC, index as u16))
                .unwrap();
        assert_eq!(
            u32::from_le_bytes(prefix[..4].try_into().unwrap()),
            native.entries()[index].reference
        );
    }
    if native.entries().len() < 0x2000 {
        assert!(
            super::template_entry_prefix(
                &packages,
                tiger_pkg::TagHash::new(0x01DC, native.entries().len() as u16)
            )
            .is_err()
        );
    }
    assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(), before);
}
use super::*;

#[test]
fn retired_entry_gap_is_emitted_as_inert_tombstones() {
    let table_offset = 16;
    let mut bytes = vec![0xA5; table_offset + 4 * ENTRY_HEADER_SIZE];

    write_reserved_entry_rows(&mut bytes, table_offset, 1, 3)
        .expect("the reserved rows should fit");

    assert!(
        bytes[table_offset..table_offset + ENTRY_HEADER_SIZE]
            .iter()
            .all(|byte| *byte == 0xA5)
    );
    for index in 1..3 {
        let row = table_offset + index * ENTRY_HEADER_SIZE;
        assert_eq!(&bytes[row..row + 4], &u32::MAX.to_le_bytes());
        assert!(
            bytes[row + 4..row + ENTRY_HEADER_SIZE]
                .iter()
                .all(|byte| *byte == 0)
        );
    }
    assert!(
        bytes[table_offset + 3 * ENTRY_HEADER_SIZE..]
            .iter()
            .all(|byte| *byte == 0xA5)
    );
}

#[test]
fn standalone_authoring_rejects_tracked_package_ids_before_package_discovery() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let error = build_standalone_package_with_references(
        directory.path(),
        0x058C,
        "w64_parhelion_assets_058c_0.pkg",
        &[NewTagSpec {
            template_tag: TagHash(0x8132_5796),
            payload: vec![1],
            storage: NewTagStorageMode::InheritTemplate,
        }],
        &[],
    )
    .expect_err("tracked package ids must not be used for standalone authored packages");

    assert!(
        error
            .to_string()
            .contains("outside the untracked AA0..=CFF window")
    );
}

#[test]
fn standalone_tags_reload_through_typed_package_directories() {
    let temporary = tempfile::tempdir().unwrap();
    let packages = temporary.path().join("packages");
    fs::create_dir(&packages).unwrap();
    let source_path = packages.join("w64_table_source_058c_0.pkg");
    let source = crate::format::build_test_package_with_physical_payload(
        0x058C,
        0,
        b"Original template payload",
    )
    .unwrap();
    fs::write(&source_path, &source).unwrap();
    let payloads = [b"Private payload".to_vec(), vec![0xA5; 0x40000 + 37]];
    let tags = payloads
        .iter()
        .map(|payload| NewTagSpec {
            template_tag: TagHash::new(0x058C, 0),
            payload: payload.clone(),
            storage: NewTagStorageMode::InheritTemplate,
        })
        .collect::<Vec<_>>();
    let artifact = build_standalone_package_with_references(
        &packages,
        0x0AA0,
        "w64_table_output_0aa0_0.pkg",
        &tags,
        &[],
    )
    .unwrap();
    let path = artifact.write_new(&packages).unwrap();
    let saved = fs::read(&path).unwrap();
    let u32_at = |offset| u32::from_le_bytes(saved[offset..offset + 4].try_into().unwrap());
    let u64_at = |offset| u64::from_le_bytes(saved[offset..offset + 8].try_into().unwrap());
    // Independent Shadowkeep/Sunrise table contract. The ordinary payload reader does not
    // check these physical type headers, which previously hid an unreadable SDK container.
    let entries = u32_at(0x110) as usize + 96;
    let entry_capacity = u64_at(entries - 16) as usize;
    let blocks = entries + entry_capacity * 16 + 32;
    for (offset, class) in [(entries, 0x8080_9EF3), (blocks, 0x8080_9EEE)] {
        assert_eq!(u32_at(offset - 20), 0x8080_9FBD);
        assert_eq!(u32_at(offset - 8), class);
    }
    assert!(entry_capacity >= u32_at(0xB4) as usize);
    assert!(u64_at(blocks - 16) >= u64::from(u32_at(0xD0)));
    let package = PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
    for (index, expected) in payloads.iter().enumerate() {
        assert_eq!(
            package
                .read_tag(TagHash::new(0x0AA0, index as u16))
                .unwrap(),
            *expected,
        );
    }
    assert_eq!(fs::read(source_path).unwrap(), source);

    let receipt = serde_json::json!({
        "package": path.file_name().unwrap().to_str().unwrap(),
        "sha256": crate::artifact::digest_file(&path).unwrap().sha256,
        "entry_table": entries,
        "block_table": blocks,
        "tags": ["81D40000", "81D40001"],
        "payload_lengths": payloads.each_ref().map(|payload| payload.len()),
        "typed_headers_accepted": true,
        "payloads_reloaded": true,
        "source_unchanged": true,
    });
    let retained = if let Some(output) = crate::test_support::artifacts("typed-package-tables") {
        fs::create_dir_all(&output).unwrap();
        fs::copy(&path, output.join(path.file_name().unwrap())).unwrap();
        output
    } else {
        temporary.keep().join("packages")
    };
    fs::write(
        retained.join("readback.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("Typed package readback: {}", retained.display());
}

#[test]
fn derives_shared_tag_enrollment_from_companion_payload_identity() {
    let package_id = 0x0914;
    let original_entry_count = 0x055F;
    let owner = appended_tag(package_id, original_entry_count, 0).unwrap();
    let companion = appended_tag(package_id, original_entry_count, 2).unwrap();
    let mut companion_payload = vec![0u8; 0x10];
    companion_payload[0x08..0x0C].copy_from_slice(&u32::from(companion).to_le_bytes());
    companion_payload[0x0C..0x10].copy_from_slice(&u32::from(owner).to_le_bytes());
    let specs = [
        NewTagSpec {
            template_tag: TagHash(0x8132_5796),
            payload: vec![1],
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: TagHash(0x8132_5797),
            payload: vec![2],
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: TagHash(0x8132_5798),
            payload: companion_payload,
            storage: NewTagStorageMode::InheritTemplate,
        },
    ];
    let metadata = [
        AppendedEntryMetadata {
            prefix: [0; 8],
            reference: 0x8080_4A53,
            file_type: SHARED_TAG_OWNER_FILE_TYPE,
            file_subtype: SHARED_TAG_FILE_SUBTYPE,
        },
        AppendedEntryMetadata {
            prefix: [0; 8],
            reference: 0x8080_4A69,
            file_type: 0x08,
            file_subtype: 0,
        },
        AppendedEntryMetadata {
            prefix: [0; 8],
            reference: SHARED_TAG_COMPANION_CLASS,
            file_type: SHARED_TAG_COMPANION_FILE_TYPE,
            file_subtype: SHARED_TAG_FILE_SUBTYPE,
        },
    ];

    assert_eq!(
        resolve_shared_tag_enrollments(package_id, original_entry_count, &specs, &metadata)
            .unwrap(),
        vec![(u32::from(owner), u32::from(companion))]
    );

    let mut malformed = metadata;
    malformed[2].reference = 0x8080_4A53;
    assert!(
        resolve_shared_tag_enrollments(package_id, original_entry_count, &specs, &malformed)
            .expect_err("a type-16 owner without its companion must fail")
            .to_string()
            .contains("missing its shared-tag companion")
    );
    assert!(
        resolve_shared_tag_enrollments(
            package_id,
            original_entry_count + 2,
            &specs[2..],
            &metadata[2..]
        )
        .expect_err("an orphan shared-tag companion must fail")
        .to_string()
        .contains("appended local owner")
    );
}

#[test]
fn rejects_invalid_and_duplicate_reference_ordinals() {
    let invalid_target = resolve_new_tag_reference(NewTagReference::Appended(2), 0, 0x058C, 100, 2)
        .expect_err("out-of-range appended target must fail");
    assert!(
        invalid_target
            .to_string()
            .contains("outside the 2 new tags")
    );

    let invalid_override = resolve_reference_modes(
        2,
        &[NewTagReferenceOverride {
            new_tag_ordinal: 2,
            reference: NewTagReference::Template,
        }],
    )
    .expect_err("out-of-range override ordinal must fail");
    assert!(
        invalid_override
            .to_string()
            .contains("outside the 2 new tags")
    );

    let duplicate = resolve_reference_modes(
        2,
        &[
            NewTagReferenceOverride {
                new_tag_ordinal: 1,
                reference: NewTagReference::Appended(0),
            },
            NewTagReferenceOverride {
                new_tag_ordinal: 1,
                reference: NewTagReference::Template,
            },
        ],
    )
    .expect_err("duplicate override ordinal must fail");
    assert!(duplicate.to_string().contains("specified more than once"));
}

#[test]
fn unchanged_payload_validation_checks_every_original_entry() {
    let source = (0u8..8).map(|value| vec![value]).collect::<Vec<_>>();
    // Entry identity is independent of storage order. A later patch can move an
    // early entry past the other entries without changing any of their payloads.
    let entries = (0..source.len())
        .map(|index| tiger_pkg::package::UEntryHeader {
            reference: 0x8080_4A69,
            file_type: 8,
            file_subtype: 0,
            starting_block: ((index * 5) % source.len()) as u32,
            starting_block_offset: 16,
            file_size: 1,
        })
        .collect::<Vec<_>>();
    let mut candidate = source.clone();
    candidate[6] = vec![0xFF];
    let error = validate_all_unchanged_entry_payloads(
        &entries,
        &BTreeSet::new(),
        |index| Ok(source[index].clone()),
        |index| Ok(candidate[index].clone()),
    )
    .expect_err("a non-sampled original entry mutation must fail");
    assert!(error.to_string().contains("payload 6"));

    let replacements = BTreeSet::from([6]);
    validate_all_unchanged_entry_payloads(
        &entries,
        &replacements,
        |index| Ok(source[index].clone()),
        |index| Ok(candidate[index].clone()),
    )
    .expect("declared replacement payloads are checked separately");
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_PACKAGE_ID"]
fn real_overlay_round_trips_mutual_references() {
    let package_directory = crate::test_support::stock_packages();
    let package_id_text = std::env::var("SUNDIAL_TEST_PACKAGE_ID")
        .expect("SUNDIAL_TEST_PACKAGE_ID must be a hexadecimal package id");
    let package_id = u16::from_str_radix(package_id_text.trim_start_matches("0x"), 16)
        .expect("SUNDIAL_TEST_PACKAGE_ID should be hexadecimal");
    let chain = discover_patch_chain(&package_directory, package_id)
        .expect("real package chain should be discoverable");
    let source_path = &chain.latest().path;
    let source = PackageD2PreBL::open(
        source_path
            .to_str()
            .expect("real package path should be Unicode"),
    )
    .expect("real source package should open");
    let original_entry_count = source.entries().len();
    let ordinary_templates = source
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.file_type != SHARED_TAG_OWNER_FILE_TYPE
                && !(entry.file_type == SHARED_TAG_COMPANION_FILE_TYPE
                    && entry.file_subtype == SHARED_TAG_FILE_SUBTYPE
                    && entry.reference == SHARED_TAG_COMPANION_CLASS)
        })
        .map(|(index, _)| TagHash::new(package_id, index as u16))
        .take(2)
        .collect::<Vec<_>>();
    assert_eq!(
        ordinary_templates.len(),
        2,
        "test package needs two ordinary non-shared entries"
    );
    let template_a = ordinary_templates[0];
    let template_b = ordinary_templates[1];
    let new_tags = [
        NewTagSpec {
            template_tag: template_a,
            payload: b"mutual-data".to_vec(),
            storage: NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: template_b,
            payload: b"mutual-header".to_vec(),
            storage: NewTagStorageMode::InheritTemplate,
        },
    ];
    let artifact = build_extended_overlay_with_references(
        &package_directory,
        package_id,
        &[],
        &new_tags,
        &[
            NewTagReferenceOverride {
                new_tag_ordinal: 0,
                reference: NewTagReference::Appended(1),
            },
            NewTagReferenceOverride {
                new_tag_ordinal: 1,
                reference: NewTagReference::Appended(0),
            },
        ],
    )
    .expect("real mutual-reference overlay should build and validate");
    let candidate_path = package_directory.join(&artifact.plan.output_file_name);
    let candidate = PackageD2PreBL::from_reader(
        candidate_path
            .to_str()
            .expect("candidate path should be Unicode"),
        Cursor::new(artifact.bytes().to_vec()),
    )
    .expect("real candidate should reopen");
    let entries = candidate.entries();
    assert_eq!(
        entries[original_entry_count].reference,
        u32::from(TagHash::new(package_id, (original_entry_count + 1) as u16))
    );
    assert_eq!(
        entries[original_entry_count + 1].reference,
        u32::from(TagHash::new(package_id, original_entry_count as u16))
    );
    let baseline = build_extended_overlay(&package_directory, package_id, &[], &new_tags[..1])
        .expect("baseline template overlay should build");
    let overridden_template = build_extended_overlay_with_references(
        &package_directory,
        package_id,
        &[],
        &new_tags[..1],
        &[NewTagReferenceOverride {
            new_tag_ordinal: 0,
            reference: NewTagReference::Template,
        }],
    )
    .expect("template override should build");
    assert_eq!(baseline.bytes(), overridden_template.bytes());
}
