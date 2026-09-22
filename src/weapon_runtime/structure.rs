//! Inspection of native declarations that have no reflected editor member.
//! Typed storage contracts also back the editor. Storage knowledge does not itself prove
//! whether a value controls damage, lifetime, attachment or another gameplay property.
use super::*;
use crate::package_runtime::references::schema::{Record, Registry};

mod codecs;
mod container;
mod labels;
pub(super) mod numeric;
mod pointer;
mod value;
use codecs::{Codecs, registry as codecs};
#[cfg(test)]
mod tests;

const MAX_OBJECTS: usize = 8_192;
const MAX_FIELDS: usize = 65_536;

#[cfg(test)]
pub(super) fn test_walk(
    data: &[u8],
    start: usize,
    schema: u32,
    record: impl FnMut(u32) -> Result<Record, String>,
) -> NativeStructure {
    walk(
        data,
        start,
        schema,
        runtime_registry().unwrap(),
        codecs().unwrap(),
        record,
    )
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NativeStructure {
    pub fields: Vec<NativeStructureField>,
    /// Pointer, reference and allocation metadata, including aliases through inline types.
    pub managed_ranges: Vec<(usize, usize)>,
    /// A partial inspection always reports its reason rather than silently omitting records.
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeStructureField {
    pub owner_offset: usize,
    pub schema: u32,
    /// Offset within the declaring native object, independent of its package placement.
    pub schema_offset: usize,
    /// Checked traversal from the selected root, independent of package placement.
    pub path: Vec<WeaponRuntimePathElement>,
    pub storage: Option<(WeaponRuntimeValueKind, u8)>,
    pub label: String,
    pub representation: String,
    pub value: String,
}

pub(super) fn inspect(
    manager: &PackageManager,
    data: &[u8],
    start: usize,
    schema: u32,
    names: &RuntimeRegistry,
) -> NativeStructure {
    let mut result = NativeStructure::default();
    let run = || -> Result<NativeStructure, String> {
        let mut registry = Registry::new()?;
        Ok(walk(data, start, schema, names, codecs()?, |handle| {
            registry.record(handle, |tag| {
                let tag = TagHash(tag);
                let entry = manager
                    .get_entry(tag)
                    .ok_or("Missing generated structure schema")?;
                if entry.file_type != 8 || entry.reference != GENERATED_SCHEMA_CLASS {
                    return Err("Invalid generated structure schema class".into());
                }
                manager.read_tag(tag).map_err(|error| error.to_string())
            })
        }))
    };
    match run() {
        Ok(decoded) => result = decoded,
        Err(error) => result.issues.push(error),
    }
    result
}

fn scalar(code: u8, schema: u32) -> Option<WeaponRuntimeValueKind> {
    // These identifier primitives have an independent native type mapping.
    // In particular, code zero alone does not imply an eight-byte value.
    if (code, schema) == (0, 0x8080_0012) {
        return Some(WeaponRuntimeValueKind::HexIdentifier { bits: 64 });
    }
    // Codes verified against the native primitive codec declarations in the same image.
    Some(match code {
        2 => WeaponRuntimeValueKind::Boolean,
        3..=6 => WeaponRuntimeValueKind::SignedInteger {
            bits: 8 << (code - 3),
        },
        7..=10 => WeaponRuntimeValueKind::UnsignedInteger {
            bits: 8 << (code - 7),
        },
        11 => WeaponRuntimeValueKind::Float32,
        13..=16 | 42 => WeaponRuntimeValueKind::Vector4Float32,
        _ => return None,
    })
}

fn scalar_text(value: &WeaponRuntimeValue) -> String {
    match value {
        WeaponRuntimeValue::Boolean(value) => value.to_string(),
        WeaponRuntimeValue::Signed(value) => value.to_string(),
        WeaponRuntimeValue::Unsigned(value) => value.to_string(),
        WeaponRuntimeValue::Float32Bits(bits) => {
            format!("{} (0x{bits:08X})", f32::from_bits(*bits))
        }
        WeaponRuntimeValue::Float64Bits(bits) => {
            format!("{} (0x{bits:016X})", f64::from_bits(*bits))
        }
        WeaponRuntimeValue::Vector4Float32Bits(bits) => bits
            .iter()
            .map(|bits| format!("{} (0x{bits:08X})", f32::from_bits(*bits)))
            .collect::<Vec<_>>()
            .join(", "),
        _ => format!("{value:?}"),
    }
}

fn label(schema: u32, offset: usize, names: &RuntimeRegistry) -> String {
    if let Some(label) = labels::behavior(schema, offset) {
        return label.into();
    }
    let mut current = schema;
    let mut visited = BTreeSet::new();
    while let Some(record) = names.records.get(&current) {
        if !visited.insert(current) {
            break;
        }
        if let Some(member) = record.members.iter().find(|member| {
            member.byte_offset as usize == offset && member.name_hash != EMPTY_NAME_HASH
        }) {
            let (mut label, inferred) = runtime_member_name(member.name_hash, names);
            if inferred {
                label.push_str(" (inferred)");
            }
            return label;
        }
        current = record.base_type;
    }
    format!("Unnamed Field +0x{offset:X}")
}

fn walk(
    data: &[u8],
    start: usize,
    schema: u32,
    names: &RuntimeRegistry,
    codecs: &Codecs,
    mut record: impl FnMut(u32) -> Result<Record, String>,
) -> NativeStructure {
    let mut result = NativeStructure::default();
    let mut pending = vec![(start, schema, None, native::root_path(schema))];
    let mut seen = BTreeSet::new();
    let mut fields = BTreeMap::new();
    let mut managed = Vec::new();
    while let Some((start, schema, active, path)) = pending.pop() {
        if !seen.insert((start, schema, active)) {
            continue;
        }
        if seen.len() > MAX_OBJECTS || fields.len() >= MAX_FIELDS {
            result
                .issues
                .push("Decoded structure reached its inspection limit.".into());
            break;
        }
        let mut truncated = false;
        let mut decode = || -> Result<(), String> {
            let declaration = record(schema)?;
            let end = object_end(data, start, declaration.size)?;
            let values = codecs.get(&schema);
            let locations = codecs::locations(values, declaration.size, active)?;
            if active == Some(0) {
                return Ok(());
            }
            let mut put =
                |offset: usize,
                 representation: String,
                 value: String,
                 storage: Option<(WeaponRuntimeValueKind, u8)>| {
                    if fields.len() >= MAX_FIELDS {
                        truncated = true;
                        return;
                    }
                    fields
                        .entry((offset, representation.clone()))
                        .or_insert_with(|| NativeStructureField {
                            owner_offset: offset,
                            schema,
                            schema_offset: offset - start,
                            path: native::field_path(&path, schema, offset - start),
                            label: label(schema, offset - start, names),
                            representation,
                            value,
                            storage,
                        });
                };
            managed.extend(declaration.fields.iter().filter_map(|&(relative, kind)| {
                start.checked_add(relative).map(|at| {
                    (
                        at,
                        match kind {
                            4 => 4,
                            9 => 16,
                            _ => 8,
                        },
                    )
                })
            }));
            for &(relative, kind) in declaration.fields.iter() {
                let at = start
                    .checked_add(relative)
                    .ok_or("Native field offset overflow")?;
                match kind {
                    4 => put(
                        at,
                        "Package Reference".into(),
                        format!("0x{:08X}", read_u32(data, at)?),
                        None,
                    ),
                    9 => put(
                        at,
                        "Resource Reference".into(),
                        format!(
                            "Asset 0x{:08X}, type 0x{:08X}, offset 0x{:X}",
                            read_u32(data, at)?,
                            read_u32(data, at + 4)?,
                            read_u64(data, at + 8)?
                        ),
                        None,
                    ),
                    3 => {
                        let pointer = pointer::read(
                            data,
                            at,
                            MAX_OBJECTS.saturating_sub(pending.len()),
                            &mut record,
                        )?;
                        let array = pointer.representation == "Typed Array";
                        put(at, pointer.representation.into(), pointer.value, None);
                        pending.extend(pointer.children.into_iter().enumerate().map(
                            |(index, (target, child, active))| {
                                (
                                    target,
                                    child,
                                    active,
                                    native::pointer_path(
                                        &path,
                                        relative,
                                        child,
                                        array.then_some(index),
                                    ),
                                )
                            },
                        ));
                    }
                    _ => return Err("Unsupported native structure reference declaration".into()),
                }
            }
            for &(relative, storage, _) in labels::native_fields(schema, declaration.size)? {
                let at = start + relative;
                let (kind, name) = match storage {
                    labels::Storage::Float32 => (WeaponRuntimeValueKind::Float32, "Float32"),
                    labels::Storage::Boolean => (WeaponRuntimeValueKind::Boolean, "Boolean"),
                    labels::Storage::Byte => (
                        WeaponRuntimeValueKind::UnsignedInteger { bits: 8 },
                        "Unsigned Byte",
                    ),
                    labels::Storage::Signed16 => (
                        WeaponRuntimeValueKind::SignedInteger { bits: 16 },
                        "Signed 16-Bit Integer",
                    ),
                    labels::Storage::Key => (
                        WeaponRuntimeValueKind::HexIdentifier { bits: 32 },
                        "Name Key",
                    ),
                };
                put(
                    at,
                    name.into(),
                    scalar_text(&decode_runtime_value(data, at, &kind)?),
                    Some((kind, 0)),
                );
            }
            let mut children = Vec::new();
            if let Some(values) = values {
                managed.extend(
                    values
                        .fields
                        .iter()
                        .filter(|field| field.code == 1 && field.params[0] == 1)
                        .map(|field| (start + field.params[1] as usize, 4)),
                );
                for (relative, field) in locations {
                    let (code, child) = (field.code, field.child);
                    let at = start
                        .checked_add(relative)
                        .ok_or("Native codec offset overflow")?;
                    let value_end = values.value_end(at, end, field)?;
                    let relocated = declaration
                        .fields
                        .iter()
                        .any(|&(offset, _)| offset == relative);
                    // The checked package relocation wins over the wire's runtime-reference operation.
                    if relocated && matches!(code, 24 | 25) {
                        continue;
                    }
                    let decoded = if code == 32 {
                        container::read(data, start, relative, value_end, &declaration)?
                    } else {
                        value::read(data, at, value_end, code, schema)?
                    };
                    if let Some((representation, text)) = decoded {
                        managed.extend(value::managed_width(code).map(|width| (at, width)));
                        put(at, representation, text, value::storage(code, schema));
                        // Runtime record prefixes stay managed. Only the proven suffix is editable.
                        for (offset, kind, name) in value::suffix_fields(code, at) {
                            put(
                                offset,
                                name.into(),
                                scalar_text(&decode_runtime_value(data, offset, &kind)?),
                                Some((kind, 0)),
                            );
                        }
                    } else if code == 1 {
                        let child_size = record(child)?.size;
                        if at
                            .checked_add(child_size)
                            .is_none_or(|last| last > value_end)
                        {
                            return Err(
                                "Inline native record exceeds its declaring structure".into()
                            );
                        }
                        put(
                            at,
                            "Inline Record".into(),
                            format!("Type 0x{child:08X}, {child_size} bytes"),
                            None,
                        );
                        let active = field.active_count(data, start, declaration.size)?;
                        children.push((at, child, active));
                    } else if !relocated {
                        // Retain the declaration even when its representation is not mapped.
                        put(
                            at,
                            format!("Unmapped Storage: {}", codecs::unresolved(code)),
                            field.description(),
                            None,
                        );
                    }
                }
            }
            for (at, child) in reflected_children(start, schema, end, names, &mut record)? {
                if !children
                    .iter()
                    .any(|&(offset, handle, _)| offset == at && handle == child)
                {
                    children.push((at, child, None));
                }
            }
            pending.extend(children.into_iter().map(|(at, child, active)| {
                (
                    at,
                    child,
                    active,
                    native::inline_path(&path, at - start, child),
                )
            }));
            if pending.len() > MAX_OBJECTS {
                return Err("Native structure exceeds its inspection limit".into());
            }
            Ok(())
        };
        if let Err(error) = decode() {
            result.issues.push(format!(
                "Type 0x{schema:08X} at owner +0x{start:X}: {error}"
            ));
        }
        if truncated {
            result
                .issues
                .push("Decoded structure reached its field limit.".into());
            break;
        }
    }
    result.fields = fields.into_values().collect();
    managed.sort_unstable();
    managed.dedup();
    result.managed_ranges = managed;
    result.issues.sort();
    result.issues.dedup();
    result
}

fn object_end(data: &[u8], start: usize, size: usize) -> Result<usize, String> {
    let end = start
        .checked_add(size)
        .ok_or("Native structure range overflow")?;
    if end > data.len() {
        return Err("Native structure extends beyond its owner".into());
    }
    Ok(end)
}

fn reflected_children(
    start: usize,
    schema: u32,
    end: usize,
    names: &RuntimeRegistry,
    record: &mut impl FnMut(u32) -> Result<Record, String>,
) -> Result<Vec<(usize, u32)>, String> {
    let mut children = Vec::new();
    let Some(reflected) = names.records.get(&schema) else {
        return Ok(children);
    };
    if !matches!(reflected.base_type, 0 | u32::MAX) {
        children.push((start, reflected.base_type));
    }
    for member in &reflected.members {
        let child = record(member.type_handle)?;
        let at = start
            .checked_add(member.byte_offset as usize)
            .ok_or("Reflected structure offset overflow")?;
        if at.checked_add(child.size).is_none_or(|last| last > end) {
            return Err("Reflected record exceeds its declaring structure".into());
        }
        children.push((at, member.type_handle));
    }
    Ok(children)
}
