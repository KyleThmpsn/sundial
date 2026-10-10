//! Package decal composition, coverage and occlusion prepared before the layer decoder.
use super::*;

fn color(package: &mut Package, rgba: [u8; 4]) -> u32 {
    let data = package.raw(0, 0, 0, rgba.to_vec());
    let mut header = vec![0; 40];
    put(&mut header, 4, &29u32.to_le_bytes());
    for at in [14, 16, 18, 20] {
        put(&mut header, at, &1u16.to_le_bytes());
    }
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    package.raw(data, 32, 1, header)
}

fn fixture(alpha: u8, behind: bool, valid: bool) -> Model {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    let base = color(&mut package, [32, 64, 96, 255]);
    let overlay = color(&mut package, [192, 64, 32, alpha]);
    let mut code = instruction(104, &[&[2]]);
    code.extend(instruction(89, &[&[0x0020_8000, 0, 1]]));
    code.extend(instruction(88 | 3 << 11, &[&[0x0010_7000, 2], &[0x5555]]));
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    code.extend(instruction(98, &[&register(1, 3, 3)]));
    for target in 0..3 {
        code.extend(instruction(101, &[&register(2, target, 15)]));
    }
    code.extend(instruction(
        69,
        &[
            &register(0, 0, 15),
            &source(1, 3, 0x44),
            &source(7, 2, 0xE4),
            &[0x0010_6000, 1],
        ],
    ));
    code.extend(instruction(
        56,
        &[&register(0, 0, 7), &source(0, 0, 0xE4), &source(0, 0, 0xFF)],
    ));
    code.extend(instruction(
        54 | 1 << 13,
        &[&register(0, 1, 1), &constant(0, 0)],
    ));
    code.extend(instruction(
        56,
        &[&register(2, 0, 7), &source(0, 0, 0xE4), &source(0, 1, 0)],
    ));
    let negative_alpha = [0x8010_0006 | 0xFF << 4, 0x41, 0];
    // Equivalent native inverse-alpha tail, including the material's opacity control.
    code.extend(instruction(
        0,
        &[&register(0, 0, 1), &negative_alpha, &literal(0.0)],
    ));
    code.extend(instruction(
        50,
        &[
            &register(2, 0, 8),
            &source(0, 1, 0),
            &source(0, 0, 0),
            &literal(if valid { 1.0 } else { 0.5 }),
        ],
    ));
    code.extend(instruction(54, &[&register(2, 1, 15), &literal(0.0)]));
    code.extend(instruction(54, &[&register(2, 2, 15), &literal(0.0)]));
    code.extend(instruction(62, &[]));
    let pixel = shader_stage(
        &mut package,
        &code,
        0,
        &[("TEXCOORD", 3)],
        &[("SV_TARGET", 0), ("SV_TARGET", 1), ("SV_TARGET", 2)],
    );
    let mut materials = Vec::new();
    for (index, texture) in [base, overlay].into_iter().enumerate() {
        let mut material = vec![0; 0x360];
        let mut binding = [0; 8];
        put(
            &mut binding,
            0,
            &(if index == 0 { 0u32 } else { 2 }).to_le_bytes(),
        );
        put(&mut binding, 4, &texture.to_le_bytes());
        array(&mut material, 0x2D0, 0x8080_7211, &binding, 8);
        if index == 1 {
            material[0x20] = 0x96;
            put(&mut material, 0x2C8, &pixel.to_le_bytes());
            let constants: Vec<_> = [1.0f32, 0.0, 0.0, 0.0]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            array(&mut material, 0x318, 0x8080_0090, &constants, 16);
            let sampler = package.raw(0, 34, 1, vec![0; 16]);
            let mut descriptor = vec![0; 52];
            for at in [4, 8, 12, 20] {
                put(&mut descriptor, at, &1u32.to_le_bytes());
            }
            put(&mut descriptor, 0, &0x15u32.to_le_bytes());
            put(&mut descriptor, 48, &f32::MAX.to_le_bytes());
            let data = package.raw(sampler, 42, 1, descriptor);
            package.set_reference(sampler, data);
            let mut row = [0; 16];
            put(&mut row, 0, &sampler.to_le_bytes());
            array(&mut material, 0x308, 0x8080_73F3, &row, 16);
        }
        materials.push(package.add(0x8080_71E8, material));
    }
    let mut vertices = Vec::new();
    // The refused writer uses the ordinary opaque path, whose equal-depth rule keeps
    // the first surface. Put that negative control in front so its fallback is visible.
    let layer_depth = match (behind, valid) {
        (true, _) => 0.1,
        (false, false) => -0.1,
        (false, true) => 0.0,
    };
    for depth in [0.0, layer_depth] {
        for [x, z] in [[-1.0f32, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            vertices.extend(
                [x, depth, z, 0.5, 0.5, 0.0, -1.0, 0.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes),
            );
        }
    }
    let vertices = package.vertex(32, vertices);
    let indices: Vec<_> = [0u16, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let data = package.raw(0, 0, 0, indices);
    let mut header = vec![0; 16];
    put(&mut header, 8, &24u64.to_le_bytes());
    let indices = package.raw(data, 32, 6, header);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 136], 136);
    put(&mut model, mesh, &vertices.to_le_bytes());
    put(&mut model, mesh + 16, &indices.to_le_bytes());
    for stage in 1..24 {
        put(
            &mut model,
            mesh + 40 + stage * 2,
            &(if stage == 1 { 1i16 } else { 2 }).to_le_bytes(),
        );
    }
    for stage in 0..2 {
        put(&mut model, mesh + 88 + stage * 2, &13u16.to_le_bytes());
    }
    let mut parts = Vec::new();
    for (index, material) in materials.into_iter().enumerate() {
        let mut part = [0; 32];
        put(&mut part, 0, &material.to_le_bytes());
        put(&mut part, 4, &(-1i16).to_le_bytes());
        put(&mut part, 6, &3u16.to_le_bytes());
        put(&mut part, 8, &((index * 6) as u32).to_le_bytes());
        put(&mut part, 12, &6u32.to_le_bytes());
        parts.extend(part);
    }
    array(&mut model, mesh + 24, 0x8080_737E, &parts, 32);
    let tag = package.add(MODEL, model);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    fixtures::load(directory.path(), tag).unwrap()
}

