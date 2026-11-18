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

/// The share of a surface's visibility that an object's probe stores. Shadowkeep's gear vertex
/// program writes `saturate(saturate(n.z * v) + v)` from a probe's value `v`, so a face turned up
/// takes twice the probe and a side takes it once. Stock records hold [`OPEN`] as their default
/// probe, at which an upward face reaches one. A converted open floor writes [`OPEN`] itself, and
/// ambient light squares visibility, so averaged over every facing an object takes
/// `1.667 * share^2` of the ambient light of the floor it stands on and `1.16 * share^0.7` of its
/// sunlight. This share gives 1.07 and 0.99, so an object as a whole takes the light of its
/// floor, with upward faces at 2.56 times the floor's ambient light and sides at 0.64. At the
/// floor's own value a player drew brighter than the level around them (1.67 times its ambient
/// light on average), and at half of it darker (0.42 times), since most of a standing figure
/// faces sideways.
pub(in crate::halo_reach) const OBJECT_SHARE: f32 = 0.8;

pub(in crate::halo_reach) fn visibility(light: f32, reference: f32) -> f32 {
    (OPEN * (light.max(0.) / reference.max(1e-4)).sqrt()).clamp(RANGE[0], RANGE[1])
}

/// The value an object's probe stores for `light` against `reference` ([`OBJECT_SHARE`]).
pub(in crate::halo_reach) fn object_visibility(light: f32, reference: f32) -> f32 {
    OBJECT_SHARE * visibility(light, reference)
}
