//! Prepared package-to-render and independently decoded GLB physical metalness checks.
use super::*;
use eframe::egui;
use paint::{constant, emit, minus, palette_at};

pub(super) fn prefix(code: &mut Vec<u32>, mode: u8) {
    let pristine = if mode == 12 {
        emit(
            code,
            54,
            &[register(0, 29, 2).to_vec(), palette_at(15, 0, 0xFF)],
        );
        source(0, 29, 0x55).to_vec()
    } else {
        source(0, 21, 0xFF).to_vec()
    };
    emit(
        code,
        0,
        &[
            register(0, 29, 1).to_vec(),
            pristine,
            minus(source(0, 22, 0xFF).to_vec()),
        ],
    );
    emit(
        code,
        50,
        &[
            register(0, 30, 1).to_vec(),
            source(0, 28, 0).to_vec(),
            source(0, 29, 0).to_vec(),
            source(0, 22, 0xFF).to_vec(),
        ],
    );
    let raw = if mode == 13 {
        palette_at(15, 2, 0xAA)
    } else {
        constant(3, 0xAA)
    };
    emit(code, 54, &[register(0, 23, 8).to_vec(), raw]);
    emit(
        code,
        0,
        &[
            register(0, 29, 1).to_vec(),
            source(0, 30, 0).to_vec(),
            minus(source(0, 23, 0xFF).to_vec()),
        ],
    );
    emit(
        code,
        50,
        &[
            register(0, 31, 1).to_vec(),
            if mode == 11 {
                source(0, 28, 0)
            } else {
                source(0, 6, 0x55)
            }
            .to_vec(),
            source(0, 29, 0).to_vec(),
            source(0, 23, 0xFF).to_vec(),
        ],
    );
}

pub(super) fn output(code: &mut Vec<u32>, mode: Option<u8>) {
    if mode.is_none_or(|mode| mode < 10) {
        return;
    }
    emit(
        code,
        54,
        &[register(2, 2, 1).to_vec(), source(0, 31, 0).to_vec()],
    );
}

pub(crate) fn case(alpha: u8, mode: u8) -> Result<Model, String> {
    let mut model = paint::case(alpha, mode)?;
    let dye = model.dyes[0].as_mut().unwrap();
    dye.vectors[10][3] = 0.2;
    dye.vectors[20][3] = 1.4;
    dye.surface = crate::dyes::material::properties(&dye.vectors).surfaces[0];
    model
        .surface_overrides
        .lock()
        .unwrap()
        .push(crate::model_preview::SurfaceOverride {
            slot: 0,
            writes: vec![(9, 0, 0.05), (10, 3, 0.35)],
        });
    Ok(model)
}

pub(crate) fn without_dye(alpha: u8, normal: bool) -> Model {
    let mut model = case(alpha, 10).unwrap();
    model.dyes.fill(None);
    model.surface_overrides.lock().unwrap().clear();
    if !normal {
        model.triangle_normals[..2].fill(None);
    }
    model
}

fn expected(alpha: u8, seconds: f32) -> f32 {
    if alpha < 40 {
        return (0.65 + 0.2 * seconds).clamp(0.0, 1.0);
    }
    let wear = ((alpha as f32 - 48.0) / 207.0).clamp(0.0, 1.0);
    let intact = (0.1 + 0.75 * (-0.15 + 1.2 * wear).clamp(0.0, 1.0)).clamp(0.0, 1.0);
    1.0 + intact * (0.35 - 1.0)
}

fn reference(alpha: u8, seconds: f32) -> Model {
    let mut model = paint::case(0, 0).unwrap();
    model.triangle_effects[..2].fill(None);
    let detail = [
        paint::linear(64),
        paint::linear(192),
        paint::linear(96),
        192.0 / 255.0,
    ];
    let (base, rough) = paint::expected(alpha, detail, seconds);
    let albedo = model.triangle_textures[0].unwrap();
    let extra = [0.0, 6.0 / 255.0, 0.1];
    model.textures[albedo].linear = Some(vec![
        [
            base[0] + extra[0],
            base[1] + extra[1],
            base[2] + extra[2],
            1.0
        ];
        32
    ]);
    let gear = model.triangle_gearstacks[0].unwrap();
    model.textures[gear].linear = Some(vec![
        [
            1.0,
            1.0 - rough,
            0.0,
            expected(alpha, seconds) * 32.0 / 255.0
        ];
        32
    ]);
    model
}

