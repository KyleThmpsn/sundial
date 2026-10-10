//! Package-to-render and GLB grain checks prepared before the blue consumer.
use super::*;
use paint::{emit, minus};

pub(super) fn output(code: &mut Vec<u32>) {
    let r = |index, mask| register(0, index, mask).to_vec();
    let s = |index, swizzle| source(0, index, swizzle).to_vec();
    emit(code, 0, &[r(7, 4), s(3, 0xAA), literal(-1.0).to_vec()]);
    for (mask, params) in [(1, 21), (2, 22)] {
        emit(
            code,
            50,
            &[
                r(7, mask),
                s(params, 0x55),
                s(7, 0xAA),
                literal(1.0).to_vec(),
            ],
        );
    }
    emit(code, 0, &[r(7, 1), s(7, 0), minus(s(7, 0x55))]);
    emit(code, 50, &[r(7, 1), s(28, 0), s(7, 0), s(7, 0x55)]);
    emit(code, 51, &[r(7, 1), s(7, 0), s(26, 0)]);
    emit(code, 0, &[r(7, 1), s(7, 0), minus(s(10, 0xFF))]);
    emit(
        code,
        29,
        &[r(0, 1), s(9, 0xFF), literal(40.0 / 255.0).to_vec()],
    );
    emit(code, 1, &[r(0, 1), s(0, 0), literal(1.0).to_vec()]);
    emit(code, 50, &[r(7, 1), s(0, 0), s(7, 0), s(10, 0xFF)]);
    emit(
        code,
        50,
        &[
            r(7, 1),
            s(7, 0),
            literal(0.125).to_vec(),
            literal(0.375).to_vec(),
        ],
    );
}

pub(crate) fn case(alpha: u8, channel: usize, primary: bool, blue: u8, basis: bool) -> Model {
    finish(
        normals::case(
            alpha,
            channel,
            primary,
            [128, 128, 255 - blue, 255],
            [128, 128, blue, 255],
        ),
        channel,
        primary,
        basis,
    )
}

fn finish(mut model: Model, channel: usize, primary: bool, basis: bool) -> Model {
    let slot = channel * 2 + usize::from(!primary);
    let dye = model.dyes[slot].as_mut().unwrap();
    for row in [9, 13] {
        dye.vectors[row][0] = 0.05;
    }
    for row in [12, 16, 19, 23] {
        dye.vectors[row] = [0.0, 0.0, 0.9, 0.0];
    }
    dye.surface = crate::dyes::material::properties(&dye.vectors).surfaces[usize::from(!primary)];
    if !basis {
        model.tangents.clear();
        model.uvs[..4].fill([0.5; 2]);
    }
    model
}

