//! Checked edits to Shadowkeep CUI widget tables and their instantiated hierarchies.
//!
//! New components live in the root table. Imported component ranges and existing identities
//! stay fixed. Their hierarchy entries join the same replicated group as their consumers.
//!
//! Each property value lies in one of the table's typed value pools, as every one of the 147,360
//! values in the 1,204 stock tables does, and a new component's properties go in the default
//! object only. Decisions given loose values in every object crashed the client as the HUD
//! loaded on entering the world (2026-10-08): it was reading the variant object, which no stock
//! decision appears in, when it jumped through an invalid address.
use std::collections::BTreeSet;

use crate::package_payload::{
    i64_at, native_array_at, relative_offset, rows_fit, u16_at, u32_at, u64_at, write_bytes,
};

const WIDGET_CLASS: u32 = 0x80804825;
const HIERARCHY_CLASS: u32 = 0x8080496A;
const DECISION: u64 = 0xFBB44B29;
/// The value type of a decision choosing between two RGBA colors, as the stock HUD tile's do.
const RGBA_DECISION: u32 = 7;
/// The header's descriptor of the pool of four-byte enumeration values. Every stock decision's
/// value type lies in it.
const ENUM_POOL: usize = 0x98;
const ENUM_POOL_CLASS: u32 = 0x80804696;
/// The identity of a table's default property object. Every stock table with component
/// properties has exactly one. Its other objects are variants holding sparse overrides, and no
/// stock decision appears in one.
const DEFAULT_OBJECT: [u8; 24] = [
    0xAF, 0x89, 0x9B, 0x7F, 0x3E, 0xFA, 0x1C, 0x70, 0x24, 0x9D, 0xFB, 0x40, 0xE3, 0xBA, 0xBA, 0xEC,
    0xB9, 0xC0, 0x52, 0xAD, 0, 0, 0, 0,
];

pub const fn widget_class() -> u32 {
    WIDGET_CLASS
}
pub const fn hierarchy_class() -> u32 {
    HIERARCHY_CLASS
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub component: u16,
    /// Each token holds a property in its low 16 bits and its array index in the high 16.
    pub path: Vec<u32>,
    pub property: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    pub source: Endpoint,
    pub target: Endpoint,
}

/// A native RGBA decision. Its index is the old hierarchy count plus its ordinal.
pub struct ColorSwitch {
    pub name: u32,
    /// A consumer with no children, after which the new sibling joins the hierarchy.
    pub sibling: u16,
    pub condition: Endpoint,
    pub when_true: Endpoint,
    pub when_false: Endpoint,
    pub outputs: Vec<Endpoint>,
}

fn array(data: &[u8], descriptor: usize, stride: usize, class: u32) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, header, rows, found) = native_array_at(data, descriptor)?;
    if header < 4
        || u32_at(data, header - 4)? != 0x80809FBD
        || found != class
        || u64_at(data, header + 8)? != u64::from(class)
    {
        return Err(format!("Invalid UI array at 0x{descriptor:X}"));
    }
    rows_fit(data, rows, count, stride)?;
    Ok((0..count).map(|i| rows + i * stride).collect())
}

fn path(data: &[u8], field: usize) -> Result<Vec<u32>, String> {
    let delta = i64_at(data, field)?;
    if delta == 0 {
        return Ok(Vec::new());
    }
    let descriptor = relative_offset(field, 0, delta)?;
    array(data, descriptor, 4, 0x80804949)?
        .into_iter()
        .map(|at| u32_at(data, at))
        .collect()
}

fn endpoint(data: &[u8], at: usize) -> Result<Endpoint, String> {
    let encoded = u64_at(data, at)?;
    let index = u16_at(data, at)?;
    if encoded != u64::from(index) * 0x10001 {
        return Err(format!("Unsupported UI component reference at 0x{at:X}"));
    }
    let property = u64_at(data, at + 16)?;
    Ok(Endpoint {
        component: index,
        path: path(data, at + 8)?,
        property: u32::try_from(property).map_err(|_| "UI selector exceeds 32 bits")?,
    })
}

pub fn bindings(data: &[u8]) -> Result<Vec<Binding>, String> {
    array(data, 0x58, 56, 0x808046D8)?
        .into_iter()
        .map(|at| {
            Ok(Binding {
                source: endpoint(data, at)?,
                target: endpoint(data, at + 24)?,
            })
        })
        .collect()
}

