//! First-person animations taken from another weapon.
//!
//! The first-person attachment (binding 0xD3A5500E) keeps one 0xE8-byte row (class 80803E72)
//! per content group in the table at its definition +0x588, with the embedded default just
//! before it. Every row of one owner names the same first-person entity at +0x78, so what a row
//! plays is decided by two keys: +0x6C follows the frame (every Adaptive Frame hand cannon names
//! 47C8047E, every Precision Frame one 56FFC1E7) and +0x20 names the family a row belongs to,
//! which exotics with animations of their own set apart. Another row of the same owner can lend
//! both keys, and only a row of the same owner, since that owner's entity is what reads them.
use crate::tag_payload::{array_at, read_u32, relative_target};
use crate::{AuthoringResult, error::invalid, item::WeaponRuntimeResourcePatch};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::{
    WEAPON_ENTITY_COMPONENT_ROW_SIZE, weapon_component_bindings,
};
use tiger_pkg::TagHash;

pub(crate) mod actions;

const BINDING: u32 = 0xD3A5_500E;
const TABLE: usize = 0x588;
const ROW_SIZE: usize = 0xE8;
const ROW_CLASS: u32 = 0x8080_3E72;
const KEYS: [usize; 2] = [0x20, 0x6C];
/// The weapon's hold in the attachment definition: a rotation quaternion, a translation in
/// metres and a scale. Each rig family holds its own weapon type at its own place.
const HOLD: usize = 0x6A8;
const HOLD_SIZE: usize = 0x20;
/// Where a row names the first-person arms rig it plays on.
const ARMS_RIG: usize = 0x78;
/// The arms rig's animation lookup, and where its data names the parameter dictionary and the
/// action state table.
const LOOKUP_CLASS: u32 = 0x8080_344B;
const PARAMETERS: usize = 0x94;
const PARAMETERS_CLASS: u32 = 0x8080_8EE1;
const STATES: usize = 0x9C;
const STATES_CLASS: u32 = 0x8080_3465;

/// One attachment's hold transform, as it is stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Hold([u8; HOLD_SIZE]);

impl Hold {
    /// Where the hold puts the weapon's origin, in the space of the bone that holds it.
    #[cfg(test)]
    pub(crate) fn translation(&self) -> [f32; 3] {
        std::array::from_fn(|axis| {
            let at = (4 + axis) * 4;
            f32::from_le_bytes(self.0[at..at + 4].try_into().expect("four bytes"))
        })
    }
}

/// The animations one weapon plays: the attachment owner its row sits in and the row's keys.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Profile {
    pub(crate) owner: u32,
    pub(crate) keys: [u32; 2],
}

struct Attachment {
    owner_tag: u32,
    owner: Vec<u8>,
    resource: usize,
    definition: usize,
}

fn attachment(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Attachment> {
    let bindings = weapon_component_bindings(entity, BINDING).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "The weapon does not have one first-person attachment",
        ));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("First-person attachment offset overflow"))?;
    let definition = relative_target(&owner, 0x18)?;
    Ok(Attachment {
        owner_tag: binding.owner_tag,
        owner,
        resource,
        definition,
    })
}

/// The row a content group selects: its own, or the embedded default the client falls back to.
fn row(attachment: &Attachment, group: Option<u32>) -> AuthoringResult<usize> {
    let (count, _, rows, class) = array_at(&attachment.owner, attachment.definition + TABLE)?;
    if class != ROW_CLASS {
        return Err(invalid(
            "First-person attachment rows have an unexpected class",
        ));
    }
    if let Some(group) = group {
        for index in 0..count {
            let row = rows + index * ROW_SIZE;
            if read_u32(&attachment.owner, row)? == group {
                return Ok(row);
            }
        }
    }
    Ok(attachment.definition + TABLE - ROW_SIZE)
}

fn keys(attachment: &Attachment, row: usize) -> AuthoringResult<[u32; 2]> {
    let mut keys = [0; 2];
    for (key, offset) in keys.iter_mut().zip(KEYS) {
        *key = read_u32(&attachment.owner, row + offset)?;
    }
    Ok(keys)
}

/// The animations `entity` plays for content group `group`.
pub(crate) fn profile(
    manager: &PackageManager,
    entity: &[u8],
    group: Option<u32>,
) -> AuthoringResult<Profile> {
    let attachment = attachment(manager, entity)?;
    let row = row(&attachment, group)?;
    Ok(Profile {
        owner: attachment.owner_tag,
        keys: keys(&attachment, row)?,
    })
}

/// The hold transform `entity`'s first-person attachment stores. The rotation must be a unit
/// quaternion and the scale finite, which is what a hold reads as in every surveyed rig.
pub(crate) fn hold(manager: &PackageManager, entity: &[u8]) -> AuthoringResult<Hold> {
    let attachment = attachment(manager, entity)?;
    let at = attachment.definition + HOLD;
    let bytes: [u8; HOLD_SIZE] = attachment
        .owner
        .get(at..at + HOLD_SIZE)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| invalid("The first-person attachment has no hold transform"))?;
    let float = |index: usize| {
        f32::from_le_bytes(
            bytes[index * 4..index * 4 + 4]
                .try_into()
                .expect("four bytes"),
        )
    };
    let length = (0..4).map(|index| float(index) * float(index)).sum::<f32>();
    if (length - 1.0).abs() > 1e-3 || !(4..8).all(|index| float(index).is_finite()) {
        return Err(invalid(
            "The first-person attachment's hold is not a rotation and translation",
        ));
    }
    Ok(Hold(bytes))
}

