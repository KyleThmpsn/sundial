//! A visual study for point emitters that have no native draw mesh.
//! The compiled spawn and motion program is not evaluated here.
use super::Model;

pub(crate) struct Source {
    pub position: [f32; 3],
    pub drift: [f32; 3],
    pub width: f32,
    pub phase: f32,
    pub period: f32,
    pub texture: usize,
    pub gradient: Option<usize>,
}

pub(super) fn study_period(program: Option<&super::assets::Program>) -> f32 {
    program
        .and_then(|program| program.lifetime_default())
        .filter(|&seconds| (0.05..=10.0).contains(&seconds))
        .unwrap_or(1.4)
}

pub(super) fn texture_index(model: &mut Model, texture: &super::texture::Texture) -> Option<usize> {
    if let Some(index) = model
        .textures
        .iter()
        .position(|item| item.tag == texture.tag)
    {
        return Some(index);
    }
    if model.textures.len() >= super::MAX_TEXTURES {
        return None;
    }
    super::texture::retain(model, texture.clone()).ok()
}

/// A 32-byte emitter record has no mesh reference and places the system at its local origin.
/// The spread is a visual study until the native spawn program can be evaluated.
pub(super) fn collect_points(model: &mut Model) {
    let materials = model
        .assets
        .particles
        .iter()
        .filter(|particle| particle.point_emitter)
        .filter_map(|particle| {
            particle.texture.as_ref().map(|texture| {
                (
                    particle.tag,
                    texture.clone(),
                    study_period(particle.program.as_ref()),
                    particle.gradient.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut tiled = 0;
    for (tag, texture, period, gradient) in materials {
        if !has_dark_border(&texture) {
            tiled += 1;
            continue;
        }
        let Some(index) = texture_index(model, &texture) else {
            continue;
        };
        let gradient = gradient
            .as_ref()
            .and_then(|texture| texture_index(model, texture));
        for particle in 0..16 {
            let seed = tag.wrapping_add((particle as u32).wrapping_mul(0x9E37_79B9));
            let angle = (seed & 0xFFFF) as f32 / 65536.0 * std::f32::consts::TAU;
            let elevation = ((seed >> 16) & 0xFFFF) as f32 / 65535.0 * 1.4 - 0.7;
            let (sin, cos) = angle.sin_cos();
            model.particle_sources.push(Source {
                position: [0.0; 3],
                drift: [cos * 0.35, sin * 0.35, elevation * 0.35],
                width: 0.06 + (particle % 5) as f32 * 0.015,
                phase: ((particle * 11 + (tag as usize & 31)) % 31) as f32 / 31.0,
                period,
                texture: index,
                gradient,
            });
        }
    }
    if tiled > 0 {
        let noun = if tiled == 1 { "material" } else { "materials" };
        model.notices.push(format!(
            "Native shader required for {tiled} point-emitter {noun}. Tiled images cannot be drawn as sprites."
        ));
    }
}

/// A black edge allows an additive image to fade into the surrounding frame. A bright edge
/// usually belongs to a tiled noise, mask, or shader input and would show as a hard card.
fn has_dark_border(texture: &super::texture::Texture) -> bool {
    let [width, height] = texture.size;
    if width < 2 || height < 2 {
        return false;
    }
    let brightness = |x: usize, y: usize| {
        let at = (y * width + x) * 4;
        texture.rgba[at..at + 3]
            .iter()
            .map(|&color| usize::from(color))
            .sum::<usize>()
    };
    let mut total = 0;
    for x in 0..width {
        total += brightness(x, 0) + brightness(x, height - 1);
    }
    for y in 1..height - 1 {
        total += brightness(0, y) + brightness(width - 1, y);
    }
    let samples = 2 * width + 2 * (height - 2);
    total < samples * 3 * 255 / 20
}
