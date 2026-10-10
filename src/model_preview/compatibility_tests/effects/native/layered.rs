//! Array and volume programs from package storage to independently specified pixels.
use super::*;

pub(crate) const CASES: [u8; 27] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 23, 24, 25, 26, 27, 28,
    29,
];

pub(crate) fn expected(index: u8) -> [u8; 3] {
    match index {
        0 | 2 | 8 => [255, 0, 0],
        1 | 3 | 7 => [0, 0, 255],
        4 | 11 | 15 | 19 | 23 | 24 | 25 | 27 => [99, 137, 188],
        26 | 28 => [0, 255, 0],
        5 => [71, 207, 137],
        6 => [188, 0, 188],
        9 => [137, 188, 225],
        10 | 29 => [137, 188, 225],
        12 | 18 => [0; 3],
        13 => [137, 137, 225],
        14 => [137; 3],
        16 => [146; 3],
        17 => [188, 0, 188],
        _ => panic!("Unknown layered texture case"),
    }
}

pub(crate) fn case(index: u8) -> Result<Model, String> {
    let volume = matches!(index, 6..=10 | 12 | 14 | 16 | 17 | 29);
    let ordinary = index == 19;
    let layers = if ordinary {
        1
    } else if volume {
        2
    } else {
        3
    };
    let mut package = Package::default();
    let texture = image(&mut package, index, volume, layers);
    let sampler = sampler(&mut package, index);
    let pixel = program(&mut package, index, volume, ordinary);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let bindings: Vec<_> = [4u32, texture]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let mut row = [0; 16];
    put(&mut row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x8080_73F3, &row, 16);
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
    if matches!(index, 23 | 24) {
        model.uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    }
    Ok(model)
}

fn layer_color(index: u8, volume: bool, layer: usize) -> [u8; 4] {
    if index == 16 {
        [if layer == 0 { 64 } else { 192 }; 4]
    } else if index == 19 {
        [32, 64, 128, 255]
    } else if volume {
        if layer == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    } else {
        [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]][layer]
    }
}

fn image(package: &mut Package, index: u8, volume: bool, layers: usize) -> u32 {
    let mut pixels = Vec::new();
    for layer in 0..layers {
        let rgba = layer_color(index, volume, layer);
        for _ in 0..4 {
            pixels.extend(rgba);
        }
    }
    // Mip-major storage differs from an array-major D3D subresource list.
    for layer in 0..if volume { 1 } else { layers } {
        pixels.extend(if volume {
            [64, 128, 192, 255]
        } else {
            [[255, 255, 0, 255], [32, 64, 128, 255], [0, 255, 255, 255]][layer]
        });
    }
    if index == 20 {
        pixels.pop();
    }
    let tail = pixels.split_off(layers * 16);
    let leading = package.raw(0, 40, 1, pixels);
    let trailing = package.raw(0, 40, 1, tail);
    let mut header = vec![0; 40];
    put(
        &mut header,
        4,
        &(if index == 16 { 29u32 } else { 28 }).to_le_bytes(),
    );
    put(&mut header, 12, &0xCAFEu16.to_le_bytes());
    put(&mut header, 14, &2u16.to_le_bytes());
    put(&mut header, 16, &2u16.to_le_bytes());
    put(
        &mut header,
        18,
        &(if volume || index == 21 { 2u16 } else { 1 }).to_le_bytes(),
    );
    put(
        &mut header,
        20,
        &(if volume { 1u16 } else { layers as u16 }).to_le_bytes(),
    );
    header[23] = if index == 22 { 8 } else { 2 };
    put(&mut header, 36, &leading.to_le_bytes());
    package.raw(trailing, 32, 1, header)
}

fn sampler(package: &mut Package, index: u8) -> u32 {
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut sampling = vec![0; 52];
    put(
        &mut sampling,
        0,
        &(if index >= 28 {
            85u32
        } else if index == 27 {
            20
        } else {
            21
        })
        .to_le_bytes(),
    );
    for offset in [4, 8, 12] {
        put(&mut sampling, offset, &3u32.to_le_bytes());
    }
    put(
        &mut sampling,
        12,
        &(if index == 7 {
            1u32
        } else if index == 9 {
            4
        } else {
            3
        })
        .to_le_bytes(),
    );
    put(
        &mut sampling,
        16,
        &(if index == 25 {
            1f32
        } else if index == 26 {
            -1.0
        } else {
            0.0
        })
        .to_le_bytes(),
    );
    put(
        &mut sampling,
        20,
        &(if index >= 28 { 8u32 } else { 1 }).to_le_bytes(),
    );
    put(&mut sampling, 48, &8f32.to_le_bytes());
    for (lane, value) in [0.25f32, 0.5, 0.75, 0.0].into_iter().enumerate() {
        put(&mut sampling, 28 + lane * 4, &value.to_le_bytes());
    }
    let data = package.raw(sampler, 42, 1, sampling);
    package.set_reference(sampler, data);
    sampler
}

