//! Preserve native owner loading boundaries when cloning private resource groups.
use super::*;
use crate::{NewTagSpec, tag_payload::write_u32};

/// A native type-16 loading owner and its type-8 dependency index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadingOwner {
    pub owner: TagHash,
    pub companion: TagHash,
}

/// Clone a weapon-local loading index, retaining the native dependencies of all
/// supplied owners. Enroll the result under its target owner, keeping its private
/// resources out of the global runtime root.
pub fn clone_scoped_dependencies(
    source: (&[u8], LoadingOwner),
    target: LoadingOwner,
    resources: &[TagHash],
    inherited: &[(&[u8], LoadingOwner)],
) -> AuthoringResult<Vec<u8>> {
    if target.owner == target.companion
        || !resources.contains(&target.owner)
        || !resources.contains(&target.companion)
    {
        return Err(invalid(
            "The loading owner and companion must belong to their resource group",
        ));
    }
    let sources = inherited
        .iter()
        .map(|(data, id)| (*data, id.companion, id.owner))
        .collect::<Vec<_>>();
    let mut payload = enroll_inherited_dependencies(
        source.0,
        source.1.companion,
        source.1.owner,
        resources,
        &sources,
    )?;
    write_u32(&mut payload, 8, target.companion.0)?;
    write_u32(&mut payload, 12, target.owner.0)?;
    parse(&payload, target.companion, target.owner)?;
    Ok(payload)
}

fn check_boundary(
    payload: &[u8],
    identity: LoadingOwner,
    assets: &BTreeSet<(u16, u16)>,
    global: &BTreeSet<(u16, u16)>,
    boundaries: &BTreeSet<(u16, u16)>,
) -> AuthoringResult<()> {
    let local = indexed_dependencies(&parse(payload, identity.companion, identity.owner)?);
    for tag in [identity.owner, identity.companion] {
        if !local.contains(&(tag.pkg_id(), tag.entry_index())) {
            return Err(validation(
                "An authored loading index omits its owner or companion",
            ));
        }
    }
    if let Some(&(package, entry)) = local
        .intersection(assets)
        .find(|entry| global.contains(entry))
    {
        return Err(validation(format!(
            "Owner {} has scoped asset {package:04X}:{entry:04X} enrolled in the global runtime root",
            identity.owner,
        )));
    }
    let own = [identity.owner, identity.companion].map(|tag| (tag.pkg_id(), tag.entry_index()));
    if let Some(&(package, entry)) = local
        .intersection(boundaries)
        .find(|tag| !own.contains(tag))
    {
        return Err(validation(format!(
            "Loading owner {} includes another private owner or companion {package:04X}:{entry:04X}",
            identity.owner,
        )));
    }
    Ok(())
}

/// Enforce the boundary in normal package emission, including extension hooks.
/// Native icon definitions are an intentional eager UI resource. Other type-16
/// asset owners retain their loading boundary, including model and dye owners.
pub(crate) fn validate_asset_loading<'a>(
    manager: &tiger_pkg::PackageManager,
    packages: impl IntoIterator<Item = (u16, &'a [NewTagSpec])>,
    runtime: &[u8],
    runtime_identity: LoadingOwner,
) -> AuthoringResult<()> {
    let packages = packages.into_iter().collect::<Vec<_>>();
    let assets = packages
        .iter()
        .flat_map(|(id, tags)| (0..tags.len()).map(move |i| (*id, i as u16)))
        .collect();
    let global = indexed_dependencies(&parse(
        runtime,
        runtime_identity.companion,
        runtime_identity.owner,
    )?);
    let mut deferred = Vec::new();
    let mut boundaries = BTreeSet::new();
    for &(package, tags) in &packages {
        for (index, tag) in tags.iter().enumerate() {
            let entry = manager
                .get_entry(tag.template_tag)
                .ok_or_else(|| invalid("Asset template is missing"))?;
            if entry.file_type == 8 && entry.reference == 0x8080_9EF9 {
                let identity = LoadingOwner {
                    owner: TagHash(read_u32(&tag.payload, 12)?),
                    companion: TagHash::new(package, index as u16),
                };
                let owner = packages
                    .iter()
                    .find(|(id, _)| *id == identity.owner.pkg_id())
                    .and_then(|(_, tags)| tags.get(identity.owner.entry_index() as usize))
                    .ok_or_else(|| {
                        invalid("Asset loading owner is outside the authored packages")
                    })?;
                let owner_entry = manager
                    .get_entry(owner.template_tag)
                    .ok_or_else(|| invalid("Asset loading owner template is missing"))?;
                if !deferred_owner(owner_entry.file_type, owner_entry.reference)? {
                    continue;
                }
                boundaries.extend(
                    [identity.owner, identity.companion]
                        .map(|tag| (tag.pkg_id(), tag.entry_index())),
                );
                deferred.push((tag.payload.as_slice(), identity));
            }
        }
    }
    for (payload, identity) in deferred {
        check_boundary(payload, identity, &assets, &global, &boundaries)?;
    }
    Ok(())
}

