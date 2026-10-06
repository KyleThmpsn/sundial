//! Native original-bytecode color witnesses, prepared before reconstructing legacy wear.
use super::*;
use effects::native::repack;
pub(in crate::model_preview) mod composition;

fn inputs() -> serde_json::Value {
    serde_json::from_str(include_str!("legacy_color/cloth.json")).unwrap()
}

fn model(
    row: &serde_json::Value,
    vectors: [[f32; 4]; 27],
    channel: usize,
    reference: bool,
) -> Model {
    let (directory, tag) = plates::fixture([0, 0, 8, 8], [8, 8], false);
    let mut model = fixtures::load(directory.path(), tag).unwrap();
    let surface = row["surface"].as_u64().unwrap() as usize;
    let alpha = row["alpha"].as_u64().unwrap() as u8;
    let slot = channel * 2 + surface;
    model.triangle_dyes.fill(slot as u8);
    model.triangle_normals.fill(None);
    model.triangle_effects.fill(None);
    let albedo = model.triangle_textures[0].unwrap();
    let mask = model.triangle_gearstacks[0].unwrap();
    let mut base = [255; 4];
    for (i, value) in base[..3].iter_mut().enumerate() {
        *value = row["base"][i].as_u64().unwrap() as u8;
    }
    for index in [albedo, mask] {
        model.textures[index].mips = None;
    }
    model.textures[albedo].rgba = base.repeat(64);
    model.textures[mask].rgba = [255, 64, 0, alpha].repeat(64);
    let detail = model.textures.len();
    model.textures.push(texture::Texture {
        mips: None,
        tag: 0x8080_ffff,
        size: [1, 1],
        rgba: vec![200, 120, 80, 255],
        linear: None,
    });
    let mut vectors = vectors;
    for row in [12, 16, 19, 23] {
        // Hold roughness constant to isolate the independently executed color equations.
        vectors[row] = [0.0, 0.0, 0.25, 0.0];
    }
    for row in [10, 14, 20, 24] {
        vectors[row][3] = 0.0;
    }
    model.dyes[slot] = Some(shader::Dye {
        surface: crate::dyes::material::properties(&vectors).surfaces[surface],
        detail: Some(detail),
        vectors,
        transform: [1.0, 1.0, 0.0, 0.0],
        normal: None,
        normal_transform: [1.0, 1.0, 0.0, 0.0],
    });
    if reference {
        let expected: [f32; 4] = std::array::from_fn(|i| {
            if i < 3 {
                row["expected"][i].as_f64().unwrap() as f32
            } else {
                1.0
            }
        });
        model.textures[albedo].linear = Some(vec![expected; 64]);
        model.textures[mask].linear = Some(vec![
            [
                1.0,
                if alpha < 40 { 64.0 / 255.0 } else { 0.25 },
                0.0,
                if alpha < 40 {
                    f32::from(alpha) / 255.0
                } else {
                    0.0
                }
            ];
            64
        ]);
        model.dyes[slot].as_mut().unwrap().detail = None;
    }
    model
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn cases() -> Vec<(String, Model)> {
    let input = inputs();
    let vectors = serde_json::from_value(input["vectors"].clone()).unwrap();
    (0..3)
        .flat_map(|channel| {
            input["cases"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(move |(index, row)| {
                    (
                        format!("legacy-cloth-{channel}-{index}"),
                        model(row, vectors, channel, false),
                    )
                })
        })
        .collect()
}

fn image(model: &Model) -> eframe::egui::ColorImage {
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
        [320, 240],
        0.0,
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

fn exported(glb: &[u8]) -> [f32; 3] {
    let len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + len]).unwrap();
    let mr = &doc["materials"][0]["pbrMetallicRoughness"];
    let Some(texture) = mr["baseColorTexture"]["index"].as_u64() else {
        return std::array::from_fn(|i| mr["baseColorFactor"][i].as_f64().unwrap() as f32);
    };
    let source = doc["textures"][texture as usize]["source"]
        .as_u64()
        .unwrap() as usize;
    let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
    let (size, pixels) = repack::png_pixels(repack::bytes(glb, &doc, view));
    let center = (size[1] / 2 * size[0] + size[0] / 2) * 4;
    std::array::from_fn(|i| {
        let v = f32::from(pixels[center + i]) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[test]
fn native_cloth_colors_survive_wear_preview_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let input = inputs();
    let vectors = serde_json::from_value(input["vectors"].clone()).unwrap();
    let mut receipt = Vec::new();
    for channel in 0..3 {
        for (index, row) in input["cases"].as_array().unwrap().iter().enumerate() {
            let name = format!("legacy-cloth-{channel}-{index}");
            let actual = model(row, vectors, channel, false);
            let reference = model(row, vectors, channel, true);
            let actual_image = image(&actual);
            let expected_image = image(&reference);
            save(out, &name, &actual_image);
            save(out, &format!("{name}-reference"), &expected_image);
            let glb = export::glb(&actual, 0.0).unwrap();
            std::fs::write(out.join(format!("{name}.glb")), &glb).unwrap();
            let maximum = actual_image
                .pixels
                .iter()
                .zip(&expected_image.pixels)
                .flat_map(|(a, b)| {
                    a.to_array()
                        .into_iter()
                        .zip(b.to_array())
                        .map(|(a, b)| a.abs_diff(b))
                })
                .max()
                .unwrap();
            let color = exported(&glb);
            let error = color
                .iter()
                .enumerate()
                .map(|(i, v)| (v - row["expected"][i].as_f64().unwrap() as f32).abs())
                .fold(0.0f32, f32::max);
            receipt.push(json!({"name":name,"inputs":row,"exported_color":color,"maximum_image_error":maximum,"export_error":error}));
            std::fs::write(
                out.join("legacy-cloth-receipt.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
            assert!(
                maximum <= 1 && error < 0.005,
                "{name}: image {maximum}, export {error}"
            );
        }
    }
}
