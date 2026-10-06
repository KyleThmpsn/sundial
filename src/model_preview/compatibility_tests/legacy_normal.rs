//! Original native normal witnesses through package loading, rendering and GLB export.
use super::*;
use effects::native::repack;
pub(super) mod fixture;

fn rows() -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(include_str!("legacy_normal/swatches.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn dye(row: &serde_json::Value, decode: [f32; 3]) -> shader::Dye {
    let case = &row["case"];
    let mut vectors = [[0.0; 4]; 27];
    vectors[1] = [1.0, 1.0, 0.0, 0.0];
    vectors[2] = [decode[0], decode[1], decode[2], 0.0];
    for surface in 0..2 {
        vectors[9 + surface * 4] = [0.25; 4];
        vectors[17 + surface * 4] = [0.25; 4];
        vectors[10 + surface * 4][1] = case["strengths"][0].as_f64().unwrap() as f32;
        vectors[20 + surface * 4][1] = case["strengths"][1].as_f64().unwrap() as f32;
        vectors[18 + surface * 4] = [-0.15, 1.2, 0.1, 0.75];
        vectors[12 + surface * 4] = [0.0, 0.0, 1.0, 0.0];
        vectors[19 + surface * 4] = [0.0, 0.0, 1.0, 0.0];
        vectors[11 + surface * 4][0] = -1.0;
    }
    shader::Dye {
        surface: crate::dyes::material::properties(&vectors).surfaces
            [case["surface"].as_u64().unwrap() as usize],
        detail: None,
        normal: None,
        transform: [1.0, 1.0, 0.0, 0.0],
        normal_transform: [1.0, 1.0, 0.0, 0.0],
        vectors,
    }
}

fn model(
    row: &serde_json::Value,
    channel: usize,
    decode: [f32; 3],
    basis: bool,
    malformed: u8,
) -> Model {
    let case = &row["case"];
    let surface = case["surface"].as_u64().unwrap() as usize;
    let slot = channel * 2 + surface;
    let mut model = fixture::load(slot as u8, malformed);
    let albedo = model.triangle_textures[0].unwrap();
    let normal = model.triangle_normals[0].unwrap();
    let mask = model.triangle_gearstacks[0].unwrap();
    // These supplied swatches replace the package images, including their mip data.
    for index in [albedo, normal, mask] {
        model.textures[index].mips = None;
    }
    model.textures[albedo].rgba = [137, 137, 137, 255].repeat(64);
    model.textures[albedo].linear = Some(vec![[0.25, 0.25, 0.25, 1.0]; 64]);
    let base: [u8; 4] = std::array::from_fn(|i| {
        if i < 3 {
            case["base"][i].as_u64().unwrap() as u8
        } else {
            255
        }
    });
    model.textures[normal].rgba = base.repeat(64);
    model.textures[mask].rgba = [255, 255, 0, case["alpha"].as_u64().unwrap() as u8].repeat(64);
    let index = model.textures.len();
    model.textures.push(texture::Texture {
        mips: None,
        tag: 0xA006,
        size: [1, 1],
        rgba: (0..4)
            .map(|i| case["detail"][i].as_u64().unwrap() as u8)
            .collect(),
        linear: None,
    });
    let mut dye = dye(row, decode);
    dye.normal = Some(index);
    model.dyes[slot] = Some(dye);
    if basis {
        model.tangents = vec![[1.0, 0.0, 0.0, 1.0]; model.vertices.len()];
    }
    model
}

fn expected(row: &serde_json::Value, decode: [f32; 3], seconds: f32) -> ([f32; 3], f32) {
    let case = &row["case"];
    let alpha = case["alpha"].as_f64().unwrap();
    let clamp = |v: f64| v.clamp(0.0, 1.0);
    let raw = clamp((alpha - 48.0) / 207.0);
    let intact = clamp(0.1 + 0.75 * clamp(-0.15 + 1.2 * raw));
    let worn = clamp(case["strengths"][1].as_f64().unwrap());
    let strength = worn + intact * (case["strengths"][0].as_f64().unwrap() - worn);
    let xy: [f64; 2] = std::array::from_fn(|i| {
        let base = case["base"][i].as_f64().unwrap() / 128.0 - 1.0;
        let detail = case["detail"][i].as_f64().unwrap() / 255.0 * f64::from(decode[0])
            + f64::from(decode[1]);
        base + if alpha >= 40.0 {
            strength * detail
        } else {
            0.0
        }
    });
    let vector = [
        xy[0],
        xy[1],
        (1.0 - xy[0] * xy[0] - xy[1] * xy[1]).max(0.0).sqrt(),
    ];
    let length = vector.iter().map(|v| v * v).sum::<f64>().sqrt();
    let base_limit =
        clamp(case["base"][2].as_f64().unwrap() / 255.0 + 0.4 + 0.2 * f64::from(seconds));
    let detail_limit = 1.0
        + strength
            * (clamp(case["detail"][2].as_f64().unwrap() / 255.0 + f64::from(decode[2])) - 1.0);
    (
        vector.map(|v| (v / length) as f32),
        (1.0 - 1.0f64.min(base_limit).min(detail_limit)) as f32,
    )
}

fn reference(
    row: &serde_json::Value,
    channel: usize,
    decode: [f32; 3],
    basis: bool,
    seconds: f32,
) -> Model {
    let mut model = model(row, channel, decode, basis, 0);
    model.triangle_effects.fill(None);
    model.triangle_normals.fill(None);
    let (normal, roughness) = expected(row, decode, seconds);
    if basis {
        model.normals.fill([normal[0], -normal[2], normal[1]]);
    }
    let mask = model.triangle_gearstacks[0].unwrap();
    let alpha = row["case"]["alpha"].as_u64().unwrap() as f32;
    // Preserve the native unpainted metal input while replacing only normal and smoothness.
    let unpainted_alpha = if alpha < 40.0 { alpha / 255.0 } else { 0.0 };
    model.textures[mask].linear = Some(vec![[1.0, 1.0 - roughness, 0.0, unpainted_alpha]; 64]);
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
            exposure: 0.4,
            background: [0; 3],
            ..Default::default()
        },
        [160, 128],
        seconds,
    )
}
fn save(out: &Path, name: &str, image: &eframe::egui::ColorImage) {
    let rgba: Vec<_> = image.pixels.iter().flat_map(|v| v.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&rgba, image.size[0], image.size[1]).unwrap(),
    )
    .unwrap();
}

