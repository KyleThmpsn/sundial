use super::*;
fn sat(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}
fn ramp(x: f32, c: [f32; 4]) -> f32 {
    sat((x + c[2]).abs() * c[1] + c[0])
}
fn curve(x: f32, p: f32) -> f32 {
    if x <= 0.0 { 0.0 } else { x.powf(p) }
}
fn color(base: [f32; 4], delta: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| base[i] + delta[i] * t)
}

fn transform(uv: [f32; 2], t: [f32; 4]) -> [f32; 2] {
    [uv[0] * t[0] + t[2], uv[1] * t[1] + t[3]]
}

fn texture_sample(model: &Model, material: &Material, slot: usize, uv: [f32; 2]) -> [f32; 4] {
    let Some(texture) = material.textures[slot].and_then(|i| model.textures.get(i)) else {
        return [0.0; 4];
    };
    let fallback = texture::Sampler {
        u: texture::AddressMode::Wrap,
        v: texture::AddressMode::Wrap,
        border: [0.0; 4],
    };
    let sampler = material
        .samplers
        .get(if slot == 2 { 3 } else { 0 })
        .unwrap_or(&fallback);
    texture.sample_material(uv, sampler, material.color[slot])
}

fn distorted_glow(
    model: &Model,
    material: &Material,
    c: &Frame,
    uv: [f32; 2],
    facing: f32,
) -> [f32; 4] {
    // Two-sided orthographic studio approximation of the native front-face view term.
    let angle = 1.0 - (1.0 - facing.abs()).powi(2);
    let tint = color(c[8], c[10], sat(angle * c[9][0] + c[9][1]));
    let distortion = texture_sample(model, material, 0, transform(uv, c[1]));
    let mapped = transform(uv, c[0]);
    let noisy = std::array::from_fn(|i| mapped[i] + distortion[i] * c[2][i] + c[2][i + 2]);
    let sampled = texture_sample(model, material, 1, noisy);
    let fade = ramp(uv[0], c[5]).powi(2) * ramp(uv[1], c[6]).powi(2) * sat(c[7][0]);
    let rgba =
        std::array::from_fn::<_, 4, _>(|i| (sampled[i] * c[4][0] + c[3][i]) * fade * tint[i]);
    [
        rgba[0] * rgba[3] * c[12][0] * c[11][0],
        rgba[1] * rgba[3] * c[12][0] * c[11][1],
        rgba[2] * rgba[3] * c[12][0] * c[11][2],
        0.0,
    ]
}

fn wave_glow(model: &Model, material: &Material, c: &Frame, uv: [f32; 2], facing: f32) -> [f32; 4] {
    let outer =
        (curve(sat(facing * c[3][0] + c[3][1]), c[4][0]) * ramp(uv[1], c[2]) * ramp(uv[0], c[1]))
            .min(1.0);
    let shape = curve(ramp(uv[1], c[13]) * ramp(uv[1], c[14]) * outer, c[15][0]);
    let mapped = transform(uv, c[0]);
    let sample =
        |slot, index: usize| texture_sample(model, material, slot, transform(mapped, c[index]))[0];
    // The DXBC resource swizzles all select red, including writes to register Z and W.
    let noise = sample(0, 7) * sample(0, 8) * 4.594793 + sample(0, 6);
    let level = c[10][0] + c[10][1] * ramp(uv[1], c[9]) + noise;
    let level = c[5][0] + (level - c[5][0]) * sat(c[11][0]);
    let level = c[12][0] + c[12][1] * level;
    let alpha = sat(shape * level * c[16][0] + c[16][1]);
    let first = color(c[18], c[19], ramp(uv[1], c[17]));
    let mask = sample(1, 21) * sample(1, 20) * ramp(uv[1], c[22]) * ramp(uv[1], c[23]);
    let mask = sat(curve(sat(mask * c[24][0] + c[24][1]), c[25][0]) * c[26][0]);
    let rgba = std::array::from_fn::<_, 4, _>(|i| {
        let first = if i == 3 { alpha } else { first[i] * alpha };
        (first * 4.594793 + mask * c[27][i] * c[28][i]) * c[29][0] * outer
    });
    // The studio displays fully visible gear. Native scene fade is supplied by the game.
    [
        rgba[0] * rgba[3] * c[31][0] * c[30][0],
        rgba[1] * rgba[3] * c[31][0] * c[30][1],
        rgba[2] * rgba[3] * c[31][0] * c[30][2],
        0.0,
    ]
}

pub(in crate::model_preview) struct Pixel {
    pub uv: [f32; 2],
    pub detail_uv: [f32; 2],
    pub facing: f32,
    pub gap: f32,
    pub exposure: f32,
}

