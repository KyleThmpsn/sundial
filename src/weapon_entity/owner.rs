//! Typed references that must stay inside a cloned component-owner partition.

use super::*;

pub(super) const EVENTS_DESCRIPTOR: usize = 0x20;
pub(super) const EVENT_ROW_CLASS: u32 = 0x8080_9BC9;
pub(super) const EVENT_ROW_SIZE: usize = 0x48;

pub(super) fn event_rows(entity: &[u8]) -> Result<Vec<usize>, String> {
    if read_u64(entity, EVENTS_DESCRIPTOR)? == 0 {
        return Ok(Vec::new());
    }
    let events = native_array(entity, EVENTS_DESCRIPTOR)?;
    if events.row_class != EVENT_ROW_CLASS {
        return Err(format!(
            "Weapon entity event array has unsupported class 0x{:08X}",
            events.row_class
        ));
    }
    checked_rows_end(events, EVENT_ROW_SIZE, entity.len(), "Weapon entity event")?;
    Ok((0..events.count)
        .map(|index| events.rows + index * EVENT_ROW_SIZE)
        .collect())
}

pub(super) fn validate_events(entity: &[u8]) -> Result<(), String> {
    event_rows(entity).map(|_| ())
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Endpoint {
    Resource(u32, Vec<(u32, usize)>),
    External(u32, u32, u64),
}

fn endpoint(
    entity: &[u8],
    offset: usize,
    replaced_owner: Option<u32>,
    aliases: &[ComponentAlias],
) -> Result<Endpoint, String> {
    let tag = read_u32(entity, offset)?;
    let class = read_u32(entity, offset + 4)?;
    let position = read_u64(entity, offset + 8)?;
    let identities = aliases
        .iter()
        .filter(|alias| {
            alias.owner_tag == tag
                && alias.concrete_class == class
                && alias.resource_offset == position
        })
        .map(|alias| (alias.binding_hash, alias.resource_index))
        .collect::<BTreeSet<_>>();
    if !identities.is_empty() {
        return Ok(Endpoint::Resource(class, identities.into_iter().collect()));
    }
    if Some(tag) == replaced_owner {
        return Err("This component group has an event connection that cannot be mapped to the donor. Choose another donor or keep the baseline.".into());
    }
    Ok(Endpoint::External(tag, class, position))
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EventKey {
    metadata: Vec<u8>,
    source: Endpoint,
    destination: Endpoint,
}

fn events_for_owner(
    entity: &[u8],
    owner: u32,
    moving: bool,
) -> Result<BTreeMap<EventKey, usize>, String> {
    let rows = event_rows(entity)?;
    if rows.is_empty() {
        return Ok(BTreeMap::new());
    }
    let aliases = weapon_component_aliases(entity)?;
    let mut result = BTreeMap::new();
    for row in rows {
        if read_u32(entity, row + 8)? != owner && read_u32(entity, row + 0x28)? != owner {
            continue;
        }
        let metadata = [
            &entity[row..row + 8],
            &entity[row + 0x18..row + 0x28],
            &entity[row + 0x38..row + 0x48],
        ]
        .concat();
        let key = EventKey {
            metadata,
            source: endpoint(entity, row + 8, moving.then_some(owner), &aliases)?,
            destination: endpoint(entity, row + 0x28, moving.then_some(owner), &aliases)?,
        };
        if result.insert(key, row).is_some() {
            return Err("This component group has duplicate event connections that cannot be matched unambiguously".into());
        }
    }
    Ok(result)
}

/// How an event endpoint that names a replaced component owner is re-pointed at the donor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPolicy {
    /// Pair the target's and donor's event sets and copy the donor's endpoint verbatim. Every
    /// endpoint into the replaced owner must address a bound resource, and both entities must
    /// declare the same connections around it. This is the only policy safe for an arbitrary
    /// donor, because it never assumes two unrelated payloads agree about an interior offset.
    Paired,
    /// Re-point each endpoint that names the replaced owner on its own, without pairing whole
    /// events. A bound endpoint still follows its binding identity, which survives the graft.
    /// An unbound one keeps its class and interior offset and changes only the owner tag, and
    /// only where the donor's own event graph proves it addresses that same class at that same
    /// offset. Use this when the two entities wire the same component differently because the
    /// rest of their components differ, which is what separates two weapon families.
    Retarget,
}

/// Only rewrite endpoints proven to address corresponding bound resources. Nested objects and
/// different event programs require explicit mappings, never a guessed owner-tag substitution.
pub(super) fn graft_event_updates(
    target: &[u8],
    donor: &[u8],
    target_owner: u32,
    donor_owner: u32,
    policy: EventPolicy,
) -> Result<Vec<(usize, [u8; 16])>, String> {
    if policy == EventPolicy::Retarget {
        return retarget_event_updates(target, donor, target_owner, donor_owner);
    }
    let moving = target_owner != donor_owner;
    let target_events = events_for_owner(target, target_owner, moving)?;
    let donor_events = events_for_owner(donor, donor_owner, moving)?;
    if target_events.keys().ne(donor_events.keys()) {
        return Err("This component group uses different event connections in the donor. Choose another donor or keep the baseline.".into());
    }
    let mut updates = Vec::new();
    for (key, target_row) in target_events {
        let donor_row = donor_events[&key];
        for relative in [8, 0x28] {
            if read_u32(target, target_row + relative)? == target_owner {
                if read_u32(donor, donor_row + relative)? != donor_owner {
                    return Err(
                        "The donor event connection leaves the selected component group".into(),
                    );
                }
                let mut bytes = [0; 16];
                bytes.copy_from_slice(&donor[donor_row + relative..donor_row + relative + 16]);
                updates.push((target_row + relative, bytes));
            }
        }
    }
    Ok(updates)
}

/// Every endpoint the entity's own event graph addresses inside one owner, as (class, offset).
/// The client authored these, so an entry is evidence that the owner really does carry a
/// receiver of that class at that offset.
fn owner_event_endpoints(entity: &[u8], owner: u32) -> Result<BTreeSet<(u32, u64)>, String> {
    let mut endpoints = BTreeSet::new();
    for row in event_rows(entity)? {
        for offset in [row + 8, row + 0x28] {
            if read_u32(entity, offset)? == owner {
                endpoints.insert((read_u32(entity, offset + 4)?, read_u64(entity, offset + 8)?));
            }
        }
    }
    Ok(endpoints)
}

fn retarget_event_updates(
    target: &[u8],
    donor: &[u8],
    target_owner: u32,
    donor_owner: u32,
) -> Result<Vec<(usize, [u8; 16])>, String> {
    if target_owner == donor_owner {
        return Ok(Vec::new());
    }
    let target_aliases = weapon_component_aliases(target)?;
    let donor_aliases = weapon_component_aliases(donor)?;
    let donor_endpoints = owner_event_endpoints(donor, donor_owner)?;
    let mut updates = Vec::new();
    for row in event_rows(target)? {
        for offset in [row + 8, row + 0x28] {
            if read_u32(target, offset)? != target_owner {
                continue;
            }
            let class = read_u32(target, offset + 4)?;
            let position = read_u64(target, offset + 8)?;
            let identities = target_aliases
                .iter()
                .filter(|alias| {
                    alias.owner_tag == target_owner
                        && alias.concrete_class == class
                        && alias.resource_offset == position
                })
                .map(|alias| (alias.binding_hash, alias.resource_index))
                .collect::<BTreeSet<_>>();
            let mut bytes = [0; 16];
            if identities.is_empty() {
                if !donor_endpoints.contains(&(class, position)) {
                    return Err(format!(
                        "The donor component has no event connection of class 0x{class:08X} at offset {position:#X}, so this one cannot follow it."
                    ));
                }
                bytes.copy_from_slice(&target[offset..offset + 16]);
                bytes[..4].copy_from_slice(&donor_owner.to_le_bytes());
            } else {
                let resolved = donor_aliases
                    .iter()
                    .filter(|alias| {
                        alias.owner_tag == donor_owner
                            && identities.contains(&(alias.binding_hash, alias.resource_index))
                    })
                    .map(|alias| (alias.concrete_class, alias.resource_offset))
                    .collect::<BTreeSet<_>>();
                let [(donor_class, donor_position)] = resolved.into_iter().collect::<Vec<_>>()[..]
                else {
                    return Err(
                        "A bound event connection does not resolve to one donor resource".into(),
                    );
                };
                bytes[..4].copy_from_slice(&donor_owner.to_le_bytes());
                bytes[4..8].copy_from_slice(&donor_class.to_le_bytes());
                bytes[8..16].copy_from_slice(&donor_position.to_le_bytes());
            }
            updates.push((offset, bytes));
        }
    }
    Ok(updates)
}

pub(super) fn event_owner_fields(entity: &[u8], owner_tag: u32) -> Result<BTreeSet<usize>, String> {
    let mut fields = BTreeSet::new();
    for row in event_rows(entity)? {
        // Each endpoint wraps a 16-byte typed owner/absolute-offset reference.
        for offset in [row + 8, row + 0x28] {
            if read_u32(entity, offset)? == owner_tag {
                if !is_native_class(read_u32(entity, offset + 4)?) {
                    return Err("Weapon entity event endpoint has an invalid class".into());
                }
                fields.insert(offset);
            }
        }
    }
    Ok(fields)
}

pub(super) fn self_reference_owner_fields(data: &[u8], owner_tag: u32) -> BTreeSet<usize> {
    let mut fields = BTreeSet::new();
    // A tag-valued integer is insufficient. Require two complete, aligned, typed
    // references in this payload that name the owner and point back to each other.
    for offset in (0..data.len().saturating_sub(15)).step_by(8) {
        if read_u32(data, offset) != Ok(owner_tag)
            || !read_u32(data, offset + 4).is_ok_and(is_native_class)
        {
            continue;
        }
        let Ok(target) = read_u64(data, offset + 8).and_then(|v| {
            usize::try_from(v).map_err(|_| "Object reference offset is too large".to_owned())
        }) else {
            continue;
        };
        if target == offset
            || target % 8 != 0
            || target.checked_add(16).is_none_or(|end| end > data.len())
            || read_u32(data, target) != Ok(owner_tag)
            || !read_u32(data, target + 4).is_ok_and(is_native_class)
            || read_u64(data, target + 8) != Ok(offset as u64)
        {
            continue;
        }
        fields.insert(offset);
        fields.insert(target);
    }
    fields
}

/// Callbacks can address an entity-bound instance without a reciprocal link.
/// The caller has already validated every target's class and paired definition.
pub(super) fn bound_reference_owner_fields(
    data: &[u8],
    owner_tag: u32,
    targets: &BTreeMap<usize, u32>,
) -> BTreeSet<usize> {
    let mut fields = BTreeSet::new();
    for offset in (0..data.len().saturating_sub(15)).step_by(8) {
        if read_u32(data, offset) != Ok(owner_tag) {
            continue;
        }
        let Ok(target) = read_u64(data, offset + 8).and_then(|value| {
            usize::try_from(value).map_err(|_| "Bound reference offset is too large".to_owned())
        }) else {
            continue;
        };
        if targets
            .get(&target)
            .is_some_and(|class| read_u32(data, offset + 4) == Ok(*class))
        {
            fields.insert(offset);
        }
    }
    fields
}

fn is_native_class(value: u32) -> bool {
    value & 0xFFFF_0000 == 0x8080_0000
}
