//! Explicit shader gradients through package reads and both preview renderers.
use super::*;

const COLORS: [[f32; 3]; 6] = [
    [0.125, 0.25, 0.5],
    [0.25, 0.5, 0.125],
    [0.5, 0.125, 0.25],
    [0.75, 0.25, 0.5],
    [0.25, 0.75, 0.125],
    [1.0, 0.5, 0.25],
];

fn vector(v: [f32; 4]) -> Vec<u32> {
    std::iter::once(0x4002).chain(v.map(f32::to_bits)).collect()
}

fn settings(case: u8) -> ([f32; 4], [f32; 4], f32, [f32; 2], u32, f32) {
    let mut dx = [0.; 4];
    let mut dy = [0.; 4];
    let (mut bias, mut bounds, mut filter) = (0., [0., 5.], 0x15);
    let lod = match case {
        0 => 0.,
        1 => {
            dx[0] = 0.125;
            2.
        }
        2 => {
            dy[1] = 0.5;
            3.
        }
        3 => {
            dx[0] = -0.25;
            dy[1] = -0.125;
            3.
        }
        4 => {
            dx[0] = 2f32.sqrt() / 16.;
            1.5
        }
        5 => {
            // Stay away from a point-mip tie, whose side depends on log2 precision.
            dx[0] = 2f32.powf(1.75) / 32.;
            filter = 0x14;
            2.
        }
        6 => {
            dx[0] = 0.125;
            bias = 1.;
            3.
        }
        7 => {
            dy[1] = 0.5;
            bounds[1] = 1.;
            1.
        }
        8 => {
            bounds[0] = 2.;
            2.
        }
        9 => 2., // sample_l, distinct from supplied gradients.
        10 => {
            dx[2] = 0.5;
            1.
        } // Cube direction +X, dZ maps to dU / 2.
        11 => {
            dy[1] = 1.;
            2.
        }
        12 => {
            dx[0] = 0.25;
            dy[1] = 0.125;
            filter = 0x55;
            1.
        }
        _ => unreachable!(),
    };
    (dx, dy, bias, bounds, filter, lod)
}

fn fixture(case: u8) -> (tempfile::TempDir, u32) {
    let mut package = Package::default();
    let cube = matches!(case, 10 | 11);
    let (dx, dy, bias, bounds, filter, _) = settings(case);
    let mut code = instruction(104, &[&[3]]);
    code.extend(instruction(
        88 | (if cube { 6 } else { 3 }) << 11,
        &[&source(7, 3, 0xE4), &[0x5555]],
    ));
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    // A temporary with swapped lanes and a sign modifier forces evaluated operand use.
    code.extend(instruction(
        54,
        &[&register(0, 0, 15), &vector([dx[1], dx[0], dx[2], dx[3]])],
    ));
    code.extend(instruction(54, &[&register(0, 1, 15), &vector(dy)]));
    let modified = [0x8010_0006 | 0xE1 << 4, 0x41, 0];
    let coordinate = vector(if cube {
        [1., 0., 0., 0.]
    } else {
        [0.3, 0.6, 0., 0.]
    });
    if case == 9 {
        code.extend(instruction(
            72,
            &[
                &register(0, 2, 15),
                &coordinate,
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
                &literal(2.),
            ],
        ));
    } else {
        code.extend(instruction(
            73,
            &[
                &register(0, 2, 15),
                &coordinate,
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
                &modified,
                &source(0, 1, 0xE4),
            ],
        ));
    }
    code.extend(instruction(54, &[&register(2, 0, 7), &source(0, 2, 0xE4)]));
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(1.)]));
    code.extend(instruction(62, &[]));
    let shader = shader_stage(&mut package, &code, 0, &[], &[("SV_TARGET", 0)]);
    let size: [usize; 2] = if cube { [8, 8] } else { [32, 16] };
    let levels = if cube { 4 } else { 6 };
    let mut pixels = Vec::new();
    for (level, rgb) in COLORS.iter().enumerate().take(levels) {
        let rgba = [rgb[0], rgb[1], rgb[2], 1.];
        let bytes = rgba
            .into_iter()
            .flat_map(|v| {
                let bits: u16 = if v == 0.125 {
                    0x3000
                } else if v == 0.25 {
                    0x3400
                } else if v == 0.5 {
                    0x3800
                } else if v == 0.75 {
                    0x3A00
                } else {
                    0x3C00
                };
                bits.to_le_bytes()
            })
            .collect::<Vec<_>>();
        pixels.extend(bytes.repeat(
            (size[0] >> level).max(1) * (size[1] >> level).max(1) * if cube { 6 } else { 1 },
        ));
    }
    let data = package.raw(0, 32, 0, pixels);
    let mut header = vec![0; 40];
    put(&mut header, 4, &10u32.to_le_bytes());
    for (at, value) in [
        (14, size[0] as u16),
        (16, size[1] as u16),
        (18, 1),
        (20, if cube { 6 } else { 1 }),
    ] {
        put(&mut header, at, &value.to_le_bytes());
    }
    header[23] = levels as u8;
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    let texture = package.raw(data, 32, 1, header);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut sampling = vec![0; 52];
    put(&mut sampling, 0, &filter.to_le_bytes());
    for at in [4, 8, 12] {
        put(&mut sampling, at, &3u32.to_le_bytes());
    }
    floats(&mut sampling, 16, &[bias]);
    put(
        &mut sampling,
        20,
        &(if case == 12 { 4u32 } else { 1 }).to_le_bytes(),
    );
    floats(&mut sampling, 44, &bounds);
    let data = package.raw(sampler, 42, 1, sampling);
    package.set_reference(sampler, data);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &shader.to_le_bytes());
    let mut row = [0; 8];
    put(&mut row, 0, &3u32.to_le_bytes());
    put(&mut row, 4, &texture.to_le_bytes());
    array(&mut material, 0x2D0, 0x80807211, &row, 8);
    let mut row = [0; 16];
    put(&mut row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x808073F3, &row, 16);
    let tag = package.add(0x808071E8, material);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    (directory, tag)
}

