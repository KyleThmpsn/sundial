//! Package-to-render and GLB normal equations, prepared before the matched consumer.
use super::*;
use paint::{constant, emit, minus};

pub(super) fn declarations(code: &mut Vec<u32>) {
    for index in 0..3 {
        code.extend(instruction(98, &[&register(1, index, 15)]));
    }
    for slot in [1, 5, 7, 9] {
        code.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
}

pub(super) fn output(code: &mut Vec<u32>, mode: Option<u8>) {
    let Some(mode) = mode.filter(|mode| *mode >= 40) else {
        return;
    };
    let r = |index, mask| register(0, index, mask).to_vec();
    let s = |index, swizzle| source(0, index, swizzle).to_vec();
    // Three structured sample/decode branches, with deliberately different rows.
    for channel in 0..3 {
        if channel < 2 {
            emit(
                code,
                49,
                &[
                    r(0, 1),
                    constant(4, 0),
                    literal(channel as f32 + 0.5).to_vec(),
                ],
            );
            let mut branch = instruction(31 | 1 << 18, &[&s(0, 0)]);
            code.append(&mut branch);
        }
        emit(
            code,
            69,
            &[
                r(2, 7),
                source(1, 3, 0xEE).to_vec(),
                source(7, 5 + channel * 2, 0xE4).to_vec(),
                vec![0x0010_6000, 1],
            ],
        );
        emit(
            code,
            50,
            &[
                r(3, 3),
                s(2, 0x44),
                constant(61 + channel, 0),
                constant(61 + channel, 0x55),
            ],
        );
        emit(
            code,
            1 << 13,
            &[
                r(3, 4),
                s(2, if mode == 45 { 0 } else { 0xAA }),
                constant(61 + channel, 0xAA),
            ],
        );
        if channel < 2 {
            code.extend(instruction(18, &[]));
        }
    }
    code.extend(instruction(21, &[]));
    code.extend(instruction(21, &[]));
    emit(
        code,
        69,
        &[
            r(4, 3),
            s(11, 0x44),
            source(7, 1, 0xE4).to_vec(),
            vec![0x0010_6000, 1],
        ],
    );
    emit(
        code,
        50,
        &[r(4, 3), s(4, 0x44), constant(60, 0), constant(60, 0x55)],
    );
    // Intact and worn normal strength are clamped before the wear mix.
    for (target, params) in [(5, 21), (6, 22)] {
        emit(
            code,
            if mode == 43 { 50 | 1 << 13 } else { 50 },
            &[r(target, 3), s(3, 0x44), s(params, 0x55), s(4, 0x44)],
        );
    }
    emit(code, 0, &[r(5, 3), s(5, 0x44), minus(s(6, 0x44))]);
    emit(code, 50, &[r(5, 3), s(28, 0), s(5, 0x44), s(6, 0x44)]);
    emit(code, 0, &[r(5, 3), s(5, 0x44), minus(s(4, 0x44))]);
    // r6 was overwritten above, so restore the mask selector independently.
    emit(
        code,
        29,
        &[r(6, 1), s(9, 0xFF), literal(40.0 / 255.0).to_vec()],
    );
    emit(code, 1, &[r(6, 1), s(6, 0), literal(1.0).to_vec()]);
    // Recompute the selected normal using the correct scalar selector.
    emit(code, 50, &[r(5, 3), s(6, 0), s(5, 0x44), s(4, 0x44)]);
    emit(code, 15, &[r(0, 1), s(5, 0x44), s(5, 0x44)]);
    emit(code, 0, &[r(0, 1), minus(s(0, 0)), literal(1.0).to_vec()]);
    emit(code, 52, &[r(0, 1), s(0, 0), literal(0.0).to_vec()]);
    emit(code, 75, &[r(5, 4), s(0, 0)]);
    emit(code, 16, &[r(0, 1), s(5, 0xE4), s(5, 0xE4)]);
    emit(code, 68, &[r(0, 1), s(0, 0)]);
    emit(code, 56, &[r(5, 7), s(0, 0), s(5, 0xE4)]);
    emit(
        code,
        56,
        &[r(6, 7), s(5, 0x55), source(1, 2, 0xE4).to_vec()],
    );
    emit(
        code,
        50,
        &[r(6, 7), s(5, 0), source(1, 1, 0xE4).to_vec(), s(6, 0xE4)],
    );
    emit(
        code,
        50,
        &[r(6, 7), s(5, 0xAA), source(1, 0, 0xE4).to_vec(), s(6, 0xE4)],
    );
    emit(code, 16, &[r(0, 1), s(6, 0xE4), s(6, 0xE4)]);
    emit(code, 68, &[r(0, 1), s(0, 0)]);
    emit(code, 56, &[r(6, 7), s(0, 0), s(6, 0xE4)]);
    super::normal_blue::output(code);
    emit(
        code,
        50 | 1 << 13,
        &[
            register(2, 1, 7).to_vec(),
            s(6, 0xE4),
            if mode == 46 {
                literal(0.375).to_vec()
            } else {
                s(7, 0)
            },
            literal(0.5).to_vec(),
        ],
    );
}

pub(super) fn material(bytes: &mut Vec<u8>, mode: Option<u8>) {
    let Some(mode) = mode.filter(|mode| *mode >= 40) else {
        return;
    };
    let channel = usize::from(mode - 40).min(2);
    let (_, rows, _) = crate::package_payload::array_at(bytes, 0x318).unwrap();
    floats(bytes, rows + 4 * 16, &[channel as f32, 0.0, 0.0, 0.0]);
    for (row, decode) in [
        (60, [255.0 / 128.0, -1.0, 0.0, 0.0]),
        (61, [255.0 / 128.0, -1.0, 0.4, 0.0]),
        (62, [2.25, -1.1, -0.1, 0.0]),
        (63, [1.5, -0.75, 0.65, 0.0]),
    ] {
        floats(bytes, rows + row * 16, &decode);
    }
    let (count, expression, _) = crate::package_payload::array_at(bytes, 0x2E8).unwrap();
    let mut expression = bytes[expression..expression + count].to_vec();
    for row in [61, 62, 63] {
        expression.extend([0x3C, 1, 0, 0x34, 2, 0x03, 0x42, row, 0x01, 0x43, row]);
    }
    array(bytes, 0x2E8, 0x80800009, &expression, 1);
    let mut inputs = vec![0; 48];
    floats(&mut inputs, 0, &[0.2; 4]);
    floats(&mut inputs, 16, &[1.0; 4]);
    floats(&mut inputs, 32, &[0.0, 0.0, 0.2, 0.0]);
    array(bytes, 0x2F8, 0x80800090, &inputs, 16);
}

pub(crate) fn case(
    alpha: u8,
    channel: usize,
    primary: bool,
    base: [u8; 4],
    detail: [u8; 4],
) -> Model {
    case_at(alpha, channel, primary, base, detail, 40 + channel as u8)
}

pub(super) fn case_at(
    alpha: u8,
    channel: usize,
    primary: bool,
    base: [u8; 4],
    detail: [u8; 4],
    mode: u8,
) -> Model {
    let mut model = opaque::paint_case(alpha, mode).unwrap();
    let slot = channel * 2 + usize::from(!primary);
    let normal = model.triangle_normals[0].unwrap();
    model.textures[normal].size = [8, 4];
    model.textures[normal].rgba = base.repeat(32);
    let index = model.textures.len();
    model.textures.push(crate::model_preview::texture::Texture {
        mips: None,
        tag: 0xA004,
        size: [1, 1],
        rgba: detail.to_vec(),
        linear: None,
    });
    let mut dye = model.dyes[0].unwrap();
    dye.normal = Some(index);
    dye.vectors[10][1] = 1.3;
    dye.vectors[20][1] = -0.2;
    dye.vectors[14] = [0.8, 0.35, 0.2, 0.0];
    dye.vectors[24] = [0.25, 1.4, 0.9, 0.0];
    dye.vectors[13] = dye.vectors[9];
    dye.vectors[21] = dye.vectors[17];
    dye.vectors[22] = dye.vectors[18];
    dye.vectors[23] = dye.vectors[19];
    let surface = usize::from(!primary);
    dye.surface = crate::dyes::material::properties(&dye.vectors).surfaces[surface];
    model.dyes[slot] = Some(dye);
    model.triangle_dyes[..2].fill(slot as u8);
    model.tangents = vec![[1.0, 0.0, 0.0, 1.0]; model.vertices.len()];
    model
}

pub(super) fn expected(
    alpha: u8,
    channel: usize,
    primary: bool,
    base: [u8; 4],
    detail: [u8; 4],
) -> [f32; 3] {
    let raw = ((alpha as f32 - 48.0) / 207.0).clamp(0.0, 1.0);
    let intact = (0.1 + 0.75 * (-0.15 + 1.2 * raw).clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let decode = [[255.0 / 128.0, -1.0], [2.25, -1.1], [1.5, -0.75]][channel];
    let strength = if alpha < 40 {
        0.0
    } else if primary {
        intact
    } else {
        1.0 + intact * (0.35 - 1.0)
    };
    let xy: [f32; 2] = std::array::from_fn(|i| {
        base[i] as f32 / 128.0 - 1.0 + strength * (detail[i] as f32 / 255.0 * decode[0] + decode[1])
    });
    let z = (1.0 - xy[0] * xy[0] - xy[1] * xy[1]).max(0.0).sqrt();
    let length = (xy[0] * xy[0] + xy[1] * xy[1] + z * z).sqrt();
    [xy[0] / length, xy[1] / length, z / length]
}

fn capture(
    out: &Path,
    channel: usize,
    alpha: u8,
    primary: bool,
    landmark: usize,
) -> serde_json::Value {
    let (base, detail) = [
        ([128, 128, 0, 255], [128, 128, 255, 255]),
        ([220, 80, 255, 255], [255, 0, 255, 255]),
    ][landmark];
    let model = case(alpha, channel, primary, base, detail);
    let n = expected(alpha, channel, primary, base, detail);
    let name = format!("native-normal-{channel}-{alpha}-{primary}-{landmark}");
    let glb = export::glb(&model, 0.0).unwrap();
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    let texture = doc["materials"][0]["normalTexture"]["index"]
        .as_u64()
        .unwrap() as usize;
    let source = doc["textures"][texture]["source"].as_u64().unwrap() as usize;
    let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
    let (_, pixels) = repack::png_pixels(repack::bytes(&glb, &doc, view));
    let actual = pixels[12 * 4..12 * 4 + 3].to_vec();
    let encoded = [n[0], -n[1], n[2]].map(|v| ((v * 0.5 + 0.5) * 255.0).round() as u8);
    assert!(
        actual.iter().zip(encoded).all(|(&a, b)| a.abs_diff(b) <= 1),
        "{name}: {actual:?} != {encoded:?}"
    );
    std::fs::write(out.join(format!("{name}.glb")), glb).unwrap();
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let scene = render::Scene {
        background: [0; 3],
        ..Default::default()
    };
    let image = render::animated_image(&model, camera, scene, [320, 240], 0.0);
    let mut reference = case(alpha, channel, primary, base, detail);
    reference.triangle_normals[..2].fill(None);
    reference.normals[..4].fill([n[0], -n[2], n[1]]);
    let witness = render::animated_image(&reference, camera, scene, [320, 240], 0.0);
    let mut count = 0;
    let mut maximum = 0;
    for (a, b) in image.pixels.iter().zip(&witness.pixels) {
        if *a != eframe::egui::Color32::BLACK && *b != eframe::egui::Color32::BLACK {
            count += 1;
            maximum = maximum.max(
                a.to_array()
                    .into_iter()
                    .zip(b.to_array())
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap(),
            );
        }
    }
    assert!(
        count > 500 && maximum <= 1,
        "{name}: {count} pixels, maximum {maximum}"
    );
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
    let reference_rgba: Vec<_> = witness.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}-reference.png")),
        export::png(&reference_rgba, 320, 240).unwrap(),
    )
    .unwrap();
    json!({"name": name, "normal_expected":n, "normal_baked":actual, "compared_pixels": count, "maximum_channel_error":maximum})
}

#[test]
fn matched_native_normals_preserve_signed_direction_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for channel in 0..3 {
        for alpha in [0, 39, 40, 96, 180, 255] {
            for primary in [false, true] {
                for landmark in 0..2 {
                    receipt.push(capture(out, channel, alpha, primary, landmark));
                }
            }
        }
    }
    std::fs::write(
        out.join("native-normal-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
