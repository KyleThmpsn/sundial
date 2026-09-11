//! Portable scalar settings identified by their native member declarations.
use super::*;

/// Proves that two scalar fields address the same inherited native setting. A matching display
/// name is insufficient. Different component schemas must inherit the very same declarations.
/// Callers must also prove the binding/resource correspondence and validate the encoded value.
pub fn runtime_fields_share_semantics(
    source: &WeaponRuntimeField,
    target: &WeaponRuntimeField,
) -> Result<bool, String> {
    Ok(share_semantics(source, target, runtime_registry()?))
}

fn scalar(kind: &WeaponRuntimeValueKind) -> bool {
    matches!(
        kind,
        WeaponRuntimeValueKind::Boolean
            | WeaponRuntimeValueKind::Float32
            | WeaponRuntimeValueKind::SignedInteger { .. }
            | WeaponRuntimeValueKind::UnsignedInteger { .. }
    )
}

fn share_semantics(
    source: &WeaponRuntimeField,
    target: &WeaponRuntimeField,
    registry: &RuntimeRegistry,
) -> bool {
    let left = &source.locator;
    let right = &target.locator;
    if source.source != WeaponRuntimeFieldSource::NativeMember
        || target.source != WeaponRuntimeFieldSource::NativeMember
        || !scalar(&source.kind)
        || source.kind != target.kind
        || left.root != right.root
        || left.type_handle != right.type_handle
        || left.byte_size != right.byte_size
        || left.path.is_empty()
    {
        return false;
    }
    match (declarations(left, registry), declarations(right, registry)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn declarations(
    locator: &WeaponRuntimeFieldLocator,
    registry: &RuntimeRegistry,
) -> Option<Vec<(u32, u32, u32)>> {
    let mut schema = locator.root_schema;
    let mut result = Vec::new();
    for element in &locator.path {
        let mut current = schema;
        let mut visited = BTreeSet::new();
        let mut matches = Vec::new();
        while !matches!(current, 0 | u32::MAX) {
            if !visited.insert(current) {
                return None;
            }
            let record = registry.records.get(&current)?;
            for member in &record.members {
                if member.name_hash == element.name_hash
                    && member.type_handle == element.type_handle
                    && member.byte_offset == element.byte_offset
                {
                    matches.push(current);
                }
            }
            current = record.base_type;
        }
        let [declaring_type] = matches.as_slice() else {
            return None;
        };
        result.push((*declaring_type, element.name_hash, element.type_handle));
        schema = element.type_handle;
    }
    Some(result)
}

#[cfg(test)]
mod tests;
