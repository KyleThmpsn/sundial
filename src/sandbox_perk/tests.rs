#[test]
fn sunrise_projection_warning_is_advisory_and_starts_after_four_entries() {
    for count in 0..=4 {
        assert!(super::sunrise_perk_projection_warning(count).is_none());
    }
    for count in [5, 16, 64, usize::MAX] {
        let warning = super::sunrise_perk_projection_warning(count).unwrap();
        assert!(warning.contains("replicated appearance data"));
        assert!(warning.contains("not a package-format limit"));
    }
}

use super::*;
use std::{collections::BTreeSet, env, path::Path};

fn write_relative(payload: &mut [u8], pointer: usize, target: usize) {
    write_relative_pointer(payload, pointer, target).unwrap();
}

fn two_array_payload(
    primary_class: u32,
    primary_stride: usize,
    primary_rows: &[u8],
    secondary_class: u32,
    secondary_stride: usize,
    secondary_rows: &[u8],
) -> Vec<u8> {
    assert_eq!(primary_rows.len() % primary_stride, 0);
    assert_eq!(secondary_rows.len() % secondary_stride, 0);
    let primary_count = primary_rows.len() / primary_stride;
    let secondary_count = secondary_rows.len() / secondary_stride;
    let primary_rows_offset = ROOT_ARRAY_HEADER + ARRAY_HEADER_SIZE;
    let primary_end = primary_rows_offset + primary_rows.len();
    let secondary_header = primary_end + ARRAY_TRAILER_SIZE;
    let secondary_rows_offset = secondary_header + ARRAY_HEADER_SIZE;
    let len = secondary_rows_offset + secondary_rows.len();
    let mut payload = vec![0_u8; len];
    payload[0..8].copy_from_slice(&(len as u64).to_le_bytes());
    payload[PRIMARY_DESCRIPTOR..PRIMARY_DESCRIPTOR + 8]
        .copy_from_slice(&(primary_count as u64).to_le_bytes());
    write_relative(&mut payload, PRIMARY_DESCRIPTOR + 8, ROOT_ARRAY_HEADER);
    payload[SECONDARY_DESCRIPTOR..SECONDARY_DESCRIPTOR + 8]
        .copy_from_slice(&(secondary_count as u64).to_le_bytes());
    write_relative(&mut payload, SECONDARY_DESCRIPTOR + 8, secondary_header);
    payload[0x28..ROOT_ARRAY_HEADER].copy_from_slice(&NESTED_ARRAY_TRAILER);
    payload[ROOT_ARRAY_HEADER..ROOT_ARRAY_HEADER + 8]
        .copy_from_slice(&(primary_count as u64).to_le_bytes());
    payload[ROOT_ARRAY_HEADER + 8..ROOT_ARRAY_HEADER + 12]
        .copy_from_slice(&primary_class.to_le_bytes());
    payload[ROOT_ARRAY_HEADER + 12..ROOT_ARRAY_HEADER + 16]
        .copy_from_slice(&0xA1B2_C3D4_u32.to_le_bytes());
    payload[primary_rows_offset..primary_end].copy_from_slice(primary_rows);
    payload[primary_end..secondary_header].copy_from_slice(&NESTED_ARRAY_TRAILER);
    payload[secondary_header..secondary_header + 8]
        .copy_from_slice(&(secondary_count as u64).to_le_bytes());
    payload[secondary_header + 8..secondary_header + 12]
        .copy_from_slice(&secondary_class.to_le_bytes());
    payload[secondary_header + 12..secondary_header + 16]
        .copy_from_slice(&0x1020_3040_u32.to_le_bytes());
    payload[secondary_rows_offset..].copy_from_slice(secondary_rows);
    payload
}

