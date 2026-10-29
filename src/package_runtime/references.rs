//! Native, declared package references. Never interpret arbitrary aligned words as tags.
//!
//! The client layout distinguishes raw pointers (2) from typed pointers (3). Only the
//! latter carry a concrete class header that permits checked traversal. Layouts include
//! inherited fields, so following the reflected base a second time is unnecessary.
//! Component owners also identify their runtime definition through a checked reciprocal
//! prefix. It can precede the typed schema view when mutable instance storage is extended.
use std::collections::{BTreeMap, BTreeSet};

use crate::package_runtime::reader::PackageManager;
use tiger_pkg::TagHash;

use crate::package_payload::{bytes_at, i64_at, relative_offset, u32_at, u64_at};

pub(crate) mod referrers;
pub(crate) mod schema;
use schema::{Record, Registry};

const MAX_OBJECTS: usize = 200_000;
/// Resources one root's closure may reach. A walk from many roots, such as every graph a whole
/// project authors, may reach that many for each, since their closures are each bounded.
const MAX_RESOURCES: usize = 50_000;

/// The most resources a walk from `roots` roots may visit.
fn resource_limit(roots: usize) -> usize {
    MAX_RESOURCES.saturating_mul(roots.max(1))
}

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
    let roots = roots.into_iter().collect::<BTreeSet<_>>();
    let mut registry = Registry::new()?;
    // Answers the walk would otherwise read one resource at a time. The walk below still
    // decides the order, the limit and which error is reported.
    let mut known = cache::prefetch(manager, &roots);
    traverse(roots, |tag| match known.remove(&tag) {
        Some(answer) => answer,
        None => {
            let resource = read_resource(manager, tag)?;
            children(tag, &resource, &mut registry, &mut |schema| {
                read_resource(manager, schema)
            })
        }
    })
}

mod cache;

struct Resource {
    kind: u8,
    class: u32,
    payload: Vec<u8>,
}

