//! Remove donor dependencies only when rewritten registry objects no longer reach them.
use std::collections::BTreeSet;

use sundial::package_authoring::{
    PackageManager, declared_payload_objects, declared_payload_references, is_valid_package_tag,
    sandbox_perk::entity::residency::{owner_requirements, resource_additions},
};
use tiger_pkg::TagHash;

use crate::{
    AuthoringResult, NewTagSpec,
    error::invalid,
    shared_tag_dependency_index::{self, native::native_companion},
};

pub(super) fn dependencies(
    manager: &PackageManager,
    roots: impl IntoIterator<Item = u32>,
) -> AuthoringResult<BTreeSet<u32>> {
    resource_additions(manager, roots)
        .map_err(|error| invalid(format!("Map registry dependencies: {error}")))
}

pub(super) fn prune(
    manager: &PackageManager,
    first: TagHash,
    tags: &mut [NewTagSpec],
    original: &BTreeSet<u32>,
    explicit: &[TagHash],
) -> AuthoringResult<()> {
    let end = usize::from(first.entry_index())
        .checked_add(tags.len())
        .filter(|&end| end <= crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT)
        .ok_or_else(|| invalid("Map registry entries exceed their package"))?;
    let own = (usize::from(first.entry_index())..end)
        .map(|index| TagHash::new(first.pkg_id(), index as u16).0)
        .collect::<BTreeSet<_>>();
    let mut roots = explicit.iter().map(|tag| tag.0).collect::<BTreeSet<_>>();
    for (index, spec) in tags.iter().enumerate() {
        if index == 1 {
            continue;
        }
        let entry = manager
            .get_entry(spec.template_tag)
            .ok_or_else(|| invalid("Map registry template is missing"))?;
        roots.extend(
            declared_payload_references(manager, spec.template_tag.0, &spec.payload)
                .map_err(invalid)?
                .into_iter()
                .map(|(_, tag)| tag),
        );
        // A concrete generated class is a package resource even when its record
        // has no reference fields. Component layout prefixes are required too.
        roots.extend(
            declared_payload_objects(manager, spec.template_tag.0, &spec.payload)
                .map_err(invalid)?
                .into_iter()
                .map(|(_, class)| class),
        );
        if entry.file_type == 8 && entry.reference == 0x8080_9C36 {
            roots.extend(
                owner_requirements(manager, spec.template_tag.0, &spec.payload)
                    .map_err(invalid)?
                    .into_iter()
                    .map(|requirement| requirement.tag),
            );
        }
    }
    roots.retain(|tag| {
        *tag != 0x811C_9DC5 && is_valid_package_tag(TagHash(*tag)) && !own.contains(tag)
    });
    let mut retained = dependencies(manager, roots)?;
    let owners = retained
        .iter()
        .filter(|&&tag| {
            manager
                .get_entry(TagHash(tag))
                .is_some_and(|e| e.file_type == 16)
        })
        .copied()
        .collect::<Vec<_>>();
    let mut companions = Default::default();
    for owner in owners {
        let (companion, payload) =
            native_companion(&manager.package_dir, manager, owner, &mut companions)?;
        retained.extend(shared_tag_dependency_index::dependencies(
            payload,
            *companion,
            TagHash(owner),
        )?);
    }
    // Preserve native entries outside the original declared graph. Their loading
    // reason has not been disproved by replacing a registry object. Investment
    // resources are excluded by resource_additions and remain unchanged as well.
    let removed = original
        .difference(&retained)
        .copied()
        .map(TagHash)
        .collect::<Vec<_>>();
    let companion = TagHash::new(first.pkg_id(), first.entry_index() + 1);
    let index = tags
        .get_mut(1)
        .ok_or_else(|| invalid("Map registry companion is missing"))?;
    index.payload = shared_tag_dependency_index::remove_dependencies(
        &index.payload,
        companion,
        first,
        &removed,
    )?;
    Ok(())
}
