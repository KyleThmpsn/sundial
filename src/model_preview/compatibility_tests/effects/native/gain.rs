//! Package-to-render and export gain witnesses prepared before the gain implementation.
use super::*;

fn linear(value: u8) -> f32 {
    let v = value as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn opaque_base_gain_tracks_alpha_time_and_static_export() {
    assert!(
        opaque::gain_case(false, true).is_err(),
        "An unrelated plate producer must be refused"
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for painted in [false, true] {
        let model = opaque::gain_case(painted, false).unwrap();
        let mut rewind = None;
        for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
            let gain = if painted {
                [1.0; 3]
            } else {
                [0.25, 0.5, 0.75].map(|v| v * (1.0 + seconds))
            };
            let base: [f32; 3] = std::array::from_fn(|i| linear([128, 96, 64][i]) * gain[i]);
            let expected = [base[0], base[1] + 6.0 / 255.0, base[2] + 0.1];
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
            for lane in [1, 2] {
                assert!(
                    (linear(actual[lane]) - linear(actual[0]) - (expected[lane] - expected[0]))
                        .abs()
                        < 0.015,
                    "Painted {painted}, time {seconds}: {actual:?} lost native gain {gain:?}"
                );
            }
            if step == 0 {
                rewind = Some(actual);
            }
            if step == 3 {
                assert_eq!(
                    Some(actual),
                    rewind,
                    "Material gain accumulated across rewind"
                );
            }
            let name = format!("opaque-gain-{painted}-{step}");
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            std::fs::write(
                output.join(format!("{name}.png")),
                export::png(&rgba, 320, 240).unwrap(),
            )
            .unwrap();
            let glb = export::glb(&model, seconds).unwrap();
            let size = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
            let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + size]).unwrap();
            let texture = doc["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
                .as_u64()
                .unwrap() as usize;
            let source = doc["textures"][texture]["source"].as_u64().unwrap() as usize;
            let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
            let (dimensions, pixels) = repack::png_pixels(repack::bytes(&glb, &doc, view));
            assert_eq!(
                dimensions,
                [8, 4],
                "The rectangular atlas changed dimensions"
            );
            let baked = &pixels[(8 + 4) * 4..(8 + 4) * 4 + 3];
            let encoded = encoded(base);
            assert!(
                baked.iter().zip(encoded).all(|(&a, b)| a.abs_diff(b) <= 1),
                "Export {baked:?} lost gain {encoded:?}"
            );
            std::fs::write(output.join(format!("{name}.glb")), &glb).unwrap();
            receipt.push(json!({"painted":painted,"seconds":seconds,"gain":gain,"expected_unlit":expected,"actual":actual,"export_actual":baked,"export_expected":encoded}));
        }
    }
    std::fs::write(
        output.join("opaque-gain-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