pub(crate) fn cases() -> Vec<(String, Model, render::Scene, [u8; 3])> {
    [0u8, 64, 255]
        .into_iter()
        .flat_map(|alpha| [false, true].map(move |behind| (alpha, behind)))
        .map(|(alpha, behind)| {
            let amount = if behind {
                0.0
            } else {
                f64::from(alpha) / 255.0
            };
            let expected = [192.0f64, 64.0, 32.0]
                .into_iter()
                .zip([32.0f64, 64.0, 96.0])
                .map(|(a, b)| {
                    let decode = |v: f64| ((v / 255.0 + 0.055) / 1.055).powf(2.4);
                    let value = (decode(a) * 0.96 + 0.0128) * amount + decode(b) * (1.0 - amount);
                    (255.0 * (1.055 * value.powf(1.0 / 2.4) - 0.055)).round() as u8
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            (
                format!("vehicle-decal-{alpha}-{behind}"),
                fixture(alpha, behind, true),
                render::Scene {
                    key: 0.0,
                    fill: 1.0,
                    ..render::Scene::unprocessed()
                },
                expected,
            )
        })
        .collect()
}

#[test]
fn native_decals_preserve_the_surface_beneath_and_respect_occlusion() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("vehicle-decals");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for (name, model, scene, expected) in cases() {
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
        let actual = image.pixels[90 * 240 + 120].to_array();
        let error = actual[..3]
            .iter()
            .zip(expected)
            .map(|(&a, b)| a.abs_diff(b))
            .max()
            .unwrap();
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 240, 180).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"case":name,"actual":actual,"expected":expected,"maximum_error":error,"notices":model.notices}));
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
    }
    let changed = fixture(64, false, false);
    let image = render::styled_image(
        &changed,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene {
            key: 0.0,
            fill: 1.0,
            ..render::Scene::unprocessed()
        },
        [240, 180],
        0.0,
        render::Style::Textured,
    );
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("altered-factor.png"),
        export::png(&rgba, 240, 180).unwrap(),
    )
    .unwrap();
    let actual = image.pixels[90 * 240 + 120].to_array();
    assert!(
        actual[..3]
            .iter()
            .zip([192u8, 64, 32])
            .all(|(&a, b)| a.abs_diff(b) <= 1),
        "An altered destination factor must retain the ordinary color-texture fallback: {actual:?}"
    );
}
