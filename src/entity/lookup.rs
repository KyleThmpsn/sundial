//! The native 80809C23 interface table, including its collision continuation bits.
use super::*;

const EMPTY: u32 = u32::MAX;
const CONTINUE: u32 = 0x8000_0000;

// Client 9EBF80 reads this exact wrapping mix before probing adjacent slots.
fn slot(key: u32, seed: u32, capacity: usize) -> usize {
    let mixed = (seed ^ key).wrapping_add(key.wrapping_shl(4));
    let mixed = (mixed ^ (mixed >> 10)).wrapping_mul(0x81);
    ((mixed ^ (mixed >> 13)) as usize) & (capacity - 1)
}

/// Construct a table from resource spans. Continuation belongs to slot placement,
/// not to an interface's callbacks, so it is always calculated afresh.
pub(super) fn build(
    seed: u32,
    definitions: &BTreeMap<u32, Vec<u8>>,
) -> Result<Vec<Vec<u8>>, String> {
    let capacity = definitions
        .len()
        .checked_mul(2)
        .and_then(|count| count.max(2).checked_next_power_of_two())
        .ok_or("Component lookup capacity overflowed")?;
    if capacity > 0x1_0000 {
        return Err("Component lookup exceeds the supported capacity".into());
    }
    let mut slots = vec![[EMPTY.to_le_bytes(), 0u32.to_le_bytes()].concat(); capacity];
    for (&key, definition) in definitions {
        if key == EMPTY || definition.len() != 8 || read_u32(definition, 0)? != key {
            return Err("Component lookup has an invalid key or row".into());
        }
        let encoded = read_u32(definition, 4)? & !CONTINUE;
        let start = encoded & 0xFFFF;
        let count = encoded >> 16;
        // The native iterator sign-extends the start and end as 16-bit indexes.
        if count == 0 || start + count > i16::MAX as u32 {
            return Err("Component lookup exceeds the native iterator range".into());
        }
        let mut index = slot(key, seed, capacity);
        while read_u32(&slots[index], 0)? != EMPTY {
            let value = read_u32(&slots[index], 4)? | CONTINUE;
            write_u32(&mut slots[index], 4, value)?;
            index = (index + 1) & (capacity - 1);
        }
        slots[index] = definition.clone();
        write_u32(&mut slots[index], 4, encoded)?;
    }
    Ok(slots)
}