fn deferred_owner(file_type: u8, class: u32) -> AuthoringResult<bool> {
    if file_type != 16 {
        return Err(invalid("An asset loading index requires a type-16 owner"));
    }
    Ok(class != sundial::package_authoring::icon_schema::ICON_DEFINITION_CLASS)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{COMPANION, OWNER, fixture};
    use super::*;

    #[test]
    fn native_icon_owners_allow_eager_ui_loading() {
        let icon = sundial::package_authoring::icon_schema::ICON_DEFINITION_CLASS;
        assert!(!deferred_owner(16, icon).unwrap());
        assert!(deferred_owner(16, 0x8080_9AD8).unwrap());
        assert!(deferred_owner(8, icon).is_err());
    }

    #[test]
    fn scoped_clone_preserves_native_resources_and_rejects_global_preloading() {
        let original = fixture();
        let source = LoadingOwner {
            owner: OWNER,
            companion: COMPANION,
        };
        let target = LoadingOwner {
            owner: TagHash::new(0xAA0, 10),
            companion: TagHash::new(0xAA0, 11),
        };
        let resources = [target.owner, target.companion, TagHash::new(0xAA0, 12)];
        let extra =
            enroll_dependencies(&original, COMPANION, OWNER, &[TagHash::new(0x1BB, 100)]).unwrap();
        let result =
            clone_scoped_dependencies((&original, source), target, &resources, &[(&extra, source)])
                .unwrap();
        let expected = indexed_dependencies(&parse(&extra, COMPANION, OWNER).unwrap())
            .into_iter()
            .chain(resources.map(|t| (t.pkg_id(), t.entry_index())))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            indexed_dependencies(&parse(&result, target.companion, target.owner).unwrap()),
            expected
        );
        assert_eq!(original, fixture());
        assert!(parse(&result, COMPANION, OWNER).is_err());
        let assets = resources
            .map(|t| (t.pkg_id(), t.entry_index()))
            .into_iter()
            .collect();
        let stock = indexed_dependencies(&parse(&original, COMPANION, OWNER).unwrap());
        let boundaries = [target.owner, target.companion]
            .map(|t| (t.pkg_id(), t.entry_index()))
            .into_iter()
            .collect();
        check_boundary(&result, target, &assets, &stock, &boundaries).unwrap();
        let mut crossed = boundaries.clone();
        crossed.insert((0xAA0, 12));
        assert!(check_boundary(&result, target, &assets, &stock, &crossed).is_err());
        let mut eager = stock;
        eager.insert((0xAA0, 12));
        assert!(check_boundary(&result, target, &assets, &eager, &boundaries).is_err());
        assert!(
            clone_scoped_dependencies((&original, source), target, &resources[..1], &[]).is_err()
        );
        assert!(clone_scoped_dependencies((&original, target), target, &resources, &[]).is_err());
    }
}
