//! Package-to-render unsigned arithmetic acceptance, prepared before instruction support.
use super::*;

fn bits(value: [u32; 4]) -> [u32; 5] {
    [0x4002, value[0], value[1], value[2], value[3]]
}

fn division(mode: usize) -> Vec<u32> {
    let mut code = instruction(104, &[&[3]]);
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    let numerator = bits([u32::MAX, 17, 0x8000_0000, 9]);
    let divisor = bits([3, 5, 2, 0]);
    let quotient = bits([1_431_655_765, 3, 1_073_741_824, u32::MAX]);
    let remainder = bits([0, 2, 0, u32::MAX]);
    code.extend(instruction(54, &[&register(0, 0, 15), &numerator]));
    let null = [0xD000];
    let quotient_destination = register(0, 0, 15);
    let remainder_destination = register(0, 1, 15);
    code.extend(instruction(
        78,
        &[
            if mode == 1 {
                &null
            } else {
                &quotient_destination
            },
            if mode == 2 {
                &null
            } else {
                &remainder_destination
            },
            &source(0, 0, 0xE4),
            &divisor,
        ],
    ));
    code.extend(instruction(
        32,
        &[
            &register(0, 2, 15),
            &source(0, if mode == 1 { 1 } else { 0 }, 0xE4),
            if mode == 1 { &remainder } else { &quotient },
        ],
    ));
    if mode == 0 {
        code.extend(instruction(
            32,
            &[&register(0, 1, 15), &source(0, 1, 0xE4), &remainder],
        ));
        code.extend(instruction(
            1,
            &[
                &register(0, 2, 15),
                &source(0, 2, 0xE4),
                &source(0, 1, 0xE4),
            ],
        ));
    }
    code
}

fn shader_code(mode: usize) -> Vec<u32> {
    let mut code = division(mode);
    code.extend(instruction(
        80,
        &[
            &register(0, 1, 15),
            &bits([u32::MAX, 17, 0x8000_0000, 9]),
            &bits([0x8000_0000, 18, 0x8000_0000, 10]),
        ],
    ));
    code.extend(instruction(
        32,
        &[
            &register(0, 1, 15),
            &source(0, 1, 0xE4),
            &bits([u32::MAX, 0, u32::MAX, 0]),
        ],
    ));
    code.extend(instruction(
        1,
        &[
            &register(0, 2, 15),
            &source(0, 2, 0xE4),
            &source(0, 1, 0xE4),
        ],
    ));
    // Include the zero-divisor W witness in each RGB channel. All four lanes must pass.
    code.extend(instruction(
        1,
        &[
            &register(0, 2, 15),
            &source(0, 2, 0xE4),
            &source(0, 2, 0xFF),
        ],
    ));
    code.extend(instruction(
        1,
        &[&register(0, 2, 15), &source(0, 2, 0xE4), &bits([1; 4])],
    ));
    code.extend(instruction(86, &[&register(0, 2, 15), &source(0, 2, 0xE4)]));
    code.extend(instruction(
        56,
        &[
            &register(2, 0, 15),
            &source(0, 2, 0xE4),
            &bits([0.125f32.to_bits(), 0.25f32.to_bits(), 0.75f32.to_bits(), 0]),
        ],
    ));
    code.extend(instruction(62, &[]));
    code
}

pub(crate) fn case(mode: usize) -> Model {
    let mut package = Package::default();
    let code = shader_code(mode);
    let shader = shader_stage(&mut package, &code, 0, &[], &[("SV_TARGET", 0)]);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &shader.to_le_bytes());
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
    let effect =
        crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model).unwrap();
    model.effects.push(effect);
    quad(&mut model, 0.0, Some(0), None);
    model
}

#[test]
fn unsigned_metadata_arithmetic_preserves_bits_aliases_and_null_outputs() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for mode in 0..3 {
        let model = case(mode);
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
            [320, 240],
            0.0,
        );
        let center = image.pixels[120 * 320 + 160].to_array();
        for (actual, expected) in center[..3].iter().zip([99u8, 137, 225]) {
            assert!(actual.abs_diff(expected) <= 1, "Mode {mode}: {center:?}");
        }
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("unsigned-arithmetic-{mode}.png")),
            export::png(&rgba, 320, 240).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"mode":mode,"expected":[99,137,225],"actual":center,"quotient":[1431655765u32,3,1073741824,u32::MAX],"remainder":[0,2,0,u32::MAX]}));
    }
    std::fs::write(
        output.join("unsigned-arithmetic-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
