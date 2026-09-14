//! Derived routing and evaluation order for authored native condition trees.
use crate::sandbox_perk::action::{self, DecodedCondition};

pub(super) fn rebuild(bytes: &mut [u8]) -> Result<(), String> {
    let decoded = action::decode(bytes)?;
    let mut ordinal = 0usize;
    let additional = if decoded.groups.len() > 1 {
        let (count, _, rows, class) = crate::package_payload::native_array_at(bytes, 0xA8)?;
        if count != decoded.groups.len() - 1 || class != 0x8080407B {
            return Err("Additional groups need matching compiled routing records.".into());
        }
        rows
    } else {
        0
    };
    for (group_index, group) in decoded.groups.iter().enumerate() {
        let mut masks = [0u64; 3];
        for (phase, conditions) in [&group.activation, &group.removal, &group.rearm]
            .into_iter()
            .enumerate()
        {
            for condition in conditions.iter().rev() {
                masks[phase] |= condition_metadata(bytes, condition, Some(&mut ordinal))?;
            }
            if conditions.is_empty() && (phase == 2 || (phase == 0 && group_index == 0)) {
                masks[phase] = 1;
            }
        }
        let mut extensions = 0u64;
        for (index, effect) in group.effects.iter().enumerate() {
            if effect.kind == 32 {
                extensions |= 1u64
                    .checked_shl(index as u32)
                    .ok_or("Too many effects for timer-extension routing.")?;
                let mut mask = 0;
                for condition in &effect.conditions {
                    mask |= condition_metadata(bytes, condition, None)?;
                }
                bytes[effect.offset + 0x20..effect.offset + 0x28]
                    .copy_from_slice(&mask.to_le_bytes());
            }
        }
        for (index, mask) in masks.into_iter().chain([extensions]).enumerate() {
            let at = if group_index == 0 {
                0x88
            } else {
                additional + (group_index - 1) * 32
            } + index * 8;
            bytes[at..at + 8].copy_from_slice(&mask.to_le_bytes());
        }
    }
    Ok(())
}

fn condition_metadata(
    bytes: &mut [u8],
    condition: &DecodedCondition,
    mut ordinal: Option<&mut usize>,
) -> Result<u64, String> {
    let at = condition.offset;
    bytes[at + 7] = if let Some(next) = ordinal.as_deref_mut() {
        let value = u8::try_from(*next)
            .ok()
            .filter(|n| *n != 255)
            .ok_or("Too many numbered conditions.")?;
        *next += 1;
        value
    } else {
        255
    };
    bytes[at + 6] = u8::from(match condition.kind {
        4 => crate::package_payload::i64_at(bytes, at + 0xC8)? != 0,
        20 | 35 => crate::package_payload::u32_at(bytes, at + 0xD4)? != super::EMPTY_KEY,
        _ => false,
    });
    let mut mask = 1u64
        .checked_shl(condition.kind.into())
        .ok_or("Condition kind exceeds the event mask.")?;
    for child in condition.children.iter().rev() {
        mask |= condition_metadata(bytes, child, ordinal.as_deref_mut())?;
    }
    for subgroup in &condition.subgroups {
        for child in subgroup.conditions.iter().rev() {
            mask |= condition_metadata(bytes, child, ordinal.as_deref_mut())?;
        }
    }
    Ok(mask)
}
