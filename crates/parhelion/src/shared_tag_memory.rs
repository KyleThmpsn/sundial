//! Stock-shaped shared-tag-memory companion records for type-16 owners.
//!
//! Every stock `0x80804A53` icon definition is immediately followed by a type-8
//! `0x80809EF9` record. That companion contains the complete transitive icon-resource closure,
//! grouped by package id. The shared dependency-index parser validates both native bitmap
//! and sparse entry lists by rebuilding their original layout byte-for-byte. Authored
//! companions use sparse lists after checking the complete icon-resource closure.
//! The same envelope also roots non-icon type-16 resources, so the canonical writer is exposed
//! independently from the stricter icon-donor reader.

use std::collections::{BTreeMap, BTreeSet};

use sundial::package_authoring::{icon_schema::ICON_DEFINITION_CLASS, is_valid_package_tag};
use tiger_pkg::{PackageManager, TagHash};

use crate::{
    AuthoringResult,
    error::{invalid, validation},
    format::SHARED_TAG_COMPANION_CLASS,
    shared_tag_dependency_index::dependency_entries,
    tag_payload::{read_u64, write_i64, write_u32, write_u64},
};

const ARRAY_MARKER: u32 = 0x8080_9FBD;
const PACKAGE_GROUP_CLASS: u64 = 0x8080_9EFB;
const ENTRY_INDEX_CLASS: u64 = 0x8080_000A;
const FIXED_PREFIX_SIZE: usize = 0x50;
const GROUP_ROWS_OFFSET: usize = 0x50;
const MAX_PACKAGE_GROUPS: usize = 4_096;

pub(crate) type SharedTagDependencies = BTreeSet<u32>;

#[derive(Clone, Debug)]
pub(crate) struct IconDefinitionCompanion {
    pub tag: TagHash,
    pub template_payload: Vec<u8>,
    pub dependencies: SharedTagDependencies,
}

pub(crate) fn dependency_set(tags: impl IntoIterator<Item = TagHash>) -> SharedTagDependencies {
    tags.into_iter().map(u32::from).collect()
}

pub(crate) fn adjacent_companion_tag(container_tag: TagHash) -> AuthoringResult<TagHash> {
    let index = container_tag.entry_index().checked_add(1).ok_or_else(|| {
        invalid(format!(
            "Icon definition {container_tag} has no adjacent entry"
        ))
    })?;
    let companion_tag = TagHash::new(container_tag.pkg_id(), index);
    if !is_valid_package_tag(companion_tag)
        || companion_tag.pkg_id() != container_tag.pkg_id()
        || companion_tag.entry_index() != index
    {
        return Err(invalid(format!(
            "Icon definition {container_tag} has no representable adjacent companion tag"
        )));
    }
    Ok(companion_tag)
}

pub(crate) fn read_and_validate_icon_companion(
    manager: &PackageManager,
    container_tag: TagHash,
) -> AuthoringResult<IconDefinitionCompanion> {
    let container_entry = manager.get_entry(container_tag).ok_or_else(|| {
        invalid(format!(
            "Icon definition {container_tag} has no package entry"
        ))
    })?;
    if container_entry.file_type != 0x10
        || container_entry.file_subtype != 0x00
        || container_entry.reference != ICON_DEFINITION_CLASS
    {
        return Err(invalid(format!(
            "Icon definition {container_tag} has type/reference {:02X}/{:02X}/0x{:08X}; stock definitions require 10/00/0x{ICON_DEFINITION_CLASS:08X}",
            container_entry.file_type, container_entry.file_subtype, container_entry.reference
        )));
    }

    let companion_tag = adjacent_companion_tag(container_tag)?;
    let companion_entry = manager.get_entry(companion_tag).ok_or_else(|| {
        invalid(format!(
            "Icon definition {container_tag} is missing adjacent shared-tag-memory companion {companion_tag}"
        ))
    })?;
    if companion_entry.file_type != 0x08
        || companion_entry.file_subtype != 0x00
        || companion_entry.reference != SHARED_TAG_COMPANION_CLASS
    {
        return Err(invalid(format!(
            "Icon companion {companion_tag} has type/reference {:02X}/{:02X}/0x{:08X}; expected 08/00/0x{SHARED_TAG_COMPANION_CLASS:08X}",
            companion_entry.file_type, companion_entry.file_subtype, companion_entry.reference
        )));
    }
    let payload = manager.read_tag(companion_tag).map_err(|error| {
        invalid(format!(
            "Could not read icon companion {companion_tag} for {container_tag}: {error}"
        ))
    })?;
    if payload.len() != companion_entry.file_size as usize {
        return Err(invalid(format!(
            "Icon companion {companion_tag} decoded to {} bytes while its entry declares {}",
            payload.len(),
            companion_entry.file_size
        )));
    }
    let dependencies = parse_canonical_payload(&payload, companion_tag, container_tag)?;
    Ok(IconDefinitionCompanion {
        tag: companion_tag,
        template_payload: payload,
        dependencies,
    })
}