/// Patches that give `entity`'s first-person attachment another rig's hold, so a pinned model
/// sits where its own rig would hold it rather than where the kept rig holds its own weapon.
pub(crate) fn hold_patches(
    manager: &PackageManager,
    entity: &[u8],
    hold: Hold,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let attachment = attachment(manager, entity)?;
    // Read through the same checks, so a kept rig without a readable hold is refused too.
    if self::hold(manager, entity)? == hold {
        return Ok(Vec::new());
    }
    let offset = (attachment.definition + HOLD)
        .checked_sub(attachment.resource)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| invalid("First-person attachment hold offset overflow"))?;
    Ok(vec![WeaponRuntimeResourcePatch {
        binding_hash: BINDING,
        resource_index: 0,
        offset,
        bytes: hold.0.to_vec(),
        graph_values: Vec::new(),
        graph_removals: Vec::new(),
        graph_trajectories: None,
    }])
}

/// Patches that make the selected group's row play `donor`'s animations.
pub(crate) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    selected: Option<u32>,
    donor: Profile,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let attachment = attachment(manager, entity)?;
    if attachment.owner_tag != donor.owner {
        return Err(invalid(
            "The animation donor's rig does not fit this model. Choose another animation donor.",
        ));
    }
    let row = row(&attachment, selected)?;
    let mut patches = Vec::new();
    for (offset, key) in KEYS.into_iter().zip(donor.keys) {
        if read_u32(&attachment.owner, row + offset)? == key {
            continue;
        }
        let offset = (row + offset)
            .checked_sub(attachment.resource)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| invalid("First-person attachment row offset overflow"))?;
        patches.push(WeaponRuntimeResourcePatch {
            binding_hash: BINDING,
            resource_index: 0,
            offset,
            bytes: key.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        });
    }
    Ok(patches)
}

/// The first-person arms rig a row plays on, with the action states its lookup reads.
pub(crate) struct ArmsRig {
    pub(crate) entity_tag: u32,
    pub(crate) entity: Vec<u8>,
    pub(crate) lookup_tag: u32,
    pub(crate) lookup: Vec<u8>,
    pub(crate) states_tag: u32,
    pub(crate) states: Vec<u8>,
    pub(crate) parameters: Vec<u8>,
    /// Where the row names the rig, from the attachment resource.
    row_field: u32,
}

impl ArmsRig {
    /// Where the lookup's data names the state table.
    pub(crate) fn states_field(&self) -> AuthoringResult<usize> {
        Ok(relative_target(&self.lookup, 0x18)? + STATES)
    }

    /// The patch that points the row at another arms rig entity.
    pub(crate) fn patch(&self, entity_tag: u32) -> WeaponRuntimeResourcePatch {
        WeaponRuntimeResourcePatch {
            binding_hash: BINDING,
            resource_index: 0,
            offset: self.row_field,
            bytes: entity_tag.to_le_bytes().to_vec(),
            graph_values: Vec::new(),
            graph_removals: Vec::new(),
            graph_trajectories: None,
        }
    }
}

/// The arms rig `entity`'s row for `group` plays on, and its one animation lookup's tables.
pub(crate) fn arms_rig(
    manager: &PackageManager,
    entity: &[u8],
    group: Option<u32>,
) -> AuthoringResult<ArmsRig> {
    let attachment = attachment(manager, entity)?;
    let row = row(&attachment, group)?;
    let entity_tag = read_u32(&attachment.owner, row + ARMS_RIG)?;
    let row_field = (row + ARMS_RIG)
        .checked_sub(attachment.resource)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| invalid("First-person attachment row offset overflow"))?;
    let read = |tag: u32, what: &str| {
        manager
            .read_tag(TagHash(tag))
            .map_err(|error| invalid(format!("Arms rig {what} 0x{tag:08X}: {error}")))
    };
    let rig = read(entity_tag, "entity")?;
    // The lookup is the component whose owner's data is an animation lookup. The entity's
    // component list names every owner once, bound or not.
    let (count, _, rows, _) = array_at(&rig, 0x10)?;
    let mut owners = Vec::new();
    for index in 0..count {
        let owner = read_u32(&rig, rows + index * WEAPON_ENTITY_COMPONENT_ROW_SIZE)?;
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    let mut lookups = Vec::new();
    for owner in owners {
        let payload = read(owner, "component owner")?;
        let data = relative_target(&payload, 0x18)?;
        if data >= 4 && read_u32(&payload, data - 4)? == LOOKUP_CLASS {
            lookups.push(owner);
        }
    }
    let [lookup_tag] = lookups[..] else {
        return Err(invalid(format!(
            "Arms rig 0x{entity_tag:08X} has {} animation lookups, not one",
            lookups.len()
        )));
    };
    let lookup = read(lookup_tag, "animation lookup")?;
    let data = relative_target(&lookup, 0x18)?;
    if read_u32(&lookup, data - 4)? != LOOKUP_CLASS {
        return Err(invalid(
            "The arms rig's animation lookup has another layout",
        ));
    }
    let table = |field: usize, class: u32, what: &str| -> AuthoringResult<(u32, Vec<u8>)> {
        let tag = read_u32(&lookup, data + field)?;
        let entry = manager
            .get_entry(TagHash(tag))
            .ok_or_else(|| invalid(format!("Arms rig {what} 0x{tag:08X} is not live")))?;
        if entry.reference != class {
            return Err(invalid(format!(
                "Arms rig {what} 0x{tag:08X} has class 0x{:08X}, expected 0x{class:08X}",
                entry.reference
            )));
        }
        Ok((tag, read(tag, what)?))
    };
    let (_, parameters) = table(PARAMETERS, PARAMETERS_CLASS, "parameter dictionary")?;
    let (states_tag, states) = table(STATES, STATES_CLASS, "state table")?;
    Ok(ArmsRig {
        entity_tag,
        entity: rig,
        lookup_tag,
        lookup,
        states_tag,
        states,
        parameters,
        row_field,
    })
}
