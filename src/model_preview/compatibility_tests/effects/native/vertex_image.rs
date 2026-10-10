//! Stage-local image bindings through packaged vertex and pixel programs.
use super::*;

pub(crate) fn expected(index: u8) -> [u8; 3] {
    if index == 3 {
        [188, 0, 188]
    } else {
        [188, 137, 99]
    }
}

fn image(package: &mut Package, index: u8, vertex: bool) -> u32 {
    let data = if !vertex {
        vec![255; 4]
    } else if index == 2 {
        vec![0, 255, 0, 255, 128, 64, 32, 255]
    } else if index == 3 {
        vec![255, 0, 0, 255, 0, 0, 255, 255]
    } else {
        vec![128, 64, 32, 255]
    };
    let payload = package.raw(0, 40, 1, data);
    let mut header = vec![0; 40];
    put(&mut header, 4, &28u32.to_le_bytes());
    for offset in [14, 16, 18, 20] {
        put(&mut header, offset, &1u16.to_le_bytes());
    }
    if vertex && matches!(index, 2 | 3) {
        put(
            &mut header,
            if index == 2 { 20 } else { 18 },
            &2u16.to_le_bytes(),
        );
    }
    header[23] = 1;
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    package.raw(payload, 32, 1, header)
}

pub(crate) fn case(index: u8) -> Result<Model, String> {
    let mut package = Package::default();
    let vertex_texture = image(&mut package, index, true);
    let pixel_texture = image(&mut package, index, false);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut description = vec![0; 52];
    put(&mut description, 0, &21u32.to_le_bytes());
    for offset in [4, 8, 12] {
        put(&mut description, offset, &3u32.to_le_bytes());
    }
    put(&mut description, 20, &1u32.to_le_bytes());
    put(&mut description, 48, &8f32.to_le_bytes());
    let data = package.raw(sampler, 42, 1, description);
    package.set_reference(sampler, data);
    let mut vertex = instruction(104, &[&[1]]);
    vertex.extend(instruction(95, &[&register(1, 0, 7)]));
    vertex.extend(instruction(101, &[&register(2, 4, 15)]));
    vertex.extend(instruction(101, &[&register(2, 5, 15)]));
    vertex.extend(instruction(
        88 | (if index == 2 {
            8
        } else if index == 3 {
            5
        } else {
            3
        }) << 11,
        &[&[0x0010_7000, 4], &[0x5555]],
    ));
    if index == 1 {
        vertex.extend(instruction(
            45,
            &[
                &register(0, 0, 15),
                &[0x4002, 0, 0, 0, 0],
                &source(7, 4, 0xE4),
            ],
        ));
    } else {
        vertex.extend(instruction(90, &[&[0x0010_6000, 1]]));
        let coordinate = [
            0x4002,
            0.5f32.to_bits(),
            0.5f32.to_bits(),
            if index == 2 {
                1f32.to_bits()
            } else {
                0.5f32.to_bits()
            },
            0,
        ];
        let dst = register(0, 0, 15);
        let resource = source(7, 4, 0xE4);
        let sampling = [0x0010_6000, 1];
        let level = literal(0.0);
        let mut operands: Vec<&[u32]> = vec![&dst, &coordinate, &resource, &sampling];
        if index != 6 {
            operands.push(&level);
        }
        vertex.extend(instruction(if index == 6 { 69 } else { 72 }, &operands));
    }
    vertex.extend(instruction(54, &[&register(2, 5, 15), &source(0, 0, 0xE4)]));
    vertex.extend(instruction(54, &[&register(2, 4, 15), &source(1, 0, 0xE4)]));
    vertex.extend(instruction(
        50,
        &[
            &register(2, 4, 1),
            &source(0, 0, 0),
            &literal(0.25),
            &source(1, 0, 0),
        ],
    ));
    vertex.extend(instruction(62, &[]));
    let vertex = shader_stage(
        &mut package,
        &vertex,
        1,
        &[("POSITION", 0)],
        &[("TEXCOORD", 4), ("TEXCOORD", 5)],
    );
    let mut pixel = instruction(104, &[&[1]]);
    pixel.extend(instruction(98, &[&register(1, 5, 15)]));
    pixel.extend(instruction(101, &[&register(2, 0, 15)]));
    pixel.extend(instruction(88 | 3 << 11, &[&[0x0010_7000, 4], &[0x5555]]));
    pixel.extend(instruction(90, &[&[0x0010_6000, 1]]));
    pixel.extend(instruction(
        72,
        &[
            &register(0, 0, 15),
            &[0x4002, 0, 0, 0, 0],
            &source(7, 4, 0xE4),
            &[0x0010_6000, 1],
            &literal(0.0),
        ],
    ));
    pixel.extend(instruction(
        56,
        &[
            &register(2, 0, 15),
            &source(0, 0, 0xE4),
            &source(1, 5, 0xE4),
        ],
    ));
    pixel.extend(instruction(54, &[&register(2, 0, 8), &literal(0.0)]));
    pixel.extend(instruction(62, &[]));
    let pixel = shader_stage(
        &mut package,
        &pixel,
        0,
        &[("TEXCOORD", 5)],
        &[("SV_TARGET", 0)],
    );
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x48, &vertex.to_le_bytes());
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    for (vertex, texture) in [(true, vertex_texture), (false, pixel_texture)] {
        let stage = if vertex { 0x48 } else { 0x2C8 };
        if !vertex || index != 4 {
            let rows = [4u32, texture]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            array(&mut material, stage + 8, 0x8080_7211, &rows, 8);
        }
        if !vertex || index != 5 {
            let mut row = [0; 16];
            put(&mut row, 0, &sampler.to_le_bytes());
            array(&mut material, stage + 0x40, 0x8080_73F3, &row, 16);
        }
    }
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
fn vertex_images_keep_stage_bindings_separate_and_reach_rendered_varyings() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("vertex-images");
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
            "Case {index}: {actual:?}"
        );
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        std::fs::write(
            output.join(format!("vertex-images-{index}.png")),
            export::png(&rgba, 320, 240).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"case":index,"expected":expected(index),"actual":actual}));
    }
    for index in 4..7 {
        let error = case(index)
            .err()
            .expect("An unavailable vertex image contract was accepted");
        receipt.push(json!({"case":index,"rejected":error}));
    }
    std::fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
