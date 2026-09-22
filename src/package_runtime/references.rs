//! Native, declared package references. Never interpret arbitrary aligned words as tags.
//!
//! The client layout distinguishes raw pointers (2) from typed pointers (3). Only the
//! latter carry a concrete class header that permits checked traversal. Layouts include
//! inherited fields, so following the reflected base a second time is unnecessary.
use std::collections::{BTreeMap, BTreeSet};

use crate::package_runtime::reader::PackageManager;
use tiger_pkg::TagHash;

use crate::package_payload::{bytes_at, i64_at, relative_offset, u32_at, u64_at};

pub(crate) mod schema;
use schema::{Record, Registry};

const MAX_OBJECTS: usize = 200_000;
const MAX_RESOURCES: usize = 50_000;

fn is_reference(tag: u32) -> bool {
    tag != 0x811C_9DC5 && super::is_valid_package_tag(TagHash(tag))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Reference {
    pub tag: u32,
    pub parent: u32,
    pub offset: usize,
}

/// Includes typed child records, nested entity graphs and raw resource backing entries.
/// Cycles are expected for self views and shared effects. Missing package records or
/// unsupported declarations are errors, not evidence that the dependency is optional.
pub(crate) fn closure(
    manager: &PackageManager,
    roots: impl IntoIterator<Item = u32>,
) -> Result<Vec<Reference>, String> {
    collect(roots, &mut Registry::new()?, |tag| {
        let entry = manager
            .get_entry(TagHash(tag))
            .ok_or_else(|| format!("Referenced package resource 0x{tag:08X} is missing"))?;
        let payload = if matches!(entry.file_type, 8 | 16) {
            let data = manager.read_tag(TagHash(tag)).map_err(|error| {
                format!("Could not read referenced resource 0x{tag:08X}: {error}")
            })?;
            if data.len() != entry.file_size as usize {
                return Err(format!(
                    "Referenced resource 0x{tag:08X} has an invalid size"
                ));
            }
            data
        } else {
            Vec::new()
        };
        Ok(Resource {
            kind: entry.file_type,
            class: entry.reference,
            payload,
        })
    })
}

struct Resource {
    kind: u8,
    class: u32,
    payload: Vec<u8>,
}

fn collect(
    roots: impl IntoIterator<Item = u32>,
    registry: &mut Registry,
    mut read: impl FnMut(u32) -> Result<Resource, String>,
) -> Result<Vec<Reference>, String> {
    let mut pending = roots.into_iter().collect::<BTreeSet<_>>();
    let mut visited = BTreeSet::new();
    let mut references = BTreeMap::new();
    while let Some(tag) = pending.pop_first() {
        if !visited.insert(tag) {
            continue;
        }
        if visited.len() > MAX_RESOURCES {
            return Err("Native reference traversal exceeded its resource limit".into());
        }
        let resource = read(tag)?;
        let children = match resource.kind {
            // GPU buffer/texture, shader and state headers refer to raw backing entries
            // through the package directory, not through reflected object fields. In
            // particular, enrolling a shader header does not enroll its DXBC bytecode.
            32..=34 => BTreeMap::from([(resource.class, usize::MAX)]),
            // These describe layouts, rather than runtime objects. Generated layouts are read
            // and validated separately when a typed object names their concrete schema.
            8 | 16 if matches!(resource.class, 0x8080_0000 | 0x8080_9BBB) => BTreeMap::new(),
            8 | 16 => walk(&resource.payload, resource.class, |handle| {
                registry.record(handle, |schema_tag| {
                    let schema = read(schema_tag)?;
                    if schema.kind != 8 || schema.class != 0x8080_0000 {
                        return Err(format!(
                            "Generated schema 0x{schema_tag:08X} has an invalid class"
                        ));
                    }
                    Ok(schema.payload)
                })
            })
            .map_err(|error| format!("Resource 0x{tag:08X}: {error}"))?,
            // Raw audio, texture and buffer payloads have no native object layout.
            _ => BTreeMap::new(),
        };
        for (child, offset) in children {
            // This uses the client's canonical package-id range, not tiger-pkg's
            // narrower convenience predicate. Names and unshipped handles are not packages.
            if !is_reference(child) {
                continue;
            }
            references.entry(child).or_insert(Reference {
                tag: child,
                parent: tag,
                offset,
            });
            if !visited.contains(&child) {
                pending.insert(child);
            }
        }
    }
    Ok(references.into_values().collect())
}

pub(crate) fn walk(
    data: &[u8],
    root: u32,
    mut record: impl FnMut(u32) -> Result<Record, String>,
) -> Result<BTreeMap<u32, usize>, String> {
    let mut pending = vec![(0usize, root)];
    let mut visited = BTreeSet::new();
    let mut references = BTreeMap::new();
    while let Some((offset, class)) = pending.pop() {
        if !visited.insert((offset, class)) {
            continue;
        }
        if visited.len() > MAX_OBJECTS {
            return Err("Native reference traversal exceeded its object limit".into());
        }
        let schema = record(class)?;
        if offset
            .checked_add(schema.size)
            .is_none_or(|end| end > data.len())
        {
            return Err(format!(
                "Native object 0x{class:08X} at 0x{offset:X} exceeds its resource"
            ));
        }
        if is_reference(class) {
            references.entry(class).or_insert(offset);
        }
        for (field, kind) in schema.fields.iter().copied() {
            let field = offset
                .checked_add(field)
                .ok_or("Native field offset overflow")?;
            match kind {
                4 | 9 => {
                    let tag = u32_at(data, field)?;
                    if is_reference(tag) {
                        references.entry(tag).or_insert(field);
                    }
                }
                3 => follow(data, field, &mut pending, &mut record)?,
                _ => return Err(format!("Unsupported native reference operation {kind}")),
            }
        }
    }
    Ok(references)
}

fn follow(
    data: &[u8],
    field: usize,
    pending: &mut Vec<(usize, u32)>,
    record: &mut impl FnMut(u32) -> Result<Record, String>,
) -> Result<(), String> {
    let relative = i64_at(data, field)?;
    if relative == 0 {
        return Ok(());
    }
    let target = relative_offset(field, 0, relative)?;
    let marker = target
        .checked_sub(4)
        .ok_or("Native pointer has no class header")?;
    let class = u32_at(data, marker)?;
    if class != 0x8080_9FBD {
        pending.push((target, class));
        return Ok(());
    }
    bytes_at::<16>(data, target)?;
    let count = usize::try_from(u64_at(data, target)?).map_err(|_| "Native array is too large")?;
    let class = u32_at(data, target + 8)?;
    let layout = record(class)?;
    let stride = layout.size;
    let rows = target
        .checked_add(16)
        .ok_or("Native array offset overflow")?;
    if (stride == 0 && count != 0)
        || count
            .checked_mul(stride)
            .and_then(|size| rows.checked_add(size))
            .is_none_or(|end| end > data.len())
    {
        return Err(format!(
            "Native reference array at +0x{target:X} has invalid bounds or stride: class 0x{class:08X}, count {count}, stride {stride}, resource size {}",
            data.len()
        ));
    }
    // Large scalar/vertex arrays contain no references. Validate their complete
    // extent without treating every element as another object to traverse.
    if layout.fields.is_empty() {
        if count != 0 && is_reference(class) {
            pending.push((rows, class));
        }
        return Ok(());
    }
    if count > MAX_OBJECTS || pending.len().saturating_add(count) > MAX_OBJECTS {
        return Err("Native reference array exceeded its object limit".into());
    }
    pending.extend((0..count).map(|i| (rows + i * stride, class)));
    Ok(())
}

#[cfg(test)]
mod tests;
