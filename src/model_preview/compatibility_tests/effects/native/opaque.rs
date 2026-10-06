//! Package-to-frame opaque RGB consumer acceptance, authored before implementation.
use super::*;

fn constant(row: u32, swizzle: u32) -> [u32; 3] {
    [0x0020_8006 | swizzle << 4, 0, row]
}

pub(crate) fn case(invalid: bool) -> Result<Model, String> {
    build(invalid, None, None)
}

pub(crate) fn gain_case(painted: bool, wrong_source: bool) -> Result<Model, String> {
    build(false, Some((painted, wrong_source)), None)
}

pub(super) fn paint_case(alpha: u8, malformed: u8) -> Result<Model, String> {
    build(false, Some((alpha >= 40, false)), Some((alpha, malformed)))
}

fn prefix(gain: Option<(bool, bool)>) -> Vec<u32> {
    let mut code = instruction(104, &[&[12]]);
    code.extend(instruction(89, &[&[0x0020_8000, 0, 4]]));
    code.extend(instruction(98, &[&register(1, 3, 15)]));
    for target in 0..3 {
        code.extend(instruction(101, &[&register(2, target, 15)]));
    }
    for slot in [0, 2, 4, 10, 11] {
        code.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    // Renamed registers, a different gain row and a rectangular placement prevent
    // matching a particular shipped shader instead of its dependencies.
    if let Some((_, wrong_source)) = gain {
        let scale = [
            0x4002,
            0.5f32.to_bits(),
            0.25f32.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
        ];
        let offset = [0x4002, 0.25f32.to_bits(), 0.125f32.to_bits(), 0, 0];
        code.extend(instruction(
            50,
            &[&register(0, 11, 3), &source(1, 3, 0xE4), &scale, &offset],
        ));
        for (temp, slot, mask) in [(1, if wrong_source { 4 } else { 0 }, 7), (9, 2, 8)] {
            code.extend(instruction(
                69,
                &[
                    &register(0, temp, mask),
                    &source(0, 11, 0x44),
                    &[0x0010_7E46, slot],
                    &[0x0010_6000, 1],
                ],
            ));
        }
        code.extend(instruction(
            29,
            &[
                &register(0, 6, 2),
                &source(0, 9, 0xFF),
                &literal(40.0 / 255.0),
            ],
        ));
        code.extend(instruction(
            1,
            &[&register(0, 6, 2), &source(0, 6, 0x55), &literal(1.0)],
        ));
        code.extend(instruction(
            56,
            &[&register(0, 10, 7), &source(0, 1, 0xE4), &constant(3, 0xE4)],
        ));
        let negative_plate = [0x8010_0E46, 0x41, 1];
        code.extend(instruction(
            50,
            &[
                &register(0, 7, 7),
                &negative_plate,
                &constant(3, 0xE4),
                &literal(0.3),
            ],
        ));
        code.extend(instruction(
            50,
            &[
                &register(0, 7, 7),
                &source(0, 6, 0x55),
                &source(0, 7, 0xE4),
                &source(0, 10, 0xE4),
            ],
        ));
    } else {
        code.extend(instruction(54, &[&register(0, 7, 7), &literal(0.1)]));
    }
    code
}

fn constants(material: &mut Vec<u8>, gain: bool, paint: bool) {
    let mut vectors = [0; 64];
    floats(
        &mut vectors,
        0,
        &[if gain { 0.1 } else { 0.5 }, 0.0, 0.0, 0.0],
    );
    floats(&mut vectors, 48, &[0.25, 0.5, 0.75, 1.0]);
    array(material, 0x318, 0x8080_0090, &vectors, 16);
    if gain {
        let mut constants = [0; 16];
        floats(&mut constants, 0, &[1.0; 4]);
        array(material, 0x2F8, 0x8080_0090, &constants, 16);
        let expression = [0x3C, 1, 0, 0x34, 0, 0x01, 0x42, 3, 0x03, 0x43, 3];
        array(material, 0x2E8, 0x8080_0009, &expression, 1);
    }
    if paint {
        super::paint::material(material);
    }
}

fn build(
    invalid: bool,
    gain: Option<(bool, bool)>,
    paint: Option<(u8, u8)>,
) -> Result<Model, String> {
    let mut package = Package::default();
    let mut code = paint.map_or_else(
        || prefix(gain),
        |(_, malformed)| super::paint::prefix(malformed),
    );
    for (temp, slot) in [(4, 10), (5, 11)] {
        code.extend(instruction(
            69,
            &[
                &register(0, temp, 7),
                &source(1, 3, 0x44),
                &[0x0010_7E46, slot],
                &[0x0010_6000, 1],
            ],
        ));
    }
    code.extend(instruction(
        50,
        &[
            &register(0, 4, 7),
            &source(0, 4, 0xE4),
            &constant(0, 0),
            &source(0, 5, 0xE4),
        ],
    ));
    if invalid {
        code.extend(instruction(122, &[&register(0, 4, 7), &source(0, 4, 0xE4)]));
    }
    // Native normalization of the additive term and gear base.
    code.extend(instruction(
        0,
        &[&register(0, 2, 7), &source(0, 7, 0xE4), &source(0, 4, 0xE4)],
    ));
    for (code_id, args) in [
        (
            52,
            vec![
                register(0, 6, 1).to_vec(),
                source(0, 2, 0x55).to_vec(),
                source(0, 2, 0).to_vec(),
            ],
        ),
        (
            52,
            vec![
                register(0, 6, 1).to_vec(),
                source(0, 2, 0xAA).to_vec(),
                source(0, 6, 0).to_vec(),
            ],
        ),
        (
            1 << 13,
            vec![
                register(0, 6, 1).to_vec(),
                source(0, 6, 0).to_vec(),
                literal(-1.0).to_vec(),
            ],
        ),
    ] {
        code.extend(instruction(
            code_id,
            &args.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        ));
    }
    let minus_scalar = [0x8010_0006, 0x41, 6];
    code.extend(instruction(
        0,
        &[&register(0, 6, 1), &minus_scalar, &literal(1.0)],
    ));
    code.extend(instruction(
        50,
        &[
            &register(0, 2, 7),
            &source(0, 7, 0xE4),
            &source(0, 6, 0),
            &source(0, 4, 0xE4),
        ],
    ));
    code.extend(instruction(
        52,
        &[&register(0, 6, 1), &source(0, 2, 0x55), &source(0, 2, 0)],
    ));
    code.extend(instruction(
        52,
        &[&register(0, 6, 1), &source(0, 2, 0xAA), &source(0, 6, 0)],
    ));
    code.extend(instruction(
        52,
        &[&register(0, 6, 1), &source(0, 6, 0), &literal(1.0)],
    ));
    code.extend(instruction(
        14,
        &[&register(0, 2, 7), &source(0, 2, 0xE4), &source(0, 6, 0)],
    ));
    code.extend(instruction(54, &[&register(2, 0, 7), &source(0, 2, 0xE4)]));
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(1.0)]));
    code.extend(instruction(54, &[&register(2, 1, 15), &literal(0.0)]));
    code.extend(instruction(54, &[&register(2, 2, 15), &literal(0.0)]));
    super::metal::output(&mut code, paint.map(|(_, mode)| mode));
    super::normals::output(&mut code, paint.map(|(_, mode)| mode));
    code.extend(instruction(62, &[]));
    let shader = shader_stage(
        &mut package,
        &code,
        0,
        &[
            ("TEXCOORD", 0),
            ("TEXCOORD", 1),
            ("TEXCOORD", 2),
            ("TEXCOORD", 3),
        ],
        &[("SV_TARGET", 0), ("SV_TARGET", 1), ("SV_TARGET", 2)],
    );
    let mut material = vec![0; 0x3A0];
    put(&mut material, 0x2C8, &shader.to_le_bytes());
    constants(&mut material, gain.is_some(), paint.is_some());
    super::normals::material(&mut material, paint.map(|(_, mode)| mode));
    // Opaque color samples must retain the explicit image's linear interpretation.
    let mut bindings = Vec::new();
    for (slot, color) in [
        (10u32, [0, 0, 255, 255]),
        (11, [0, if gain.is_some() { 6 } else { 64 }, 0, 255]),
    ] {
        let pixels = package.raw(0, 0, 0, color.to_vec());
        let mut header = vec![0; 0x40];
        put(&mut header, 0, &4u32.to_le_bytes());
        put(&mut header, 4, &28u32.to_le_bytes());
        for at in [14, 16, 18, 20] {
            put(&mut header, at, &1u16.to_le_bytes());
        }
        put(&mut header, 36, &u32::MAX.to_le_bytes());
        let tag = package.raw(pixels, 32, 1, header);
        bindings.extend(slot.to_le_bytes());
        bindings.extend(tag.to_le_bytes());
    }
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut sampling = vec![0; 52];
    for at in [4, 8, 12] {
        put(&mut sampling, at, &1u32.to_le_bytes());
    }
    let data = package.raw(sampler, 42, 1, sampling);
    package.set_reference(sampler, data);
    let mut row = [0; 16];
    put(&mut row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x8080_73F3, &row, 16);
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
    let effect = crate::model_preview::effects::native::load_opaque(
        &manager,
        material,
        Ok(&[]),
        0,
        if gain.is_some() {
            [2.0, 4.0, -0.5, -0.5]
        } else {
            [1.0, 1.0, 0.0, 0.0]
        },
        &mut model,
    )?
    .ok_or("Missing opaque RGB consumer")?;
    model.effects.push(effect);
    for (tag, rgba) in [
        (
            0xDEAD,
            if gain.is_some() {
                [128, 96, 64, 255]
            } else {
                [0, 0, 0, 255]
            },
        ),
        (0xBEEF, [128, 128, 255, 255]),
    ] {
        model.textures.push(crate::model_preview::texture::Texture {
            mips: None,
            tag,
            size: [1, 1],
            rgba: rgba.to_vec(),
            linear: None,
        });
    }
    quad(&mut model, 0.0, None, None);
    model.triangle_textures = vec![Some(model.textures.len() - 2); 2];
    model.triangle_normals = vec![Some(model.textures.len() - 1); 2];
    model.triangle_effects = vec![Some(0); 2];
    model.uvs.fill([0.5, 0.5]);
    if let Some((painted, _)) = gain {
        let index = model.textures.len();
        model.textures.push(crate::model_preview::texture::Texture {
            mips: None,
            tag: 0xA11,
            size: [8, 4],
            rgba: [255, 128, 0, if painted { 255 } else { 0 }].repeat(32),
            linear: None,
        });
        model.triangle_gearstacks = vec![Some(index); 2];
        model.textures[index - 2].size = [8, 4];
        model.textures[index - 2].rgba = [128, 96, 64, 255].repeat(32);
        model.uvs[..4].copy_from_slice(&[
            [0.25, 0.125],
            [0.75, 0.125],
            [0.75, 0.375],
            [0.25, 0.375],
        ]);
    }
    // A rear emissive red draw must be hidden by the opaque color consumer.
    quad(&mut model, 0.1, None, Some([1.0, 0.0, 0.0]));
    model.triangle_textures.extend([None; 2]);
    model.triangle_normals.extend([None; 2]);
    if let Some((alpha, _)) = paint {
        super::paint::finish(&mut model, alpha);
    }
    Ok(model)
}

