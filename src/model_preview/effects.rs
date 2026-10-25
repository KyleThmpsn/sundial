//! Native transparent programs use validated family ports or bounded DXBC evaluation.
//! Neither path selects behavior by an item tag. Channels use recovered initial values.
use super::*;
pub(super) use crate::dyes::material::program::ObjectInput;
use crate::dyes::material::program::{ObjectInputs, Program};
pub(super) mod native;
mod read;
mod shade;
pub(crate) use native::Motion;
pub(super) use read::{inputs, load};
pub(super) use shade::{Pixel, sample};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Kind {
    #[default]
    Unavailable = 0,
    Gradient = 1,
    SoftGradient = 2,
    GearGlow = 3,
    GearFresnel = 4,
    ScrollingMasks = 5,
    DistortedGlow = 6,
    WaveGlow = 7,
    Native = 8,
}

#[derive(Default)]
pub(crate) struct Material {
    pub kind: Kind,
    pub constants: Vec<[f32; 4]>,
    pub program: Option<Program>,
    /// Slots 5 through 7 for scrolling masks, 3 and 4 for glow. Gear plates are per draw.
    pub textures: [Option<usize>; 3],
    pub color: [bool; 3],
    pub samplers: Vec<texture::Sampler>,
    pub native: Option<native::Native>,
    /// A validated native surface contract without a retained RGB or vertex program.
    pub(in crate::model_preview) normal: Option<native::LegacyNormal>,
}

pub(super) type Frame = [[f32; 4]; 128];

impl Material {
    pub(in crate::model_preview) fn sampling(&self) -> Option<[usize; 5]> {
        let units = self.normal.as_ref()?.sampling()?;
        units
            .iter()
            .all(|&unit| self.samplers.get(unit).is_some_and(|s| s.filter.is_some()))
            .then_some(units)
    }
    pub(in crate::model_preview) fn opaque(&self) -> bool {
        self.normal.is_some() || self.native.as_ref().is_some_and(|n| n.opaque() && !n.decal)
    }
    pub(in crate::model_preview) fn decal(&self) -> bool {
        self.native.as_ref().is_some_and(|n| n.decal)
    }
    pub fn frame(&self, seconds: f32) -> Option<Frame> {
        if self.kind == Kind::Unavailable || self.constants.len() > 128 {
            return None;
        }
        let mut frame = [[0.0; 4]; 128];
        frame[..self.constants.len()].copy_from_slice(&self.constants);
        if let Some(program) = &self.program {
            let output = &mut frame[..self.constants.len()];
            if self.kind == Kind::Native {
                program.run_shader_into(output, seconds).ok()?;
            } else {
                program.run_into(output, seconds).ok()?;
            }
        }
        Some(frame)
    }

    pub fn remap_textures(&mut self, indices: &[Option<usize>]) {
        if let Some(native) = &mut self.native {
            native.remap_textures(indices);
        }
        for index in &mut self.textures {
            *index = index.and_then(|index| indices.get(index).copied().flatten());
        }
    }
}

pub(super) fn frames(model: &Model, seconds: f32) -> Vec<Option<Frame>> {
    model
        .effects
        .iter()
        .map(|material| material.frame(seconds))
        .collect()
}

pub(super) fn index(model: &Model, triangle: usize) -> Option<usize> {
    model.triangle_effects.get(triangle).copied().flatten()
}

pub(super) fn transparent(model: &Model, triangle: usize) -> bool {
    index(model, triangle).is_some_and(|index| !model.effects[index].opaque())
}