pub fn component_count(hierarchy: &[u8]) -> Result<u16, String> {
    let rows = array(hierarchy, 8, 4, 0x80804616)?;
    let count = u16::try_from(rows.len()).map_err(|_| "Too many UI components")?;
    if count == 0 || count >= 0x7FFF || u32_at(hierarchy, 24)? != u32::from(count) * 0x10001 {
        return Err("Unsupported UI hierarchy component count".into());
    }
    let mut seen = BTreeSet::new();
    for at in rows {
        let (parent, child) = (u16_at(hierarchy, at)?, u16_at(hierarchy, at + 2)?);
        if child >= count || !seen.insert(child) || (parent != 0x7FFF && !seen.contains(&parent)) {
            return Err("UI hierarchy is not a unique parent-first tree".into());
        }
    }
    Ok(count)
}

fn put_pointer(data: &mut [u8], at: usize, target: usize) -> Result<(), String> {
    let delta = i64::try_from(target)
        .map_err(|_| "UI pointer overflow")?
        .checked_sub(i64::try_from(at).map_err(|_| "UI pointer overflow")?)
        .ok_or("UI pointer overflow")?;
    write_bytes(data, at, &delta.to_le_bytes())
}

fn align(data: &mut Vec<u8>, alignment: usize) {
    data.resize(data.len().next_multiple_of(alignment), 0);
}

/// Append a fresh typed array and retarget only its descriptor. Old targets remain available
/// to other relative pointers. Callers relocate pointers in copied rows explicitly.
fn append_array(
    data: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    count: usize,
    rows: &[u8],
) -> Result<usize, String> {
    align(data, 16);
    data.extend_from_slice(&[0; 12]);
    data.extend_from_slice(&0x80809FBDu32.to_le_bytes());
    let header = data.len();
    data.extend_from_slice(&(count as u64).to_le_bytes());
    data.extend_from_slice(&u64::from(class).to_le_bytes());
    let start = data.len();
    data.extend_from_slice(rows);
    write_bytes(data, descriptor, &(count as u64).to_le_bytes())?;
    put_pointer(data, descriptor + 8, header)?;
    Ok(start)
}

fn copied_rows(data: &[u8], offsets: &[usize], stride: usize) -> Result<Vec<u8>, String> {
    let mut rows = Vec::with_capacity(offsets.len() * stride);
    for &at in offsets {
        rows_fit(data, at, 1, stride)?;
        rows.extend_from_slice(&data[at..at + stride]);
    }
    Ok(rows)
}

fn relocated_array(
    data: &mut Vec<u8>,
    descriptor: usize,
    class: u32,
    stride: usize,
    old: &[usize],
    extra: usize,
    pointers: &[usize],
) -> Result<usize, String> {
    let mut rows = copied_rows(data, old, stride)?;
    rows.resize(rows.len() + extra * stride, 0);
    let start = append_array(data, descriptor, class, old.len() + extra, &rows)?;
    for (index, &from) in old.iter().enumerate() {
        for &offset in pointers {
            let delta = i64_at(data, from + offset)?;
            if delta != 0 {
                let target = relative_offset(from, offset, delta)?;
                if target >= data.len() {
                    return Err("UI pointer leaves its payload".into());
                }
                put_pointer(data, start + index * stride + offset, target)?;
            }
        }
    }
    Ok(start)
}

fn finish(data: &mut [u8]) -> Result<(), String> {
    let size = u32::try_from(data.len()).map_err(|_| "UI payload is too large")?;
    write_bytes(data, 0, &size.to_le_bytes())
}

/// Remove each exact route once, preserving all other binding bytes and relative targets.
pub fn remove_bindings(source: &[u8], remove: &[Binding]) -> Result<Vec<u8>, String> {
    let rows = array(source, 0x58, 56, 0x808046D8)?;
    let decoded = bindings(source)?;
    let mut excluded = BTreeSet::new();
    for binding in remove {
        let matching = decoded
            .iter()
            .enumerate()
            .filter(|(_, value)| *value == binding)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matching.len() != 1
            || !excluded.insert(matching[0])
            || i64_at(source, rows[matching[0]] + 48)? != 0
        {
            return Err("The expected UI color route is missing, repeated or converted".into());
        }
    }
    let kept = rows
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !excluded.contains(i))
        .map(|(_, at)| at)
        .collect::<Vec<_>>();
    let mut data = source.to_vec();
    relocated_array(&mut data, 0x58, 0x808046D8, 56, &kept, 0, &[8, 32, 48])?;
    finish(&mut data)?;
    Ok(data)
}

fn write_endpoint(data: &mut Vec<u8>, at: usize, endpoint: &Endpoint) -> Result<(), String> {
    write_bytes(
        data,
        at,
        &(u64::from(endpoint.component) * 0x10001).to_le_bytes(),
    )?;
    write_bytes(data, at + 16, &u64::from(endpoint.property).to_le_bytes())?;
    write_path(data, at + 8, &endpoint.path)
}

