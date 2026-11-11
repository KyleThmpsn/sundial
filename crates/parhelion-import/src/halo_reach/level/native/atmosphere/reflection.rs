//! The mean diffuse reference of the native atmosphere reflection, before halo and sky geometry.
//! Shadowkeep 86657 generates 64 by 64 texels with `80B35F97`, point-copies them into a 512 by
//! 512 target with `80B9E2AD`, then linearly downsamples with `80B9E2A2`. The copy replaces
//! directions below Z -0.57 with global channel 129, black in the converted-map captures. Its
//! tint weight, channel 130, was zero. The clamp-filtered mip chain preserves the copied mean.
//! This reference averages the volume's time slices. It does not predict the live reflection's
//! sky geometry, halo, environment overrides or variation between unequal time slices.
use super::{LUMINANCE, TEXEL_BYTES};
use anyhow::{Result, ensure};

const GENERATOR_SIZE: usize = 64;
const REFLECTION_SIZE: usize = 512;

/// Mean luminance after the native generator and black-ground copy. Sampler `80C7631D` is
/// linear with U wrap and V clamp. `80C70C85` point-samples the generated reflection with clamp.
pub(super) fn luminance(data: &[u8], [width, height, depth]: [usize; 3]) -> Result<f32> {
    let mut slice = vec![0f64; width * height];
    for (index, texel) in data.chunks_exact(TEXEL_BYTES).enumerate() {
        let rgba: [f32; 4] = std::array::from_fn(|lane| {
            half::f16::from_le_bytes([texel[lane * 2], texel[lane * 2 + 1]]).to_f32()
        });
        ensure!(
            rgba.iter().all(|value| value.is_finite() && *value >= 0.),
            "Atmosphere volume texel {index} is not finite nonnegative radiance"
        );
        slice[index % (width * height)] += rgba[..3]
            .iter()
            .zip(LUMINANCE)
            .map(|(&value, weight)| f64::from(value) * f64::from(weight))
            .sum::<f64>()
            / depth as f64;
    }

    // The tint pass uses pixel-center UVs. Each retained destination texel point-samples one
    // generator texel. Count those weights before evaluating the smaller generated image.
    let mut weights = [0u32; GENERATOR_SIZE * GENERATOR_SIZE];
    for y in 0..REFLECTION_SIZE {
        for x in 0..REFLECTION_SIZE {
            let [u, v] = [x, y].map(|p| 2. * (p as f64 + 0.5) / REFLECTION_SIZE as f64 - 1.);
            if 1. - 2. * (u * u + v * v) >= -0.57 {
                weights[y / 8 * GENERATOR_SIZE + x / 8] += 1;
            }
        }
    }

    let mut total = 0.;
    for y in 0..GENERATOR_SIZE {
        for x in 0..GENERATOR_SIZE {
            let weight = weights[y * GENERATOR_SIZE + x];
            if weight == 0 {
                continue;
            }
            // The original generator subtracts SV_Position's half texel before dividing by 64.
            let [px, py] = [x, y].map(|p| 2. * p as f64 / GENERATOR_SIZE as f64 - 1.);
            let radius = px * px + py * py;
            let h = (1. - radius).max(0.).sqrt();
            let direction = [2. * px * h, 2. * py * h, 1. - 2. * radius.min(1.)];
            let divisor = direction[0].abs() + direction[1].abs();
            let u = if divisor > 0.00001 {
                let diamond = (direction[0].abs() - direction[1].abs()) / divisor * 0.125 + 0.125;
                let sign = |value: f64| if value >= 0. { 1. } else { -1. };
                ((diamond * sign(direction[0]) + 0.25) * sign(direction[1])).rem_euclid(1.) - 1.
            } else {
                -0.5
            };
            let v = 0.5 - 0.5 * direction[2];
            let sx = u * width as f64 - 0.5;
            let sy = v * height as f64 - 0.5;
            let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
            let (fx, fy) = (sx - sx.floor(), sy - sy.floor());
            let mut light = 0.;
            for dy in 0..2 {
                for dx in 0..2 {
                    let column = (ix + dx).rem_euclid(width as i64) as usize;
                    let row = (iy + dy).clamp(0, height as i64 - 1) as usize;
                    light += slice[row * width + column]
                        * if dx == 0 { 1. - fx } else { fx }
                        * if dy == 0 { 1. - fy } else { fy };
                }
            }
            total += light * f64::from(weight);
        }
    }
    let mean = (total / (REFLECTION_SIZE * REFLECTION_SIZE) as f64) as f32;
    ensure!(
        mean.is_finite() && mean >= 0.,
        "Atmosphere diffuse sky reference is not finite nonnegative radiance"
    );
    Ok(mean)
}
