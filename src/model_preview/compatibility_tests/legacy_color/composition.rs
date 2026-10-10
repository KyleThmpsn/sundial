//! Bright gear bases must reach the packaged native RGB consumer before normalization.
use super::*;

pub(in crate::model_preview) fn case(base: [u8; 3], tint: [f32; 3]) -> Model {
    let mut model = effects::native::opaque::gain_case(true, false).unwrap();
    model.triangle_dyes = vec![0; model.triangles.len()];
    let albedo = model.triangle_textures[0].unwrap();
    model.textures[albedo].rgba = [base[0], base[1], base[2], 255].repeat(32);
    let mut vectors = [[0.0; 4]; 27];
    for row in [9, 13, 17, 21] {
        vectors[row] = [tint[0], tint[1], tint[2], 1.0];
    }
    for row in [12, 16, 19, 23] {
        vectors[row] = [0.0, 0.0, 0.25, 0.0];
    }
    for row in [11, 15] {
        vectors[row][0] = -1.0;
    }
    model.dyes[0] = Some(shader::Dye {
        surface: crate::dyes::material::properties(&vectors).surfaces[0],
        detail: None,
        vectors,
        transform: [1.0, 1.0, 0.0, 0.0],
        normal: None,
        normal_transform: [1.0, 1.0, 0.0, 0.0],
    });
    model
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "used by the Windows GPU verification")
)]
pub(in crate::model_preview) fn cases() -> Vec<(String, Model)> {
    [[208; 3], [240; 3], [255; 3], [240, 220, 180]]
        .into_iter()
        .enumerate()
        .map(|(index, base)| {
            (
                format!("native-bright-composition-{index}"),
                case(base, [0.5; 3]),
            )
        })
        .collect()
}

fn linear(byte: u8) -> f32 {
    let value = f32::from(byte) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn bounded(base: [f32; 3], extra: [f32; 3]) -> [f32; 3] {
    let sum: [f32; 3] = std::array::from_fn(|i| base[i] + extra[i]);
    let weight = 1.0 - (sum.into_iter().fold(0.0f32, f32::max) - 1.0).clamp(0.0, 1.0);
    let color: [f32; 3] = std::array::from_fn(|i| base[i] * weight + extra[i]);
    let peak = color.into_iter().fold(1.0f32, f32::max);
    color.map(|value| value / peak)
}

#[test]
fn bright_bases_are_normalized_once_after_native_extra_rgb() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (index, plate) in [[208; 3], [240; 3], [255; 3], [240, 220, 180]]
        .into_iter()
        .enumerate()
    {
        // Native plate overlay has unit gain for these plates and an additive bias.
        let base = plate.map(|v| 0.5 + (linear(v) - 0.25).clamp(0.0, 1.0));
        let expected = bounded(base, [0.0, 6.0 / 255.0, 0.1]);
        let export_expected = bounded(base, [0.0; 3]);
        let actual = case(plate, [0.5; 3]);
        let mut reference = case(plate, [0.5; 3]);
        reference.triangle_effects.fill(None);
        let albedo = reference.triangle_textures[0].unwrap();
        reference.textures[albedo].linear =
            Some(vec![[expected[0], expected[1], expected[2], 1.0]; 32]);
        let mask = reference.triangle_gearstacks[0].unwrap();
        reference.textures[mask].linear = Some(vec![[1.0, 0.25, 0.0, 0.0]; 32]);
        let actual_image = image(&actual);
        let target = image(&reference);
        let name = format!("native-bright-composition-{index}");
        save(output, &name, &actual_image);
        save(output, &format!("{name}-reference"), &target);
        let glb = export::glb(&actual, 0.0).unwrap();
        std::fs::write(output.join(format!("{name}.glb")), &glb).unwrap();
        let json_size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_size]).unwrap();
        let texture = doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .unwrap() as usize;
        let source = doc["textures"][texture]["source"].as_u64().unwrap() as usize;
        let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
        let (_, pixels) = repack::png_pixels(repack::bytes(&glb, &doc, view));
        let exported: [f32; 3] = std::array::from_fn(|i| linear(pixels[4 * (8 + 4) + i]));
        let image_error = actual_image
            .pixels
            .iter()
            .zip(&target.pixels)
            .flat_map(|(a, b)| a.to_array().into_iter().zip(b.to_array()))
            .map(|(a, b)| a.abs_diff(b))
            .max()
            .unwrap();
        let export_error = exported
            .into_iter()
            .zip(export_expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        receipt.push(
            json!({"name":name,"plate":plate,"base":base,"expected":expected,
            "export_expected":export_expected,"exported":exported,
            "maximum_image_error":image_error,"export_error":export_error}),
        );
        std::fs::write(
            output.join("native-bright-composition-receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        assert!(image_error <= 1, "{name}: native color error {image_error}");
        assert!(
            export_error < 0.005,
            "{name}: base export error {export_error}"
        );
    }
}