pub(in crate::model_preview) fn sample(
    model: &Model,
    material: &Material,
    c: &Frame,
    gear: &shader::Bindings<'_>,
    p: Pixel,
) -> [f32; 4] {
    let v = p.facing * p.facing;
    let mut output = match material.kind {
        Kind::Gradient => {
            let fade = sat(v * c[4][0] + c[4][1]).powi(2) * curve(ramp(p.uv[1], c[5]), c[6][0]);
            let t = curve(ramp(p.uv[1], c[0]), c[1][0]).min(1.0);
            let rgba = color(c[2], c[3], t).map(|x| x * fade);
            let rgb = std::array::from_fn::<_, 3, _>(|i| rgba[i] * rgba[3] * c[8][0] * c[7][i]);
            [rgb[0], rgb[1], rgb[2], 0.0]
        }
        Kind::SoftGradient => {
            let t = curve(ramp(p.uv[1], c[1]), c[2][0]).min(1.0);
            let tint = color(c[3], c[4], t);
            let facing = sat(v * c[0][0] + c[0][1]).powi(2);
            let fade = (sat(p.gap * c[7][0] + c[7][1]) * ramp(p.uv[1], c[5]) * ramp(p.uv[0], c[6]))
                .powi(2);
            let rgb = std::array::from_fn::<_, 3, _>(|i| {
                tint[i] * facing * fade * fade * c[9][0] * c[8][i]
            });
            [rgb[0], rgb[1], rgb[2], 0.0]
        }
        Kind::GearGlow => {
            let base = gear.effect_base(p.uv, p.detail_uv);
            let mask = gear.effect_mask(p.uv);
            let extra = color(c[4], c[5], sat((1.0 - mask[1]) * c[3][0] + c[3][1]));
            let rgba = std::array::from_fn::<_, 4, _>(|i| (base[i] + extra[i]) * c[6][0]);
            [
                rgba[0] * rgba[3] * c[7][0],
                rgba[1] * rgba[3] * c[7][1],
                rgba[2] * rgba[3] * c[7][2],
                0.0,
            ]
        }
        Kind::GearFresnel => {
            let base = gear.effect_base(p.uv, p.detail_uv);
            let mask = gear.effect_mask(p.uv);
            let a = color(
                c[5],
                c[6],
                curve(sat(v * c[3][0] + c[3][1]), c[4][0]).min(1.0),
            );
            let b = color(
                c[9],
                c[10],
                curve(sat(v * c[7][0] + c[7][1]), c[8][0]).min(1.0),
            );
            let tint = color(
                c[15],
                c[16],
                curve(sat(v * c[13][0] + c[13][1]), c[14][0]).min(1.0),
            );
            let amount = c[12][0] + c[12][1] * sat(mask[1] * c[11][0] + c[11][1]);
            let fade = curve(sat(p.gap * c[17][0] + c[17][1]), c[18][0]);
            let rgba = std::array::from_fn::<_, 4, _>(|i| {
                (base[i] + (a[i] + b[i] * amount) * tint[i] * fade) * c[19][0]
            });
            [
                rgba[0] * rgba[3] * c[20][0],
                rgba[1] * rgba[3] * c[20][1],
                rgba[2] * rgba[3] * c[20][2],
                0.0,
            ]
        }
        Kind::ScrollingMasks => masks(model, material, c, gear, p.uv, v),
        Kind::DistortedGlow => distorted_glow(model, material, c, p.uv, p.facing),
        Kind::WaveGlow => wave_glow(model, material, c, p.uv, v),
        Kind::Native | Kind::Unavailable => [0.0; 4],
    };
    for value in &mut output[..3] {
        *value = (*value * p.exposure).max(0.0);
    }
    output[3] = sat(output[3]);
    if output.iter().any(|v| !v.is_finite()) {
        [0.0; 4]
    } else {
        output
    }
}

fn masks(
    model: &Model,
    material: &Material,
    c: &Frame,
    gear: &shader::Bindings<'_>,
    uv: [f32; 2],
    facing: f32,
) -> [f32; 4] {
    let sample = |slot, uv| texture_sample(model, material, slot, uv);
    let mapped = transform(uv, c[13]);
    let a = sample(1, transform(mapped, c[19]));
    let b = sample(1, transform(mapped, c[22]));
    let distort = transform([mapped[1], mapped[0]], c[18]);
    let noise_uv =
        std::array::from_fn(|i| distort[i] + (a[i] - c[21][i] + b[i] - c[23][i]) * c[20][0]);
    let noise = sample(2, noise_uv)[0];
    let mask = sample(0, transform(mapped, c[14]))[0];
    let amount = (c[16][0] + c[17][0] * sat(mask * c[15][0] + c[15][1])) * noise;
    let amount = sat(
        amount * curve(ramp(uv[0], c[10]), c[11][0]) * ramp(uv[1], c[12]) * c[24][0] + c[24][1],
    );
    let mut tint = color(c[25], c[26], amount);
    let highlight = color(
        c[29],
        c[30],
        (ramp(uv[0], c[27]) * ramp(uv[1], c[28])).powi(2),
    );
    let sparkle = sample(1, transform(mapped, c[32]));
    let edge = ramp(mapped[1], c[31]);
    let length = color(c[8], c[9], ramp(uv[1], c[7]));
    let angle = color(
        c[5],
        c[6],
        curve(sat(facing * c[3][0] + c[3][1]), c[4][0]).min(1.0),
    );
    let plate = gear.effect_plate(uv);
    for i in 0..3 {
        tint[i] = (tint[i] + highlight[i] * sparkle[i] * edge) * length[i] * angle[i] * c[33][i];
        tint[i] = (tint[i] * plate[0] + c[34][i] * (c[35][i] * plate[1] + c[36][i] * plate[2]))
            * c[37][0];
    }
    let alpha = sat(gear.effect_mask(uv)[1]) * c[38][0];
    [
        tint[0] * c[38][0] * alpha * c[40][0],
        tint[1] * c[38][0] * alpha * c[40][1],
        tint[2] * c[38][0] * alpha * c[40][2],
        alpha * c[39][0],
    ]
}
