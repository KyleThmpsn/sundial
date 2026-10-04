//! Cross-family appearances: move the appearance's rig and animations onto the gameplay
//! runtime, so the model keeps its own bones and its own reload.
//!
//! A weapon family's runtime entity keeps its presentation in four component owners: the
//! skeleton the gear parts are weighted to, the animation lookup and animation set that drive
//! the gun's own moving parts, and the first-person attachment that reaches the hands. Every
//! gameplay component the player feels lives in a different owner, so promoting those four
//! leaves trigger, barrel, magazine, reload timing and stats exactly as the gameplay donor
//! wrote them.
//!
//! Families do not agree about all of this, so it is attempted rather than assumed. Auto
//! rifles, hand cannons and pulse rifles interchange; a shotgun places its skeleton's event
//! receiver at a different offset and is refused. A refusal is not a failed build: the
//! appearance's parts are pinned to the gameplay rig's root bone instead, which draws the
//! model without its moving parts. See `item::emission::reskin`.
use crate::tag_payload::{array_at, read_u32, read_u64};
use crate::{AuthoringResult, error::invalid, item::WeaponRuntimeResourcePatch};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::entity::{
    WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, graft_weapon_component_bindings_with, owner::EventPolicy,
    weapon_component_bindings,
};
use tiger_pkg::TagHash;

const SKELETON: u32 = 0x1C80_DD4A;
const ANIMATION_LOOKUP: u32 = 0x681C_2C0D;
const ANIMATION_SET: u32 = 0x8983_4B2B;
const FIRST_PERSON_ATTACHMENT: u32 = 0xD3A5_500E;

/// Promoted together. A rig without its clips, or clips without the hands that hold them,
/// would leave the weapon half-converted and is worse than not trying.
pub(crate) const PRESENTATION_BINDINGS: [u32; 4] = [
    SKELETON,
    ANIMATION_LOOKUP,
    ANIMATION_SET,
    FIRST_PERSON_ATTACHMENT,
];

/// Replace the gameplay runtime's presentation owners with the appearance runtime's.
///
/// This uses the retargeting event policy: the two entities wire the same component through
/// different neighbours, because every other component of the weapon differs, so their event
/// sets cannot be paired. Each connection into a replaced owner keeps its class and interior
/// offset and changes only which owner it names, and only where the appearance's own event
/// graph already addresses that same class at that same offset.
pub(crate) fn graft_presentation(target: &mut Vec<u8>, donor: &[u8]) -> Result<(), String> {
    let grafts = PRESENTATION_BINDINGS
        .iter()
        .map(|binding| (*binding, donor))
        .collect::<Vec<_>>();
    graft_weapon_component_bindings_with(target, &grafts, EventPolicy::Retarget)
}

/// Whether this appearance's presentation can be promoted onto this runtime, decided by doing
/// it on a copy. Predicting it would mean restating every rule the graft already enforces.
pub(crate) fn presentation_graft_applies(target: &[u8], donor: &[u8]) -> bool {
    graft_presentation(&mut target.to_vec(), donor).is_ok()
}

/// The stat translator's per-type keys: the translation group hash at +0x10, then three values
/// of that type's own.
const TRANSLATOR_KEY_CLASS: u32 = 0x8080_38C7;
const TRANSLATOR_KEY_SIZE: usize = 0x28;
const TRANSLATOR_KEY_HASH: usize = 0x10;
/// Each type's conversion, in the keys' order: three arrays (translations, the routing entries
/// that write each output to a component input, and a third), one 16-byte descriptor each.
const TRANSLATOR_TABLE_CLASS: u32 = 0x8080_3975;
const TRANSLATOR_TABLE_SIZE: usize = 0x30;

