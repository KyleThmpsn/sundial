//! Authored mip footprints through package loading, image rendering and saved models.
use super::*;
mod fixture;

fn model(case: u8) -> Model {
    let (directory, tag, detail_tags) = fixture::build(case);
    let mut model = fixtures::load(directory.path(), tag).unwrap();
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let details: Vec<_> = detail_tags
        .into_iter()
        .map(|tag| texture::load_model(&manager, tag, &mut model).unwrap())
        .collect();
    let mut vectors = [[0.0; 4]; 27];
    vectors[0] = [3.0, 3.0, 0.125, 0.375];
    vectors[1] = [if case == 7 { -2.0 } else { 2.0 }; 4];
    vectors[1][2..].fill(0.25);
    vectors[2] = [255.0 / 128.0, -1.0, 0.4, 0.0];
    for surface in 0..2 {
        vectors[9 + surface * 4] = [0.3, 0.4, 0.5, 1.0];
        vectors[17 + surface * 4] = [0.3, 0.4, 0.5, 1.0];
        vectors[10 + surface * 4] = [0.6, 0.75, 0.0, 0.0];
        vectors[20 + surface * 4] = [0.6, 0.75, 0.0, 0.0];
        vectors[12 + surface * 4] = [0.0, 1.0, 0.0, 1.0];
        vectors[19 + surface * 4] = [0.0, 1.0, 0.0, 1.0];
        vectors[18 + surface * 4] = [0.0, 1.0, 0.0, 1.0];
        vectors[11 + surface * 4][0] = -1.0;
    }
    model.dyes[0] = Some(shader::Dye {
        surface: crate::dyes::material::properties(&vectors).surfaces[0],
        detail: Some(details[0]),
        normal: Some(details[1]),
        transform: vectors[0],
        normal_transform: vectors[1],
        vectors,
    });
    let factor = if case == 6 {
        0.0005
    } else if case == 7 {
        -1.0
    } else {
        1.0
    };
    model.uvs = model
        .vertices
        .iter()
        .map(|p| [p[0] * factor, p[2] * factor])
        .collect();
    model.detail_uvs = model.uvs.clone();
    model.triangle_detail_uv.fill(true);
    model.tangents = vec![[1.0, 0.0, 0.0, 1.0]; model.vertices.len()];
    model
}

fn sample(case: u8, slot: usize, scale: f64) -> [f32; 4] {
    let sampler = [2, 3, 4, 0, 1][slot];
    let (filter, bias, range) = fixture::settings(case, sampler);
    let factor: f64 = if case == 6 { 0.0005 } else { 1.0 };
    let detail = match slot {
        3 => 3.0,
        4 => 2.0,
        _ => 1.0,
    };
    let footprint = fixture::EDGE as f64 * factor * detail / scale;
    let lod = (footprint.log2() + f64::from(bias)).clamp(f64::from(range[0]), f64::from(range[1]));
    let levels = if case == 10 { 1 } else { fixture::LEVELS };
    let lod = lod.clamp(0.0, (levels - 1) as f64);
    let (a, b, blend) = if filter & 1 == 0 {
        (lod.round() as usize, lod.round() as usize, 0.0)
    } else {
        (lod.floor() as usize, lod.ceil() as usize, lod.fract())
    };
    let decode = |level| {
        let value = fixture::pixel(slot, level, case == 9).map(f64::from);
        std::array::from_fn::<_, 4, _>(|lane| {
            if matches!(slot, 0 | 3) && lane < 3 && !(case == 9 && slot == 0) {
                if value[lane] <= 0.04045 {
                    value[lane] / 12.92
                } else {
                    ((value[lane] + 0.055) / 1.055).powf(2.4)
                }
            } else {
                value[lane]
            }
        })
    };
    let a = decode(a);
    let b = decode(b);
    std::array::from_fn(|lane| (a[lane] * (1.0 - blend) + b[lane] * blend) as f32)
}

fn reference(case: u8, size: [usize; 2]) -> Model {
    let mut model = model(case);
    // Three one-unit squares, at x = 0, 1.2 and 2.4. Derive screen density independently.
    let radius = (3.4f64.powi(2) + 1.0).sqrt() * 0.5;
    let scale = size[0].min(size[1]) as f64 * 0.43 / radius;
    let dye = model.dyes[0].unwrap();
    let images = [
        model.triangle_textures[0].unwrap(),
        model.triangle_normals[0].unwrap(),
        model.triangle_gearstacks[0].unwrap(),
        dye.detail.unwrap(),
        dye.normal.unwrap(),
    ];
    for (slot, index) in images.into_iter().enumerate() {
        let value = sample(case, slot, scale);
        model.textures[index] = texture::Texture {
            mips: None,
            tag: model.textures[index].tag,
            size: [1, 1],
            rgba: value
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .to_vec(),
            linear: Some(vec![value]),
        };
    }
    for effect in &mut model.effects {
        effect.samplers.clear();
    }
    model
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn cases() -> Vec<(String, Model)> {
    (0..=10)
        .map(|case| (format!("native-sampling-{case}"), model(case)))
        .collect()
}

fn image(model: &Model, size: [usize; 2]) -> eframe::egui::ColorImage {
    render::animated_image(
        model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene {
            background: [0; 3],
            ..Default::default()
        },
        size,
        0.0,
    )
}

fn save(out: &Path, name: &str, image: &eframe::egui::ColorImage) {
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&rgba, image.size[0], image.size[1]).unwrap(),
    )
    .unwrap();
}

fn export_geometry(glb: &[u8]) -> usize {
    let size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + size]).unwrap();
    doc["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|mesh| mesh["primitives"].as_array().unwrap())
        .map(|primitive| {
            doc["accessors"][primitive["indices"].as_u64().unwrap() as usize]["count"]
                .as_u64()
                .unwrap() as usize
        })
        .sum()
}

#[test]
fn native_mip_images_and_sampler_footprints_survive_preview_and_saved_artifacts() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for case in 0..=10 {
        let model = model(case);
        let glb = export::glb(&model, 0.0).unwrap();
        std::fs::write(out.join(format!("native-sampling-{case}.glb")), &glb).unwrap();
        assert_eq!(model.triangles.len(), 6);
        assert_eq!(
            export_geometry(&glb),
            18,
            "{case}: saved geometry is incomplete"
        );
        for size in [[160, 128], [320, 256]] {
            let name = format!("native-sampling-{case}-{}", size[0]);
            let actual = image(&model, size);
            let expected = image(&reference(case, size), size);
            save(out, &name, &actual);
            save(out, &format!("{name}-reference"), &expected);
            let maximum = actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .flat_map(|(a, b)| {
                    a.to_array()
                        .into_iter()
                        .zip(b.to_array())
                        .map(|(a, b)| a.abs_diff(b))
                })
                .max()
                .unwrap();
            let visible = actual
                .pixels
                .iter()
                .filter(|p| **p != eframe::egui::Color32::BLACK)
                .count();
            receipt.push(json!({"case":case,"size":size,"maximum_channel_error":maximum,
                "visible_pixels":visible,"opaque_triangles":model.triangles.len(),
                "sampling":"Independently specified authored levels, footprint, sRGB decode, bias and LOD clamp"}));
            std::fs::write(
                out.join("native-sampling.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
            assert!(maximum <= 2, "{name}: channel difference {maximum}");
            assert!(visible > 500, "{name}: only {visible} visible pixels");
        }
    }
}
