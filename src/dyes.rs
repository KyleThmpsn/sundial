//! Read-only Shadowkeep dye colors and material properties.
use crate::package_runtime::reader::PackageManager;
use crate::{
    entity::{
        SANDBOX_PATTERN_ENTITY_ASSIGNMENT_CLASS, SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG,
        weapon_entity_assignment,
    },
    package_authoring::{open_shadowkeep_package_manager, resolve_live_named_tag},
    package_payload::{bytes_at, native_array_at},
};
use std::{collections::BTreeMap, path::Path};
use tiger_pkg::TagHash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponDyeColors {
    /// Linear RGB albedo tints, before textures, lighting, wear and shader overrides.
    pub primary: [f32; 3],
    pub secondary: [f32; 3],
    /// Each surface's row of the global iridescence lookup, or -1 for none.
    pub iridescence: [f32; 2],
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
pub(crate) mod source;
pub use source::{DyeSource, DyeTextureSource, decode_source_material};

/// A dye surface's finish, painted or worn, as the gear dye material describes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DyeFinish {
    /// Linear color.
    pub albedo: [f32; 3],
    /// Detail color, detail normal and detail smoothness strengths, then metalness.
    pub params: [f32; 4],
    /// The smoothness remap: offset, scale, minimum and range.
    pub smoothness: [f32; 4],
}

/// One of a dye's two surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DyeSurfaceMaterial {
    pub paint: DyeFinish,
    /// What wear exposes.
    pub worn: DyeFinish,
    /// How the gear's wear mask becomes this surface's wear: offset, scale, minimum and range.
    pub wear: [f32; 4],
    /// Linear color the surface glows with where its gear allows.
    pub emissive: [f32; 3],
    /// The surface's row of the global iridescence lookup, or -1 for none.
    pub iridescence: f32,
}

/// A texture's RGBA8 pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DyeTexture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// A dye's material: its primary and secondary surfaces and the detail textures they share. The
/// detail color texture is sRGB with smoothness in alpha, and the detail normal texture is linear
/// with the normal in red and green and occlusion in blue. Each is at most
/// [`DYE_TEXTURE_EDGE`] pixels a side, and missing when the dye binds none or it cannot be read.
#[derive(Clone, Debug, PartialEq)]
pub struct DyeMaterial {
    pub surfaces: [DyeSurfaceMaterial; 2],
    /// The 27 vectors the surfaces are read from, which an editor writes into.
    pub vectors: [[f32; 4]; 27],
    /// The detail color and detail normal textures' scale (x, y) and offset (z, w).
    pub detail_transform: [f32; 4],
    pub normal_transform: [f32; 4],
    /// The detail textures' tags, and their pixels.
    pub detail_tag: Option<u32>,
    pub normal_tag: Option<u32>,
    pub detail: Option<DyeTexture>,
    pub normal: Option<DyeTexture>,
}

/// The two surfaces a dye's 27 vectors describe, as [`load_dye_materials`] reads them.
#[must_use]
pub fn dye_surfaces(vectors: &[[f32; 4]; 27]) -> [DyeSurfaceMaterial; 2] {
    material::properties(vectors).surfaces.map(surface_material)
}

fn surface_material(surface: material::Surface) -> DyeSurfaceMaterial {
    let finish = |albedo, params, smoothness| DyeFinish {
        albedo,
        params,
        smoothness,
    };
    DyeSurfaceMaterial {
        paint: finish(surface.albedo, surface.params, surface.roughness),
        worn: finish(
            surface.worn_albedo,
            surface.worn_params,
            surface.worn_roughness,
        ),
        wear: surface.wear,
        emissive: surface.emissive,
        iridescence: surface.iridescence,
    }
}

/// The largest side of a [`DyeMaterial`] texture.
pub const DYE_TEXTURE_EDGE: usize = 256;

/// The materials of the requested dye rows, read as the model preview reads them. Call off the UI
/// thread and drop the result's reader before installation.
pub fn load_dye_materials(
    packages: &Path,
    indices: &[u16],
) -> Result<BTreeMap<u16, Result<DyeMaterial, String>>, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let texture = |tag: Option<u32>| {
        crate::model_preview::texture::load(&manager, tag?)
            .ok()
            .map(|texture| shrink(texture, DYE_TEXTURE_EDGE))
    };
    Ok(material::load(&manager, indices)?
        .into_iter()
        .map(|(index, material)| {
            let material = material.map(|material| {
                let detail_tag = material.detail.clone().ok().flatten();
                let normal_tag = material.normal.clone().ok().flatten();
                DyeMaterial {
                    surfaces: material.surfaces.map(surface_material),
                    vectors: material.vectors,
                    detail_transform: material.detail_transform,
                    normal_transform: material.normal_transform,
                    detail_tag,
                    normal_tag,
                    detail: texture(detail_tag),
                    normal: texture(normal_tag),
                }
            });
            (index, material)
        })
        .collect())
}

