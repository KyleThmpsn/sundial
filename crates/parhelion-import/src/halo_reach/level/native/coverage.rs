//! Coverage for ground lighting samples and the static probe bake. Projected and transmitting
//! layers do not define solid geometry. A supported static cutout samples its authored alpha mask
//! at the ray hit.
//! This is a top-mip bake without a pixel footprint, colored transmission or animated coverage.
use crate::halo_reach::{
    cache::Cache,
    level::Level,
    material::{self, Mapping, Material},
    resource::Pages,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

struct Texture {
    extent: [usize; 2],
    alpha: Vec<u8>,
}

pub(super) struct Mask {
    layers: Vec<Layer>,
    scale: f32,
}

struct Layer {
    texture: Arc<Texture>,
    transform: [f32; 4],
    address: [u8; 2],
    point: bool,
    source: Value,
}

pub(super) enum Coverage {
    Solid,
    Excluded(&'static str),
    Mask(Mask),
    Conservative(&'static str),
}

impl Coverage {
    pub(super) fn report(&self) -> Value {
        match self {
            Self::Solid => json!({"mode": "solid"}),
            Self::Excluded(reason) => json!({"mode": "excluded", "reason": reason}),
            Self::Mask(mask) => json!({
                "mode": "alpha_mask", "scale": mask.scale, "cutoff": 0.5,
                "layers": mask.layers.iter().map(|layer| json!({
                    "source": layer.source, "transform": layer.transform, "address": layer.address,
                    "filter": if layer.point { "point_top_mip" } else { "bilinear_top_mip" },
                })).collect::<Vec<_>>(),
            }),
            Self::Conservative(reason) => json!({"mode": "conservative_solid", "reason": reason}),
        }
    }

    pub(super) fn covered(&self, uv: [f32; 2]) -> bool {
        match self {
            Self::Mask(mask) => mask
                .layers
                .iter()
                .try_fold(mask.scale, |alpha, layer| Some(alpha * layer.alpha(uv)?))
                .is_none_or(|alpha| alpha >= 0.5),
            Self::Excluded(_) => false,
            Self::Solid | Self::Conservative(_) => true,
        }
    }
}

/// One coverage rule per material. Only materials used by level geometry read bitmap data.
/// A known mask whose bitmap cannot be read fails the build instead of opening holes silently.
pub(super) fn read(cache: &Cache, pages: &mut Pages, level: &Level) -> Result<Vec<Coverage>> {
    let mut used = vec![false; level.materials.len()];
    for primitive in &level.primitives {
        if let Some(index) = primitive.material {
            *used
                .get_mut(index)
                .context("Probe material outside level")? = true;
        }
    }
    let mut textures = HashMap::<(u32, usize), Arc<Texture>>::new();
    level
        .materials
        .iter()
        .zip(used)
        .map(|(material, used)| {
            if !used {
                return Ok(Coverage::Solid);
            }
            read_material(cache, pages, material, &mut textures).with_context(|| {
                format!(
                    "Probe coverage of {}",
                    material
                        .tag
                        .as_ref()
                        .map_or("unnamed material", |tag| tag.path.as_str())
                )
            })
        })
        .collect()
}

fn read_material(
    cache: &Cache,
    pages: &mut Pages,
    material: &Material,
    textures: &mut HashMap<(u32, usize), Arc<Texture>>,
) -> Result<Coverage> {
    let option = |name: &str| material.options.get(name).map(String::as_str);
    if material.tag.is_none() {
        return Ok(Coverage::Conservative("missing_material_identity"));
    }
    if material.decal() {
        return Ok(Coverage::Excluded("projected_decal"));
    }
    if material.glass() {
        return Ok(Coverage::Excluded("glass"));
    }
    if material.water() {
        return Ok(Coverage::Excluded("water"));
    }
    if matches!(
        option("blend_mode"),
        Some("additive" | "multiply" | "double_multiply" | "alpha_blend" | "pre_multiplied_alpha")
    ) {
        return Ok(Coverage::Excluded("blended_surface"));
    }
    if !matches!(option("blend_mode"), None | Some("opaque")) {
        return Ok(Coverage::Conservative("unknown_blend_mode"));
    }
    let albedo = match option("alpha_test") {
        None | Some("none") => return Ok(Coverage::Solid),
        Some("simple" | "from_texture") => false,
        Some("from_albedo_alpha") => true,
        _ => return Ok(Coverage::Conservative("combined_or_unknown_alpha_test")),
    };
    if !matches!(option("parallax"), None | Some("off")) {
        return Ok(Coverage::Conservative("parallax_mask_coordinates"));
    }
    let animated = |name: &str| {
        material
            .properties
            .first()
            .is_some_and(|property| property.overlays.contains_key(name))
    };
    let (roles, tinted): (&[&[&str]], bool) = if albedo {
        // These source albedo programs multiply their coverage. Blended and custom albedo
        // programs require their own evaluation, so they stay conservative here.
        match option("albedo") {
            Some("default") => (&[&["base_map"], &["detail_map"]], true),
            Some("simple") => (&[&["base_map"]], true),
            Some("detail") => (&[&["base_map"], &["detail_map"]], false),
            Some("constant_color") => (&[], true),
            _ => return Ok(Coverage::Conservative("combined_albedo_coverage")),
        }
    } else {
        (&[&["alpha_mask_map", "alpha_test_map", "alpha_map"]], false)
    };
    if tinted && animated("albedo_color") {
        return Ok(Coverage::Conservative("animated_material_coverage"));
    }
    let scale = if tinted {
        material.constant("albedo_color").unwrap_or([1.; 4])[3]
    } else {
        1.
    };
    ensure!(scale.is_finite(), "Nonfinite probe alpha scale");
    let mut mappings = Vec::new();
    for roles in roles {
        let Some(mapping) = material.mapping(roles) else {
            return Ok(Coverage::Conservative("alpha_mask_not_bound"));
        };
        if !mapping.functions.is_empty() || animated(&mapping.role) {
            return Ok(Coverage::Conservative("animated_material_coverage"));
        }
        if mapping.address & 15 > 4 || mapping.address >> 4 > 4 || mapping.filter > 6 {
            return Ok(Coverage::Conservative("unsupported_mask_sampler"));
        }
        mappings.push(mapping);
    }
    let layers = mappings
        .into_iter()
        .map(|mapping| read_layer(cache, pages, mapping, textures))
        .collect::<Result<_>>()?;
    Ok(Coverage::Mask(Mask { layers, scale }))
}

fn read_layer(
    cache: &Cache,
    pages: &mut Pages,
    mapping: &Mapping,
    textures: &mut HashMap<(u32, usize), Arc<Texture>>,
) -> Result<Layer> {
    ensure!(
        mapping.transform.iter().all(|value| value.is_finite()),
        "Nonfinite probe mask transform"
    );
    let bitmap = mapping
        .bitmap
        .as_ref()
        .context("Missing probe alpha bitmap")?;
    let frame = mapping.static_frame(cache)?;
    let key = (bitmap.datum, frame);
    let texture = match textures.get(&key) {
        Some(texture) => Arc::clone(texture),
        None => {
            let image = material::image(cache, pages, bitmap, frame, false)?;
            ensure!(
                image.rgba.len() == image.width * image.height * 4,
                "Incomplete probe alpha bitmap"
            );
            let texture = Arc::new(Texture {
                extent: [image.width, image.height],
                alpha: image.rgba.chunks_exact(4).map(|texel| texel[3]).collect(),
            });
            textures.insert(key, Arc::clone(&texture));
            texture
        }
    };
    let source = json!({"bitmap": bitmap, "frame": frame, "extent": texture.extent});
    Ok(Layer {
        texture,
        transform: mapping.transform,
        address: [mapping.address & 15, mapping.address >> 4],
        point: mapping.filter == 1,
        source,
    })
}

impl Layer {
    fn alpha(&self, uv: [f32; 2]) -> Option<f32> {
        let mut uv: [f32; 2] =
            std::array::from_fn(|k| uv[k] * self.transform[k] + self.transform[k + 2]);
        if !uv.iter().all(|value| value.is_finite()) {
            return None;
        }
        for (value, mode) in uv.iter_mut().zip(self.address) {
            *value = match mode {
                0 => value.rem_euclid(1.),
                1 => value.clamp(0., 1.),
                2 => 1. - (value.rem_euclid(2.) - 1.).abs(),
                3 if *value < -1. || *value > 2. => return Some(0.),
                3 => *value,
                4 => value.abs().min(1.),
                _ => return None,
            };
        }
        let position: [f32; 2] = std::array::from_fn(|k| uv[k] * self.texture.extent[k] as f32);
        if self.point {
            return Some(self.texel(position.map(|value| value.floor() as i64)));
        }
        let position = position.map(|value| value - 0.5);
        let origin = position.map(|value| value.floor() as i64);
        let fraction: [f32; 2] = std::array::from_fn(|k| position[k] - position[k].floor());
        let row = |dy: i64| {
            self.texel([origin[0], origin[1] + dy]) * (1. - fraction[0])
                + self.texel([origin[0] + 1, origin[1] + dy]) * fraction[0]
        };
        Some(row(0) * (1. - fraction[1]) + row(1) * fraction[1])
    }

    fn texel(&self, at: [i64; 2]) -> f32 {
        let mut coordinate = [0usize; 2];
        for k in 0..2 {
            let size = self.texture.extent[k] as i64;
            coordinate[k] = match self.address[k] {
                0 => at[k].rem_euclid(size),
                1 | 4 => at[k].clamp(0, size - 1),
                2 => {
                    let wrapped = at[k].rem_euclid(2 * size);
                    if wrapped < size {
                        wrapped
                    } else {
                        2 * size - 1 - wrapped
                    }
                }
                3 if !(0..size).contains(&at[k]) => return 0.,
                3 => at[k],
                _ => return 1.,
            } as usize;
        }
        f32::from(self.texture.alpha[coordinate[1] * self.texture.extent[0] + coordinate[0]]) / 255.
    }
}
