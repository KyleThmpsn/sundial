//! Complete unconnected owners composed before private allocation and selector rebinding.
use super::*;
use sundial::package_authoring::{
    entity::{
        extend_weapon_components, weapon_component_binding_hashes, weapon_component_bindings,
    },
    sandbox_perk::program::NativeAssetComponent,
};

/// Keep the complete provider partition of each source owner. Singleton collisions
/// must fail instead of silently losing a source callback or replacing a host role.
pub(super) fn append(
    manager: &PackageManager,
    target: &mut Vec<u8>,
    components: &[NativeAssetComponent],
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let mut sources = Vec::new();
    for component in components {
        let source = read_tag(
            manager,
            TagHash(component.source_graph),
            "added component graph",
        )?;
        let (count, _, rows, class) = crate::tag_payload::array_at(&source, 16)?;
        if count != 1
            || class != 0x80809C04
            || read_u64(&source, 32)? != 0
            || read_u64(&source, 48)? != 0
            || read_u64(&source, 0x78)? != 0
            || read_u32(&source, 0x88)? != u32::MAX
            || (read_u32(target, 0x88)? != u32::MAX && component.replication.is_none())
            || component.patches.is_empty()
        {
            return Err(invalid(
                "An added component needs one unconnected owner without condition masks and a replication witness when its host requires one.",
            ));
        }
        let owner = read_u32(&source, rows)?;
        let patches = checked_asset_patches(
            manager,
            &source,
            &component.patches,
            &BTreeSet::new(),
            "Added component",
        )?;
        let mut expected = bindings(target)?;
        for (key, values) in bindings(&source)? {
            if values
                .iter()
                .any(|(_, reference)| reference[..4] != owner.to_le_bytes())
            {
                return Err(invalid(
                    "An added component exports an owner outside its graph.",
                ));
            }
            expected.entry(key).or_default().extend(values);
        }
        for values in expected.values_mut() {
            values.sort_unstable();
        }
        sundial::package_authoring::sandbox_perk::entity::residency::inspect(
            manager,
            component.source_graph,
        )
        .map_err(invalid)?;
        extend_weapon_components(
            manager,
            target,
            &source,
            &BTreeSet::from([owner]),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .map_err(invalid)?;
        if bindings(target)? != expected {
            return Err(invalid(
                "Component composition would lose an existing or added interface resource.",
            ));
        }
        sources.push((source, patches));
    }
    let mut result = Vec::new();
    for (source, mut patches) in sources {
        remap_patches(&source, target, &mut patches)?;
        result.extend(patches);
    }
    Ok(result)
}

/// Complete callback and source descriptor bytes. Only the component ordinal is
/// normalized, after checking that it selects the same actual owner.
type Bindings = BTreeMap<u32, Vec<([u8; 40], [u8; 24])>>;

fn bindings(graph: &[u8]) -> AuthoringResult<Bindings> {
    // The shared reader validates omitted tables and rejects active unresolved selectors.
    if weapon_component_binding_hashes(graph)
        .map_err(invalid)?
        .is_empty()
    {
        return Ok(BTreeMap::new());
    }
    let (count, _, rows, class) = crate::tag_payload::array_at(graph, 0x48)?;
    if class != 0x80809C25 {
        return Err(invalid(
            "Component interface table has an unexpected row class.",
        ));
    }
    let (owner_count, _, owners, owner_class) = crate::tag_payload::array_at(graph, 16)?;
    let (map_count, _, maps, map_class) = crate::tag_payload::array_at(graph, 0x58)?;
    let (reference_count, _, references, reference_class) =
        crate::tag_payload::array_at(graph, 0x68)?;
    if owner_class != 0x80809C04
        || map_class != 0x80809C22
        || reference_class != 0x80809C20
        || map_count != reference_count
    {
        return Err(invalid(
            "Component provider tables have inconsistent native layouts.",
        ));
    }
    let mut result = BTreeMap::new();
    for index in 0..count {
        let key = read_u32(graph, rows + index * 8)?;
        if key == u32::MAX {
            continue;
        }
        let mut values = Vec::new();
        for binding in weapon_component_bindings(graph, key).map_err(invalid)? {
            if binding.component_index >= owner_count
                || binding.descriptor_index >= reference_count
                || read_u32(graph, owners + binding.component_index * 12)? != binding.owner_tag
            {
                return Err(invalid(
                    "Component provider names an incorrect owner ordinal.",
                ));
            }
            let map =
                crate::tag_payload::read_array::<40>(graph, maps + binding.descriptor_index * 40)?;
            let mut reference = crate::tag_payload::read_array::<24>(
                graph,
                references + binding.descriptor_index * 24,
            )?;
            reference[16..20].fill(0);
            values.push((map, reference));
        }
        values.sort_unstable();
        if result.insert(key, values).is_some() {
            return Err(invalid("Component interface table repeats a binding."));
        }
    }
    Ok(result)
}

pub(super) fn remap_patches(
    source: &[u8],
    target: &[u8],
    patches: &mut [WeaponRuntimeResourcePatch],
) -> AuthoringResult<()> {
    for patch in patches {
        patch.resource_index =
            resource_index(source, target, patch.binding_hash, patch.resource_index)?;
    }
    Ok(())
}

/// A provider's owner, concrete view and offset stay fixed when graph spans move.
pub(super) fn resource_index(
    source: &[u8],
    target: &[u8],
    key: u32,
    index: u16,
) -> AuthoringResult<u16> {
    let bindings = weapon_component_bindings(source, key).map_err(invalid)?;
    let old = bindings
        .get(usize::from(index))
        .ok_or_else(|| invalid("A component edit selects a missing original resource."))?;
    let matches = weapon_component_bindings(target, key)
        .map_err(invalid)?
        .into_iter()
        .filter(|binding| {
            binding.owner_tag == old.owner_tag
                && binding.concrete_class == old.concrete_class
                && binding.resource_offset == old.resource_offset
        })
        .collect::<Vec<_>>();
    let [binding] = matches.as_slice() else {
        return Err(invalid(
            "A component edit has no unique resource after composition.",
        ));
    };
    u16::try_from(binding.resource_index)
        .map_err(|_| invalid("A composed resource index is too large."))
}
