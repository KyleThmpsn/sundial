//! The matched native painted recipe. Selected dye properties remain authoritative.
use super::*;

pub(super) fn remap(value: f32, map: [f32; 4]) -> f32 {
    saturate(map[2] + map[3] * saturate(map[0] + map[1] * value))
}

pub(super) fn intact(alpha: f32, dye: &Surface) -> f32 {
    remap(saturate((alpha - 48.0) / 207.0), dye.wear)
}

fn color(base: [f32; 3], detail: [f32; 4], dye: [f32; 3], strength: f32) -> [f32; 3] {
    std::array::from_fn(|i| {
        let detailed = saturate(overlay(detail[i], dye[i]));
        saturate(overlay(base[i], mix(dye[i], detailed, saturate(strength))))
    })
}

fn smoothness(raw: f32, detail: f32, map: [f32; 4], strength: f32) -> f32 {
    mix(
        remap(raw, map),
        remap(saturate(overlay(raw, detail)), map),
        saturate(strength),
    )
}

pub(super) fn apply(
    sample: &mut Sample,
    base: [f32; 3],
    mask: [f32; 4],
    detail: Option<[f32; 4]>,
    dye: &Surface,
    smooth: [f32; 2],
) {
    if mask[3] < 40.0 {
        sample.roughness = 1.0 - smooth[1];
        return;
    }
    let detail = detail.unwrap_or([0.25; 4]);
    let intact = intact(mask[3], dye);
    let pristine = color(base, detail, dye.albedo, dye.params[0]);
    let worn = color(base, detail, dye.worn_albedo, dye.worn_params[0]);
    sample.albedo = std::array::from_fn(|i| mix(worn[i], pristine[i], intact));
    sample.roughness = 1.0
        - mix(
            smoothness(smooth[0], detail[3], dye.worn_roughness, dye.worn_params[2]),
            smoothness(smooth[0], detail[3], dye.roughness, dye.params[2]),
            intact,
        );
    sample.metal = mix(
        saturate(dye.worn_params[3]),
        saturate(dye.params[3]),
        intact,
    );
}