fn write_path(data: &mut Vec<u8>, field: usize, path: &[u32]) -> Result<(), String> {
    // Stock paths always point to a descriptor, including a shared zero-count descriptor.
    // A null path pointer is not a verified substitute for an empty native path.
    align(data, 8);
    let descriptor = data.len();
    data.extend_from_slice(&[0; 16]);
    if !path.is_empty() {
        let rows = path
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        append_array(data, descriptor, 0x80804949, path.len(), &rows)?;
    }
    put_pointer(data, field, descriptor)
}

struct Plan {
    first: u16,
    count: usize,
    names: Vec<usize>,
    classes: Vec<usize>,
    objects: Vec<usize>,
    tree_rows: Vec<(u16, u16)>,
    added_bindings: Vec<Binding>,
}

fn plan(widget: &[u8], hierarchy: &[u8], switches: &[ColorSwitch]) -> Result<Plan, String> {
    let first = component_count(hierarchy)?;
    let count = usize::from(first)
        .checked_add(switches.len())
        .filter(|&n| n < 0x7FFF)
        .ok_or("Too many UI color decisions")?;
    let names = array(widget, 0x18, 4, 0x80804618)?;
    let classes = array(widget, 0x38, 16, 0x80804622)?;
    if names.len() != classes.len() {
        return Err("UI component names and classes differ".into());
    }
    for &at in &classes {
        let encoded = u64_at(widget, at + 8)?;
        if encoded & 0xFFFF != 0 || encoded >> 16 >= u64::from(first) {
            return Err("UI local class index is outside its hierarchy".into());
        }
    }
    let objects = array(widget, 0x48, 64, 0x8080462A)?;
    if objects.is_empty() {
        return Err("UI widget table has no property object".into());
    }
    let mut seen_names = names
        .iter()
        .map(|&at| u32_at(widget, at))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut tree_rows = array(hierarchy, 8, 4, 0x80804616)?
        .into_iter()
        .map(|at| Ok((u16_at(hierarchy, at)?, u16_at(hierarchy, at + 2)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let mut added_bindings = Vec::new();
    for (ordinal, switch) in switches.iter().enumerate() {
        let index = first + ordinal as u16;
        if !seen_names.insert(switch.name) {
            return Err("Duplicate UI component name".into());
        }
        let sibling = tree_rows
            .iter()
            .position(|&(_, child)| child == switch.sibling)
            .ok_or("UI color consumer is absent from its hierarchy")?;
        let parent = tree_rows[sibling].0;
        if tree_rows
            .iter()
            .any(|&(parent, _)| parent == switch.sibling)
        {
            return Err("The UI color insertion sibling has children".into());
        }
        tree_rows.insert(sibling + 1, (parent, index));
        for (source, property) in [
            (&switch.condition, 0x202),
            (&switch.when_true, 0x203),
            (&switch.when_false, 0x204),
        ] {
            if usize::from(source.component) >= count {
                return Err("UI color source is outside the hierarchy".into());
            }
            added_bindings.push(Binding {
                source: source.clone(),
                target: Endpoint {
                    component: index,
                    path: Vec::new(),
                    property,
                },
            });
        }
        for target in &switch.outputs {
            if usize::from(target.component) >= count {
                return Err("UI color target is outside the hierarchy".into());
            }
            added_bindings.push(Binding {
                source: Endpoint {
                    component: index,
                    path: Vec::new(),
                    property: 0x205,
                },
                target: target.clone(),
            });
        }
    }
    Ok(Plan {
        first,
        count,
        names,
        classes,
        objects,
        tree_rows,
        added_bindings,
    })
}

/// Every field naming a property value: each component property's value (+0x10) in every
/// object, and each animated value link (+0) under an object's animation containers.
fn value_fields(data: &[u8]) -> Result<Vec<usize>, String> {
    let mut fields = Vec::new();
    for object in array(data, 0x48, 64, 0x8080462A)? {
        for component in array(data, object + 0x20, 24, 0x808046D4)? {
            for property in array(data, component + 8, 24, 0x80804858)? {
                fields.push(property + 0x10);
            }
        }
        for container in array(data, object + 0x30, 24, 0x808046F3)? {
            for record in array(data, container + 8, 32, 0x808046F5)? {
                for set in array(data, record + 8, 48, 0x808046F7)? {
                    fields.extend(array(data, set + 0x20, 16, 0x808046FB)?);
                }
            }
        }
    }
    Ok(fields)
}

/// The enumeration pool row holding `value`, adding one when no row does. The pool then moves to
/// the end of the payload with the new row, and every value naming one of its rows names the
/// moved row, since a value outside the pools holds none of the table's values.
fn enum_value(data: &mut Vec<u8>, value: u32) -> Result<usize, String> {
    let rows = array(data, ENUM_POOL, 4, ENUM_POOL_CLASS)?;
    let mut values = Vec::with_capacity((rows.len() + 1) * 4);
    for &at in &rows {
        let found = u32_at(data, at)?;
        if found == value {
            return Ok(at);
        }
        values.extend_from_slice(&found.to_le_bytes());
    }
    values.extend_from_slice(&value.to_le_bytes());
    let fields = value_fields(data)?;
    let start = append_array(data, ENUM_POOL, ENUM_POOL_CLASS, rows.len() + 1, &values)?;
    if let Some(&old) = rows.first() {
        let moved = old..old + rows.len() * 4;
        for field in fields {
            let delta = i64_at(data, field)?;
            if delta == 0 {
                continue;
            }
            let target = relative_offset(field, 0, delta)?;
            if moved.contains(&target) {
                put_pointer(data, field, start + (target - old))?;
            }
        }
    }
    Ok(start + rows.len() * 4)
}

/// The table's default property object.
fn default_object(data: &[u8], objects: &[usize]) -> Result<usize, String> {
    let mut found = objects
        .iter()
        .copied()
        .filter(|&at| data.get(at..at + DEFAULT_OBJECT.len()) == Some(&DEFAULT_OBJECT[..]));
    match (found.next(), found.next()) {
        (Some(object), None) => Ok(object),
        _ => Err("UI widget table has no single default property object".into()),
    }
}

/// A decision's one stored property, its value type in the enumeration pool. Its three inputs
/// are bound, as 129 stock decisions storing nothing else have theirs.
fn decision_properties(
    data: &mut Vec<u8>,
    component: usize,
    value_type: usize,
) -> Result<(), String> {
    let property = append_array(data, component + 8, 0x80804858, 1, &[0; 24])?;
    write_path(data, property, &[])?;
    write_bytes(data, property + 8, &0x201u64.to_le_bytes())?;
    put_pointer(data, property + 16, value_type)
}

/// Add native Boolean/RGBA decisions to a root table's default property object, with matching
/// class names, local class rows, bindings and hierarchy siblings. Inputs may name earlier
/// switches. This edits owned copies and returns nothing on a validation failure.
pub fn color_switches(
    widget: &[u8],
    hierarchy: &[u8],
    switches: &[ColorSwitch],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let Plan {
        first,
        count,
        names,
        classes,
        objects,
        tree_rows,
        added_bindings,
    } = plan(widget, hierarchy, switches)?;
    let mut data = widget.to_vec();
    let value_type = enum_value(&mut data, RGBA_DECISION)?;
    let mut name_rows = copied_rows(widget, &names, 4)?;
    let mut class_rows = copied_rows(widget, &classes, 16)?;
    for (ordinal, switch) in switches.iter().enumerate() {
        name_rows.extend_from_slice(&switch.name.to_le_bytes());
        class_rows.extend_from_slice(&DECISION.to_le_bytes());
        class_rows.extend_from_slice(&((u64::from(first) + ordinal as u64) << 16).to_le_bytes());
    }
    append_array(
        &mut data,
        0x18,
        0x80804618,
        names.len() + switches.len(),
        &name_rows,
    )?;
    append_array(
        &mut data,
        0x38,
        0x80804622,
        classes.len() + switches.len(),
        &class_rows,
    )?;
    let object = default_object(&data, &objects)?;
    let components = array(&data, object + 32, 24, 0x808046D4)?;
    let rows = relocated_array(
        &mut data,
        object + 32,
        0x808046D4,
        24,
        &components,
        switches.len(),
        &[16],
    )?;
    for ordinal in 0..switches.len() {
        let at = rows + (components.len() + ordinal) * 24;
        write_bytes(
            &mut data,
            at,
            &(u32::from(first) + ordinal as u32).to_le_bytes(),
        )?;
        decision_properties(&mut data, at, value_type)?;
    }
    let old_bindings = array(widget, 0x58, 56, 0x808046D8)?;
    let rows = relocated_array(
        &mut data,
        0x58,
        0x808046D8,
        56,
        &old_bindings,
        added_bindings.len(),
        &[8, 32, 48],
    )?;
    for (i, binding) in added_bindings.iter().enumerate() {
        let at = rows + (old_bindings.len() + i) * 56;
        write_endpoint(&mut data, at, &binding.source)?;
        write_endpoint(&mut data, at + 24, &binding.target)?;
    }
    let mut tree = hierarchy.to_vec();
    let rows = tree_rows
        .into_iter()
        .flat_map(|(parent, child)| parent.to_le_bytes().into_iter().chain(child.to_le_bytes()))
        .collect::<Vec<_>>();
    append_array(&mut tree, 8, 0x80804616, count, &rows)?;
    write_bytes(&mut tree, 24, &((count as u32) * 0x10001).to_le_bytes())?;
    finish(&mut data)?;
    finish(&mut tree)?;
    component_count(&tree)?;
    Ok((data, tree))
}