fn finished_payload() -> Vec<u8> {
    let mut details = Vec::new();
    for marker in [0x31_u8, 0x52, 0x73] {
        details.extend(
            (0..FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE)
                .map(|index| marker.wrapping_add(u8::try_from(index).unwrap())),
        );
    }
    let mut primary = vec![0_u8; 3 * FINISHED_SANDBOX_PERK_ROW_SIZE];
    for (index, (perk, key, detail_index, trailing)) in [
        (0x1111_1111_u32, 0xAAAA_0001_u32, Some(2_usize), 0x1111_u64),
        (0x2222_2222, 0xAAAA_0002, None, 0x2222),
        (0x3333_3333, 0xAAAA_0003, Some(0), 0x3333),
    ]
    .into_iter()
    .enumerate()
    {
        let row = index * FINISHED_SANDBOX_PERK_ROW_SIZE;
        primary[row..row + 4].copy_from_slice(&perk.to_le_bytes());
        primary[row + 4..row + 8].copy_from_slice(&key.to_le_bytes());
        primary[row + 16..row + 24].copy_from_slice(&trailing.to_le_bytes());
        if let Some(detail_index) = detail_index {
            // Patched after the two arrays have their final positions.
            primary[row + 8..row + 16]
                .copy_from_slice(&(i64::try_from(detail_index).unwrap() + 1).to_le_bytes());
        }
    }
    let mut payload = two_array_payload(
        FINISHED_SANDBOX_PERK_ROW_CLASS,
        FINISHED_SANDBOX_PERK_ROW_SIZE,
        &primary,
        FINISHED_SANDBOX_PERK_DETAIL_ROW_CLASS,
        FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE,
        &details,
    );
    let layout = finished_catalog_layout(&payload).unwrap();
    for (index, detail_index) in [(0_usize, 2_usize), (2, 0)] {
        let row = layout.primary.rows + index * FINISHED_SANDBOX_PERK_ROW_SIZE;
        let target = layout.secondary.rows + detail_index * FINISHED_SANDBOX_PERK_DETAIL_ROW_SIZE;
        write_relative(&mut payload, row + 8, target);
    }
    payload
}

fn runtime_map_payload() -> Vec<u8> {
    let mut primary = Vec::new();
    for (key, tag) in [
        (0x1000_0000_u32, 0x8100_0001_u32),
        (0x3000_0000, 0x8100_0003),
        (0x5000_0000, 0x8100_0005),
    ] {
        primary.extend_from_slice(&key.to_le_bytes());
        primary.extend_from_slice(&tag.to_le_bytes());
    }
    two_array_payload(
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_CLASS,
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_SIZE,
        &primary,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_CLASS,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_SIZE,
        &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88],
    )
}

#[test]
fn finished_rows_resolve_optional_aligned_detail_rows() {
    let payload = finished_payload();
    let first = finished_sandbox_perk_at(&payload, 0).unwrap();
    let second = finished_sandbox_perk_at(&payload, 1).unwrap();
    assert_eq!(first.perk_hash, 0x1111_1111);
    assert_eq!(first.runtime_key, 0xAAAA_0001);
    assert_eq!(first.trailing, 0x1111);
    assert_eq!(first.detail.unwrap()[0], 0x73);
    assert_eq!(second.detail, None);
    assert!(finished_sandbox_perk_at(&payload, 3).is_err());
}

