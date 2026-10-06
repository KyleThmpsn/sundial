//! Package-to-render native color ordering, boundary and authored dye witnesses.
//! Prepared before the bounded native paint implementation.
use super::*;

pub(super) fn constant(row: u32, swizzle: u32) -> Vec<u32> {
    vec![0x0020_8006 | swizzle << 4, 0, row]
}

fn palette(lane: u32, swizzle: u32) -> Vec<u32> {
    palette_at(12, lane, swizzle)
}
pub(super) fn palette_at(temp: u32, lane: u32, swizzle: u32) -> Vec<u32> {
    let address = source(0, temp, lane * 0x55);
    vec![0x0620_8006 | swizzle << 4, 0, 0, address[0], address[1]]
}

pub(in crate::model_preview::compatibility_tests) fn minus(mut value: Vec<u32>) -> Vec<u32> {
    value[0] |= 0x8000_0000;
    value.insert(1, 0x41);
    value
}

pub(in crate::model_preview::compatibility_tests) fn emit(
    code: &mut Vec<u32>,
    opcode: u32,
    args: &[Vec<u32>],
) {
    code.extend(instruction(
        opcode,
        &args.iter().map(Vec::as_slice).collect::<Vec<_>>(),
    ));
}

fn factors(code: &mut Vec<u32>, value: u32, high: u32, low: u32, malformed: bool) {
    emit(
        code,
        56 | 1 << 13,
        &[
            register(0, high, 7).to_vec(),
            source(0, value, 0xE4).to_vec(),
            literal(if malformed { 3.0 } else { 4.0 }).to_vec(),
        ],
    );
    emit(
        code,
        1 << 13,
        &[
            register(0, low, 7).to_vec(),
            source(0, value, 0xE4).to_vec(),
            literal(-0.25).to_vec(),
        ],
    );
}

fn color(code: &mut Vec<u32>, target: u32, color: Vec<u32>, params: u32) {
    emit(
        code,
        50 | 1 << 13,
        &[
            register(0, target, 7).to_vec(),
            color.clone(),
            source(0, 17, 0xE4).to_vec(),
            source(0, 18, 0xE4).to_vec(),
        ],
    );
    emit(
        code,
        0,
        &[
            register(0, target, 7).to_vec(),
            source(0, target, 0xE4).to_vec(),
            minus(color.clone()),
        ],
    );
    emit(
        code,
        50,
        &[
            register(0, target, 7).to_vec(),
            source(0, params, 0).to_vec(),
            source(0, target, 0xE4).to_vec(),
            color,
        ],
    );
    emit(
        code,
        50 | 1 << 13,
        &[
            register(0, target, 7).to_vec(),
            source(0, target, 0xE4).to_vec(),
            source(0, 19, 0xE4).to_vec(),
            source(0, 20, 0xE4).to_vec(),
        ],
    );
}

fn remapped(code: &mut Vec<u32>, target: u32, field: Vec<u32>, raw: Vec<u32>) {
    let lane = |swizzle: u32| {
        let mut value = field.clone();
        value[0] = value[0] & !0xFF0 | swizzle << 4;
        value
    };
    emit(
        code,
        50 | 1 << 13,
        &[register(0, target, 1).to_vec(), lane(0x55), raw, lane(0)],
    );
    emit(
        code,
        50 | 1 << 13,
        &[
            register(0, target, 1).to_vec(),
            lane(0xFF),
            source(0, target, 0).to_vec(),
            lane(0xAA),
        ],
    );
}

