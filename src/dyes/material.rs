//! The 27-vector Shadowkeep layout, not the later 21-vector dye layout.
use super::*;
pub(crate) mod program;
#[cfg(test)]
pub(crate) mod verification;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Surface {
    pub albedo: [f32; 3],
    pub worn_albedo: [f32; 3],
    pub params: [f32; 4],
    pub worn_params: [f32; 4],
    pub roughness: [f32; 4],
    pub worn_roughness: [f32; 4],
    pub wear: [f32; 4],
    pub emissive: [f32; 3],
    /// Row of the global iridescence lookup, or -1 for none. Even rows tint the colour,
    /// odd rows tint the highlight.
    pub iridescence: f32,
}

pub(crate) struct Material {
    pub surfaces: [Surface; 2],
    pub detail_transform: [f32; 4],
    pub detail: Result<Option<u32>, String>,
    pub normal: Result<Option<u32>, String>,
    pub normal_transform: [f32; 4],
    pub animation: Result<Option<Animation>, String>,
    pub(crate) vectors: [[f32; 4]; 27],
}

#[derive(Clone)]
pub(crate) struct Animation {
    program: program::Program,
    base: [[f32; 4]; 27],
    source: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub vectors: [[f32; 4]; 27],
    pub surfaces: [Surface; 2],
    pub detail_transform: [f32; 4],
    pub normal_transform: [f32; 4],
}
impl Animation {
    /// Evaluate the frame and apply authoring in the same order as native emission. Source
    /// shader edits mask their owned lanes after expression stores. Stock dyes keep their order.
    pub fn at_edited(
        &self,
        seconds: f32,
        edit: impl FnOnce(&mut [[f32; 4]; 27]),
    ) -> Result<Frame, String> {
        let mut frame = self.base;
        if self.source {
            frame = self.program.run(frame, seconds)?;
            edit(&mut frame);
        } else {
            edit(&mut frame);
            frame = self.program.run(frame, seconds)?;
        }
        Ok(properties(&frame))
    }
}

pub(super) fn source_animation(
    scope: &[u8],
    channels: &[[f32; 4]],
    base: [[f32; 4]; 27],
) -> Result<Option<Animation>, String> {
    program::Program::read(scope, channels).map(|program| {
        program.map(|program| Animation {
            program,
            base,
            source: true,
        })
    })
}

/// Writes an editor's values into a dye's vectors, where a build writes them: a vector, its lane
/// and the value. Writes outside the 27 vectors are ignored.
pub(crate) fn apply_writes(vectors: &mut [[f32; 4]; 27], writes: &[(usize, usize, f32)]) {
    for &(vector, lane, value) in writes {
        if let Some(slot) = vectors
            .get_mut(vector)
            .and_then(|vector| vector.get_mut(lane))
        {
            *slot = value;
        }
    }
}

pub(crate) fn load(
    manager: &PackageManager,
    indices: &[u16],
) -> Result<BTreeMap<u16, Result<Material, String>>, String> {
    let channels = global_channels(manager);
    read_dyes(manager, indices, |constants, scope| {
        let mut material = decode(constants)?;
        (material.detail, material.normal) = detail_textures(scope);
        material.animation = program::Program::read(scope, &channels).map(|p| {
            p.map(|program| Animation {
                program,
                base: material.vectors,
                source: false,
            })
        });
        Ok(material)
    })
}

/// Each global channel's default value, from the render globals' channel table (class
/// 0x8080858D: channel ids at +0x08, default values at +0x18). A dye program that reads a channel
/// sees what the game holds when nothing drives it. Empty when the table cannot be read.
pub(crate) fn global_channels(manager: &PackageManager) -> Vec<[f32; 4]> {
    let Some((tag, _)) = manager.get_all_by_reference(0x8080_858D).into_iter().next() else {
        return Vec::new();
    };
    let Ok(table) = manager.read_tag(tag) else {
        return Vec::new();
    };
    let Ok((count, _, rows, _)) = native_array_at(&table, 0x18) else {
        return Vec::new();
    };
    (0..count.min(1024))
        .map_while(|index| {
            let row = table.get(rows + index * 16..rows + index * 16 + 16)?;
            Some(std::array::from_fn(|lane| {
                f32::from_le_bytes(row[lane * 4..lane * 4 + 4].try_into().unwrap())
            }))
        })
        .collect()
}

