//! Read-only Shadowkeep dye colors and material properties.
use crate::package_runtime::reader::PackageManager;
use crate::{
    package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag},
    package_payload::{bytes_at, native_array_at},
    weapon_entity::{
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        weapon_entity_assignment,
    },
};
use std::{collections::BTreeMap, path::Path};
use tiger_pkg::TagHash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponDyeColors {
    /// Linear RGB albedo tints, before textures, lighting, wear and shader overrides.
    pub primary: [f32; 3],
    pub secondary: [f32; 3],
}

fn word(data: &[u8], offset: usize) -> Result<u32, String> {
    bytes_at(data, offset).map(u32::from_le_bytes)
}

fn typed(manager: &PackageManager, tag: u32, class: u32) -> Result<Vec<u8>, String> {
    let tag = TagHash(tag);
    if manager
        .get_entry(tag)
        .is_none_or(|entry| entry.reference != class)
    {
        return Err(format!("Dye reference {tag} is not class {class:08X}"));
    }
    manager.read_tag(tag).map_err(|error| error.to_string())
}

/// Loads only the requested reference rows. Call off the UI thread and drop the reader before installation.
pub fn load_weapon_dye_colors(
    packages: &Path,
    indices: &[u16],
) -> Result<BTreeMap<u16, Result<WeaponDyeColors, String>>, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    load_with_manager(&manager, indices)
}

pub(crate) fn load_with_manager(
    manager: &PackageManager,
    indices: &[u16],
) -> Result<BTreeMap<u16, Result<WeaponDyeColors, String>>, String> {
    read_dyes(manager, indices, |constants, _| decode_colors(constants))
}

fn read_dyes<T>(
    manager: &PackageManager,
    indices: &[u16],
    decode: impl Fn(&[u8], &[u8]) -> Result<T, String>,
) -> Result<BTreeMap<u16, Result<T, String>>, String> {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None)?)
        .map_err(|error| error.to_string())?;
    let dyes = typed(manager, word(&globals, 16 + 67 * 16)?, 0x8080_5DE8)?;
    let (count, _, rows, class) = native_array_at(&dyes, 8)?;
    if class != 0x8080_5DEC || count > dyes.len().saturating_sub(rows) / 8 {
        return Err("Unsupported art-dye reference array".into());
    }
    let assignments = typed(
        manager,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS,
    )?;
    Ok(indices
        .iter()
        .copied()
        .map(|index| {
            let result = (|| {
                if usize::from(index) >= count {
                    return Err("Dye reference is outside the installed table".into());
                }
                let manifest = word(&dyes, rows + usize::from(index) * 8 + 4)?;
                let relation_tag = weapon_entity_assignment(&assignments, manifest)?
                    .ok_or("Dye has no sandbox assignment")?;
                let relation = typed(manager, relation_tag, 0x8080_744A)?;
                let dye = typed(manager, word(&relation, 0x10)?, 0x8080_71CD)?;
                let scope = typed(manager, word(&dye, 0x0C)?, 0x8080_71F3)?;
                // Animated scopes initialize their buffer from inline vectors. The
                // external allocation can contain only zeros until the game binds it.
                if let Some(constants) = inline_constants(&scope)? {
                    return decode(constants, &scope);
                }
                let header = TagHash(word(&scope, 0xBC)?);
                let entry = manager
                    .get_entry(header)
                    .ok_or("Dye has no constant buffer")?;
                let constants = manager
                    .read_tag(TagHash(entry.reference))
                    .map_err(|error| error.to_string())?;
                decode(&constants, &scope)
            })();
            (index, result)
        })
        .collect())
}

pub(crate) mod material;

fn inline_constants(scope: &[u8]) -> Result<Option<&[u8]>, String> {
    if u64::from_le_bytes(bytes_at(scope, 0x88)?) == 0 {
        return Ok(None);
    }
    let (count, _, rows, class) = native_array_at(scope, 0x88)?;
    if count != 27 || class != 0x8080_0090 {
        return Err("Unsupported inline dye material buffer layout".into());
    }
    scope
        .get(rows..rows + 27 * 16)
        .map(Some)
        .ok_or_else(|| "Truncated inline dye material buffer".into())
}

fn decode_colors(constants: &[u8]) -> Result<WeaponDyeColors, String> {
    // Bungie's 2019 Gear Dye Material Properties layout: 27 vec4s, albedo at 9/13.
    // https://github.com/Bungie-net/api/wiki/3D-Content-Documentation
    // Modern dye layouts place these elsewhere and must not be silently decoded here.
    if constants.len() != 27 * 16 {
        return Err("Unsupported dye material buffer layout".into());
    }
    let color = |vector: usize| -> Result<[f32; 3], String> {
        let mut rgb = [0.0; 3];
        for (channel, value) in rgb.iter_mut().enumerate() {
            *value = f32::from_le_bytes(bytes_at(constants, vector * 16 + channel * 4)?);
            if !value.is_finite() || *value < 0.0 {
                return Err("Invalid dye albedo tint".into());
            }
        }
        Ok(rgb)
    };
    Ok(WeaponDyeColors {
        primary: color(9)?,
        secondary: color(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_material_requires_complete_native_vectors() {
        let mut scope = vec![0; 0x100 + 27 * 16];
        scope[0x88..0x90].copy_from_slice(&27_u64.to_le_bytes());
        scope[0x90..0x98].copy_from_slice(&0x60_u64.to_le_bytes());
        scope[0xF0..0xF8].copy_from_slice(&27_u64.to_le_bytes());
        scope[0xF8..0xFC].copy_from_slice(&0x8080_0090_u32.to_le_bytes());
        assert_eq!(inline_constants(&scope).unwrap().unwrap().len(), 27 * 16);
        assert!(inline_constants(&scope[..scope.len() - 1]).is_err());
        scope[0x88..0x90].fill(0);
        assert!(inline_constants(&scope).unwrap().is_none());
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_INSTALL pointing to the supported Shadowkeep build"]
    fn native_weapon_dye_colors() {
        let packages = std::path::PathBuf::from(std::env::var("SUNDIAL_TEST_INSTALL").unwrap())
            .join("packages");
        let colors =
            load_weapon_dye_colors(&packages, &[7656, 7657, 12464, 7098, u16::MAX]).unwrap();
        assert_eq!(
            colors[&7656].as_ref().unwrap().primary,
            [0.50888133, 0.43415368, 0.31398875]
        );
        assert_eq!(
            colors[&7657].as_ref().unwrap().secondary,
            [0.026290512, 0.027513452, 0.037901044]
        );
        assert_eq!(colors[&12464].as_ref().unwrap().primary, [0.07132241; 3]);
        assert_eq!(
            colors[&7098].as_ref().unwrap().primary,
            [0.024555832, 0.04699264, 0.056698017]
        );
        assert!(colors[&u16::MAX].is_err());
    }
    #[test]
    fn shadowkeep_dyes_use_albedo_not_emissive_or_roughness_vectors() {
        let mut bytes = vec![0; 27 * 16];
        for (vector, rgb) in [(9, [0.1_f32, 0.2, 0.3]), (13, [0.4, 0.5, 0.6])] {
            for (channel, value) in rgb.into_iter().enumerate() {
                let offset = vector * 16 + channel * 4;
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        assert_eq!(
            decode_colors(&bytes).unwrap(),
            WeaponDyeColors {
                primary: [0.1, 0.2, 0.3],
                secondary: [0.4, 0.5, 0.6]
            }
        );
        assert!(decode_colors(&bytes[..21 * 16]).is_err());
        bytes[9 * 16..9 * 16 + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode_colors(&bytes).is_err());
    }
}
