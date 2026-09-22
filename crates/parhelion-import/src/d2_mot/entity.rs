//! Resource pointers embedded in native model entities are (owner, class, offset)
//! triples in addition to the top-level component rows. They must move together.
use crate::d2_mot::payload::Payload;
use anyhow::{Result, ensure};
pub fn owner_slots(entity: &Payload, owner: &Payload, source: u32) -> Result<Vec<usize>> {
    let bindings = entity.array(16, 12, Some(0x80809C04))?;
    let roots = bindings
        .into_iter()
        .filter(|&o| entity.u32(o).ok() == Some(source))
        .collect::<Vec<_>>();
    ensure!(roots.len() == 1, "expected one primary model owner binding");
    let mut slots = vec![];
    for offset in (0..entity.0.len().saturating_sub(3)).step_by(4) {
        if entity.u32(offset)? != source {
            continue;
        }
        if !roots.contains(&offset) {
            let class = entity.u32(offset + 4)?;
            ensure!(
                class & 0xffff0000 == 0x80800000,
                "owner occurrence at {offset:X} is not a typed resource pointer"
            );
            let target = usize::try_from(entity.u64(offset + 8)?)?;
            ensure!(
                target
                    .checked_add(16)
                    .is_some_and(|end| end <= owner.0.len()),
                "resource pointer at {offset:X} exceeds its owner"
            );
        }
        slots.push(offset);
    }
    ensure!(
        slots.len() > roots.len(),
        "model entity lacks internal resource pointers"
    );
    Ok(slots)
}
pub fn reject_stale_owner(entity: &Payload, source: u32) -> Result<()> {
    for offset in (0..entity.0.len().saturating_sub(3)).step_by(4) {
        ensure!(
            entity.u32(offset)? != source,
            "stale donor owner reference at entity offset {offset:X}"
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Payload, Payload) {
        let mut b = vec![0u8; 192];
        b[16..24].copy_from_slice(&1u64.to_le_bytes());
        b[24..32].copy_from_slice(&56i64.to_le_bytes());
        b[80..88].copy_from_slice(&1u64.to_le_bytes());
        b[88..92].copy_from_slice(&0x80809C04u32.to_le_bytes());
        for o in [96, 128, 160] {
            b[o..o + 4].copy_from_slice(&0x80EC2727u32.to_le_bytes());
            if o != 96 {
                b[o + 4..o + 8].copy_from_slice(&0x808072B8u32.to_le_bytes());
                b[o + 8..o + 16].copy_from_slice(&16u64.to_le_bytes());
            }
        }
        (Payload(b), Payload(vec![0; 64]))
    }
    #[test]
    fn remaps_internal_references_as_well_as_component_row() {
        let (mut entity, owner) = fixture();
        let slots = owner_slots(&entity, &owner, 0x80EC2727).unwrap();
        assert_eq!(slots, vec![96, 128, 160]);
        entity.0[96..100].copy_from_slice(&0x81D4001Eu32.to_le_bytes());
        assert!(reject_stale_owner(&entity, 0x80EC2727).is_err());
        for slot in slots {
            entity.0[slot..slot + 4].copy_from_slice(&0x81D4001Eu32.to_le_bytes());
        }
        reject_stale_owner(&entity, 0x80EC2727).unwrap();
    }
    #[test]
    fn rejects_untyped_or_out_of_bounds_resource() {
        let (mut entity, owner) = fixture();
        entity.0[132..136].fill(0);
        assert!(owner_slots(&entity, &owner, 0x80EC2727).is_err());
        let (mut entity, owner) = fixture();
        entity.0[136..144].copy_from_slice(&64u64.to_le_bytes());
        assert!(owner_slots(&entity, &owner, 0x80EC2727).is_err());
    }
}
