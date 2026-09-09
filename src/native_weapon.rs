//! Shared native weapon facts. No UI state, package writes, or authoring policy.

use crate::{
    investment_schema::*,
    package_payload::{bytes_at, i64_at, native_array_at, relative_offset, u16_at, u32_at},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Element {
    Arc,
    Solar,
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageFamily {
    Legacy,
    Modern,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseDamage {
    NoMarker,
    Fixed(DamageFamily, Element),
    Duplicate,
    Variable,
}

pub const fn fixed_damage_marker(index: u16) -> Option<(DamageFamily, Element)> {
    use DamageFamily::*;
    use Element::*;
    match index {
        LEGACY_ARC_DAMAGE_PERK_INDEX => Some((Legacy, Arc)),
        LEGACY_SOLAR_DAMAGE_PERK_INDEX => Some((Legacy, Solar)),
        LEGACY_VOID_DAMAGE_PERK_INDEX => Some((Legacy, Void)),
        MODERN_ARC_DAMAGE_PERK_INDEX => Some((Modern, Arc)),
        MODERN_SOLAR_DAMAGE_PERK_INDEX => Some((Modern, Solar)),
        MODERN_VOID_DAMAGE_PERK_INDEX => Some((Modern, Void)),
        _ => None,
    }
}

pub fn classify_base_damage(perks: &[u16]) -> BaseDamage {
    if [462, 463, 464].iter().all(|perk| perks.contains(perk)) {
        return BaseDamage::Variable;
    }
    classify_fixed_damage(perks)
}

/// Fixed carrier facts, independent of dynamic perk actions such as The Fundamentals.
/// Catalog policy may mark those actions variable; explicit authoring preserves them.
pub fn classify_fixed_damage(perks: &[u16]) -> BaseDamage {
    let markers = perks
        .iter()
        .filter_map(|perk| fixed_damage_marker(*perk))
        .collect::<Vec<_>>();
    match markers.as_slice() {
        [] => BaseDamage::NoMarker,
        [(family, element)] => BaseDamage::Fixed(*family, *element),
        values if values.windows(2).any(|pair| pair[0].1 != pair[1].1) => BaseDamage::Variable,
        _ => BaseDamage::Duplicate,
    }
}

pub fn base_sandbox_perks(data: &[u8]) -> Result<Vec<u16>, String> {
    let pointer = ITEM_INVESTMENT_STAT_POINTER_OFFSET;
    let resource = relative_offset(pointer, 0, i64_at(data, pointer)?)?;
    let marker = resource
        .checked_sub(4)
        .ok_or("Investment resource has no class marker")?;
    if u32_at(data, marker)? != ITEM_INVESTMENT_STAT_RESOURCE_CLASS {
        return Err("Item has no recognized investment resource".into());
    }
    let descriptor = resource
        .checked_add(ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET)
        .ok_or("Perk descriptor overflow")?;
    if bytes_at::<16>(data, descriptor)? == [0; 16] {
        return Ok(Vec::new());
    }
    let (count, _, rows, class) = native_array_at(data, descriptor)?;
    if class != ITEM_SANDBOX_PERK_ROW_CLASS || count > 64 {
        return Err("Unknown or oversized base sandbox-perk array".into());
    }
    (0..count)
        .map(|index| {
            let row = rows
                .checked_add(
                    index
                        .checked_mul(ITEM_SANDBOX_PERK_ROW_SIZE)
                        .ok_or("Perk row overflow")?,
                )
                .ok_or("Perk row overflow")?;
            bytes_at::<ITEM_SANDBOX_PERK_ROW_SIZE>(data, row)?;
            u16_at(data, row)
        })
        .collect()
}

/// Shared animation compatibility; unknown is not silently treated as compatible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationCompatibility {
    Compatible,
    DifferentGroups,
    Unchecked,
}

pub const fn animation_compatibility(
    base: Option<u32>,
    appearance: Option<u32>,
) -> AnimationCompatibility {
    match (base, appearance) {
        (Some(a), Some(b)) if a == b => AnimationCompatibility::Compatible,
        (Some(_), Some(_)) => AnimationCompatibility::DifferentGroups,
        _ => AnimationCompatibility::Unchecked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markers_ignore_unrelated_perks_but_preserve_ambiguity() {
        assert_eq!(
            classify_base_damage(&[84, 1048]),
            BaseDamage::Fixed(DamageFamily::Legacy, Element::Solar)
        );
        assert_eq!(classify_base_damage(&[84, 84]), BaseDamage::Duplicate);
        assert_eq!(
            classify_base_damage(&[84, LEGACY_ARC_DAMAGE_PERK_INDEX]),
            BaseDamage::Variable
        );
        assert_eq!(classify_base_damage(&[462, 463, 464]), BaseDamage::Variable);
        assert_eq!(classify_base_damage(&[1048]), BaseDamage::NoMarker);
    }
    #[test]
    fn unknown_animation_is_not_compatible() {
        assert_eq!(
            animation_compatibility(Some(1), Some(1)),
            AnimationCompatibility::Compatible
        );
        assert_eq!(
            animation_compatibility(Some(1), Some(2)),
            AnimationCompatibility::DifferentGroups
        );
        assert_eq!(
            animation_compatibility(None, Some(1)),
            AnimationCompatibility::Unchecked
        );
    }
}
