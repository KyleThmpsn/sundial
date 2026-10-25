//! Validate related native configuration after all draft edits have been composed.
use super::*;
use crate::runtime::{
    encode_weapon_runtime_field_value, load_weapon_runtime_graph_for_entity,
    resolve_weapon_runtime_field,
};

/// Refuses native edits that change a field's validated storage contract or reverse a timer
/// range. Checks the complete draft, so authors can move both endpoints in either order.
pub fn validate_values(
    manager: &PackageManager,
    entity: &[u8],
    values: &[WeaponRuntimeValueOverride],
) -> Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }
    let mut payloads = BTreeMap::<u32, Vec<u8>>::new();
    let mut writes = BTreeMap::<u32, Vec<std::ops::Range<usize>>>::new();
    let mut encoded = Vec::new();
    for value in values {
        let resolved = resolve_weapon_runtime_field(manager, entity, &value.locator)?;
        let bytes = encode_weapon_runtime_field_value(&resolved.field, &value.value)?;
        let end = resolved
            .owner_offset
            .checked_add(bytes.len())
            .ok_or("Ability value range overflows")?;
        encoded.push((resolved.owner_tag, resolved.owner_offset, end, bytes));
    }
    // Match the compiler's nesting rule: specific fields overwrite their wider containers.
    encoded.sort_by_key(|(owner, start, end, _)| (*owner, *start, std::cmp::Reverse(*end)));
    for (owner, start, end, bytes) in encoded {
        let payload = match payloads.entry(owner) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(manager.read_tag(owner).map_err(|error| error.to_string())?)
            }
        };
        let target = payload
            .get_mut(start..end)
            .ok_or("Ability value is outside its owner")?;
        target.copy_from_slice(&bytes);
        writes.entry(owner).or_default().push(start..end);
    }
    let graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, 0, entity)?;
    let mut seen = BTreeSet::new();
    for (owner, binding, index, root) in graph
        .resources
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(move |root| {
                    (
                        resource.owner_tag,
                        resource.binding_hash,
                        resource.resource_index,
                        root,
                    )
                })
        })
        .chain(graph.owners.iter().flat_map(|owner| {
            owner.roots.iter().map(move |root| {
                (
                    owner.owner_tag,
                    owner.anchor_binding_hash,
                    owner.anchor_resource_index,
                    root,
                )
            })
        }))
    {
        let Some(payload) = payloads.get(&owner) else {
            continue;
        };
        if !seen.insert((owner, root.schema, root.owner_offset)) {
            continue;
        }
        let original = manager.read_tag(owner).map_err(|error| error.to_string())?;
        let old = native::fields(manager, &original, root, binding, index)?;
        if old.is_empty() {
            continue;
        }
        let after = native::fields(manager, payload, root, binding, index)?;
        for field in old {
            let start = field.owner_offset as usize;
            let end = start + field.locator.byte_size as usize;
            if !writes[&owner]
                .iter()
                .any(|range| range.start < end && start < range.end)
            {
                continue;
            }
            let current = after.iter().find(|current| current.locator == field.locator)
                .ok_or_else(|| format!("{} no longer has valid native settings. Check its minimum and maximum values.", field.name))?;
            if current.value != field.value {
                let setting = native::setting(owner, root, current, Some(payload))
                    .ok_or("Native setting cannot be validated")?;
                let number = match &current.value {
                    WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits),
                    WeaponRuntimeValue::Signed(value) => *value as f32,
                    WeaponRuntimeValue::Unsigned(value) => *value as f32,
                    _ => return Err("Native setting has an incompatible value".into()),
                };
                if let Kind::Native(property) = setting.kind {
                    property.validate(number * property.scale())?;
                }
            }
        }
    }
    Ok(())
}