#[test]
fn clone_append_rebases_pointers_and_preserves_unmodified_data() {
    let payload = finished_payload();
    let old = finished_catalog_layout(&payload).unwrap();
    let old_pointers = (0..old.primary.count)
        .map(|index| {
            i64_at(
                &payload,
                old.primary.rows + index * FINISHED_SANDBOX_PERK_ROW_SIZE + 8,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let old_secondary_header = payload[old.secondary.header..old.secondary.rows].to_vec();
    let old_secondary_rows = payload[old.secondary.rows..old.secondary_end].to_vec();
    let donor = finished_sandbox_perk_at(&payload, 2).unwrap();

    let (authored, new_index) =
        clone_and_append_finished_sandbox_perk(&payload, 2, 0x4444_4444, 0xAAAA_0004).unwrap();
    let new = finished_catalog_layout(&authored).unwrap();
    assert_eq!(new_index, 3);
    assert_eq!(new.primary.count, old.primary.count + 1);
    assert_eq!(new.secondary.count, old.secondary.count + 1);
    assert_eq!(authored.len(), payload.len() + 0x18 + 0x1C);

    // The prefix, native headers, row identities and trailing values remain byte-identical;
    // only the documented counts, size, secondary pointer and primary pointers change.
    assert_eq!(&authored[0x28..0x30], &payload[0x28..0x30]);
    assert_eq!(&authored[0x38..0x40], &payload[0x38..0x40]);
    assert_finished_primary_rows_rebased(&payload, &authored, old, &old_pointers);
    assert_finished_secondary_rows_preserved(
        &payload,
        &authored,
        old,
        new,
        &old_secondary_header,
        &old_secondary_rows,
    );
    assert_appended_finished_row(&authored, new_index, &donor);
}

#[test]
fn private_tooltip_edits_preserve_stock_rows_and_runtime_identity() {
    let payload = finished_payload();
    let source = finished_sandbox_perk_at(&payload, 2).unwrap();
    let category = finished_sandbox_perk_at(&payload, 0)
        .unwrap()
        .detail
        .unwrap();
    for hidden in [false, true] {
        let (authored, index) = clone_and_append_presented_finished_sandbox_perk(
            &payload,
            2,
            0x4444_4444,
            0xAAAA_0004,
            FinishedSandboxPerkPresentation {
                name: Some(FinishedSandboxPerkName {
                    bank_index: 12,
                    string_hash: 34,
                }),
                description: Some(FinishedSandboxPerkName {
                    bank_index: 56,
                    string_hash: 78,
                }),
                category_source_index: Some(0),
                hidden,
            },
        )
        .unwrap();
        for original in 0..3 {
            assert_eq!(
                finished_sandbox_perk_at(&authored, original).unwrap(),
                finished_sandbox_perk_at(&payload, original).unwrap()
            );
        }
        let result = finished_sandbox_perk_at(&authored, index).unwrap();
        assert_eq!(result.runtime_key, 0xAAAA_0004);
        assert_eq!(result.trailing, source.trailing);
        let detail = result.detail.unwrap();
        assert_private_tooltip_detail(&detail, &source.detail.unwrap(), &category, hidden);
    }
}

fn assert_private_tooltip_detail(detail: &[u8], source: &[u8], category: &[u8], hidden: bool) {
    if hidden {
        assert_eq!(&detail[..8], &[0xFF, 0xFF, 0, 0, 0xC5, 0x9D, 0x1C, 0x81]);
        assert_eq!(&detail[8..16], &detail[..8]);
        assert_eq!(&detail[16..20], &[0xFF, 0xFF, 0, 0]);
        assert_eq!(&detail[20..24], &0x811C_9DC5_u32.to_le_bytes());
    } else {
        assert_eq!(u32_at(detail, 0).unwrap(), 12);
        assert_eq!(u32_at(detail, 4).unwrap(), 34);
        assert_eq!(u32_at(detail, 8).unwrap(), 56);
        assert_eq!(u32_at(detail, 12).unwrap(), 78);
        assert_eq!(&detail[16..20], &source[16..20]);
        assert_eq!(&detail[20..24], &category[20..24]);
    }
}

fn assert_finished_primary_rows_rebased(
    payload: &[u8],
    authored: &[u8],
    old: TwoArrayLayout,
    old_pointers: &[i64],
) {
    for (index, old_pointer) in old_pointers.iter().copied().enumerate() {
        let row = old.primary.rows + index * FINISHED_SANDBOX_PERK_ROW_SIZE;
        assert_eq!(&authored[row..row + 8], &payload[row..row + 8]);
        assert_eq!(&authored[row + 16..row + 24], &payload[row + 16..row + 24]);
        let new_pointer = i64_at(authored, row + 8).unwrap();
        let expected_pointer = if old_pointer == 0 {
            0
        } else {
            old_pointer + 0x18
        };
        assert_eq!(new_pointer, expected_pointer);
    }
}

fn assert_finished_secondary_rows_preserved(
    payload: &[u8],
    authored: &[u8],
    old: TwoArrayLayout,
    new: TwoArrayLayout,
    old_secondary_header: &[u8],
    old_secondary_rows: &[u8],
) {
    assert_eq!(
        &authored[old.primary_end + 0x18..new.secondary.header],
        &payload[old.primary_end..old.secondary.header]
    );
    assert_eq!(
        &authored[new.secondary.header + 8..new.secondary.rows],
        &old_secondary_header[8..]
    );
    assert_eq!(
        &authored[new.secondary.rows..new.secondary.rows + old_secondary_rows.len()],
        old_secondary_rows
    );
}

fn assert_appended_finished_row(authored: &[u8], new_index: usize, donor: &FinishedSandboxPerk) {
    let appended = finished_sandbox_perk_at(authored, new_index).unwrap();
    assert_eq!(appended.perk_hash, 0x4444_4444);
    assert_eq!(appended.runtime_key, 0xAAAA_0004);
    assert_eq!(appended.trailing, donor.trailing);
    assert_eq!(appended.detail, donor.detail);
}

#[test]
fn clone_append_rejects_identity_collisions_and_malformed_pointers() {
    let payload = finished_payload();
    assert!(
        clone_and_append_finished_sandbox_perk(&payload, 0, 0x1111_1111, 0xBBBB_0001)
            .unwrap_err()
            .contains("hash")
    );
    assert!(
        clone_and_append_finished_sandbox_perk(&payload, 0, 0xBBBB_0001, 0xAAAA_0003)
            .unwrap_err()
            .contains("runtime key")
    );
    assert!(
        clone_and_append_finished_sandbox_perk(&payload, 1, 0xBBBB_0001, 0xBBBB_0002)
            .unwrap_err()
            .contains("no detail row")
    );

    let mut malformed = payload;
    let layout = finished_catalog_layout(&malformed).unwrap();
    let row = layout.primary.rows;
    write_relative(&mut malformed, row + 8, layout.secondary.rows + 1);
    assert!(validate_finished_sandbox_perk_catalog(&malformed).is_err());
}

#[test]
fn runtime_assignment_insert_is_sorted_and_preserves_auxiliary_bytes() {
    let payload = runtime_map_payload();
    let old = runtime_map_layout(&payload).unwrap();
    let old_auxiliary_header = payload[old.secondary.header..old.secondary.rows].to_vec();
    let old_auxiliary_rows = payload[old.secondary.rows..old.secondary_end].to_vec();

    let authored =
        insert_sandbox_perk_runtime_assignment(&payload, 0x2000_0000, 0x8123_4567).unwrap();
    let new = runtime_map_layout(&authored).unwrap();
    let assignments = (0..new.primary.count)
        .map(|index| sandbox_perk_runtime_assignment_at(&authored, index).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        assignments
            .iter()
            .map(|row| row.runtime_key)
            .collect::<Vec<_>>(),
        [0x1000_0000, 0x2000_0000, 0x3000_0000, 0x5000_0000]
    );
    assert_eq!(assignments[1].runtime_tag, 0x8123_4567);
    assert_eq!(&authored[0x28..0x30], &payload[0x28..0x30]);
    assert_eq!(&authored[0x38..0x40], &payload[0x38..0x40]);
    assert_eq!(
        &authored[new.secondary.header + 8..new.secondary.rows],
        &old_auxiliary_header[8..]
    );
    assert_eq!(&authored[new.secondary.rows..], old_auxiliary_rows);
    assert_eq!(u64_at(&authored, SECONDARY_DESCRIPTOR).unwrap(), 2);
    assert_eq!(u64_at(&authored, new.secondary.header).unwrap(), 2);
}

#[test]
fn runtime_assignment_insert_rejects_duplicates_unsorted_rows_and_bad_layouts() {
    let payload = runtime_map_payload();
    assert!(
        insert_sandbox_perk_runtime_assignment(&payload, 0x3000_0000, 0x8123_4567)
            .unwrap_err()
            .contains("already exists")
    );

    let mut unsorted = payload.clone();
    let layout = runtime_map_layout(&unsorted).unwrap();
    unsorted[layout.primary.rows + 8..layout.primary.rows + 12]
        .copy_from_slice(&0x0800_0000_u32.to_le_bytes());
    assert!(validate_sandbox_perk_runtime_map(&unsorted).is_err());

    let mut bad_size = payload.clone();
    bad_size[0..8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(validate_sandbox_perk_runtime_map(&bad_size).is_err());

    let mut bad_trailer = payload;
    bad_trailer[0x28] = 1;
    assert!(validate_sandbox_perk_runtime_map(&bad_trailer).is_err());
}

#[test]
fn runtime_map_rejects_primary_only_rebuild_with_stale_auxiliary_descriptor() {
    let mut payload = runtime_map_payload();
    let layout = runtime_map_layout(&payload).unwrap();
    payload.truncate(layout.primary_end);
    let size = payload.len() as u64;
    payload[..8].copy_from_slice(&size.to_le_bytes());
    assert!(validate_sandbox_perk_runtime_map(&payload).is_err());
}

#[test]
fn runtime_map_writers_grow_auxiliary_storage_at_word_boundary() {
    let mut rows = Vec::new();
    for key in 1u32..=32 {
        rows.extend_from_slice(&key.to_le_bytes());
        rows.extend_from_slice(&0x81234567u32.to_le_bytes());
    }
    let payload = two_array_payload(
        SANDBOX_PERK_RUNTIME_ASSIGNMENT_ROW_CLASS,
        8,
        &rows,
        SANDBOX_PERK_RUNTIME_AUXILIARY_ROW_CLASS,
        4,
        &[0; 4],
    );
    validate_sandbox_perk_runtime_map(&payload).unwrap();
    let outputs = [
        insert_sandbox_perk_runtime_assignment(&payload, 33, 0x81234568).unwrap(),
        crate::weapon_entity::append_weapon_entity_assignment(payload, 33, 0x81234568).unwrap(),
    ];
    for output in outputs {
        let layout = runtime_map_layout(&output).unwrap();
        assert_eq!(layout.primary.count, 33);
        assert_eq!(layout.secondary.count, 2);
        assert_eq!(&output[layout.secondary.rows..], &[0; 8]);
        validate_sandbox_perk_runtime_map(&output).unwrap();
        let mut undersized = output;
        undersized.truncate(undersized.len() - 4);
        write_u64(&mut undersized, SECONDARY_DESCRIPTOR, 1).unwrap();
        write_u64(&mut undersized, layout.secondary.header, 1).unwrap();
        let size = undersized.len() as u64;
        write_u64(&mut undersized, 0, size).unwrap();
        assert!(validate_sandbox_perk_runtime_map(&undersized).is_err());
    }
}

#[test]
fn action_graph_candidates_require_a_structured_tag_lane() {
    let mut action = vec![0_u8; 0x80];
    let action_len = action.len();
    action[..8].copy_from_slice(&(action_len as u64).to_le_bytes());

    // The graph tag alone is insufficient, even when it is followed by the zero high word
    // used by a real 64-bit tag lane.  This models an incidental scalar/hash collision.
    let graph_tag = 0x8152_82E1_u32;
    action[0x20..0x24].copy_from_slice(&graph_tag.to_le_bytes());
    action[0x24..0x28].copy_from_slice(&0_u32.to_le_bytes());

    // A real direct graph member has its non-sentinel native class handle exactly 0x14
    // bytes before the tag lane.
    action[0x34..0x38].copy_from_slice(&0x8080_3E44_u32.to_le_bytes());
    action[0x48..0x4C].copy_from_slice(&graph_tag.to_le_bytes());
    action[0x4C..0x50].copy_from_slice(&0_u32.to_le_bytes());

    // A sentinel member class must not be accepted either.
    action[0x44..0x48].copy_from_slice(&u32::MAX.to_le_bytes());
    action[0x58..0x5C].copy_from_slice(&0x8152_82E2_u32.to_le_bytes());
    action[0x5C..0x60].copy_from_slice(&0_u32.to_le_bytes());

    validate_runtime_action_payload(&action, Some(action.len())).unwrap();
    assert_eq!(
        structured_action_graph_reference_candidates(&action).unwrap(),
        vec![(TagHash(graph_tag), 0x48)]
    );
}

#[test]
fn action_graph_candidates_reject_bad_action_size() {
    let mut action = vec![0_u8; 0x20];
    action[..8].copy_from_slice(&0x1F_u64.to_le_bytes());
    assert!(validate_runtime_action_payload(&action, Some(action.len())).is_err());
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES with a clean Shadowkeep package directory"]
fn clean_stock_perk_actions_prove_raw_aligned_tag_scans_are_unsafe() {
    let packages = env::var_os("PARHELION_CLEAN_STOCK_PACKAGES")
        .expect("PARHELION_CLEAN_STOCK_PACKAGES must name a clean package directory");
    let install = Path::new(&packages)
        .parent()
        .expect("clean packages need an install root");
    let manager = crate::package_runtime::open_shadowkeep_packages(install)
        .expect("open clean stock packages");
    let globals_tag =
        crate::package_runtime::resolve_live_named_tag(&manager, "investment_globals", None)
            .expect("resolve investment_globals");
    let globals = manager
        .read_tag(globals_tag)
        .expect("read investment_globals");
    let catalog_tag = TagHash(
        investment_globals_table_tag(&globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT)
            .expect("resolve finished sandbox-perk catalog"),
    );
    validate_structured_tag_entry(
        &manager,
        catalog_tag,
        FINISHED_SANDBOX_PERK_CATALOG_CLASS,
        "finished sandbox-perk catalog",
    )
    .expect("validate finished sandbox-perk catalog entry");
    let catalog = manager
        .read_tag(catalog_tag)
        .expect("read finished sandbox-perk catalog");
    let runtime_map = manager
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .expect("read runtime map");

    let mut actions = BTreeSet::new();
    for index in 0..finished_sandbox_perk_count(&catalog).expect("count finished perks") {
        let perk = finished_sandbox_perk_at(&catalog, index).expect("read finished perk");
        if let Some(assignment) = sandbox_perk_runtime_assignment(&runtime_map, perk.runtime_key)
            .expect("resolve runtime assignment")
        {
            actions.insert(TagHash(assignment.runtime_tag));
        }
    }
    assert!(
        !actions.is_empty(),
        "stock finished perks had no runtime actions"
    );

    let mut unstructured = Vec::new();
    for action_tag in actions {
        // The runtime map can retain keys for package contexts not mounted by the selected
        // package directory. Only census actions whose resource is actually resolvable.
        let Ok(payload) = manager.read_tag(action_tag) else {
            continue;
        };
        validate_runtime_action_payload(&payload, None).expect("validate runtime action size");
        let structured = structured_action_graph_reference_candidates(&payload)
            .expect("scan structured graph references")
            .into_iter()
            .filter_map(|(tag, offset)| {
                manager.get_entry(tag).and_then(|entry| {
                    (entry.file_type == 8 && entry.reference == WEAPON_ENTITY_CLASS)
                        .then_some(offset)
                })
            })
            .collect::<BTreeSet<_>>();
        let all_entity_tags = (0..payload.len().saturating_sub(3))
            .step_by(4)
            .filter_map(|offset| {
                let tag = TagHash(u32_at(&payload, offset).ok()?);
                manager.get_entry(tag).and_then(|entry| {
                    (entry.file_type == 8 && entry.reference == WEAPON_ENTITY_CLASS)
                        .then_some(offset)
                })
            })
            .collect::<BTreeSet<_>>();
        for offset in all_entity_tags.difference(&structured) {
            unstructured.push((action_tag, *offset));
        }
    }
    assert!(
        !unstructured.is_empty(),
        "the clean stock set no longer demonstrates an unsafe raw aligned-tag collision; re-review the scanner invariant"
    );
    eprintln!(
        "clean stock has {} valid weapon-entity tag occurrences outside the direct native-member/tag/zero lane; they are intentionally excluded from private perk graph cloning: {unstructured:?}",
        unstructured.len()
    );
}