pub(crate) fn build_icon_companion_payload(
    template_payload: &[u8],
    companion_tag: TagHash,
    container_tag: TagHash,
    dependencies: &SharedTagDependencies,
) -> AuthoringResult<Vec<u8>> {
    build_shared_tag_companion_payload(template_payload, companion_tag, container_tag, dependencies)
}

pub(crate) fn build_shared_tag_companion_payload(
    template_payload: &[u8],
    companion_tag: TagHash,
    owner_tag: TagHash,
    dependencies: &SharedTagDependencies,
) -> AuthoringResult<Vec<u8>> {
    validate_fixed_prefix(template_payload)?;
    if !is_valid_package_tag(companion_tag) || !is_valid_package_tag(owner_tag) {
        return Err(invalid(
            "A shared-tag-memory companion requires valid companion and owner tags",
        ));
    }
    if companion_tag.pkg_id() != owner_tag.pkg_id()
        || companion_tag.entry_index() != owner_tag.entry_index().saturating_add(1)
    {
        return Err(invalid(format!(
            "Shared-tag-memory companion {companion_tag} is not immediately after owner {owner_tag}"
        )));
    }
    if !dependencies.contains(&u32::from(owner_tag))
        || !dependencies.contains(&u32::from(companion_tag))
    {
        return Err(invalid(format!(
            "Shared-tag-memory companion {companion_tag} dependency closure must include itself and owner {owner_tag}"
        )));
    }

    let groups = grouped_dependencies(dependencies)?;
    let payload = encode_canonical_payload(template_payload, companion_tag, owner_tag, &groups)?;
    let parsed = parse_canonical_payload(&payload, companion_tag, owner_tag)?;
    if &parsed != dependencies {
        return Err(validation(format!(
            "Shared-tag-memory companion {companion_tag} dependency closure did not survive serialization"
        )));
    }
    Ok(payload)
}

pub(crate) fn validate_shared_tag_companion_payload(
    payload: &[u8],
    companion_tag: TagHash,
    owner_tag: TagHash,
) -> AuthoringResult<SharedTagDependencies> {
    parse_canonical_payload(payload, companion_tag, owner_tag)
}

#[cfg(test)]
pub(crate) fn validate_icon_companion_payload(
    payload: &[u8],
    companion_tag: TagHash,
    container_tag: TagHash,
) -> AuthoringResult<SharedTagDependencies> {
    validate_shared_tag_companion_payload(payload, companion_tag, container_tag)
}

fn parse_canonical_payload(
    payload: &[u8],
    companion_tag: TagHash,
    container_tag: TagHash,
) -> AuthoringResult<SharedTagDependencies> {
    validate_fixed_prefix(payload)?;
    let entries = dependency_entries(payload, companion_tag, container_tag)
        .map_err(|error| error.context(format!("Icon Companion: {companion_tag}")))?;
    let mut dependencies = SharedTagDependencies::new();
    for (package_id, entry_index) in entries {
        let tag = TagHash::new(package_id, entry_index);
        if !is_valid_package_tag(tag)
            || tag.pkg_id() != package_id
            || tag.entry_index() != entry_index
            || !dependencies.insert(u32::from(tag))
        {
            return Err(invalid(format!(
                "Icon companion {companion_tag} contains malformed or duplicate entry {entry_index} in package 0x{package_id:04X}"
            )));
        }
    }
    if grouped_dependencies(&dependencies)?.len() as u64 != read_u64(payload, 0x10)? {
        return Err(invalid(format!(
            "Icon companion {companion_tag} contains an empty package group"
        )));
    }
    Ok(dependencies)
}

fn validate_fixed_prefix(payload: &[u8]) -> AuthoringResult<()> {
    if payload.len() < FIXED_PREFIX_SIZE
        || read_u64(payload, 0x20)? != 0
        || read_u64(payload, 0x28)? != 0
        || payload[0x30..0x3C].iter().any(|byte| *byte != 0)
    {
        return Err(invalid(
            "Icon companion does not have the proven stock fixed envelope",
        ));
    }
    Ok(())
}

fn grouped_dependencies(
    dependencies: &SharedTagDependencies,
) -> AuthoringResult<BTreeMap<u16, Vec<u16>>> {
    if dependencies.is_empty() {
        return Err(invalid(
            "An icon companion dependency closure cannot be empty",
        ));
    }
    let mut groups = BTreeMap::<u16, Vec<u16>>::new();
    for raw in dependencies {
        let tag = TagHash(*raw);
        if !is_valid_package_tag(tag) {
            return Err(invalid(format!(
                "Icon companion dependency 0x{raw:08X} is not a valid package tag"
            )));
        }
        groups
            .entry(tag.pkg_id())
            .or_default()
            .push(tag.entry_index());
    }
    if groups.len() > MAX_PACKAGE_GROUPS {
        return Err(invalid(format!(
            "Icon companion dependency closure spans too many packages ({})",
            groups.len()
        )));
    }
    for indices in groups.values_mut() {
        indices.sort_unstable();
        indices.dedup();
    }
    Ok(groups)
}