fn case(index: u8, retain: Option<&Path>) -> (String, Model, [u8; 3]) {
    let (directory, tag) = fixture(index);
    if let Some(output) = retain {
        std::fs::create_dir_all(output).unwrap();
        for entry in std::fs::read_dir(directory.path()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), output.join(entry.file_name())).unwrap();
        }
    }
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    let effect =
        crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model).unwrap();
    model.effects.push(effect);
    quad(&mut model, 0., Some(0), None);
    let level = settings(index).5;
    let low = COLORS[level.floor() as usize];
    let high = COLORS[level.ceil() as usize];
    let expected = encoded(std::array::from_fn(|i| {
        low[i] * (1. - level.fract()) + high[i] * level.fract()
    }));
    (format!("native-gradient-{index}"), model, expected)
}

pub(super) fn render_cases(cases: &mut Vec<(String, Model, [u8; 3])>) {
    cases.extend((0..13).map(|index| case(index, None)));
}

#[test]
fn gradient_operands_select_authored_mips_in_saved_previews() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("gradients");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for index in 0..13 {
        let (name, model, expected) = case(index, Some(&output.join(format!("case-{index}"))));
        for size in [[160, 128], [320, 256]] {
            let image = render::animated_image(
                &model,
                render::Camera {
                    yaw: 0.,
                    pitch: 0.,
                    ..Default::default()
                },
                render::Scene {
                    filmic: false,
                    bloom: false,
                    background: [0; 3],
                    ..render::Scene::unit_exposure()
                },
                size,
                0.,
            );
            let actual = image.pixels[size[0] * (size[1] / 2) + size[0] / 2].to_array();
            let rgba = image
                .pixels
                .iter()
                .flat_map(|v| v.to_array())
                .collect::<Vec<_>>();
            std::fs::write(
                output.join(format!("{name}-{}.png", size[0])),
                export::png(&rgba, size[0], size[1]).unwrap(),
            )
            .unwrap();
            receipt.push(json!({"case":index,"size":size,"expected":expected,"actual":actual,"lod":settings(index).5}));
            std::fs::write(
                output.join("readback.json"),
                serde_json::to_vec_pretty(&receipt).unwrap(),
            )
            .unwrap();
            assert!(
                (0..3).all(|i| actual[i].abs_diff(expected[i]) <= 1),
                "{name}: {actual:?} != {expected:?}"
            );
        }
    }
}
