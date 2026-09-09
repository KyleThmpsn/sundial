//! Native ammo override in weapon-content properties, including perk-selected variants.
//!
//! Shadowkeep C9D8C0 selects definition +80 or a 1C0-byte row from +240/+248.
//! E8F660 reads that property's +34 enable flag and +35 zero-based ammo class.
use sundial::package_authoring::weapon_entity::weapon_component_bindings;
use tiger_pkg::{PackageManager, TagHash};

use crate::AuthoringResult;
use crate::error::invalid;
use crate::weapon::{WeaponAmmoType, WeaponRuntimeResourcePatch};

const CONTENT_BINDING: u32 = 0x5F0D_D954;
const CONTENT_INSTANCE: u32 = 0x8080_3ACB;
const CONTENT_DEFINITION: u32 = 0x8080_3AC9;
const PROPERTY_CLASS: u32 = 0x8080_3ACF;
const DEFINITION_SIZE: usize = 0x270;
const PROPERTY_SIZE: usize = 0x1C0;

fn bytes<const N: usize>(data: &[u8], offset: usize) -> AuthoringResult<[u8; N]> {
    offset
        .checked_add(N)
        .and_then(|end| data.get(offset..end))
        .and_then(|value| value.try_into().ok())
        .ok_or_else(|| invalid("Native ammo properties exceed their component owner"))
}

pub(crate) fn property_offsets(data: &[u8], definition: usize) -> AuthoringResult<Vec<usize>> {
    let end = definition
        .checked_add(DEFINITION_SIZE)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| invalid("Native ammo definition is truncated"))?;
    let mut properties = vec![definition + 0x80];
    let count = u64::from_le_bytes(bytes(data, definition + 0x240)?);
    let relative = i64::from_le_bytes(bytes(data, definition + 0x248)?);
    if count > 4096 {
        return Err(invalid(
            "Native ammo variant count exceeds the supported bound",
        ));
    }
    if count != 0 {
        let header = (definition + 0x248)
            .checked_add_signed(
                isize::try_from(relative)
                    .map_err(|_| invalid("Native ammo variant pointer overflow"))?,
            )
            .ok_or_else(|| invalid("Native ammo variant pointer overflow"))?;
        let table_end = header
            .checked_add(16)
            .and_then(|start| start.checked_add(count as usize * PROPERTY_SIZE))
            .filter(|table_end| *table_end <= data.len())
            .ok_or_else(|| invalid("Native ammo variant array is truncated"))?;
        if header < end && table_end > definition {
            return Err(invalid("Native ammo variant array overlaps its definition"));
        }
        if u64::from_le_bytes(bytes(data, header)?) != count
            || u32::from_le_bytes(bytes(data, header + 8)?) != PROPERTY_CLASS
        {
            return Err(invalid(
                "Native ammo variant count/class does not match its descriptor",
            ));
        }
        properties.extend((0..count as usize).map(|index| header + 16 + index * PROPERTY_SIZE));
    }
    for &property in &properties {
        let [enabled, kind] = bytes(data, property + 0x34)?;
        if enabled > 1 || (enabled == 1 && kind > 2) {
            return Err(invalid(
                "Native ammo properties contain an unsupported override flag/class",
            ));
        }
    }
    Ok(properties)
}

