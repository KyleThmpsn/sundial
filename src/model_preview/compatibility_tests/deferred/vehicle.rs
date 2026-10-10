//! Package-to-display witnesses for conditional emission and complete vehicle image sets.
use super::*;

pub(super) fn writer(code: &mut Vec<u32>, valid: bool) {
    code.extend(instruction(54, &[&register(0, 11, 1), &constant(0, 0)]));
    code.extend(instruction(
        49,
        &[&register(0, 11, 2), &literal(0.00001), &source(0, 11, 0)],
    ));
    code.extend(instruction(
        0,
        &[
            &register(0, 11, 1),
            &source(0, 11, 0),
            &literal(1.0 / 128.0),
        ],
    ));
    code.extend(instruction(47, &[&register(0, 11, 1), &source(0, 11, 0)]));
    code.extend(instruction(
        50 | 1 << 13,
        &[
            &register(0, 11, 1),
            &source(0, 11, 0),
            &literal(1.0 / 13.0),
            &literal(7.0 / 13.0),
        ],
    ));
    code.extend(instruction(
        0,
        &[
            &register(0, 11, 1),
            &source(0, 11, 0),
            &literal(if valid { 1.0 + 2.0 / 255.0 } else { 1.0 }),
        ],
    ));
    code.extend(instruction(
        56,
        &[&register(0, 11, 1), &source(0, 11, 0), &literal(0.5)],
    ));
    code.extend(instruction(
        55,
        &[
            &register(2, 2, 2),
            &source(0, 11, 0x55),
            &source(0, 11, 0),
            &literal(0.5),
        ],
    ));
}

pub(super) fn prefix_material(package: &mut Package) -> u32 {
    let pixels = package.raw(0, 0, 0, vec![16, 32, 64, 255]);
    let mut header = vec![0; 40];
    put(&mut header, 0, &4u32.to_le_bytes());
    put(&mut header, 4, &29u32.to_le_bytes());
    for at in [14, 16, 18, 20] {
        put(&mut header, at, &1u16.to_le_bytes());
    }
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    let texture = package.raw(pixels, 32, 1, header);
    let mut material = vec![0; 0x360];
    let mut row = [0; 8];
    put(&mut row, 4, &texture.to_le_bytes());
    array(&mut material, 0x2D0, 0x8080_7211, &row, 8);
    package.add(0x8080_71E8, material)
}

fn expected(power: f64, exposure: f64) -> [u8; 3] {
    // Native target quantization and deferred consumer, independently executed with the
    // original vehicle program. This fixture keeps its albedo independent of the light.
    let y = if power > 0.00001 {
        (((power + 1.0 / 128.0).log2() + 7.0) / 13.0).clamp(0.0, 1.0) * 0.5
            + (1.0 + 2.0 / 255.0) * 0.5
    } else {
        0.5
    };
    let y = (y * 255.0).round_ties_even() / 255.0;
    let intensity =
        (13.0 * (2.0 * y - (1.0 + 2.0 / 255.0)).clamp(0.0, 1.0) - 7.0).exp2() - 1.0 / 128.0;
    [192.0f64, 64.0, 32.0].map(|byte| {
        let linear = ((byte / 255.0 + 0.055) / 1.055).powf(2.4) * intensity * exposure;
        let encoded = if linear <= 0.0031308 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

pub(crate) fn emission_cases() -> Vec<(String, Model, render::Scene, [u8; 3])> {
    let mut cases = Vec::new();
    for (power, exposure, prefix) in [
        (0.0, 1.0, 0),
        (0.125, 1.0, 0),
        (1.0, 0.5, 0),
        (8.0, 1.0, 32),
    ] {
        cases.push((
            format!("vehicle-emission-{power}-{exposure}-{prefix}"),
            fixture(None, 0, 64, [128, 128], Some((power, true)), prefix, false),
            render::Scene {
                key: 0.0,
                fill: 0.0,
                exposure,
                filmic: false,
                bloom: false,
                ..Default::default()
            },
            expected(f64::from(power), f64::from(exposure)),
        ));
    }
    cases
}

pub(crate) fn studio_cases() -> Vec<(String, Model)> {
    [
        ("gray", [118, 118, 118]),
        ("color", [192, 64, 32]),
        ("white", [235, 235, 235]),
    ]
    .into_iter()
    .map(|(name, rgb)| {
        let mut model = case(false, 0, 64, [128, 128]);
        // The package supplies the complete surface. Replace only its color swatch,
        // retaining the production scene, normal, light, material and output paths.
        for pixel in model.textures[0].rgba.chunks_exact_mut(4) {
            pixel[..3].copy_from_slice(&rgb);
        }
        (format!("studio-{name}"), model)
    })
    .collect()
}

#[test]
fn default_studio_preserves_gray_and_color_without_white_clipping() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("studio");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for (name, model) in studio_cases() {
        let image = render::styled_image(
            &model,
            render::Camera {
                yaw: 0.0,
                pitch: 0.0,
                ..Default::default()
            },
            render::Scene::default(),
            [240, 180],
            0.0,
            render::Style::Textured,
        );
        let actual = image.pixels[90 * 240 + 120].to_array();
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 240, 180).unwrap(),
        )
        .unwrap();
        // Display policy: middle gray stays near the middle of the display range,
        // off-white retains headroom, and the colored paint keeps distinct channels.
        let ranges = match name.as_str() {
            "studio-gray" => [(112, 134); 3],
            "studio-white" => [(188, 216); 3],
            _ => [(165, 192), (66, 90), (45, 68)],
        };
        let passed = actual[..3]
            .iter()
            .zip(ranges)
            .all(|(&v, (low, high))| (low..=high).contains(&v));
        receipt
            .push(json!({"case":name,"center":actual,"acceptable_ranges":ranges,"passed":passed}));
        std::fs::write(
            output.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        assert!(passed, "{name}: {actual:?}");
    }
}

#[test]
fn vehicle_surfaces_keep_their_emission_and_complete_material_bindings() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("vehicles");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for (name, model, scene, expected) in emission_cases() {
        let image = render::styled_image(
            &model,
            render::Camera {
                yaw: 0.0,
                pitch: 0.0,
                ..Default::default()
            },
            scene,
            [240, 180],
            0.0,
            render::Style::Textured,
        );
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 240, 180).unwrap(),
        )
        .unwrap();
        let actual = image.pixels[90 * 240 + 120].to_array();
        let error = actual[..3]
            .iter()
            .zip(expected)
            .map(|(&a, b)| a.abs_diff(b))
            .max()
            .unwrap();
        receipt.push(json!({"case":name,"expected":expected,"actual":actual,"maximum_error":error,"notices":model.notices}));
        std::fs::write(
            output.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        assert!(
            error <= 1,
            "{name}: {actual:?}, expected {expected:?}, {:?}",
            model.notices
        );
        assert!(
            !model.notices.iter().any(|n| n.contains("texture budget")),
            "{name}: {:?}",
            model.notices
        );
    }
    let changed = fixture(None, 0, 64, [128, 128], Some((8.0, false)), 0, false);
    let image = render::styled_image(
        &changed,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene {
            key: 0.0,
            fill: 0.0,
            ..render::Scene::unprocessed()
        },
        [240, 180],
        0.0,
        render::Style::Textured,
    );
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("altered-emission.png"),
        export::png(&rgba, 240, 180).unwrap(),
    )
    .unwrap();
    assert_eq!(
        image.pixels[90 * 240 + 120],
        eframe::egui::Color32::BLACK,
        "An altered emission encoding must not introduce a glow under disabled lighting"
    );
}
