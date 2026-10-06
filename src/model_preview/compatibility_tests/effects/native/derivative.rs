//! Transformed-UV derivative acceptance prepared before affine recovery.
use super::*;

pub(crate) fn case(mode: u8) -> Result<Model, String> {
    let mut package = Package::default();
    let mut code = instruction(104, &[&[2]]);
    code.extend(instruction(89, &[&[0x0020_8000, 0, 1]]));
    code.extend(instruction(98, &[&register(1, 3, 3)]));
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    if mode == 2 {
        code.extend(instruction(31 | 1 << 18, &[&source(1, 3, 0)]));
    }
    code.extend(instruction(
        50,
        &[
            &register(0, 0, 3),
            &source(1, 3, 0xE4),
            &[0x0020_8E46, 0, 0],
            &[0x0020_80E6, 0, 0],
        ],
    ));
    if mode == 2 {
        code.extend(instruction(21, &[]));
    }
    if mode == 1 {
        code.extend(instruction(
            56,
            &[&register(0, 0, 3), &source(0, 0, 0xE4), &source(0, 0, 0xE4)],
        ));
    }
    code.extend(instruction(54, &[&register(0, 0, 4), &source(0, 0, 0)]));
    code.extend(instruction(122, &[&register(0, 1, 1), &source(0, 0, 0xAA)]));
    let negated = [0x8010_0006 | 0x55 << 4, 0x41, 0];
    code.extend(instruction(124, &[&register(0, 1, 2), &negated]));
    code.extend(instruction(
        56,
        &[&register(2, 0, 3), &source(0, 1, 0xE4), &literal(32.0)],
    ));
    code.extend(instruction(54, &[&register(2, 0, 12), &literal(0.0)]));
    code.extend(instruction(62, &[]));
    let shader = shader_stage(
        &mut package,
        &code,
        0,
        &[("TEXCOORD", 3)],
        &[("SV_TARGET", 0)],
    );
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &shader.to_le_bytes());
    let mut constants = [0; 16];
    floats(&mut constants, 0, &[2.0, -3.0, 17.0, -29.0]);
    array(&mut material, 0x318, 0x8080_0090, &constants, 16);
    let material = package.add(0x8080_71E8, material);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    let effect = crate::model_preview::effects::load(&manager, material, Ok(&[]), 0, &mut model)?;
    model.effects.push(effect);
    quad(&mut model, 0.0, Some(0), None);
    model.uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    Ok(model)
}

#[test]
fn affine_temporary_derivatives_keep_screen_scale_and_lane_writes() {
    let model = case(0).unwrap();
    assert!(case(1).is_err(), "A nonlinear derivative was accepted");
    assert!(
        case(2).is_err(),
        "A branch-dependent derivative was accepted"
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for size in [[320, 240], [480, 320]] {
        let image = render::animated_image(
            &model,
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
        );
        let center = image.pixels[size[1] / 2 * size[0] + size[0] / 2].to_array();
        let projected_width = 2.0 * size[1] as f32 * 0.43 / 2.0_f32.sqrt();
        let expected = encoded([64.0 / projected_width, 96.0 / projected_width, 0.0]);
        for lane in 0..3 {
            assert!(
                center[lane].abs_diff(expected[lane]) <= 1,
                "{size:?}: {center:?} != {expected:?}"
            );
        }
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("affine-derivative-{}.png", size[0])),
            export::png(&rgba, size[0], size[1]).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"size":size,"projected_width":projected_width,"expected":expected,"actual":center}));
    }
    std::fs::write(
        output.join("affine-derivative-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