fn read_resource(manager: &PackageManager, tag: u32) -> Result<Resource, String> {
    let entry = manager
        .get_entry(TagHash(tag))
        .ok_or_else(|| format!("Referenced package resource 0x{tag:08X} is missing"))?;
    let payload = if matches!(entry.file_type, 8 | 16) {
        let data = manager
            .read_tag(TagHash(tag))
            .map_err(|error| format!("Could not read referenced resource 0x{tag:08X}: {error}"))?;
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
}

/// The resources one resource references, with the offsets they are declared at, before
/// the package-range filter the walk applies.
fn children(
    tag: u32,
    resource: &Resource,
    registry: &mut Registry,
    read: &mut impl FnMut(u32) -> Result<Resource, String>,
) -> Result<BTreeMap<u32, usize>, String> {
    Ok(match resource.kind {
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
    })
}

#[cfg(test)]
fn collect(
    roots: impl IntoIterator<Item = u32>,
    registry: &mut Registry,
    mut read: impl FnMut(u32) -> Result<Resource, String>,
) -> Result<Vec<Reference>, String> {
    traverse(roots, |tag| {
        let resource = read(tag)?;
        children(tag, &resource, registry, &mut read)
    })
}

fn traverse(
    roots: impl IntoIterator<Item = u32>,
    mut children: impl FnMut(u32) -> Result<BTreeMap<u32, usize>, String>,
) -> Result<Vec<Reference>, String> {
    let mut pending = roots.into_iter().collect::<BTreeSet<_>>();
    let limit = resource_limit(pending.len());
    let mut visited = BTreeSet::new();
    let mut references = BTreeMap::new();
    while let Some(tag) = pending.pop_first() {
        if !visited.insert(tag) {
            continue;
        }
        if visited.len() > limit {
            return Err("Native reference traversal exceeded its resource limit".into());
        }
        for (child, offset) in children(tag)? {
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

/// Every reference field `tag`'s payload declares, in payload order: the field's offset and the
/// tag it names, each field once. Object class headers are not fields, and a resource without an
/// object layout has none.
pub(crate) fn declared_fields(
    manager: &PackageManager,
    tag: u32,
) -> Result<Vec<(usize, u32)>, String> {
    let resource = read_resource(manager, tag)?;
    payload_fields(manager, tag, &resource.payload)
}

/// Declared fields in an authored payload using its stock template's native layout. Raw
/// resource backing bytes have no fields. No source payload is read or substituted here.
pub(crate) fn payload_fields(
    manager: &PackageManager,
    template: u32,
    payload: &[u8],
) -> Result<Vec<(usize, u32)>, String> {
    payload_records(manager, template, payload, |kind| kind != 0)
}

/// Typed object views, including the original reciprocal definition of a sealed owner.
/// Raw pointers and object references do not authorize traversal into arbitrary bytes.
pub(crate) fn payload_objects(
    manager: &PackageManager,
    template: u32,
    payload: &[u8],
) -> Result<Vec<(usize, u32)>, String> {
    payload_records(manager, template, payload, |kind| kind == 0)
}

/// Object-reference fields retain a resource, a concrete class and an owner-relative offset.
/// Keep them distinct from four-byte resource references when relocating component storage.
pub(crate) fn payload_object_references(
    manager: &PackageManager,
    template: u32,
    payload: &[u8],
) -> Result<Vec<(usize, u32)>, String> {
    payload_records(manager, template, payload, |kind| kind == 9)
}

fn payload_records(
    manager: &PackageManager,
    template: u32,
    payload: &[u8],
    include: impl Fn(u32) -> bool,
) -> Result<Vec<(usize, u32)>, String> {
    let entry = manager
        .get_entry(TagHash(template))
        .ok_or_else(|| format!("Reference template 0x{template:08X} is missing"))?;
    if !matches!(entry.file_type, 8 | 16) || matches!(entry.reference, 0x8080_0000 | 0x8080_9BBB) {
        return Ok(Vec::new());
    }
    let mut registry = Registry::new()?;
    let mut fields = Vec::new();
    visit(
        payload,
        entry.reference,
        |handle| {
            registry.record(handle, |schema_tag| {
                let schema = read_resource(manager, schema_tag)?;
                if schema.kind != 8 || schema.class != 0x8080_0000 {
                    return Err(format!(
                        "Generated schema 0x{schema_tag:08X} has an invalid class"
                    ));
                }
                Ok(schema.payload)
            })
        },
        |reference, offset, kind| {
            if include(kind) {
                fields.push((offset, reference));
            }
        },
    )
    .map_err(|error| format!("Reference template 0x{template:08X}: {error}"))?;
    fields.sort_unstable();
    fields.dedup();
    Ok(fields)
}

pub(crate) fn walk(
    data: &[u8],
    root: u32,
    record: impl FnMut(u32) -> Result<Record, String>,
) -> Result<BTreeMap<u32, usize>, String> {
    let mut references = BTreeMap::new();
    visit(data, root, record, |tag, offset, _| {
        if is_reference(tag) {
            references.entry(tag).or_insert(offset);
        }
    })?;
    Ok(references)
}

/// Visits each object of the tree rooted at `root`, calling `found` with each reference: an
/// object's class, at the object's offset, and each tag a reference field holds, at the field's
/// offset, the last argument holding layout kind 4 or 9 for a field and zero for a class.
fn visit(
    data: &[u8],
    root: u32,
    mut record: impl FnMut(u32) -> Result<Record, String>,
    mut found: impl FnMut(u32, usize, u32),
) -> Result<(), String> {
    let mut pending = vec![(0usize, root)];
    if root == 0x8080_9C36
        && let Some(definition) = component_definition(data)?
    {
        pending.push(definition);
    }
    let mut visited = BTreeSet::new();
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
        // Typed views include native declarations as well as generated package schemas.
        // Only resource traversal filters the class into the package-reference domain.
        found(class, offset, 0);
        for (field, kind) in schema.fields.iter().copied() {
            let field = offset
                .checked_add(field)
                .ok_or("Native field offset overflow")?;
            match kind {
                4 | 9 => {
                    let tag = u32_at(data, field)?;
                    if is_reference(tag) {
                        found(tag, field, kind);
                    }
                }
                3 => follow(data, field, &mut pending, &mut record)?,
                _ => return Err(format!("Unsupported native reference operation {kind}")),
            }
        }
    }
    Ok(())
}

/// A sealed owner retains the original definition because its instance uses an
/// owner-relative address. Header +24 instead names a second typed view beyond
/// mutable storage. Both views contain live reference fields. This is a checked
/// component-prefix contract, not permission to traverse arbitrary raw pointers.
fn component_definition(data: &[u8]) -> Result<Option<(usize, u32)>, String> {
    let instance_relative = i64_at(data, 16)?;
    let schema_relative = i64_at(data, 24)?;
    if instance_relative == 0 || schema_relative == 0 {
        return Ok(None);
    }
    let instance = relative_offset(16, 0, instance_relative)?;
    let schema = relative_offset(24, 0, schema_relative)?;
    bytes_at::<16>(data, instance)?;
    let definition = usize::try_from(u64_at(data, instance + 8)?)
        .map_err(|_| "Component definition offset is too large")?;
    if definition == schema {
        return Ok(None);
    }
    if instance < 0x60 || definition < 0x60 || definition >= schema {
        return Err("Component runtime definition is outside its original record stream".into());
    }
    bytes_at::<16>(data, definition)?;
    let class = u32_at(data, definition - 4)?;
    if class != u32_at(data, instance + 4)?
        || class != u32_at(data, schema - 4)?
        || u32_at(data, definition + 4)? != u32_at(data, instance - 4)?
        || u64_at(data, definition + 8)? != instance as u64
        || u32_at(data, definition)? != u32_at(data, instance)?
    {
        return Err("Component runtime definition has an invalid reciprocal prefix".into());
    }
    Ok(Some((definition, class)))
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
