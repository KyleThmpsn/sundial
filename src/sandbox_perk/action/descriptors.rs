//! Native callbacks that refer back to the action record that owns their settings.
use super::*;

/// A checked self-reference to relocate when an action receives a private package tag.
pub struct SelfReference {
    pub reference_offset: usize,
    pub target_offset: usize,
}

/// Read relocation sites without allocating tags or modifying package data.
/// Do not infer self-references merely because a resource happens to share an owner tag.
pub fn self_references(payload: &[u8]) -> Result<Vec<SelfReference>, String> {
    let decoded = decode(payload)?;
    let mut references = Vec::new();
    for effect in decoded.effects() {
        // All 43 stock kind-33 descriptors point to their own record. CD3020
        // stores the descriptor and CD3570 calls 1088E80 through this reference.
        // Its type remains fixed, but owner and offset change when authored.
        if effect.class != 0x8080_3E3C {
            continue;
        }
        let at = effect.offset;
        if u64_at(payload, at + 0xA0)? != u64::from(effect.class)
            || u32_at(payload, at + 0xAC)? != effect.class
        {
            return Err("Incoming damage callback has an incompatible native descriptor.".into());
        }
        references.push(SelfReference {
            reference_offset: at + 0xA8,
            target_offset: at,
        });
    }
    Ok(references)
}