#[test]
fn opaque_native_rgb_keeps_color_depth_and_exported_geometry() {
    let model = case(false).unwrap();
    assert!(
        case(true).is_err(),
        "A sampled temporary derivative must stay unavailable"
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
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
        0.0,
    );
    let actual = image.pixels[120 * 320 + 160].to_array();
    // At a black gear base, sampled additive RGB is [0, 64/255, 0.5].
    let expected = encoded([0.0, 64.0 / 255.0, 0.5]);
    let linear = actual.map(|value| {
        let v = value as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    assert!(
        linear[0] < 0.2,
        "The rear red draw leaked through: {actual:?}"
    );
    for (lane, value) in [(1, 64.0 / 255.0), (2, 0.5)] {
        assert!(
            (linear[lane] - linear[0] - value).abs() < 0.02,
            "{actual:?} lost the independent additive color {value}"
        );
    }
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("opaque-native-color.png"),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
    let glb = export::glb(&model, 0.0).unwrap();
    let count = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&glb[20..20 + count]).unwrap();
    assert!(
        !document["meshes"][0]["primitives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        document["asset"]["extras"]["omitted"]
            .as_str()
            .unwrap()
            .contains("opaque")
    );
    std::fs::write(output.join("opaque-native-color.glb"), glb).unwrap();
    std::fs::write(output.join("opaque-native-color-receipt.json"), serde_json::to_vec_pretty(&json!({"expected":expected,"actual":actual,"unsupported_dependency_rejected":true,"opaque_geometry_exported":true,"static_export_color_limit_explicit":true})).unwrap()).unwrap();
}