/// Resolve after component grafts. Normal owner cloning/retargeting applies these edits privately.
pub(super) fn patches(
    manager: &PackageManager,
    entity: &[u8],
    ammo: WeaponAmmoType,
) -> AuthoringResult<Vec<WeaponRuntimeResourcePatch>> {
    let bindings = weapon_component_bindings(entity, CONTENT_BINDING).map_err(invalid)?;
    let [binding] = bindings.as_slice() else {
        return Err(invalid(
            "Native ammo authoring requires one weapon-content component",
        ));
    };
    if binding.concrete_class != CONTENT_INSTANCE {
        return Err(invalid(
            "Native ammo authoring encountered an unsupported weapon-content class",
        ));
    }
    let owner = manager
        .read_tag(TagHash(binding.owner_tag))
        .map_err(|error| invalid(error.to_string()))?;
    let instance = usize::try_from(binding.resource_offset)
        .map_err(|_| invalid("Native ammo component offset overflow"))?;
    instance
        .checked_add(0xA0)
        .filter(|end| *end <= owner.len())
        .ok_or_else(|| invalid("Native ammo component is truncated"))?;
    if u32::from_le_bytes(bytes(&owner, instance)?) != binding.owner_tag
        || u32::from_le_bytes(bytes(&owner, instance + 4)?) != CONTENT_DEFINITION
    {
        return Err(invalid(
            "Native ammo definition must be a typed reference within the same owner",
        ));
    }
    let definition = usize::try_from(u64::from_le_bytes(bytes(&owner, instance + 8)?))
        .map_err(|_| invalid("Native ammo definition offset overflow"))?;
    let replacement = [1, ammo as u8 - 1];
    property_offsets(&owner, definition)?
        .into_iter()
        .filter_map(|property| {
            let offset = property + 0x34;
            // Preserve owners already using the requested class byte-for-byte.
            (owner[offset..offset + 2] != replacement).then_some(offset)
        })
        .map(|offset| {
            let relative = offset
                .checked_sub(instance)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    invalid("Native ammo field cannot be addressed from its component")
                })?;
            Ok(WeaponRuntimeResourcePatch {
                binding_hash: CONTENT_BINDING,
                resource_index: 0,
                offset: relative,
                bytes: replacement.to_vec(),
                graph_values: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut data = vec![0; 0x300 + 16 + 2 * PROPERTY_SIZE];
        data[0x240..0x248].copy_from_slice(&2_u64.to_le_bytes());
        data[0x248..0x250].copy_from_slice(&0xB8_i64.to_le_bytes());
        data[0x300..0x308].copy_from_slice(&2_u64.to_le_bytes());
        data[0x308..0x30C].copy_from_slice(&PROPERTY_CLASS.to_le_bytes());
        data
    }

    #[test]
    fn includes_default_and_every_selected_variant() {
        assert_eq!(
            property_offsets(&fixture(), 0).unwrap(),
            vec![0x80, 0x310, 0x4D0]
        );
    }

    #[test]
    fn supports_a_default_without_variants() {
        let mut data = vec![0; DEFINITION_SIZE];
        assert_eq!(property_offsets(&data, 0).unwrap(), vec![0x80]);
        // Native selection never dereferences the array pointer when the count is zero.
        data[0x248..0x250].copy_from_slice(&i64::MIN.to_le_bytes());
        assert_eq!(property_offsets(&data, 0).unwrap(), vec![0x80]);
    }

    #[test]
    fn rejects_bad_count_class_pointer_and_truncation() {
        for (offset, replacement) in [(0x300, 3_u8), (0x308, 0_u8), (0x24F, 0x7F_u8)] {
            let mut data = fixture();
            data[offset] = replacement;
            assert!(property_offsets(&data, 0).is_err());
        }
        let data = fixture();
        assert!(property_offsets(&data[..data.len() - 1], 0).is_err());
        assert!(property_offsets(&data[..0x100], 0).is_err());
        assert!(property_offsets(&data, usize::MAX).is_err());
    }

    #[test]
    fn rejects_unsupported_enabled_enum_but_accepts_inactive_sentinel() {
        let mut data = fixture();
        data[0xB4..0xB6].copy_from_slice(&[0, 255]);
        assert!(property_offsets(&data, 0).is_ok());
        data[0xB4] = 1;
        assert!(property_offsets(&data, 0).is_err());
        data[0xB4..0xB6].copy_from_slice(&[2, 0]);
        assert!(property_offsets(&data, 0).is_err());
        data[0xB4..0xB6].copy_from_slice(&[1, 1]);
        data[0x504..0x506].copy_from_slice(&[1, 3]);
        assert!(property_offsets(&data, 0).is_err());
    }

    #[test]
    fn rejects_array_aliasing_the_definition() {
        let mut data = fixture();
        data[0x248..0x250].copy_from_slice(&(-0x48_i64).to_le_bytes());
        assert!(property_offsets(&data, 0).is_err());
    }
}