/// Detail textures by tag, each at most [`DYE_TEXTURE_EDGE`] pixels a side, for showing another
/// dye's textures on an unbuilt one. Call off the UI thread.
pub fn load_dye_textures(
    packages: &Path,
    tags: &[u32],
) -> Result<BTreeMap<u32, Result<DyeTexture, String>>, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    Ok(tags
        .iter()
        .map(|&tag| {
            let texture = crate::model_preview::texture::load(&manager, tag)
                .map(|texture| shrink(texture, DYE_TEXTURE_EDGE));
            (tag, texture)
        })
        .collect())
}

/// Halves a texture, averaging each two by two block, until it fits `edge` pixels a side.
fn shrink(texture: crate::model_preview::texture::Texture, edge: usize) -> DyeTexture {
    let [mut width, mut height] = texture.size;
    let mut rgba = texture.rgba;
    while width > edge || height > edge {
        let (half_width, half_height) = ((width / 2).max(1), (height / 2).max(1));
        let mut half = vec![0; half_width * half_height * 4];
        for y in 0..half_height {
            for x in 0..half_width {
                for channel in 0..4 {
                    let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .into_iter()
                        .map(|(dx, dy)| {
                            let column = (x * 2 + dx).min(width - 1);
                            let row = (y * 2 + dy).min(height - 1);
                            u32::from(rgba[(row * width + column) * 4 + channel])
                        })
                        .sum();
                    half[(y * half_width + x) * 4 + channel] = ((sum + 2) / 4) as u8;
                }
            }
        }
        (width, height, rgba) = (half_width, half_height, half);
    }
    DyeTexture {
        width,
        height,
        rgba,
    }
}

/// One authored row of the game's iridescence lookup: its id and its colors across the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IridescenceRow {
    pub id: i16,
    pub colors: Vec<[u8; 3]>,
}

/// The rows of the game's iridescence lookup that hold authored colors, for choosing one by
/// eye. Rows past the authored ones hold a magenta placeholder the game skips, and so does this.
/// Call off the UI thread.
pub fn load_iridescence_rows(packages: &Path) -> Result<Vec<IridescenceRow>, String> {
    const SAMPLES: usize = 16;
    let manager = open_shadowkeep_package_manager(packages)?;
    let texture = crate::model_preview::texture::iridescence(&manager)
        .ok_or("The iridescence lookup is missing")?;
    let [width, height] = texture.size;
    if width == 0 || texture.rgba.len() != width * height * 4 {
        return Err("The iridescence lookup has an unexpected size".into());
    }
    let mut rows = Vec::new();
    for row in 0..height.min(usize::from(i16::MAX as u16)) {
        let colors = (0..SAMPLES)
            .map(|sample| {
                let x = (sample * (width - 1)) / (SAMPLES - 1);
                let at = (row * width + x) * 4;
                [texture.rgba[at], texture.rgba[at + 1], texture.rgba[at + 2]]
            })
            .collect::<Vec<_>>();
        let placeholder = colors
            .iter()
            .all(|[red, green, blue]| *red > 200 && *green < 60 && *blue > 200);
        if !placeholder {
            rows.push(IridescenceRow {
                id: i16::try_from(row).map_err(|_| "Iridescence row overflow")?,
                colors,
            });
        }
    }
    Ok(rows)
}

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
    let iridescence = |vector: usize| -> Result<f32, String> {
        let value = f32::from_le_bytes(bytes_at(constants, vector * 16)?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err("Invalid dye iridescence".into())
        }
    };
    Ok(WeaponDyeColors {
        primary: color(9)?,
        secondary: color(13)?,
        iridescence: [iridescence(11)?, iridescence(15)?],
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
                secondary: [0.4, 0.5, 0.6],
                iridescence: [0.0, 0.0],
            }
        );
        assert!(decode_colors(&bytes[..21 * 16]).is_err());
        bytes[9 * 16..9 * 16 + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode_colors(&bytes).is_err());
    }
}
