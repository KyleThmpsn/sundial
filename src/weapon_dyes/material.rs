//! The 27-vector Shadowkeep layout, not the later 21-vector dye layout.
use super::*;
mod program;

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
}
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub surfaces: [Surface; 2],
    pub detail_transform: [f32; 4],
    pub normal_transform: [f32; 4],
}
impl Animation {
    pub fn at(&self, seconds: f32) -> Result<Frame, String> {
        Ok(properties(&self.program.run(self.base, seconds)?))
    }
}

pub(crate) fn load(
    manager: &PackageManager,
    indices: &[u16],
) -> Result<BTreeMap<u16, Result<Material, String>>, String> {
    read_dyes(manager, indices, |constants, scope| {
        let mut material = decode(constants)?;
        material.detail = detail_texture(scope, 3);
        material.normal = detail_texture(scope, 4);
        material.animation = program::Program::read(scope).map(|p| {
            p.map(|program| Animation {
                program,
                base: material.vectors,
            })
        });
        Ok(material)
    })
}

fn detail_texture(scope: &[u8], slot: u32) -> Result<Option<u32>, String> {
    // Pixel scope texture bindings. Slot 3 is detail diffuse, slot 4 detail normal.
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

fn decode(constants: &[u8]) -> Result<Material, String> {
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

fn properties(vectors: &[[f32; 4]; 27]) -> Frame {
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
