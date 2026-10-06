//! Packaged float texture consumers followed by a native attenuation pass.
use super::*;

pub(crate) fn case(format: u32, cube: bool) -> Model {
    let mut package = Package::default();
    let pixels = pixels(format, cube);
    let payload = package.raw(0, 40, 1, pixels);
    let mut header = vec![0; 40];
    put(&mut header, 4, &format.to_le_bytes());
    put(&mut header, 12, &0xCAFEu16.to_le_bytes());
    for offset in [14, 16] {
        put(
            &mut header,
            offset,
            &(if cube { 2u16 } else { 1 }).to_le_bytes(),
        );
    }
    put(&mut header, 18, &1u16.to_le_bytes());
    put(
        &mut header,
        20,
        &(if cube { 6u16 } else { 1 }).to_le_bytes(),
    );
    header[23] = if cube { 2 } else { 1 };
    put(&mut header, 36, &u32::MAX.to_le_bytes());
    let texture = package.raw(payload, 32, 1, header);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut sampling = vec![0; 52];
    for offset in [4, 8, 12] {
        put(
            &mut sampling,
            offset,
            &(if format == 29 { 4u32 } else { 1 }).to_le_bytes(),
        );
    }
    if format == 29 {
        for (i, value) in [0.5f32, 0.25, 1.0, 0.0].into_iter().enumerate() {
            put(&mut sampling, 28 + i * 4, &value.to_le_bytes());
        }
    }
    let data = package.raw(sampler, 42, 1, sampling);
    package.set_reference(sampler, data);
    let mut code = Vec::new();
    code.extend(instruction(104, &[&[1]]));
    code.extend(instruction(98, &[&register(1, 3, 3)]));
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    code.extend(instruction(
        88 | (if cube { 6 } else { 3 }) << 11,
        &[&[0x0010_7000, 3], &[0x5555]],
    ));
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    if cube {
        let direction = [0x4002, 1f32.to_bits(), 0, 0, 0];
        code.extend(instruction(
            72,
            &[
                &register(2, 0, 15),
                &direction,
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
                &literal(1.0),
            ],
        ));
    } else if format == 29 {
        let outside = [0x4002, (-2f32).to_bits(), (-2f32).to_bits(), 0, 0];
        code.extend(instruction(
            69,
            &[
                &register(2, 0, 15),
                &outside,
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
            ],
        ));
    } else {
        code.extend(instruction(
            69,
            &[
                &register(2, 0, 15),
                &source(1, 3, 0xE4),
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
            ],
        ));
    }
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(0.0)]));
    code.extend(instruction(62, &[]));
    let pixel = shader_stage(
        &mut package,
        &code,
        0,
        &[("TEXCOORD", 3)],
        &[("SV_TARGET", 0)],
    );
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let bindings: Vec<_> = [3u32, texture]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let mut sampling_row = [0; 16];
    put(&mut sampling_row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x8080_73F3, &sampling_row, 16);
    let hdr = package.add(0x8080_71E8, material);
    let attenuate = [
        instruction(54, &[&register(2, 0, 15), &literal(0.0)]),
        instruction(54, &[&register(2, 0, 8), &literal(0.875)]),
        instruction(62, &[]),
    ]
    .concat();
    let pixel = shader_stage(&mut package, &attenuate, 0, &[], &[("SV_TARGET", 0)]);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let attenuate = package.add(0x8080_71E8, material);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    for (index, tag) in [hdr, attenuate].into_iter().enumerate() {
        let material =
            crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model).unwrap();
        model.effects.push(material);
        quad(&mut model, 0.0, Some(index), None);
    }
    model
}

fn pixels(format: u32, cube: bool) -> Vec<u8> {
    let encode = |rgb: [f32; 3]| match format {
        10 => {
            // These exact powers of two have unambiguous IEEE half representations.
            rgb.into_iter()
                .chain([0.0])
                .flat_map(|v| {
                    let bits = if v == 0.0 {
                        0
                    } else {
                        ((((v.to_bits() >> 23) & 255) as i32 - 127 + 15) as u16) << 10
                    };
                    bits.to_le_bytes()
                })
                .collect::<Vec<_>>()
        }
        26 => {
            let lane = |v: f32, fraction: u32| -> u32 {
                ((((v.to_bits() >> 23) & 255) as i32 - 127 + 15) as u32) << fraction
            };
            (lane(rgb[0], 6) | lane(rgb[1], 6) << 11 | lane(rgb[2], 5) << 22)
                .to_le_bytes()
                .to_vec()
        }
        29 => vec![255, 0, 0, 255],
        _ => panic!("Unknown fixture format"),
    };
    let mut pixels = Vec::new();
    if cube {
        for rgb in [[4.0, 0.25, 1.0], [8.0, 0.5, 0.125]] {
            for face in 0..6 {
                for _ in 0..if rgb[0] == 4.0 { 4 } else { 1 } {
                    pixels.extend(encode(if face == 0 { rgb } else { [0.125, 1.0, 4.0] }));
                }
            }
        }
    } else {
        pixels = encode([4.0, 0.25, 1.0]);
    }
    pixels
}

