//! A private perk's residency chain: the template it starts from and the chain that keeps
//! its runtime assets loaded.
use super::*;

pub(in crate::item) fn read_private_perk_residency_template(
    manager: &PackageManager,
    tag: TagHash,
    expected_file_type: u8,
    expected_reference: u32,
    expected_size: usize,
    description: &str,
) -> AuthoringResult<Vec<u8>> {
    let entry = manager
        .get_entry(tag)
        .ok_or_else(|| invalid(format!("{description} template {tag} is not live")))?;
    if entry.file_type != expected_file_type
        || entry.file_subtype != 0
        || entry.reference != expected_reference
    {
        return Err(invalid(format!(
            "{description} template {tag} has type/reference {:02X}/{:02X}/0x{:08X}; expected {expected_file_type:02X}/00/0x{expected_reference:08X}",
            entry.file_type, entry.file_subtype, entry.reference
        )));
    }
    if entry.file_size as usize != expected_size {
        return Err(invalid(format!(
            "{description} template {tag} declares size 0x{:X}; expected 0x{expected_size:X}",
            entry.file_size
        )));
    }
    let payload = read_tag(manager, tag, description)?;
    if payload.len() != expected_size
        || usize::try_from(read_u64(&payload, 0)?).ok() != Some(payload.len())
    {
        return Err(invalid(format!(
            "{description} template {tag} decoded with a non-stock payload extent"
        )));
    }
    Ok(payload)
}

pub(in crate::item) fn build_private_perk_residency_chain(
    manager: &PackageManager,
    runtime_tag_allocator: AppendedTagAllocator,
    first_ordinal: usize,
    authored_action_tag: TagHash,
) -> AuthoringResult<Vec<NewTagSpec>> {
    let b9_ordinal = first_ordinal;
    let ba_ordinal =
        AppendedTagAllocator::checked_ordinal(first_ordinal, 1, "Private perk residency BA")?;
    let root_ordinal =
        AppendedTagAllocator::checked_ordinal(first_ordinal, 2, "Private perk residency root")?;
    let companion_ordinal = AppendedTagAllocator::checked_ordinal(
        first_ordinal,
        3,
        "Private perk residency companion",
    )?;
    let authored_b9_tag = runtime_tag_allocator.assigned_tag(
        b9_ordinal,
        "Private perk residency B9",
        "private perk residency B9",
    )?;
    let authored_ba_tag = runtime_tag_allocator.assigned_tag(
        ba_ordinal,
        "Private perk residency BA",
        "private perk residency BA",
    )?;
    let authored_root_tag = runtime_tag_allocator.assigned_tag(
        root_ordinal,
        "Private perk residency root",
        "private perk residency root",
    )?;
    let authored_companion_tag = runtime_tag_allocator.assigned_tag(
        companion_ordinal,
        "Private perk residency companion",
        "private perk residency companion",
    )?;

    let mut b9 = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        0x08,
        PRIVATE_PERK_RESIDENCY_B9_CLASS,
        PRIVATE_PERK_RESIDENCY_B9_SIZE,
        "private perk residency B9",
    )?;
    let mut ba = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        0x08,
        PRIVATE_PERK_RESIDENCY_BA_CLASS,
        PRIVATE_PERK_RESIDENCY_BA_SIZE,
        "private perk residency BA",
    )?;
    let mut root = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        0x10,
        PRIVATE_PERK_RESIDENCY_ROOT_CLASS,
        PRIVATE_PERK_RESIDENCY_ROOT_SIZE,
        "private perk residency root",
    )?;
    let companion_template = read_private_perk_residency_template(
        manager,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        0x08,
        crate::format::SHARED_TAG_COMPANION_CLASS,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE_SIZE,
        "private perk residency companion",
    )?;

    retarget_exact_tag_occurrences(
        &mut root,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        authored_ba_tag,
        &PRIVATE_PERK_RESIDENCY_ROOT_BA_OFFSETS,
        "Private perk residency root-to-BA reference",
    )?;
    validate_exact_tag_occurrences(
        &root,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        &[],
        "Private perk residency root self-reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut ba,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        authored_root_tag,
        &PRIVATE_PERK_RESIDENCY_BA_ROOT_OFFSETS,
        "Private perk residency BA-to-root reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut ba,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        authored_b9_tag,
        &PRIVATE_PERK_RESIDENCY_BA_B9_OFFSETS,
        "Private perk residency BA-to-B9 references",
    )?;
    validate_exact_tag_occurrences(
        &ba,
        PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
        &[],
        "Private perk residency BA self-reference",
    )?;
    retarget_exact_tag_occurrences(
        &mut b9,
        PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
        authored_b9_tag,
        &PRIVATE_PERK_RESIDENCY_B9_SELF_OFFSETS,
        "Private perk residency B9 self-references",
    )?;
    retarget_exact_tag_occurrences(
        &mut b9,
        PRIVATE_PERK_RESIDENCY_DONOR_ACTION_TAG,
        authored_action_tag,
        &PRIVATE_PERK_RESIDENCY_B9_ACTION_OFFSETS,
        "Private perk residency B9-to-action reference",
    )?;

    validate_exact_tag_occurrences(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        &PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS,
        "Private perk residency companion self-reference",
    )?;
    validate_exact_tag_occurrences(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
        &PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS,
        "Private perk residency companion owner reference",
    )?;
    let stock_common_dependencies = [
        TagHash::new(0x0238, 0x0B90),
        TagHash::new(0x0238, 0x0EDC),
        TagHash::new(0x0238, 0x0EDD),
    ];
    let stock_dependencies = dependency_set(
        [
            PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
            PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        ]
        .into_iter()
        .chain(stock_common_dependencies),
    );
    let parsed_stock_dependencies = validate_shared_tag_companion_payload(
        &companion_template,
        PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
        PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
    )?;
    if parsed_stock_dependencies != stock_dependencies {
        return Err(invalid(format!(
            "Private perk residency companion dependency closure changed: expected {stock_dependencies:?}, found {parsed_stock_dependencies:?}"
        )));
    }
    let authored_dependencies = dependency_set(
        [authored_root_tag, authored_companion_tag]
            .into_iter()
            .chain(stock_common_dependencies),
    );
    let companion = build_shared_tag_companion_payload(
        &companion_template,
        authored_companion_tag,
        authored_root_tag,
        &authored_dependencies,
    )?;
    // The canonical writer validates the envelope and exact dependency closure.
    // Package order changes the last sparse list and therefore the encoded length.
    // A size captured from one allocation cannot validate another package.
    validate_exact_tag_occurrences(
        &companion,
        authored_companion_tag,
        &PRIVATE_PERK_RESIDENCY_COMPANION_SELF_OFFSETS,
        "Authored private perk residency companion self-reference",
    )?;
    validate_exact_tag_occurrences(
        &companion,
        authored_root_tag,
        &PRIVATE_PERK_RESIDENCY_COMPANION_OWNER_OFFSETS,
        "Authored private perk residency companion owner reference",
    )?;

    Ok(vec![
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_B9_TEMPLATE,
            payload: b9,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_BA_TEMPLATE,
            payload: ba,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_ROOT_TEMPLATE,
            payload: root,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
        NewTagSpec {
            template_tag: PRIVATE_PERK_RESIDENCY_COMPANION_TEMPLATE,
            payload: companion,
            storage: crate::NewTagStorageMode::InheritTemplate,
        },
    ])
}