fn encode_canonical_payload(
    template_payload: &[u8],
    companion_tag: TagHash,
    container_tag: TagHash,
    groups: &BTreeMap<u16, Vec<u16>>,
) -> AuthoringResult<Vec<u8>> {
    let group_count = groups.len();
    let mut payload = template_payload[..0x30].to_vec();
    write_u32(&mut payload, 0x08, u32::from(companion_tag))?;
    write_u32(&mut payload, 0x0C, u32::from(container_tag))?;
    write_u64(&mut payload, 0x10, group_count as u64)?;
    write_i64(&mut payload, 0x18, 0x28)?;
    write_u64(&mut payload, 0x20, 0)?;
    write_u64(&mut payload, 0x28, 0)?;

    payload.resize(0x3C, 0);
    push_u32(&mut payload, ARRAY_MARKER);
    push_u64(&mut payload, group_count as u64);
    push_u64(&mut payload, PACKAGE_GROUP_CLASS);
    if payload.len() != GROUP_ROWS_OFFSET {
        return Err(validation(
            "Icon companion fixed envelope did not end at its stock group-table offset",
        ));
    }

    let mut row_offsets = Vec::with_capacity(group_count);
    for (package_id, indices) in groups {
        row_offsets.push(payload.len());
        push_u64(&mut payload, u64::from(*package_id));
        payload.resize(payload.len() + 0x10, 0);
        push_u64(&mut payload, indices.len() as u64);
        push_i64(&mut payload, 0);
    }

    for ((_, indices), row) in groups.iter().zip(row_offsets) {
        let entries_start = align_up(
            payload
                .len()
                .checked_add(0x14)
                .ok_or_else(|| invalid("Icon companion tail offset overflowed"))?,
            0x10,
        )?;
        let marker_offset = entries_start - 0x14;
        payload.resize(marker_offset, 0);
        push_u32(&mut payload, ARRAY_MARKER);
        push_u64(&mut payload, indices.len() as u64);
        push_u64(&mut payload, ENTRY_INDEX_CLASS);
        if payload.len() != entries_start {
            return Err(validation(
                "Icon companion entry-index array did not satisfy stock alignment",
            ));
        }
        for index in indices {
            push_u16(&mut payload, *index);
        }
        let relative_field = row + 0x20;
        let count_target = entries_start - 0x10;
        let relative = i64::try_from(count_target)
            .and_then(|target| i64::try_from(relative_field).map(|field| target - field))
            .map_err(|_| invalid("Icon companion relative pointer overflowed"))?;
        write_i64(&mut payload, relative_field, relative)?;
    }

    let payload_len = u64::try_from(payload.len())
        .map_err(|_| invalid("Icon companion payload length overflowed"))?;
    write_u64(&mut payload, 0x00, payload_len)?;
    Ok(payload)
}

fn align_up(value: usize, alignment: usize) -> AuthoringResult<usize> {
    let mask = alignment - 1;
    value
        .checked_add(mask)
        .map(|aligned| aligned & !mask)
        .ok_or_else(|| invalid("Icon companion alignment overflowed"))
}

fn push_u16(data: &mut Vec<u8>, value: u16) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(data: &mut Vec<u8>, value: u32) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(data: &mut Vec<u8>, value: u64) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(data: &mut Vec<u8>, value: i64) {
    data.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tiger_pkg::{DestinyVersion, GameVersion};

    use super::*;

    #[test]
    #[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
    fn unrelated_stock_companions_round_trip_byte_identically_when_configured() {
        let package_directory = std::env::var_os("SUNDIAL_TEST_PACKAGES")
            .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
        let manager = PackageManager::new(
            Path::new(&package_directory),
            GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
            None,
        )
        .expect("configured Shadowkeep packages should open");

        for container_tag in [
            TagHash(0x8132_5796),
            TagHash(0x8132_57A0),
            TagHash(0x8132_E45F),
            TagHash(0x81A2_84FC),
        ] {
            let companion = read_and_validate_icon_companion(&manager, container_tag)
                .expect("stock companion should parse canonically");
            assert!(companion.dependencies.contains(&u32::from(container_tag)));
            assert!(companion.dependencies.contains(&u32::from(companion.tag)));
            let rebuilt = build_icon_companion_payload(
                &companion.template_payload,
                companion.tag,
                container_tag,
                &companion.dependencies,
            )
            .expect("stock companion should serialize");
            assert_eq!(rebuilt, companion.template_payload);
        }
    }
}
