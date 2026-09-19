//! Editable native values addressed by checked traversal, never an unchecked absolute offset.
use super::*;

const ROOT: u32 = 0x504E_5200;
const INLINE: u32 = 0x504E_4900;
const POINTER: u32 = 0x504E_5000;
const ELEMENT: u32 = 0x504E_4100;
const VALUE: u32 = 0x504E_5600;

fn step(name_hash: u32, type_handle: u32, offset: usize) -> WeaponRuntimePathElement {
    WeaponRuntimePathElement {
        name_hash,
        type_handle,
        byte_offset: u32::try_from(offset).unwrap_or(u32::MAX),
    }
}

pub(super) fn root_path(schema: u32) -> Vec<WeaponRuntimePathElement> {
    vec![step(ROOT, schema, 0)]
}

pub(super) fn field_path(
    path: &[WeaponRuntimePathElement],
    schema: u32,
    offset: usize,
) -> Vec<WeaponRuntimePathElement> {
    let mut path = path.to_vec();
    path.push(step(VALUE, schema, offset));
    path
}

pub(super) fn inline_path(
    path: &[WeaponRuntimePathElement],
    offset: usize,
    schema: u32,
) -> Vec<WeaponRuntimePathElement> {
    let mut path = path.to_vec();
    path.push(step(INLINE, schema, offset));
    path
}

pub(super) fn pointer_path(
    path: &[WeaponRuntimePathElement],
    offset: usize,
    schema: u32,
    index: Option<usize>,
) -> Vec<WeaponRuntimePathElement> {
    let mut path = path.to_vec();
    path.push(step(
        POINTER,
        if index.is_some() { 0x8080_9FBD } else { schema },
        offset,
    ));
    if let Some(index) = index {
        path.push(step(ELEMENT, schema, index));
    }
    path
}

pub(super) fn is_native(locator: &WeaponRuntimeFieldLocator) -> bool {
    locator.path.first().is_some_and(|step| {
        step.name_hash == ROOT && step.byte_offset == 0 && step.type_handle == locator.root_schema
    })
}

fn encoding(field: &NativeStructureField) -> u32 {
    field
        .storage
        .as_ref()
        .map_or(0, |(_, code)| u32::from(*code))
}

fn value(
    data: &[u8],
    field: &NativeStructureField,
    kind: &WeaponRuntimeValueKind,
) -> Result<WeaponRuntimeValue, String> {
    let code = encoding(field);
    if code == 0 {
        return decode_runtime_value(data, field.owner_offset, kind);
    }
    let bits = structure::numeric::decode(code as u8, read_u32(data, field.owner_offset)?)
        .ok_or("Unknown native numeric encoding")?;
    Ok(if code == 45 {
        WeaponRuntimeValue::Float32Bits(bits)
    } else {
        WeaponRuntimeValue::Signed(i64::from(bits as i32))
    })
}

fn fields(
    data: &[u8],
    root: &WeaponRuntimeRoot,
    binding: u32,
    index: u16,
    structure: &NativeStructure,
) -> Result<Vec<WeaponRuntimeField>, String> {
    let mut fields = Vec::new();
    // A traversal that failed has no complete write contract.
    if !structure.issues.is_empty() {
        return Ok(fields);
    }
    for field in &structure.fields {
        let Some((kind, _)) = field.storage.clone() else {
            continue;
        };
        let end = field.owner_offset + kind.byte_size() as usize;
        if structure
            .managed_ranges
            .iter()
            .any(|&(start, size)| field.owner_offset < start + size && start < end)
        {
            continue;
        }
        if field.path.len() > MAX_RUNTIME_SCHEMA_DEPTH
            || field.path.iter().any(|step| step.byte_offset == u32::MAX)
        {
            continue;
        }
        let mut path = field.path.clone();
        path.last_mut()
            .ok_or("Native value has no traversal path")?
            .name_hash += encoding(field);
        let value = value(data, field, &kind)?;
        let label = format!(
            "{} / Type 0x{:08X} +0x{:X}",
            root.kind.label(),
            field.schema,
            field.schema_offset
        );
        fields.push(WeaponRuntimeField {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: None,
                binding_hash: binding,
                resource_index: index,
                root: root.kind,
                root_schema: root.schema,
                path,
                type_handle: field.schema,
                value_offset: field.schema_offset as u32,
                byte_size: kind.byte_size(),
            },
            owner_offset: u32::try_from(field.owner_offset)
                .map_err(|_| "Native value offset exceeds 32 bits")?,
            name: field.label.clone(),
            path_label: format!("{label} / {}", field.label),
            kind,
            value,
            source: WeaponRuntimeFieldSource::NativeDeclaration,
            generated_kind: None,
            name_inferred: field.label.ends_with("(inferred)"),
        });
    }
    Ok(fields)
}

