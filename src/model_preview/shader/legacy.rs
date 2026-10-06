//! Direction and pre-iridescence blue limits for a validated native dye consumer.
use super::*;

impl Bindings<'_> {
    pub(super) fn legacy_decode(&self) -> Option<[[f32; 2]; 2]> {
        let base = self.legacy_normal?;
        let detail = self
            .dye
            .map_or([2.0, -1.0], |dye| [dye.vectors[2][0], dye.vectors[2][1]]);
        Some([[base[0], base[1]], detail])
    }

    pub(super) fn decoded_strength(&self, alpha: f32, dye: &Surface) -> f32 {
        let pristine = if self.legacy_normal.is_some() {
            dye.params[1]
        } else {
            saturate(dye.params[1])
        };
        mix(
            saturate(dye.worn_params[1]),
            pristine,
            paint::intact(alpha, dye),
        )
    }

    pub(super) fn legacy_grain(
        &self,
        surface: &mut Sample,
        mask: Option<[f32; 4]>,
        uv: [f32; 2],
        coordinates: [f32; 2],
        offset: f32,
    ) {
        let Some(texture) = self.normal else {
            return;
        };
        let mut limit = saturate(self.sample(texture, uv, 2, None)[2] / 255.0 + offset);
        if let (Some(dye), Some(detail), Some(mask)) = (self.dye, self.detail_normal, mask) {
            let offset = dye.vectors[2][2];
            if offset.is_finite() {
                let uv = std::array::from_fn(|i| {
                    coordinates[i] * dye.normal_transform[i] + dye.normal_transform[i + 2]
                });
                let blue = self.sample(detail, uv, 4, Some(dye.normal_transform))[2] / 255.0;
                limit = limit.min(mix(
                    1.0,
                    saturate(blue + offset),
                    self.decoded_strength(mask[3], &dye.surface),
                ));
            }
        }
        surface.roughness = surface.roughness.max(1.0 - limit);
    }
}