pub(crate) fn expected(format: u32, cube: bool) -> [u8; 3] {
    encoded(if format == 29 {
        [0.0625, 0.03125, 0.125]
    } else if cube {
        [1.0, 0.0625, 0.015625]
    } else {
        [0.5, 0.03125, 0.125]
    })
}

#[test]
fn packaged_hdr_color_plate_survives_basic_material_export() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for format in [10u32, 26, 29] {
        let mut package = Package::default();
        let pixels = match format {
            10 => [0x4400u16, 0x3400, 0x3C00, 0x3C00]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            26 => (17u32 << 6 | (13u32 << 6) << 11 | (15u32 << 5) << 22)
                .to_le_bytes()
                .to_vec(),
            29 => vec![64, 128, 192, 255],
            _ => unreachable!(),
        };
        let payload = package.raw(0, 40, 1, pixels);
        let mut header = vec![0; 40];
        put(&mut header, 4, &format.to_le_bytes());
        for offset in [14, 16, 18, 20] {
            put(&mut header, offset, &1u16.to_le_bytes());
        }
        put(&mut header, 36, &u32::MAX.to_le_bytes());
        let texture = package.raw(payload, 32, 1, header);
        let mut material = vec![0; 0x3A0];
        let bindings: Vec<_> = [0u32, texture]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
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
        let texture =
            crate::model_preview::texture::material(&manager, material, &mut model).unwrap();
        quad(&mut model, 0.0, None, None);
        model.triangle_textures = vec![Some(texture); model.triangles.len()];
        let glb = export::glb(&model, 0.0).unwrap();
        let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
        let primitive = &document["meshes"][0]["primitives"][0];
        let material = &document["materials"][primitive["material"].as_u64().unwrap() as usize];
        let texture = material["pbrMetallicRoughness"]["baseColorTexture"]["index"]
            .as_u64()
            .unwrap() as usize;
        let image =
            &document["images"][document["textures"][texture]["source"].as_u64().unwrap() as usize];
        let view = &document["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
        let offset = 28 + length + view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let png = &glb[offset..offset + view["byteLength"].as_u64().unwrap() as usize];
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 1);
        assert_eq!(png[25], 6);
        let mut compressed = Vec::new();
        let mut at = 8;
        while at + 12 <= png.len() {
            let size = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            if &png[at + 4..at + 8] == b"IDAT" {
                compressed.extend(&png[at + 8..at + 8 + size]);
            }
            at += size + 12;
        }
        let mut raw = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(compressed.as_slice()),
            &mut raw,
        )
        .unwrap();
        // Source RGB [4, 0.25, 1] is linear. Display clipping followed by
        // sRGB encoding yields these bytes, independently of preview helpers.
        let expected = if format == 29 {
            [64, 128, 192, 255]
        } else {
            [255, 137, 255, 255]
        };
        assert_eq!(&raw[1..], &expected, "Color format {format}");
        assert_eq!(raw.len(), 5);
        assert_eq!(raw[0], 0);
        let name = format!("hdr-basic-export-{format}");
        std::fs::write(output.join(format!("{name}.png")), png).unwrap();
        std::fs::write(output.join(format!("{name}.glb")), glb).unwrap();
        receipt.push(json!({"format":format,"source_linear_rgb":if format==29 {None} else {Some([4.0,0.25,1.0])},"expected_rgba":expected,"exported_rgba":&raw[1..]}));
    }
    std::fs::write(
        output.join("hdr-basic-export-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn packaged_hdr_values_survive_sampling_and_later_attenuation() {
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (format, cube) in [10, 26]
        .into_iter()
        .flat_map(|f| [false, true].map(|c| (f, c)))
        .chain([(29, false)])
    {
        let model = case(format, cube);
        let image = render::animated_image(
            &model,
            render::Camera {
                yaw: 0.0,
                pitch: 0.0,
                zoom: 0.5,
                pan: [0.0; 2],
            },
            render::Scene {
                background: [0; 3],
                ..Default::default()
            },
            [320, 240],
            0.0,
        );
        let center = image.pixels[120 * 320 + 160].to_array();
        let expected = expected(format, cube);
        for lane in 0..3 {
            assert!(
                center[lane].abs_diff(expected[lane]) <= 1,
                "Format {format}, cube {cube}: {center:?} != {expected:?}"
            );
        }
        let name = format!("hdr-{format}-{}", if cube { "cube" } else { "2d" });
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 320, 240).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"format":format,"cube":cube,"sampler_border":format==29,"sampled_linear_rgb":if format==29 { [0.5,0.25,1.0] } else if cube { [8.0,0.5,0.125] } else { [4.0,0.25,1.0] },"attenuating_alpha":0.875,"expected":expected,"center":center,"notices":model.notices}));
    }
    std::fs::write(
        output.join("hdr-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