/// A dye's detail diffuse and detail normal textures. Armor dyes bind them at slots 3 and 4,
/// cloth dyes at 5 and 6, and suit dyes at 7 and 8, so the pair the scope binds is the one.
fn detail_textures(scope: &[u8]) -> (Result<Option<u32>, String>, Result<Option<u32>, String>) {
    for (diffuse, normal) in [(3, 4), (5, 6), (7, 8)] {
        let pair = (
            detail_texture(scope, diffuse),
            detail_texture(scope, normal),
        );
        if !matches!(pair, (Ok(None), Ok(None))) {
            return pair;
        }
    }
    (Ok(None), Ok(None))
}

fn detail_texture(scope: &[u8], slot: u32) -> Result<Option<u32>, String> {
    // Pixel scope texture bindings, a slot and a texture per row.
    if u64::from_le_bytes(bytes_at(scope, 0x40)?) == 0 {
        return Ok(None);
    }
    let (count, _, rows, class) = native_array_at(scope, 0x40)?;
    if class != 0x8080_7211 || count > 64 || count > scope.len().saturating_sub(rows) / 8 {
        return Err("Unsupported dye texture bindings".into());
    }
    for row in (0..count).map(|i| rows + i * 8) {
        if word(scope, row)? == slot {
            return Ok(Some(word(scope, row + 4)?).filter(|t| !matches!(*t, 0 | u32::MAX)));
        }
    }
    Ok(None)
}

pub(super) fn decode(constants: &[u8]) -> Result<Material, String> {
    decode_colors(constants)?;
    let mut vectors = [[0.0; 4]; 27];
    for (i, vector) in vectors.iter_mut().enumerate() {
        for (j, value) in vector.iter_mut().enumerate() {
            *value = f32::from_le_bytes(bytes_at(constants, i * 16 + j * 4)?);
            if !value.is_finite() || value.abs() > 1.0e6 {
                return Err("Invalid dye material parameter".into());
            }
        }
    }
    let frame = properties(&vectors);
    Ok(Material {
        surfaces: frame.surfaces,
        detail_transform: frame.detail_transform,
        normal_transform: frame.normal_transform,
        detail: Ok(None),
        normal: Ok(None),
        animation: Ok(None),
        vectors,
    })
}

/// The surfaces and texture transforms a dye's 27 vectors describe.
pub(crate) fn properties(vectors: &[[f32; 4]; 27]) -> Frame {
    let rgb = |i: usize| [vectors[i][0], vectors[i][1], vectors[i][2]].map(|v| v.max(0.0));
    let surface = |index: usize| {
        let base = 9 + index * 4;
        let worn = 17 + index * 4;
        Surface {
            albedo: rgb(base),
            worn_albedo: rgb(worn),
            params: vectors[base + 1],
            worn_params: vectors[worn + 3],
            roughness: vectors[base + 3],
            worn_roughness: vectors[worn + 2],
            wear: vectors[worn + 1],
            emissive: rgb(3 + index),
            iridescence: vectors[base + 2][0],
        }
    };
    Frame {
        vectors: *vectors,
        surfaces: [surface(0), surface(1)],
        detail_transform: vectors[0],
        normal_transform: vectors[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_binding_selects_diffuse_and_rejects_truncated_rows() {
        let mut scope = vec![0; 0xA0];
        scope[0x40..0x48].copy_from_slice(&2_u64.to_le_bytes());
        scope[0x48..0x50].copy_from_slice(&0x38_u64.to_le_bytes());
        scope[0x80..0x88].copy_from_slice(&2_u64.to_le_bytes());
        scope[0x88..0x8C].copy_from_slice(&0x8080_7211_u32.to_le_bytes());
        for (offset, slot, tag) in [(0x90, 4_u32, 0x80AA_0001_u32), (0x98, 3, 0x80AA_0002)] {
            scope[offset..offset + 4].copy_from_slice(&slot.to_le_bytes());
            scope[offset + 4..offset + 8].copy_from_slice(&tag.to_le_bytes());
        }
        assert_eq!(detail_texture(&scope, 3).unwrap(), Some(0x80AA_0002));
        assert!(detail_texture(&scope[..0x9F], 3).is_err());
        scope[0x40..0x48].fill(0);
        assert_eq!(detail_texture(&scope, 3).unwrap(), None);
    }

    #[test]
    fn native_material_layout_keeps_primary_secondary_and_wear_separate() {
        let mut bytes: Vec<_> = (0..27)
            .flat_map(|i| [i as f32; 4].into_iter().flat_map(f32::to_le_bytes))
            .collect();
        let material = decode(&bytes).unwrap();
        assert_eq!(material.surfaces[0].params, [10.0; 4]);
        assert_eq!(material.surfaces[1].worn_albedo, [21.0; 3]);
        assert_eq!(material.surfaces[0].wear, [18.0; 4]);
        assert_eq!(material.surfaces[1].roughness, [16.0; 4]);
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        bytes[18 * 16..18 * 16 + 4].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }
}