fn program(package: &mut Package, index: u8, volume: bool, ordinary: bool) -> u32 {
    let affine = matches!(index, 23 | 24);
    let mut code = instruction(104, &[&[1]]);
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    code.extend(instruction(
        88 | (if ordinary {
            3
        } else if volume {
            5
        } else {
            8
        }) << 11,
        &[&[0x0010_7000, 4], &[0x5555]],
    ));
    if matches!(index, 13 | 14) {
        code.extend(instruction(
            61 | 2 << 11,
            &[&register(0, 0, 15), &[0x4001, 1], &source(7, 4, 0xE4)],
        ));
        code.extend(instruction(86, &[&register(0, 0, 15), &source(0, 0, 0xE4)]));
        code.extend(instruction(
            56,
            &[&register(2, 0, 15), &source(0, 0, 0xE4), &literal(0.25)],
        ));
    } else if matches!(index, 11 | 12 | 18 | 19) {
        let coordinate = match index {
            11 => [0x4002, 0, 0, 1, 1],
            12 => [0x4002, 0, 0, 2, 0],
            18 => [0x4002, 0, 0, 0, u32::MAX],
            _ => [0x4002, 1, 1, 0, 0],
        };
        code.extend(instruction(
            45,
            &[&register(2, 0, 15), &coordinate, &source(7, 4, 0xE4)],
        ));
    } else {
        sampling_code(&mut code, index, affine);
    }
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(0.0)]));
    code.extend(instruction(62, &[]));
    let inputs: &[(&str, u32)] = if affine { &[("TEXCOORD", 3)] } else { &[] };
    shader_stage(package, &code, 0, inputs, &[("SV_TARGET", 0)])
}

fn sampling_code(code: &mut Vec<u32>, index: u8, affine: bool) {
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    if affine {
        code.extend(instruction(98, &[&register(1, 3, 3)]));
        if index == 24 {
            code.extend([53 | 3 << 11, 6, 1024f32.to_bits(), 0, 0, 0]);
        }
        code.extend(instruction(
            56,
            &[
                &register(0, 0, 3),
                &source(1, 3, 0xE4),
                &if index == 24 {
                    source(9, 0, 0)
                } else {
                    literal(1024.0)
                },
            ],
        ));
        code.extend(instruction(54, &[&register(0, 0, 4), &literal(1.0)]));
    }
    let z: f32 = match index {
        0 => 0.5,
        1 => 1.5,
        2 => -20.0,
        3 => 99.0,
        7 | 8 => -0.25,
        9 => 2.0,
        6 | 16 | 17 | 29 => 0.5,
        _ => 1.0,
    };
    let coordinate = if affine {
        source(0, 0, 0xE4).to_vec()
    } else {
        vec![0x4002, 0.5f32.to_bits(), 0.5f32.to_bits(), z.to_bits(), 0]
    };
    let lod = if matches!(index, 4 | 10 | 26) {
        1.0
    } else if matches!(index, 5 | 27) {
        0.5
    } else {
        0.0
    };
    let gradients = [0x4002, 1f32.to_bits(), 0, 0, 0];
    let minor = [0x4002, 0, 0.0625f32.to_bits(), 0, 0];
    let mut operands: Vec<&[u32]> = vec![];
    let destination = register(2, 0, 15);
    let resource = source(7, 4, 0xE4);
    let sample_register = [0x0010_6000, 1];
    let level = literal(lod);
    operands.extend([
        destination.as_slice(),
        &coordinate,
        &resource,
        &sample_register,
    ]);
    if index == 15 {
        operands.extend([gradients.as_slice(), &gradients]);
    } else if index >= 28 {
        operands.extend([gradients.as_slice(), &minor]);
    } else if index != 17 && !affine {
        operands.push(&level);
    }
    code.extend(instruction(
        if index == 15 || index >= 28 {
            73
        } else if index == 17 || affine {
            69
        } else {
            72
        },
        &operands,
    ));
}

#[test]
fn layered_textures_preserve_slices_mips_and_native_sampling_coordinates() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("layered");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for index in CASES {
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
            output.join(format!("layered-{index}.png")),
            export::png(&rgba, 320, 240).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"case":index,"expected":expected(index),"actual":actual}));
    }
    for index in 20..23 {
        let error = case(index)
            .err()
            .expect("Invalid layered texture was accepted");
        receipt.push(json!({"case":index,"rejected":error}));
    }
    std::fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