fn roughness(alpha: u8, channel: usize, primary: bool, blue: u8, seconds: f32) -> f32 {
    if alpha < 40 {
        return 1.0 - (0.32 + 0.2 * seconds) * 0.5 * (1.0 + seconds);
    }
    let wear = ((alpha as f32 - 48.0) / 207.0).clamp(0.0, 1.0);
    let intact = (0.1 + 0.75 * (-0.15 + 1.2 * wear).clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let strength = if primary {
        intact
    } else {
        1.0 + intact * (0.35 - 1.0)
    };
    let limit = (blue as f32 / 255.0 + [0.4, -0.1, 0.65][channel] + 0.2 * seconds).clamp(0.0, 1.0);
    1.0 - 0.9f32.min(1.0 + strength * (limit - 1.0))
}

fn reference(
    alpha: u8,
    channel: usize,
    primary: bool,
    blue: u8,
    basis: bool,
    seconds: f32,
) -> Model {
    let mut model = case(alpha, channel, primary, blue, basis);
    let slot = channel * 2 + usize::from(!primary);
    model.triangle_effects[..2].fill(None);
    model.triangle_normals[..2].fill(None);
    let albedo = model.triangle_textures[0].unwrap();
    let detail = [
        paint::linear(64),
        paint::linear(192),
        paint::linear(96),
        192.0 / 255.0,
    ];
    let color = paint::expected(alpha, detail, seconds).0;
    let extra = [0.0, 6.0 / 255.0, 0.1];
    model.textures[albedo].linear = Some(vec![
        std::array::from_fn(|i| if i < 3 {
            color[i] + extra[i]
        } else {
            1.0
        });
        32
    ]);
    let gear = model.triangle_gearstacks[0].unwrap();
    model.textures[gear].linear = Some(vec![
        [
            1.0,
            1.0 - roughness(alpha, channel, primary, blue, seconds),
            0.0,
            if alpha < 40 {
                (alpha as f32 / 32.0).clamp(0.0, 1.0) * 32.0 / 255.0
            } else {
                0.0
            }
        ];
        32
    ]);
    let dye = model.dyes[slot].as_mut().unwrap();
    dye.detail = None;
    dye.normal = None;
    dye.surface = crate::dyes::material::Surface {
        iridescence: -1.0,
        ..Default::default()
    };
    model.surface_overrides.lock().unwrap().clear();
    if basis {
        let n = super::normals::expected(
            alpha,
            channel,
            primary,
            [128, 128, 255 - blue, 255],
            [128, 128, blue, 255],
        );
        model.normals[..4].fill([n[0], -n[2], n[1]]);
    }
    model
}

fn image(model: &Model, seconds: f32) -> eframe::egui::ColorImage {
    render::animated_image(
        model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene {
            filmic: false,
            bloom: false,
            background: [0; 3],
            ..render::Scene::unit_exposure()
        },
        [320, 240],
        seconds,
    )
}

fn save(out: &Path, name: &str, image: &eframe::egui::ColorImage) {
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
}

fn material(glb: &[u8]) -> (f32, bool) {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    let first = &doc["materials"][0];
    let mr = &first["pbrMetallicRoughness"];
    let roughness = if let Some(texture) = mr["metallicRoughnessTexture"]["index"].as_u64() {
        let source = doc["textures"][texture as usize]["source"]
            .as_u64()
            .unwrap() as usize;
        let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
        let (size, pixels) = repack::png_pixels(repack::bytes(glb, &doc, view));
        let center = (size[1] / 2 * size[0] + size[0] / 2) * 4;
        assert_eq!(pixels[center], 255, "Normal blue was baked as occlusion");
        pixels[center + 1] as f32 / 255.0
    } else {
        mr["roughnessFactor"].as_f64().unwrap() as f32
    };
    (roughness, first["normalTexture"].is_object())
}

#[derive(Clone, Copy)]
struct Capture {
    alpha: u8,
    channel: usize,
    primary: bool,
    blue: u8,
    basis: bool,
}
impl Capture {
    fn run(&self, out: &Path, seconds: f32, step: usize) -> serde_json::Value {
        let Self {
            alpha,
            channel,
            primary,
            blue,
            basis,
        } = *self;
        let model = case(alpha, channel, primary, blue, basis);
        let name = format!("native-normal-blue-{channel}-{alpha}-{primary}-{blue}-{basis}-{step}");
        let actual = image(&model, seconds);
        let witness = image(
            &reference(alpha, channel, primary, blue, basis, seconds),
            seconds,
        );
        save(out, &name, &actual);
        save(out, &format!("{name}-reference"), &witness);
        let glb = export::glb(&model, seconds).unwrap();
        std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
        let mut count = 0;
        let mut maximum = 0;
        for (a, b) in actual.pixels.iter().zip(&witness.pixels) {
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
        let (baked, has_normal) = material(&glb);
        let expected = roughness(alpha, channel, primary, blue, seconds);
        assert!(
            (baked - expected).abs() <= 0.5 / 255.0 + 1e-6,
            "{name}: roughness {baked} != {expected}"
        );
        assert_eq!(has_normal, basis, "{name}: basis omission");
        json!({"name":name,"alpha":alpha,"channel":channel,"primary":primary,"blue":blue,"basis":basis,"seconds":seconds,"roughness_expected":expected,"roughness_baked":baked,"compared_pixels":count,"maximum_channel_error":maximum})
    }
}

fn fallback(out: &Path, mode: u8) -> serde_json::Value {
    let model = finish(
        normals::case_at(180, 2, false, [128, 128, 255, 255], [0, 0, 0, 255], mode),
        2,
        false,
        true,
    );
    let name = format!("native-normal-blue-fallback-{mode}");
    let glb = export::glb(&model, 0.0).unwrap();
    let (baked, has_normal) = material(&glb);
    assert!((baked - 0.1).abs() <= 0.5 / 255.0 + 1e-6, "{name}: {baked}");
    assert!(has_normal, "{name}: supported direction was lost");
    if mode == 45 {
        assert!(
            model.notices.iter().any(|n| n.contains("normal grain")),
            "Unsupported grain was silent"
        );
    }
    save(out, &name, &image(&model, 0.0));
    std::fs::write(out.join(format!("{name}.glb")), glb).unwrap();
    json!({"name":name,"mode":mode,"roughness_expected":0.1,"roughness_baked":baked,"notices":model.notices})
}

pub(crate) fn spatial_case(channel: usize) -> Model {
    let mut model = case(180, channel, true, 0, true);
    let detail = model.dyes[channel * 2].unwrap().normal.unwrap();
    model.textures[detail].size = [2, 1];
    model.textures[detail].rgba = vec![128, 128, 0, 255, 128, 128, 255, 255];
    model.detail_uvs[..4].copy_from_slice(&[[0.25, 0.5], [0.75, 0.5], [0.75, 0.5], [0.25, 0.5]]);
    model
}

fn spatial(out: &Path, channel: usize) -> serde_json::Value {
    let model = spatial_case(channel);
    let name = format!("native-normal-blue-spatial-{channel}");
    let glb = export::glb(&model, 0.0).unwrap();
    save(out, &name, &image(&model, 0.0));
    std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    let index = doc["materials"][0]["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"]
        .as_u64()
        .expect("Varying grain must bake an ORM texture") as usize;
    let source = doc["textures"][index]["source"].as_u64().unwrap() as usize;
    let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
    let (size, pixels) = repack::png_pixels(repack::bytes(&glb, &doc, view));
    assert_eq!(size, [8, 4]);
    let raw = (180.0 - 48.0) / 207.0;
    let intact = 0.1 + 0.75 * (-0.15 + 1.2 * raw);
    let mut landmarks = Vec::new();
    for y in 0..2 {
        for x in 2..6 {
            let coordinate = (x as f32 + 0.5) / 8.0;
            let blue = 2.0 * coordinate - 0.5;
            let limit = (blue + [0.4, -0.1, 0.65][channel]).clamp(0.0, 1.0);
            let expected = 1.0 - 0.9f32.min(1.0 + intact * (limit - 1.0));
            let at = (y * 8 + x) * 4;
            assert_eq!(pixels[at], 255, "{name}: blue became occlusion");
            let actual = pixels[at + 1] as f32 / 255.0;
            assert!(
                (actual - expected).abs() <= 0.5 / 255.0 + 1e-6,
                "{name} ({x},{y}): {actual} != {expected}"
            );
            landmarks
                .push(json!({"x":x,"y":y,"roughness_expected":expected,"roughness_actual":actual}));
        }
    }
    json!({"name":name,"channel":channel,"size":size,"landmarks":landmarks})
}

#[test]
fn native_normal_blue_keeps_grain_without_ambient_occlusion() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for channel in 0..3 {
        for alpha in [0, 39, 40, 96, 180, 255] {
            for primary in [false, true] {
                for blue in [0, 255] {
                    receipt.push(
                        Capture {
                            alpha,
                            channel,
                            primary,
                            blue,
                            basis: true,
                        }
                        .run(out, 0.0, 10),
                    );
                }
            }
        }
    }
    for basis in [false, true] {
        for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
            receipt.push(
                Capture {
                    alpha: 180,
                    channel: 1,
                    primary: false,
                    blue: 0,
                    basis,
                }
                .run(out, seconds, step),
            );
        }
    }
    for mode in [45, 46] {
        receipt.push(fallback(out, mode));
    }
    let spatial: Vec<_> = (0..3).map(|channel| spatial(out, channel)).collect();
    std::fs::write(
        out.join("native-normal-blue-spatial-receipt.json"),
        serde_json::to_vec_pretty(&spatial).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("native-normal-blue-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