fn smoothness(code: &mut Vec<u32>) {
    emit(
        code,
        56 | 1 << 13,
        &[
            register(0, 29, 1).to_vec(),
            constant(1, 0),
            literal(4.0).to_vec(),
        ],
    );
    emit(
        code,
        1 << 13,
        &[
            register(0, 30, 1).to_vec(),
            constant(1, 0),
            literal(-0.25).to_vec(),
        ],
    );
    emit(
        code,
        50 | 1 << 13,
        &[
            register(0, 31, 1).to_vec(),
            source(0, 16, 0xFF).to_vec(),
            source(0, 29, 0).to_vec(),
            source(0, 30, 0).to_vec(),
        ],
    );
    remapped(code, 29, palette_at(15, 2, 0xE4), source(0, 31, 0).to_vec());
    remapped(code, 30, palette(3, 0xE4), source(0, 31, 0).to_vec());
    for (raw, detailed, params) in [(26, 29, 21), (27, 30, 22)] {
        emit(
            code,
            0,
            &[
                register(0, detailed, 1).to_vec(),
                minus(source(0, raw, 0).to_vec()),
                source(0, detailed, 0).to_vec(),
            ],
        );
        emit(
            code,
            50,
            &[
                register(0, raw, 1).to_vec(),
                source(0, params, 0xAA).to_vec(),
                source(0, detailed, 0).to_vec(),
                source(0, raw, 0).to_vec(),
            ],
        );
    }
    emit(
        code,
        0,
        &[
            register(0, 29, 1).to_vec(),
            minus(source(0, 27, 0).to_vec()),
            source(0, 26, 0).to_vec(),
        ],
    );
    emit(
        code,
        50,
        &[
            register(0, 26, 1).to_vec(),
            source(0, 28, 0).to_vec(),
            source(0, 29, 0).to_vec(),
            source(0, 27, 0).to_vec(),
        ],
    );
}

