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

use sundial::package_authoring::PackageManager;
use sundial::package_authoring::{icon_schema::ICON_DEFINITION_CLASS, is_valid_package_tag};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult,
    error::{invalid, validation},
    format::SHARED_TAG_COMPANION_CLASS,
    shared_tag_dependency_index::dependency_entries,
    tag_payload::{read_u64, write_u32, write_u64},
};

const FIXED_PREFIX_SIZE: usize = 0x50;
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
    let mut template = template_payload[..0x30].to_vec();
    write_u32(&mut template, 0x08, companion_tag.0)?;
    write_u32(&mut template, 0x0C, container_tag.0)?;
    write_u64(&mut template, 0x20, 0)?;
    write_u64(&mut template, 0x28, 0)?;
    let groups = groups
        .iter()
        .map(|(&package, entries)| {
            (
                package,
                sundial::package_authoring::loading_index::Group {
                    bitmap: vec![],
                    indices: entries.clone(),
                },
            )
        })
        .collect();
    crate::shared_tag_dependency_index::encode(&template, &groups)
}

#[cfg(test)]
mod legacy_tests;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tiger_pkg::{DestinyVersion, GameVersion};

    use super::*;

    #[test]
    fn sparse_companion_writer_matches_the_legacy_native_bytes() {
        let template = vec![0xAB; 0x50];
        for lengths in [[1, 2, 7], [8, 9, 32], [13, 64, 257]] {
            let groups = [0x123, 0xE06, 0x1FFF]
                .into_iter()
                .zip(lengths)
                .map(|(package, count)| (package, (0..count).collect()))
                .collect();
            let expected = legacy_tests::encode_canonical_payload(
                &template,
                TagHash(0x80A00001),
                TagHash(0x80A00000),
                &groups,
            )
            .unwrap();
            assert_eq!(
                encode_canonical_payload(
                    &template,
                    TagHash(0x80A00001),
                    TagHash(0x80A00000),
                    &groups
                )
                .unwrap(),
                expected
            );
        }
    }

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
            let rebuilt = build_shared_tag_companion_payload(
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