/// Patches that make the stat translator convert stats as the base weapon's type when the
/// appearance's rig moves across.
///
/// The translator keeps one table per weapon type, and the authored pattern row names the
/// appearance's type once its rig moves, so a hand cannon wearing a sidearm's rig fired at sidearm
/// rates. The appearance type's table entry is pointed at the base type's arrays and takes the
/// base type's values. Every key keeps its hash and place, so however the client finds a key, it
/// still finds it. The patched translator is a private copy.
pub(crate) fn stat_table_patches(
    manager: &PackageManager,
    entity: &[u8],
    row_group: u32,
    own_group: u32,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    if row_group == own_group {
        return Ok(Vec::new());
    }
    let bindings =
        weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid("The weapon does not have one stat translator"));
    };
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let resource = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Stat translator offset overflow"))?;
    let keys = translator_array(&owner, TRANSLATOR_KEY_CLASS)?;
    let tables = translator_array(&owner, TRANSLATOR_TABLE_CLASS)?;
    if keys.0 != tables.0 {
        return Err(invalid(format!(
            "The stat translator has {} type keys but {} type tables",
            keys.0, tables.0
        )));
    }
    let key = |index: usize| keys.1 + index * TRANSLATOR_KEY_SIZE;
    let find = |group: u32| -> AuthoringResult<Option<usize>> {
        for index in 0..keys.0 {
            if read_u32(&owner, key(index) + TRANSLATOR_KEY_HASH)? == group {
                return Ok(Some(index));
            }
        }
        Ok(None)
    };
    // A type the translator has no table for was never converted by it, so there is nothing to
    // redirect.
    let Some(row) = find(row_group)? else {
        return Ok(Vec::new());
    };
    let own = find(own_group)?.ok_or_else(|| {
        invalid(format!(
            "The stat translator has no table for the base weapon's type 0x{own_group:08X}"
        ))
    })?;
    let relative = |at: usize| {
        at.checked_sub(resource)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| invalid("Stat translator patch offset overflow"))
    };
    let patch = |offset: u32, bytes: Vec<u8>| WeaponRuntimeResourcePatch {
        binding_hash: WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
        resource_index: 0,
        offset,
        bytes,
        graph_values: Vec::new(),
        graph_removals: Vec::new(),
        graph_trajectories: None,
    };
    let source = tables.1 + own * TRANSLATOR_TABLE_SIZE;
    let target = tables.1 + row * TRANSLATOR_TABLE_SIZE;
    let mut table = Vec::with_capacity(TRANSLATOR_TABLE_SIZE);
    for descriptor in (0..TRANSLATOR_TABLE_SIZE).step_by(16) {
        let (count, header, _, _) = sundial::package_authoring::native_payload::native_array_at(
            &owner,
            source + descriptor,
        )
        .map_err(|error| invalid(format!("Stat translator table: {error}")))?;
        let pointer = target + descriptor + 8;
        let offset = i64::try_from(header)
            .and_then(|header| i64::try_from(pointer).map(|pointer| header - pointer))
            .map_err(|_| invalid("Stat translator pointer overflow"))?;
        table.extend_from_slice(&(count as u64).to_le_bytes());
        table.extend_from_slice(&offset.to_le_bytes());
    }
    let values = owner
        .get(key(own) + TRANSLATOR_KEY_HASH + 4..key(own) + TRANSLATOR_KEY_SIZE)
        .ok_or_else(|| invalid("Stat translator key is truncated"))?
        .to_vec();
    Ok(vec![
        patch(relative(target)?, table),
        patch(relative(key(row) + TRANSLATOR_KEY_HASH + 4)?, values),
    ])
}

/// The one native array of `class` in the translator owner: its row count and first row.
fn translator_array(owner: &[u8], class: u32) -> AuthoringResult<(usize, usize)> {
    let mut found = None;
    for descriptor in (0..owner.len().saturating_sub(16)).step_by(8) {
        let Ok((count, rows, found_class)) =
            array_at(owner, descriptor).map(|(count, _, rows, class)| (count, rows, class))
        else {
            continue;
        };
        if found_class != class || read_u64(owner, descriptor)? == 0 {
            continue;
        }
        match found {
            Some((_, other)) if other != rows => {
                return Err(invalid(format!(
                    "The stat translator holds more than one array of class 0x{class:08X}"
                )));
            }
            _ => found = Some((count, rows)),
        }
    }
    found.ok_or_else(|| {
        invalid(format!(
            "The stat translator has no array of class 0x{class:08X}"
        ))
    })
}