fn baked(glb: &[u8]) -> f64 {
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    let mr = &doc["materials"][0]["pbrMetallicRoughness"];
    if let Some(texture) = mr["metallicRoughnessTexture"]["index"].as_u64() {
        let source = doc["textures"][texture as usize]["source"]
            .as_u64()
            .unwrap() as usize;
        let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
        let (_, pixels) = repack::png_pixels(repack::bytes(glb, &doc, view));
        pixels[12 * 4 + 2] as f64 / 255.0
    } else {
        mr["metallicFactor"].as_f64().unwrap()
    }
}

fn image(model: &Model, seconds: f32) -> egui::ColorImage {
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
            key: 0.0,
            fill: 1.0,
            background: [0; 3],
            ..render::Scene::unit_exposure()
        },
        [320, 240],
        seconds,
    )
}

fn save_image(out: &Path, name: &str, image: &egui::ColorImage) {
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
}

fn without_dye_frames(out: &Path, receipt: &mut Vec<serde_json::Value>) {
    for alpha in [0, 39] {
        for normal in [false, true] {
            without_dye_panel(out, receipt, alpha, normal);
        }
    }
}

fn without_dye_panel(out: &Path, receipt: &mut Vec<serde_json::Value>, alpha: u8, normal: bool) {
    let model = without_dye(alpha, normal);
    let mut rewind = None;
    for (step, seconds) in [0.0, 0.5, 5.0, 0.0].into_iter().enumerate() {
        let actual = image(&model, seconds);
        let mut reference_model = reference(alpha, seconds);
        if !normal {
            reference_model.triangle_normals[..2].fill(None);
        }
        let reference = image(&reference_model, seconds);
        let center = actual.pixels[120 * 320 + 160].to_array();
        let witness = reference.pixels[120 * 320 + 160].to_array();
        assert!(
            center.iter().zip(witness).all(|(&a, b)| a.abs_diff(b) <= 2),
            "No selected dye, alpha {alpha}, normal {normal}, time {seconds}: {center:?} != {witness:?}"
        );
        if step == 0 {
            rewind = Some(center);
        }
        if step == 3 {
            assert_eq!(rewind, Some(center));
        }
        let name = format!("native-metal-without-dye-{alpha}-{normal}-{step}");
        save_image(out, &name, &actual);
        save_image(out, &format!("{name}-reference"), &reference);
        let glb = export::glb(&model, seconds).unwrap();
        let metal = baked(&glb);
        let expected = expected(alpha, seconds);
        assert!(
            (metal - expected as f64).abs() < 0.005,
            "{name}: {metal} != {expected}"
        );
        std::fs::write(out.join(format!("{name}.glb")), glb).unwrap();
        receipt.push(json!({"alpha":alpha,"seconds":seconds,"normal":normal,"selected_dye":false,
                    "name":name,"expected_metal":expected,"baked_metal":metal,"actual":center,"reference":witness}));
    }
}

#[test]
fn native_metal_uses_evaluated_constants_selected_dyes_and_exports() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for alpha in [0, 39, 40, 180, 255] {
        let model = case(alpha, 10).unwrap();
        let mut rewind = None;
        for (step, seconds) in [0.0, 0.5, 5.0, 0.0].into_iter().enumerate() {
            let expected = expected(alpha, seconds);
            let actual = image(&model, seconds);
            let reference = image(&reference(alpha, seconds), seconds);
            let center = actual.pixels[120 * 320 + 160].to_array();
            let witness = reference.pixels[120 * 320 + 160].to_array();
            assert!(
                center.iter().zip(witness).all(|(&a, b)| a.abs_diff(b) <= 2),
                "Alpha {alpha}, time {seconds}: {center:?}, independent panel {witness:?}"
            );
            if step == 0 {
                rewind = Some(center);
            }
            if step == 3 {
                assert_eq!(rewind, Some(center));
            }
            let name = format!("native-metal-{alpha}-{step}");
            save_image(out, &name, &actual);
            save_image(out, &format!("{name}-reference"), &reference);
            let glb = export::glb(&model, seconds).unwrap();
            let metal = baked(&glb);
            assert!(
                (metal - expected as f64).abs() < 0.005,
                "{name}: {metal} != {expected}"
            );
            std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
            receipt.push(
                json!({"alpha":alpha,"seconds":seconds,"expected_metal":expected,
                "baked_metal":metal,"actual":center,"reference":witness}),
            );
        }
    }
    for mode in [11, 12, 13] {
        let model = case(0, mode).unwrap();
        assert!(
            model.notices.iter().any(|n| n.contains("metalness")),
            "No fallback diagnostic for {mode}"
        );
        let glb = export::glb(&model, 0.0).unwrap();
        assert_eq!(baked(&glb), 0.0);
        std::fs::write(
            out.join(format!("native-metal-unavailable-{mode}.glb")),
            glb,
        )
        .unwrap();
    }
    without_dye_frames(out, &mut receipt);
    std::fs::write(
        out.join("native-metal-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
