//! Immediate tables through package loading, shader execution and saved pixel output.
use super::*;

fn table() -> Vec<u32> {
    vec![
        53 | (3 << 11),
        14,
        0.125f32.to_bits(),
        0.25f32.to_bits(),
        0.75f32.to_bits(),
        0,
        u32::MAX,
        0x8000_0000,
        0x1234_5678,
        0,
        0.75f32.to_bits(),
        0.125f32.to_bits(),
        0.25f32.to_bits(),
        0,
    ]
}

fn code(case: u8) -> Vec<u32> {
    let mut code = instruction(104, &[&[2]]);
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    let mut values = table();
    if case == 4 {
        values.pop();
    }
    if case == 6 {
        values[0] = 53 | (2 << 11);
    }
    if case != 7 {
        code.extend(values);
    }
    if case == 5 {
        code.extend(table());
    }
    let mut value = source(9, if case == 9 { 3 } else { 0 }, 0xE4).to_vec();
    if matches!(case, 1 | 2) {
        code.extend(instruction(
            54,
            &[
                &register(0, 1, 15),
                &[0x4001, if case == 1 { 1 } else { u32::MAX }],
            ],
        ));
        // Immediate row 1 plus an unsigned register index, with a yzxw permutation.
        value = vec![0x00D0_9006 | 0xC9 << 4, if case == 1 { 1 } else { 0 }];
        value.extend(source(0, 1, 0));
    }
    code.extend(instruction(54, &[&register(0, 0, 15), &value]));
    if case == 2 {
        // An out-of-range dynamic load returns zero. Its bits, not a float cast,
        // participate in the equality check and produce the known color.
        code.extend(instruction(
            32,
            &[&register(0, 0, 15), &source(0, 0, 0xE4), &[0x4001, 0]],
        ));
        code.extend(instruction(
            32,
            &[
                &register(0, 1, 15),
                &source(9, 1, 0xE4),
                &[0x4002, u32::MAX, 0x8000_0000, 0x1234_5678, 0],
            ],
        ));
        code.extend(instruction(
            1,
            &[
                &register(0, 0, 15),
                &source(0, 0, 0xE4),
                &source(0, 1, 0xE4),
            ],
        ));
        code.extend(instruction(
            1,
            &[
                &register(0, 0, 15),
                &source(0, 0, 0xE4),
                &source(9, 0, 0xE4),
            ],
        ));
    }
    if case == 3 {
        code.extend(instruction(54, &[&register(0, 0, 1), &literal(0.0)]));
        for _ in 0..600 {
            code.extend(instruction(
                0,
                &[&register(0, 0, 1), &source(0, 0, 0), &literal(1.0 / 1024.0)],
            ));
        }
    }
    if case == 8 {
        code.extend(instruction(54, &[&register(9, 0, 15), &literal(0.0)]));
    }
    code.extend(instruction(54, &[&register(2, 0, 15), &source(0, 0, 0xE4)]));
    code.extend(instruction(62, &[]));
    code
}

pub(crate) fn expected(case: u8) -> [u8; 3] {
    if case == 3 {
        [201, 137, 225]
    } else {
        [99, 137, 225]
    }
}

pub(crate) fn case(index: u8) -> Result<Model, String> {
    let mut package = Package::default();
    let pixel = shader_stage(&mut package, &code(index), 0, &[], &[("SV_TARGET", 0)]);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let tag = package.add(0x8080_71E8, material);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    let effect = crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model)?;
    model.effects.push(effect);
    quad(&mut model, 0.0, Some(0), None);
    Ok(model)
}

#[test]
fn immediate_tables_preserve_dynamic_indexing_and_long_shader_output() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("immediate");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for index in 0..4 {
        let model = case(index).unwrap();
        let image = render::animated_image(
            &model,
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
            0.0,
        );
        let actual = image.pixels[120 * 320 + 160].to_array();
        assert!(
            (0..3).all(|lane| actual[lane].abs_diff(expected(index)[lane]) <= 1),
            "{index}: {actual:?}"
        );
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        std::fs::write(
            output.join(format!("immediate-{index}.png")),
            export::png(&rgba, 320, 240).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"case":index,"expected":expected(index),"actual":actual}));
    }
    for index in 4..10 {
        let error = case(index)
            .err()
            .expect("Malformed or writable immediate table was accepted");
        receipt.push(json!({"case":index,"rejected":error}));
    }
    std::fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
