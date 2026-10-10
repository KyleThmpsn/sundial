//! Outgoing damage profiles: the resources an ability's projectiles, detonations, areas and
//! contact attacks name for the damage they deal, and the damage type each one gives.
//!
//! A profile's root is a `80804B02` record. Its byte at `+0x20` is the damage type, which the
//! client copies into the damage event when nothing typed the event first (`B8807E` reads it,
//! `B88088` writes event `+27` in the archived 86657 image). The root names its own tag at `+0`
//! and its paired `80804B01` block at an absolute offset at `+8`, and that block names the tag
//! too, so a copy renames both. Profiles share the generic `80809C36` directory class with
//! many other resources, so a profile is recognized by that shape, never by its class alone.
use crate::package_payload::u32_at;
use crate::package_runtime::reader::PackageManager;

use super::spawns::Spawn;

/// The directory class profiles are filed under, among many other resources.
const DIRECTORY_CLASS: u32 = 0x8080_9C36;
/// The profile root's marker, and its paired block's.
const ROOT_CLASS: u32 = 0x8080_4B02;
const PAIRED_CLASS: u32 = 0x8080_4B01;
/// Where the payload header points at the root, relative to itself.
const ROOT_POINTER: usize = 0x18;
/// Where the root keeps its paired block's absolute offset.
const PAIRED_OFFSET: usize = 0x8;
/// Where the root keeps the damage type.
const MODE: usize = 0x20;

/// A damage type as the client encodes it in a profile: Kinetic 0, Solar 1, Arc 2, Void 3. It
/// is not the public numbering, which counts from 1 in another order.
pub const KINETIC: u8 = 0;
pub const SOLAR: u8 = 1;
pub const ARC: u8 = 2;
pub const VOID: u8 = 3;

/// One damage profile: where its root and paired block sit, and its damage type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Profile {
    pub root: usize,
    pub paired: usize,
    pub mode: u8,
}

/// `payload` as the damage profile `tag`, when it has a profile's shape: a header pointing at a
/// `80804B02` root that names `tag`, a paired `80804B01` block that names `tag` too, and a damage
/// type the client knows.
#[must_use]
pub fn profile(tag: u32, payload: &[u8]) -> Option<Profile> {
    let word = |at: usize| payload.get(at..at.checked_add(8)?)?.try_into().ok();
    let relative = i64::from_le_bytes(word(ROOT_POINTER)?);
    let root = usize::try_from(i64::try_from(ROOT_POINTER).ok()?.checked_add(relative)?).ok()?;
    let paired =
        usize::try_from(u64::from_le_bytes(word(root.checked_add(PAIRED_OFFSET)?)?)).ok()?;
    let marked = |at: usize, class: u32| {
        at.checked_sub(4)
            .is_some_and(|marker| u32_at(payload, marker).ok() == Some(class))
            && u32_at(payload, at).ok() == Some(tag)
    };
    let mode = *payload.get(root.checked_add(MODE)?)?;
    (marked(root, ROOT_CLASS) && marked(paired, PAIRED_CLASS) && mode <= VOID).then_some(Profile {
        root,
        paired,
        mode,
    })
}

/// Every place `entity`'s component owners name a damage profile, with the profile, by owner and
/// offset as a runtime resource patch names a place. A word that names a resource of the
/// profiles' directory class counts only once the resource has a profile's shape.
pub fn references(
    manager: &PackageManager,
    entity_tag: u32,
    entity: &[u8],
) -> Result<Vec<(Spawn, Profile)>, String> {
    let mut checked = std::collections::BTreeMap::<u32, Option<Profile>>::new();
    let mut found = Vec::new();
    for place in super::spawns::places(manager, entity, |value, class| {
        value != entity_tag && class == DIRECTORY_CLASS
    })? {
        let shape = match checked.get(&place.graph) {
            Some(shape) => *shape,
            None => {
                let shape = profile(place.graph, &manager.read_tag(place.graph)?);
                checked.insert(place.graph, shape);
                shape
            }
        };
        if let Some(shape) = shape {
            found.push((place, shape));
        }
    }
    Ok(found)
}

/// Makes `payload`, a copy of a profile, the profile `copy` with damage type `mode`: its root and
/// paired block name the copy, and only the damage type's own byte changes.
pub fn retype(payload: &mut [u8], profile: Profile, copy: u32, mode: u8) -> Result<(), String> {
    if mode > VOID {
        return Err(format!("Damage type {mode} is not one the client knows"));
    }
    for at in [profile.root, profile.paired] {
        payload
            .get_mut(at..at + 4)
            .ok_or("A damage profile is shorter than its shape")?
            .copy_from_slice(&copy.to_le_bytes());
    }
    *payload
        .get_mut(profile.root + MODE)
        .ok_or("A damage profile is shorter than its shape")? = mode;
    Ok(())
}
