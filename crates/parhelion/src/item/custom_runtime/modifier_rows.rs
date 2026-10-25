//! Rows added to an attached entity's Property Modifiers: one more settings record each, written
//! after the entity's own records in its private copy.
//!
//! The records are an array in the component's owner, reached through a descriptor that counts
//! them and points at their header. A copy cannot grow the array where it is, since the owner's
//! other objects follow it, so the copy gets a new array at the end of the owner holding every
//! stock record as the asset's values leave it, then the added rows, and the descriptor is
//! repointed at it. The stock records stay where they were, patched by the asset's values as
//! before, which keeps every value's locator true, and go unread once the descriptor moves. The
//! owner writer places the header on a 16-byte boundary with the typed-array marker before it.
use super::*;
use sundial::package_authoring::{
    runtime::{
        encode_weapon_runtime_field_value, load_weapon_runtime_graph_for_entity,
        resolve_weapon_runtime_field,
    },
    sandbox_perk::{entity::modifiers, program::ModifierRow},
};

/// The append that gives `graph`'s modifier records `rows` more, or none when there are no rows
/// to add. Refuses an entity without modifier records, or whose records sit in more than one
/// component.
pub(in crate::item) fn appends(
    manager: &PackageManager,
    graph: u32,
    values: &[WeaponRuntimeValueOverride],
    rows: &[ModifierRow],
) -> AuthoringResult<Vec<WeaponRuntimeResourceAppend>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let payload = read_tag(manager, TagHash(graph), "attached entity")?;
    let loaded = load_weapon_runtime_graph_for_entity(manager, 0, 0, graph, &payload)
        .map_err(|error| invalid(format!("Attached entity 0x{graph:08X}: {error}")))?;
    let records = modifiers::discover(&loaded);
    let Some(first) = records.first() else {
        return Err(invalid(format!(
            "Entity 0x{graph:08X} has no modifier rows to add to"
        )));
    };
    let owner = first.owner_tag;
    if records.iter().any(|record| record.owner_tag != owner) {
        return Err(invalid(format!(
            "Entity 0x{graph:08X} keeps modifier rows in more than one component"
        )));
    }
    let binding = first.amount.locator.binding_hash.get();
    let index = first.amount.locator.resource_index;
    let resource = weapon_component_bindings(&payload, binding)
        .map_err(invalid)?
        .get(usize::from(index))
        .filter(|resource| resource.owner_tag == owner)
        .map(|resource| resource.resource_offset)
        .ok_or_else(|| invalid("The modifier rows' component is not bound as its records say"))?;
    let resource = usize::try_from(resource)
        .map_err(|_| invalid("The modifier rows' resource offset does not fit"))?;
    // The stock records as the asset's values leave them.
    let mut patched = read_tag(manager, TagHash(owner), "modifier owner")?;
    for (value_index, value) in values.iter().enumerate() {
        let locator = value.locator.for_graph(graph.into()).map_err(invalid)?;
        let resolved = resolve_weapon_runtime_field(manager, &payload, &locator)
            .map_err(|error| invalid(format!("Runtime value {value_index} is stale: {error}")))?;
        if resolved.owner_tag != owner {
            continue;
        }
        let bytes = encode_weapon_runtime_field_value(&resolved.field, &value.value)
            .map_err(|error| invalid(format!("Runtime value {value_index} is invalid: {error}")))?;
        let end = resolved.owner_offset + bytes.len();
        patched
            .get_mut(resolved.owner_offset..end)
            .ok_or_else(|| invalid(format!("Runtime value {value_index} is outside its owner")))?
            .copy_from_slice(&bytes);
    }
    let array = modifiers::record_array(&patched, &records).map_err(invalid)?;
    let descriptor = array
        .descriptor
        .checked_sub(resource)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| invalid("The modifier rows' descriptor lies before their component"))?;
    let total = records.len() + rows.len();
    let mut bytes = patched[array.header..array.header + 16].to_vec();
    bytes[..8].copy_from_slice(&(total as u64).to_le_bytes());
    for record in &records {
        let at = record.owner_offset as usize;
        bytes.extend_from_slice(&patched[at..at + modifiers::RECORD_SIZE]);
    }
    let template = &patched[first.owner_offset as usize..][..modifiers::RECORD_SIZE];
    for row in rows {
        bytes.extend(modifiers::row_bytes(template, row).map_err(invalid)?);
    }
    Ok(vec![WeaponRuntimeResourceAppend {
        binding_hash: binding,
        resource_index: index,
        bytes,
        slots: Vec::new(),
        arrays: vec![(descriptor, 0, total as u64)],
        pointers: Vec::new(),
        references: Vec::new(),
    }])
}
