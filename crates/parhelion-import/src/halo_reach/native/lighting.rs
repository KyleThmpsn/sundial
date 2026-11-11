//! One native visibility reference for map surfaces, objects and decorators.

/// Shadowkeep's open-ground visibility. Native diffuse environment light squares this value.
pub(in crate::halo_reach) const OPEN: f32 = 0.505;
pub(in crate::halo_reach) const RANGE: [f32; 2] = [0.04, 0.8];
/// The direct-light power from Shadowkeep's default global channel 83.
pub(in crate::halo_reach) const DIRECT_POWER: f32 = 0.7;
pub(in crate::halo_reach) const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

pub(in crate::halo_reach) fn luminance(color: [f32; 3]) -> f32 {
    color
        .iter()
        .zip(LUMINANCE)
        .map(|(value, weight)| value * weight)
        .sum()
}

pub(in crate::halo_reach) fn visibility(light: f32, reference: f32) -> f32 {
    (OPEN * (light.max(0.) / reference.max(1e-4)).sqrt()).clamp(RANGE[0], RANGE[1])
}
