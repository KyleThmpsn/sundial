//! Perk conditions that name one ability. On a Specific Ability (class `80803DFF`) and Ends on a
//! Specific Ability (class `80803DFE`) require the event's ability to equal the key stored at
//! +0x10, the tag of a stock ability's entity (Solar Grenade `80B80888`, Golden Gun `80BAA30C`).
//! An ability copied to an entity of its own is named by the copy's tag, so a perk that must
//! follow the copy has its key moved.
use crate::package_payload::{u32_at, write_bytes};
use crate::sandbox_perk::action;

/// The condition classes that store an ability's entity, and where they store it.
const CONDITION_CLASSES: [u32; 2] = [0x8080_3DFF, 0x8080_3DFE];
const KEY: usize = 0x10;

/// Every ability key in an action's conditions, nested ones included: its payload offset and
/// the entity it names.
pub fn references(payload: &[u8]) -> Result<Vec<(usize, u32)>, String> {
    action::decode(payload)?
        .conditions()
        .into_iter()
        .filter(|condition| CONDITION_CLASSES.contains(&condition.class))
        .map(|condition| {
            let at = condition
                .offset
                .checked_add(KEY)
                .ok_or("Ability key offset overflowed")?;
            Ok((at, u32_at(payload, at)?))
        })
        .collect()
}

/// Moves each ability key that names a `from` entity in `moves` to its `to` entity. Returns how
/// many keys moved.
pub fn retarget(payload: &mut [u8], moves: &[(u32, u32)]) -> Result<usize, String> {
    let mut moved = 0;
    for (at, key) in references(payload)? {
        if let Some(&(_, to)) = moves.iter().find(|(from, _)| *from == key) {
            write_bytes(payload, at, &to.to_le_bytes())?;
            moved += 1;
        }
    }
    Ok(moved)
}