pub(super) fn prefix(malformed: u8) -> Vec<u32> {
    let mut code = instruction(104, &[&[32]]);
    code.extend(instruction(89 | 1 << 11, &[&[0x0020_8000, 0, 64]]));
    code.extend(instruction(98, &[&register(1, 3, 15)]));
    if malformed >= 40 {
        super::normals::declarations(&mut code);
    }
    for target in 0..3 {
        code.extend(instruction(101, &[&register(2, target, 15)]));
    }
    for slot in [0, 2, 4, 10, 11] {
        code.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    let scale = vec![0x4002, 0.5f32.to_bits(), 0.25f32.to_bits(), 0, 0];
    let offset = vec![0x4002, 0.25f32.to_bits(), 0.125f32.to_bits(), 0, 0];
    emit(
        &mut code,
        50,
        &[
            register(0, 11, 3).to_vec(),
            source(1, 3, 0xE4).to_vec(),
            scale,
            offset,
        ],
    );
    for (target, slot, mask, uv) in [
        (1, 0, 7, source(0, 11, 0x44)),
        (9, 2, 8, source(0, 11, 0x44)),
        (16, 4, 15, source(1, 3, 0xEE)),
    ] {
        emit(
            &mut code,
            69,
            &[
                register(0, target, mask).to_vec(),
                uv.to_vec(),
                vec![0x0010_7E46, slot],
                vec![0x0010_6000, 1],
            ],
        );
    }
    let threshold = 40.0 / 255.0 + if malformed == 1 { 0.0000001 } else { 0.0 };
    emit(
        &mut code,
        29,
        &[
            register(0, 6, 2).to_vec(),
            source(0, 9, 0xFF).to_vec(),
            literal(threshold).to_vec(),
        ],
    );
    emit(
        &mut code,
        1,
        &[
            register(0, 6, 2).to_vec(),
            source(0, 6, 0x55).to_vec(),
            literal(1.0).to_vec(),
        ],
    );
    // A different palette base, same nine-row semantic relationships.
    emit(
        &mut code,
        54,
        &[register(0, 13, 1).to_vec(), literal(0.0).to_vec()],
    );
    emit(
        &mut code,
        56,
        &[
            register(0, 13, 1).to_vec(),
            source(0, 13, 0).to_vec(),
            literal(9.0).to_vec(),
        ],
    );
    let biases = vec![
        0x4002,
        6f32.to_bits(),
        12f32.to_bits(),
        11f32.to_bits(),
        13f32.to_bits(),
    ];
    emit(
        &mut code,
        0,
        &[
            register(0, 12, 15).to_vec(),
            source(0, 13, 0).to_vec(),
            biases,
        ],
    );
    emit(
        &mut code,
        28,
        &[register(0, 12, 15).to_vec(), source(0, 12, 0xE4).to_vec()],
    );
    let biases = vec![0x4002, 8f32.to_bits(), 14f32.to_bits(), 10f32.to_bits(), 0];
    emit(
        &mut code,
        0,
        &[
            register(0, 15, 15).to_vec(),
            source(0, 13, 0).to_vec(),
            biases,
        ],
    );
    emit(
        &mut code,
        28,
        &[register(0, 15, 15).to_vec(), source(0, 15, 0xE4).to_vec()],
    );
    emit(
        &mut code,
        54 | 1 << 13,
        &[register(0, 21, 15).to_vec(), palette_at(15, 0, 0xE4)],
    );
    emit(
        &mut code,
        54 | 1 << 13,
        &[register(0, 22, 15).to_vec(), palette_at(15, 1, 0xE4)],
    );
    factors(&mut code, 16, 17, 18, false);
    factors(&mut code, 1, 19, 20, malformed == 2);
    color(&mut code, 23, palette(0, 0xE4), 21);
    color(&mut code, 24, palette(2, 0xE4), 22);
    emit(
        &mut code,
        1 << 13,
        &[
            register(0, 6, 4).to_vec(),
            source(0, 9, 0xFF).to_vec(),
            literal(-48.0 / 255.0).to_vec(),
        ],
    );
    emit(
        &mut code,
        56 | 1 << 13,
        &[
            register(0, 6, 4).to_vec(),
            source(0, 6, 0xAA).to_vec(),
            literal(255.0 / 207.0).to_vec(),
        ],
    );
    remapped(&mut code, 28, palette(1, 0xE4), source(0, 6, 0xAA).to_vec());
    remapped(&mut code, 26, palette_at(15, 2, 0xE4), constant(1, 0));
    remapped(&mut code, 27, palette(3, 0xE4), constant(1, 0));
    smoothness(&mut code);
    emit(
        &mut code,
        0,
        &[
            register(0, 25, 7).to_vec(),
            minus(source(0, 24, 0xE4).to_vec()),
            source(0, 23, 0xE4).to_vec(),
        ],
    );
    emit(
        &mut code,
        50,
        &[
            register(0, 7, 7).to_vec(),
            source(0, 28, 0).to_vec(),
            source(0, 25, 0xE4).to_vec(),
            source(0, 24, 0xE4).to_vec(),
        ],
    );
    emit(&mut code, 54, &[register(0, 1, 8).to_vec(), constant(1, 0)]);
    emit(
        &mut code,
        56,
        &[
            register(0, 10, 15).to_vec(),
            source(0, 1, 0xE4).to_vec(),
            constant(2, 0xE4),
        ],
    );
    emit(
        &mut code,
        50,
        &[
            register(0, 7, 7).to_vec(),
            minus(source(0, 1, 0xE4).to_vec()),
            constant(2, 0xE4),
            source(0, 7, 0xE4).to_vec(),
        ],
    );
    emit(
        &mut code,
        50,
        &[
            register(0, 7, 7).to_vec(),
            source(0, 6, 0x55).to_vec(),
            source(0, 7, 0xE4).to_vec(),
            source(0, 10, 0xE4).to_vec(),
        ],
    );
    if (10..40).contains(&malformed) {
        super::metal::prefix(&mut code, malformed);
    }
    code
}

pub(super) fn material(bytes: &mut Vec<u8>) {
    let mut vectors = vec![0; 64 * 16];
    floats(&mut vectors, 0, &[0.1, 0.0, 0.0, 0.0]);
    floats(&mut vectors, 16, &[0.32, 0.0, 0.0, 0.0]);
    floats(&mut vectors, 32, &[0.25, 0.5, 0.75, 0.5]);
    floats(&mut vectors, 48, &[0.0, 0.0, 0.65, 0.0]);
    for row in 6..60 {
        floats(&mut vectors, row * 16, &[0.0, 0.9, 0.0, 1.0]);
    }
    array(bytes, 0x318, 0x80800090, &vectors, 16);
    let mut constants = [0; 32];
    floats(&mut constants, 0, &[0.2; 4]);
    floats(&mut constants, 16, &[1.0; 4]);
    array(bytes, 0x2F8, 0x80800090, &constants, 16);
    let mut expression = vec![
        0x3C, 1, 0, 0x34, 0, 0x03, 0x42, 1, 0x01, 0x43, 1, 0x3C, 1, 0, 0x34, 1, 0x01, 0x42, 2,
        0x03, 0x43, 2,
    ];
    expression.extend([0x3C, 1, 0, 0x34, 0, 0x03, 0x42, 3, 0x01, 0x43, 3]);
    array(bytes, 0x2E8, 0x80800009, &expression, 1);
}

pub(super) fn finish(model: &mut Model, alpha: u8) {
    let index = model.triangle_gearstacks[0].unwrap();
    model.textures[index].rgba = [255, 240, 0, alpha].repeat(32);
    let detail = model.textures.len();
    model.textures.push(crate::model_preview::texture::Texture {
        mips: None,
        tag: 0xD371,
        size: [3, 1],
        rgba: [[64, 192, 96, 192], [192, 32, 224, 32], [128, 96, 32, 128]]
            .into_iter()
            .flatten()
            .collect(),
        linear: None,
    });
    let mut vectors = [[0.0; 4]; 27];
    vectors[0] = [1.0, 1.0, 0.0, 0.0];
    vectors[1] = vectors[0];
    vectors[9] = [0.15, 0.6, 0.03, 1.0];
    vectors[10] = [0.8, 0.0, 0.2, 0.0];
    vectors[11] = [-1.0, 0.0, 0.0, 0.0];
    vectors[12] = [-0.2, 1.1, 0.15, 0.65];
    vectors[17] = [0.7, 0.02, 0.3, 1.0];
    vectors[18] = [-0.15, 1.2, 0.1, 0.75];
    vectors[19] = [0.2, 0.5, 0.1, 0.8];
    vectors[20] = [0.25, 0.0, 0.9, 0.0];
    model.dyes[0] = Some(crate::model_preview::shader::Dye {
        surface: crate::dyes::material::properties(&vectors).surfaces[0],
        detail: Some(detail),
        normal: None,
        transform: vectors[0],
        normal_transform: vectors[1],
        vectors,
    });
    model.triangle_dyes = vec![0; 4];
    model.detail_uvs = model.uvs.clone();
    model.detail_uvs[..4].fill([1.0 / 6.0, 0.5]);
    model.triangle_detail_uv = vec![true; 2];
}

pub(crate) fn case(alpha: u8, malformed: u8) -> Result<Model, String> {
    opaque::paint_case(alpha, malformed)
}

pub(super) fn linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn map(v: f32, m: [f32; 4]) -> f32 {
    (m[2] + m[3] * (m[0] + m[1] * v).clamp(0.0, 1.0)).clamp(0.0, 1.0)
}
fn layer(base: [f32; 3], detail: [f32; 4], color: [f32; 3], strength: f32) -> [f32; 3] {
    std::array::from_fn(|i| {
        let tint = (color[i] * (4.0 * detail[i]).clamp(0.0, 1.0)
            + (detail[i] - 0.25).clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
        let mixed = color[i] + strength.clamp(0.0, 1.0) * (tint - color[i]);
        (mixed * (base[i] * 4.0).clamp(0.0, 1.0) + (base[i] - 0.25).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    })
}

pub(super) fn expected(alpha: u8, detail: [f32; 4], seconds: f32) -> ([f32; 3], f32) {
    let base = [128, 96, 64].map(linear);
    let smooth = 0.32 + 0.2 * seconds;
    if alpha < 40 {
        return (
            std::array::from_fn(|i| base[i] * [0.25, 0.5, 0.75][i] * (1.0 + seconds)),
            1.0 - smooth * 0.5 * (1.0 + seconds),
        );
    }
    let intact = map(
        ((alpha as f32 - 48.0) / 207.0).clamp(0.0, 1.0),
        [-0.15, 1.2, 0.1, 0.75],
    );
    let a = layer(base, detail, [0.7, 0.02, 0.3], 0.25);
    let b = layer(base, detail, [0.05, 0.6, 0.03], 0.8);
    let color = std::array::from_fn(|i| a[i] + intact * (b[i] - a[i]));
    let detailed = (detail[3] * (4.0 * smooth).clamp(0.0, 1.0) + (smooth - 0.25).clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
    let s = |m, p| {
        let raw = map(smooth, m);
        raw + p * (map(detailed, m) - raw)
    };
    let worn = s([0.2, 0.5, 0.1, 0.8], 0.9);
    let pristine = s([-0.2, 1.1, 0.15, 0.65], 0.2);
    (color, 1.0 - (worn + intact * (pristine - worn)))
}

fn preview_color(alpha: u8, actual: [u8; 4], expected: [f32; 3], landmark: usize, seconds: f32) {
    if alpha < 40 {
        return;
    }
    for i in [1, 2] {
        let extra = [0.0, 6.0 / 255.0, 0.1];
        assert!(
            (linear(actual[i]) - linear(actual[0]) - (expected[i] - expected[0] + extra[i])).abs()
                < 0.02,
            "Alpha {alpha}, detail {landmark}, time {seconds}: {actual:?}, expected {expected:?}"
        );
    }
}

fn image_pixels(glb: &[u8], doc: &serde_json::Value, texture: u64) -> ([usize; 2], Vec<u8>) {
    let source = doc["textures"][texture as usize]["source"]
        .as_u64()
        .unwrap() as usize;
    let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
    repack::png_pixels(repack::bytes(glb, doc, view))
}

fn baked(glb: &[u8], expected: [f32; 3], rough: f32, name: &str) -> ([u8; 3], f64) {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    let mr = &doc["materials"][0]["pbrMetallicRoughness"];
    let (size, pixels) = image_pixels(glb, &doc, mr["baseColorTexture"]["index"].as_u64().unwrap());
    assert_eq!(size, [8, 4]);
    let pixel: [u8; 3] = pixels[12 * 4..12 * 4 + 3].try_into().unwrap();
    let encoded = encoded(expected);
    assert!(
        pixel.iter().zip(encoded).all(|(&a, b)| a.abs_diff(b) <= 1),
        "Export {name}: {pixel:?}, expected {encoded:?}"
    );
    let rough_actual = if let Some(texture) = mr["metallicRoughnessTexture"]["index"].as_u64() {
        image_pixels(glb, &doc, texture).1[12 * 4 + 1] as f64 / 255.0
    } else {
        mr["roughnessFactor"].as_f64().unwrap()
    };
    assert!(
        (rough_actual - rough as f64).abs() < 0.005,
        "Roughness {name}: {rough_actual} != {rough}"
    );
    (pixel, rough_actual)
}

#[test]
fn native_paint_preserves_stage_order_selected_dyes_boundaries_and_exports() {
    for malformed in [1, 2] {
        assert!(
            case(180, malformed).is_err(),
            "Malformed native paint {malformed} was accepted"
        );
    }
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for alpha in [0, 39, 40, 96, 180, 255] {
        let mut model = case(alpha, 0).unwrap();
        // A selected authored dye must override the deliberately unrelated stored bank.
        model
            .surface_overrides
            .lock()
            .unwrap()
            .push(crate::model_preview::SurfaceOverride {
                slot: 0,
                writes: vec![(9, 0, 0.05)],
            });
        for (landmark, color) in [[64, 192, 96, 192], [192, 32, 224, 32], [128, 96, 32, 128]]
            .into_iter()
            .enumerate()
        {
            model.detail_uvs[..4].fill([(landmark as f32 + 0.5) / 3.0, 0.5]);
            let detail = [
                linear(color[0]),
                linear(color[1]),
                linear(color[2]),
                color[3] as f32 / 255.0,
            ];
            let mut rewind = None;
            for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
                let (expected, rough) = expected(alpha, detail, seconds);
                let image = render::animated_image(
                    &model,
                    render::Camera {
                        yaw: 0.0,
                        pitch: 0.0,
                        ..Default::default()
                    },
                    render::Scene {
                        key: 0.0,
                        fill: 1.0,
                        background: [0; 3],
                        ..Default::default()
                    },
                    [320, 240],
                    seconds,
                );
                let actual = image.pixels[120 * 320 + 160].to_array();
                preview_color(alpha, actual, expected, landmark, seconds);
                if step == 0 {
                    rewind = Some(actual);
                }
                if step == 3 {
                    assert_eq!(rewind, Some(actual));
                }
                let name = format!("native-paint-{alpha}-{landmark}-{step}");
                let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                std::fs::write(
                    out.join(format!("{name}.png")),
                    export::png(&rgba, 320, 240).unwrap(),
                )
                .unwrap();
                let glb = export::glb(&model, seconds).unwrap();
                let (pixel, rough_actual) = baked(&glb, expected, rough, &name);
                std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
                receipt.push(json!({"alpha":alpha,"landmark":landmark,"seconds":seconds,"selected_dye_red":0.05,"expected_unlit":expected,"actual":actual,"baked":pixel,"roughness_expected":rough,"roughness_actual":rough_actual}));
            }
        }
    }
    std::fs::write(
        out.join("native-paint-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