fn glb_landmarks(glb: &[u8]) -> (Option<[u8; 3]>, f32, f32) {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    let material = &doc["materials"][0];
    let pixel = |texture: u64| {
        let image = doc["textures"][texture as usize]["source"]
            .as_u64()
            .unwrap() as usize;
        let view = doc["images"][image]["bufferView"].as_u64().unwrap() as usize;
        let (size, pixels) = repack::png_pixels(repack::bytes(glb, &doc, view));
        let at = (size[1] / 2 * size[0] + size[0] / 2) * 4;
        <[u8; 3]>::try_from(&pixels[at..at + 3]).unwrap()
    };
    let normal = material["normalTexture"]["index"].as_u64().map(pixel);
    let mr = &material["pbrMetallicRoughness"];
    let rough = mr["metallicRoughnessTexture"]["index"]
        .as_u64()
        .map_or_else(
            || mr["roughnessFactor"].as_f64().unwrap() as f32,
            |v| f32::from(pixel(v)[1]) / 255.0,
        );
    let ao = material["occlusionTexture"]["index"]
        .as_u64()
        .map_or(1.0, |v| f32::from(pixel(v)[0]) / 255.0);
    (normal, rough, ao)
}

fn selected_row(over_unit: bool) -> serde_json::Value {
    let mut row = rows().remove(4);
    row["case"]["name"] = json!(if over_unit {
        "legacy-over-unit"
    } else {
        "legacy-neutral"
    });
    row["case"]["base"] = json!(if over_unit {
        [250, 240, 80]
    } else {
        [128, 128, 80]
    });
    row
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn cases() -> Vec<(String, Model)> {
    let mut cases = Vec::new();
    for channel in 0..3 {
        for row in rows() {
            cases.push((
                format!("{}-{channel}", row["case"]["name"].as_str().unwrap()),
                model(&row, channel, [255.0 / 128.0, -1.0, 0.4], true, 0),
            ));
        }
        for over_unit in [false, true] {
            let row = selected_row(over_unit);
            for basis in [false, true] {
                cases.push((
                    format!("legacy-selected-{channel}-{over_unit}-{basis}"),
                    model(&row, channel, [2.25, -1.1, -0.1], basis, 0),
                ));
            }
        }
    }
    cases
}

#[test]
fn native_selected_decode_and_missing_basis_keep_independent_surface_properties() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for channel in 0..3 {
        for over_unit in [false, true] {
            let row = selected_row(over_unit);
            for basis in [false, true] {
                let decode = [2.25, -1.1, -0.1];
                let model = model(&row, channel, decode, basis, 0);
                let actual = image(&model, 0.0);
                let reference = image(&reference(&row, channel, decode, basis, 0.0), 0.0);
                let name = format!("legacy-selected-{channel}-{over_unit}-{basis}");
                save(out, &name, &actual);
                save(out, &format!("{name}-reference"), &reference);
                let maximum = actual
                    .pixels
                    .iter()
                    .zip(&reference.pixels)
                    .flat_map(|(a, b)| {
                        a.to_array()
                            .into_iter()
                            .zip(b.to_array())
                            .map(|(a, b)| a.abs_diff(b))
                    })
                    .max()
                    .unwrap();
                assert!(maximum <= 1, "{name}: render error {maximum}");
                let glb = export::glb(&model, 0.0).unwrap();
                std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
                let (actual_normal, rough, ao) = glb_landmarks(&glb);
                let (normal, expected_rough) = expected(&row, decode, 0.0);
                assert_eq!(actual_normal.is_some(), basis, "{name}");
                if let Some(actual) = actual_normal {
                    assert_eq!(
                        actual,
                        [normal[0], -normal[1], normal[2]]
                            .map(|v| ((v * 0.5 + 0.5) * 255.0).round() as u8),
                        "{name}"
                    );
                }
                assert!(
                    (rough - expected_rough).abs() <= 0.5 / 255.0 + 1e-6,
                    "{name}: {rough} != {expected_rough}"
                );
                assert_eq!(ao, 1.0);
                receipt.push(json!({"name":name,"normal":actual_normal,"roughness":rough,"expected_roughness":expected_rough,"maximum_channel_error":maximum}));
            }
        }
    }
    // Unsupported dependency graphs keep the pre-existing material fallback.
    let row = rows().remove(5);
    for malformed in 1..=8 {
        let model = model(&row, 0, [255.0 / 128.0, -1.0, 0.4], true, malformed);
        let glb = export::glb(&model, 0.0).unwrap();
        let name = format!("legacy-unsupported-{malformed}");
        save(out, &name, &image(&model, 0.0));
        std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
        let (_, rough, ao) = glb_landmarks(&glb);
        assert_eq!(
            rough, 0.0,
            "{name}: unsupported blue recipe altered smoothness"
        );
        let expected_ao = 32.0 * 64.0 / (255.0 * 255.0);
        assert!(
            (ao - expected_ao).abs() <= 0.5 / 255.0,
            "{name}: fallback occlusion {ao}"
        );
        receipt.push(
            json!({"name":name,"roughness":rough,"occlusion":ao,"expected_occlusion":expected_ao}),
        );
    }
    std::fs::write(
        out.join("legacy-selected.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn native_normal_and_blue_limits_survive_selected_dyes_rendering_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for channel in 0..3 {
        for (index, row) in rows().into_iter().enumerate() {
            let decode = [255.0 / 128.0, -1.0, 0.4];
            let (normal, rough) = expected(&row, decode, 0.0);
            // Compare the independent algebra with the original executed shader witnesses.
            for (axis, lane) in [1, 2, 0].into_iter().enumerate() {
                assert!(
                    (normal[axis] - row["expected"]["world_normal"][lane].as_f64().unwrap() as f32)
                        .abs()
                        < 1e-6
                );
            }
            assert!(
                (rough - (1.0 - row["expected"]["smoothness"].as_f64().unwrap() as f32)).abs()
                    < 1e-6
            );
            for (step, seconds) in [0.0, 1.0, 0.0].into_iter().enumerate() {
                let model = model(&row, channel, decode, true, 0);
                let actual = image(&model, seconds);
                let reference = image(&reference(&row, channel, decode, true, seconds), seconds);
                let name = format!("legacy-normal-{channel}-{index}-{step}");
                save(out, &name, &actual);
                save(out, &format!("{name}-reference"), &reference);
                let maximum = actual
                    .pixels
                    .iter()
                    .zip(&reference.pixels)
                    .flat_map(|(a, b)| {
                        a.to_array()
                            .into_iter()
                            .zip(b.to_array())
                            .map(|(a, b)| a.abs_diff(b))
                    })
                    .max()
                    .unwrap();
                assert!(maximum <= 1, "{name}: render error {maximum}");
                assert!(
                    actual
                        .pixels
                        .iter()
                        .filter(|v| **v != eframe::egui::Color32::BLACK)
                        .count()
                        > 100
                );
                let glb = export::glb(&model, seconds).unwrap();
                std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
                let (actual_normal, actual_rough, ao) = glb_landmarks(&glb);
                let (normal, rough) = expected(&row, decode, seconds);
                let packed = [normal[0], -normal[1], normal[2]]
                    .map(|v| ((v * 0.5 + 0.5) * 255.0).round() as u8);
                assert_eq!(actual_normal.unwrap(), packed, "{name}");
                assert!(
                    (actual_rough - rough).abs() <= 0.5 / 255.0 + 1e-6,
                    "{name}: {actual_rough} != {rough}"
                );
                assert_eq!(ao, 1.0, "{name}: normal blue became occlusion");
                receipt.push(json!({"name":name,"seconds":seconds,"normal":normal,"packed":packed,"roughness":rough,"exported_roughness":actual_rough,"maximum_channel_error":maximum}));
            }
        }
    }
    std::fs::write(
        out.join("legacy-normal.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