pub(super) fn append_fields(
    resources: &mut [WeaponRuntimeResource],
    owners: &mut [WeaponRuntimeOwner],
    payloads: &BTreeMap<u32, Vec<u8>>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    // Prefer already editable reflected values, then component roots, then owner roots.
    for (tag, root) in resources
        .iter()
        .flat_map(|r| {
            std::iter::once(&r.instance)
                .chain(r.definition.iter())
                .map(move |root| (r.owner_tag, root))
        })
        .chain(
            owners
                .iter()
                .flat_map(|o| o.roots.iter().map(move |root| (o.owner_tag, root))),
        )
    {
        for field in &root.fields {
            if field.source != WeaponRuntimeFieldSource::OpaqueNativeType {
                seen.insert((tag, field.owner_offset, field.locator.byte_size));
            }
        }
    }
    for resource in resources {
        for root in std::iter::once(&mut resource.instance).chain(resource.definition.iter_mut()) {
            let extra = fields(
                &payloads[&resource.owner_tag],
                root,
                resource.binding_hash,
                resource.resource_index,
                &root.structure,
            )?;
            root.fields.extend(extra.into_iter().filter(|f| {
                seen.insert((resource.owner_tag, f.owner_offset, f.locator.byte_size))
            }));
        }
    }
    for owner in owners {
        for root in &mut owner.roots {
            let extra = fields(
                &payloads[&owner.owner_tag],
                root,
                owner.anchor_binding_hash,
                owner.anchor_resource_index,
                &root.structure,
            )?;
            root.fields.extend(
                extra.into_iter().filter(|f| {
                    seen.insert((owner.owner_tag, f.owner_offset, f.locator.byte_size))
                }),
            );
        }
    }
    Ok(())
}

pub(super) fn resolve(
    manager: &PackageManager,
    data: &[u8],
    root: &WeaponRuntimeRoot,
    locator: &WeaponRuntimeFieldLocator,
    registry: &RuntimeRegistry,
) -> Result<Option<WeaponRuntimeField>, String> {
    if !is_native(locator) {
        return Ok(root.fields.iter().find(|f| f.locator == *locator).cloned());
    }
    let structure = structure::inspect(
        manager,
        data,
        root.owner_offset as usize,
        root.schema,
        registry,
    );
    let fields = fields(
        data,
        root,
        locator.binding_hash,
        locator.resource_index,
        &structure,
    )?;
    Ok(fields.into_iter().find(|f| f.locator == *locator))
}

/// Encode a resolved field, preserving its proven native numeric storage transform.
pub fn encode_weapon_runtime_field_value(
    field: &WeaponRuntimeField,
    value: &WeaponRuntimeValue,
) -> Result<Vec<u8>, String> {
    let bytes = encode_weapon_runtime_value(&field.kind, value)?;
    if !is_native(&field.locator) {
        return Ok(bytes);
    }
    let code = field
        .locator
        .path
        .last()
        .map_or(0, |step| step.name_hash.wrapping_sub(VALUE));
    if code == 0 {
        return Ok(bytes);
    }
    let bits = u32::from_le_bytes(
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Encoded native value must have four bytes")?,
    );
    let code = u8::try_from(code).map_err(|_| "Unknown native numeric encoding")?;
    let stored = structure::numeric::encode(code, bits).ok_or("Unknown native numeric encoding")?;
    Ok(stored.to_le_bytes().to_vec())
}

/// Read a field-sized slice using the same storage contract as the package writer.
pub fn decode_weapon_runtime_field_value(
    field: &WeaponRuntimeField,
    bytes: &[u8],
) -> Result<WeaponRuntimeValue, String> {
    if bytes.len() != field.kind.byte_size() as usize {
        return Err("Native field bytes have the wrong size".into());
    }
    let code = if is_native(&field.locator) {
        field
            .locator
            .path
            .last()
            .map_or(0, |step| step.name_hash.wrapping_sub(VALUE))
    } else {
        0
    };
    if code == 0 {
        return decode_runtime_value(bytes, 0, &field.kind);
    }
    let code = u8::try_from(code).map_err(|_| "Unknown native numeric encoding")?;
    let bits = structure::numeric::decode(code, read_u32(bytes, 0)?)
        .ok_or("Unknown native numeric encoding")?;
    decode_runtime_value(&bits.to_le_bytes(), 0, &field.kind)
}

#[cfg(test)]
mod tests;
