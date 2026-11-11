//! Source beam systems (`beam`): each beam's color, intensity and thickness along its length.
//!
//! A beam system holds one block of 0x224-byte beam definitions. The 2011-07-07 Reach tag build's
//! `beam_definition_block_fields` initializer names the fields in order: the render method at
//! `+0x04`, then the header fields, and the profile thickness at `+0x148`, color at `+0x168` and
//! intensity at `+0x1E8`, each a 32-byte effect property. The Reach `c_beam_states` source
//! numbers their inputs: 0 and 1 the capped and uncapped profile percentile, 2 game time, 3 system
//! age, 4 beam random, 5 and 6 the correlations, 7 beam length, 8 the LOD and 9 and 10 the effect
//! scales.
use super::property::Property;
use crate::halo_reach::{
    cache::{Cache, Tag},
    material,
};
use anyhow::{Result, bail, ensure};

pub(in crate::halo_reach) struct Beam {
    pub name: String,
    /// The render method's albedo option. A `diffuse_only` beam draws its profile color, while a
    /// palettized one takes its hue from a gradient texture.
    pub albedo: Option<String>,
    thickness: Property,
    color: Property,
    intensity: Property,
}

/// Samples taken along each beam, from its origin to its end.
const SAMPLES: usize = 17;

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// A sampled input's value at `percentile` along the beam. Profile percentile follows the
/// sample, the beam starts at system age and game time zero, and random and correlation inputs
/// take their midpoint and effect scales one. The beam length and LOD inputs are not sampled.
fn input(index: u8, percentile: f32) -> Result<f32> {
    Ok(match index {
        0 | 1 => percentile,
        2 | 3 => 0.,
        4..=6 => 0.5,
        9 | 10 => 1.,
        other => bail!("Source beam property reads input {other}"),
    })
}

impl Beam {
    /// The drawn color at `percentile` along the beam and the light it emits there: the color's
    /// luminance times intensity and thickness.
    fn emitted(&self, percentile: f32) -> Result<([f32; 3], f32)> {
        let input = |index| input(index, percentile);
        let [r, g, b, _] = self.color.value(&input)?;
        let intensity = self.intensity.value(&input)?[0];
        let thickness = self.thickness.value(&input)?[0];
        let color = [r, g, b];
        Ok((
            color,
            luminance(color) * intensity.max(0.) * thickness.max(0.),
        ))
    }
}

/// Every beam of source beam system `tag` with its name, each read on its own.
pub(in crate::halo_reach) fn each(cache: &Cache, tag: &Tag) -> Result<Vec<(String, Result<Beam>)>> {
    ensure!(tag.group == "beam", "Expected a source beam system");
    cache
        .block(tag.address()?, 0x224)?
        .into_iter()
        .map(|at| Ok((cache.string_id(cache.u32(at)?)?, beam(cache, tag, at))))
        .collect()
}

fn beam(cache: &Cache, tag: &Tag, at: usize) -> Result<Beam> {
    let material = material::embedded(cache, tag.clone(), at + 4)?;
    Ok(Beam {
        name: cache.string_id(cache.u32(at)?)?,
        albedo: material.options.get("albedo").cloned(),
        thickness: Property::read(cache, at + 0x148)?,
        color: Property::read(cache, at + 0x168)?,
        intensity: Property::read(cache, at + 0x1e8)?,
    })
}

/// The color at the brightest point of `beams` that draw their profile color, sampled along each,
/// and the beam it comes from. A palettized beam's hue is in its gradient texture, so it is left
/// out. `None` when no such beam emits light.
pub(in crate::halo_reach) fn brightest<'a>(
    beams: impl IntoIterator<Item = &'a Beam>,
) -> Result<Option<([f32; 3], &'a str, f32)>> {
    let mut best: Option<([f32; 3], &str, f32, f32)> = None;
    for beam in beams {
        if beam.albedo.as_deref() != Some("diffuse_only") {
            continue;
        }
        for step in 0..SAMPLES {
            let percentile = step as f32 / (SAMPLES - 1) as f32;
            let (color, weight) = beam.emitted(percentile)?;
            if weight > 0. && best.is_none_or(|(_, _, _, w)| weight > w) {
                best = Some((color, &beam.name, percentile, weight));
            }
        }
    }
    Ok(best.map(|(color, name, percentile, _)| (color, name, percentile)))
}
