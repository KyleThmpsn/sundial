//! Package-to-render expectations prepared before native program support.
use super::*;
pub(crate) mod derivative;
mod gain;
mod gradients;
pub(crate) mod hdr;
pub(crate) mod immediate;
pub(crate) mod integer;
pub(crate) mod layered;
pub(crate) mod metal;
pub(crate) mod normal_blue;
pub(crate) mod normals;
pub(crate) mod opaque;
pub(crate) mod paint;
pub(in crate::model_preview::compatibility_tests) mod repack;
mod stored;
pub(crate) mod vertex_image;
pub(in crate::model_preview) use stored::motion_case;
pub(crate) use stored::opaque_detail_case;

pub(in crate::model_preview::compatibility_tests) fn instruction(
    code: u32,
    operands: &[&[u32]],
) -> Vec<u32> {
    let length = 1 + operands.iter().map(|v| v.len()).sum::<usize>();
    std::iter::once(code | (length as u32) << 24)
        .chain(operands.iter().flat_map(|v| v.iter().copied()))
        .collect()
}

pub(in crate::model_preview::compatibility_tests) fn register(
    kind: u32,
    index: u32,
    mask: u32,
) -> [u32; 2] {
    [0x0010_0002 | kind << 12 | mask << 4, index]
}

pub(in crate::model_preview::compatibility_tests) fn source(
    kind: u32,
    index: u32,
    swizzle: u32,
) -> [u32; 2] {
    [0x0010_0006 | kind << 12 | swizzle << 4, index]
}

pub(in crate::model_preview::compatibility_tests) fn literal(v: f32) -> [u32; 2] {
    [0x4001, v.to_bits()]
}

fn shader(package: &mut Package, code: &[u32]) -> u32 {
    shader_stage(package, code, 0, &[], &[])
}

pub(in crate::model_preview::compatibility_tests) fn shader_stage(
    package: &mut Package,
    code: &[u32],
    stage: u32,
    inputs: &[(&str, u32)],
    outputs: &[(&str, u32)],
) -> u32 {
    let words = [
        vec![stage << 16 | 0x50, (code.len() + 2) as u32],
        code.to_vec(),
    ]
    .concat();
    let signature = |rows: &[(&str, u32)]| {
        let mut bytes = vec![0; 8 + rows.len() * 24];
        put(&mut bytes, 0, &(rows.len() as u32).to_le_bytes());
        for (i, &(name, register)) in rows.iter().enumerate() {
            let offset = bytes.len() as u32;
            put(&mut bytes, 8 + i * 24, &offset.to_le_bytes());
            let semantic_index = if name == "SV_TARGET"
                || name == "TEXCOORD"
                    && !(std::ptr::eq(rows, inputs)
                        && inputs.iter().any(|(name, _)| *name == "SV_VERTEXID"))
            {
                register
            } else {
                0
            };
            put(&mut bytes, 12 + i * 24, &semantic_index.to_le_bytes());
            if name == "SV_VERTEXID" {
                put(&mut bytes, 16 + i * 24, &6u32.to_le_bytes());
            }
            put(&mut bytes, 20 + i * 24, &3u32.to_le_bytes());
            put(&mut bytes, 24 + i * 24, &register.to_le_bytes());
            bytes[28 + i * 24] = 15;
            bytes.extend(name.as_bytes());
            bytes.push(0);
        }
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        bytes
    };
    let chunks = [
        (
            b"SHEX",
            words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        (b"ISGN", signature(inputs)),
        (b"OSGN", signature(outputs)),
    ];
    let mut dxbc = vec![0; 44];
    dxbc[..4].copy_from_slice(b"DXBC");
    put(&mut dxbc, 20, &1u32.to_le_bytes());
    put(&mut dxbc, 28, &3u32.to_le_bytes());
    for (i, (name, chunk)) in chunks.iter().enumerate() {
        let at = dxbc.len() as u32;
        put(&mut dxbc, 32 + i * 4, &at.to_le_bytes());
        dxbc.extend(*name);
        dxbc.extend((chunk.len() as u32).to_le_bytes());
        dxbc.extend(chunk);
    }
    let size = dxbc.len() as u32;
    put(&mut dxbc, 24, &size.to_le_bytes());
    let header = package.raw(0, 33, 0, vec![0; 40]);
    let data = package.raw(header, 41, 0, dxbc);
    package.set_reference(header, data);
    header
}

fn material(
    cube: bool,
    face: usize,
    mip: usize,
    enabled: bool,
    alpha: f32,
    displaced: bool,
) -> (tempfile::TempDir, u32) {
    let mut package = Package::default();
    let mut code = Vec::new();
    code.extend(instruction(104, &[&[2]]));
    code.extend(instruction(89, &[&[0x0020_8000, 0, 3]]));
    code.extend(instruction(98, &[&register(1, 3, 3)]));
    code.extend(instruction(101, &[&register(2, 0, 15)]));
    code.extend(instruction(
        88 | (if cube { 6 } else { 3 }) << 11,
        &[&[0x0010_7000, 3], &[0x5555]],
    ));
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    // Compare the authored switch, then sample only its enabled branch.
    code.extend(instruction(
        57,
        &[&register(0, 1, 1), &[0x0020_800A, 0, 0], &literal(0.0)],
    ));
    code.extend(instruction(31 | 1 << 18, &[&source(0, 1, 0)]));
    if cube {
        let axis = [
            [1.0f32, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ][face];
        let direction = [
            0x4002,
            axis[0].to_bits(),
            axis[1].to_bits(),
            axis[2].to_bits(),
            0,
        ];
        code.extend(instruction(
            72,
            &[
                &register(0, 0, 15),
                &direction,
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
                &literal(mip as f32),
            ],
        ));
    } else {
        code.extend(instruction(
            69,
            &[
                &register(0, 0, 15),
                &source(1, 3, 0xE4),
                &source(7, 3, 0xE4),
                &[0x0010_6000, 1],
            ],
        ));
    }
    // Native stages retain infinite clamp bounds, including those computed by TFX.
    code.extend(instruction(
        52,
        &[
            &register(0, 0, 15),
            &source(0, 0, 0xE4),
            &[0x0020_8E46, 0, 2],
        ],
    ));
    // zxy is deliberately different from both the stored RGB order and a scalar splat.
    code.extend(instruction(54, &[&register(2, 0, 7), &source(0, 0, 0xD2)]));
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(alpha)]));
    code.extend(instruction(18, &[]));
    code.extend(instruction(54, &[&register(2, 0, 15), &literal(0.0)]));
    code.extend(instruction(21, &[]));
    code.extend(instruction(62, &[]));
    let pixel = shader(&mut package, &code);
    let faces = [
        [32u8, 64, 96, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [255, 0, 255, 255],
    ];
    let pixels = if cube {
        // Native payload order is mip then face. Each mip-one face is visibly darker.
        let mut bytes: Vec<u8> = faces
            .iter()
            .flat_map(|c| std::iter::repeat_n(*c, 4).flatten())
            .collect();
        bytes.extend(
            faces
                .iter()
                .flat_map(|c| [c[0] / 2, c[1] / 2, c[2] / 2, c[3]]),
        );
        bytes
    } else {
        vec![32, 64, 96, 255]
    };
    let (data, large) = if cube && mip == 1 {
        let large = package.raw(0, 32, 0, pixels[..96].to_vec());
        (package.raw(0, 32, 0, pixels[96..].to_vec()), large)
    } else {
        (package.raw(0, 32, 0, pixels), u32::MAX)
    };
    let mut header = vec![0; 40];
    put(&mut header, 4, &28u32.to_le_bytes());
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
    put(&mut header, 36, &large.to_le_bytes());
    let texture = package.raw(data, 32, 1, header);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut sampling = vec![0; 52];
    for offset in [4, 8, 12] {
        put(&mut sampling, offset, &1u32.to_le_bytes());
    }
    let data = package.raw(sampler, 42, 1, sampling);
    package.set_reference(sampler, data);
    let mut material = vec![0; 0x3A0];
    material[0x20] = 0x88;
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    if displaced {
        let mut code = Vec::new();
        code.extend(instruction(89, &[&[0x0020_8000, 0, 1]]));
        for index in 0..3 {
            code.extend(instruction(95, &[&register(1, index, 15)]));
        }
        for index in [3, 4] {
            code.extend(instruction(101, &[&register(2, index, 15)]));
        }
        code.extend(instruction(54, &[&register(2, 4, 15), &source(1, 0, 0xE4)]));
        code.extend(instruction(
            50,
            &[
                &register(2, 4, 1),
                &source(1, 1, 0),
                &[0x0020_800A, 0, 0],
                &source(1, 0, 0),
            ],
        ));
        code.extend(instruction(54, &[&register(2, 3, 15), &source(1, 2, 0xE4)]));
        code.extend(instruction(62, &[]));
        let vertex = shader_stage(
            &mut package,
            &code,
            1,
            &[("POSITION", 0), ("COLOR", 1), ("TEXCOORD", 2)],
            &[("TEXCOORD", 3), ("TEXCOORD", 4)],
        );
        put(&mut material, 0x48, &vertex.to_le_bytes());
        let mut vectors = [0; 16];
        floats(&mut vectors, 0, &[8.0, 0.0, 0.0, 0.0]);
        array(&mut material, 0x98, 0x80800090, &vectors, 16);
    }
    let mut row = [0; 8];
    put(&mut row, 0, &3u32.to_le_bytes());
    put(&mut row, 4, &texture.to_le_bytes());
    array(&mut material, 0x2D0, 0x80807211, &row, 8);
    let mut row = [0; 16];
    put(&mut row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x808073F3, &row, 16);
    // The expression reads a different initialized output and copies it into the switch.
    // Treating output reads as zero would incorrectly suppress every enabled case.
    let mut constant = [0; 48];
    floats(
        &mut constant,
        16,
        &[if enabled { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    );
    floats(&mut constant, 32, &[f32::NEG_INFINITY; 4]);
    array(&mut material, 0x318, 0x80800090, &constant, 16);
    let mut expression = vec![0x42, 1, 0x43, 0];
    if cube {
        let mut constants = [0; 32];
        floats(&mut constants, 0, &[-1.0; 4]);
        array(&mut material, 0x2F8, 0x80800090, &constants, 16);
        expression.extend([0x34, 0, 0x34, 1, 0x04, 0x43, 2]);
    }
    array(&mut material, 0x2E8, 0x80800009, &expression, 1);
    let tag = package.add(0x808071E8, material);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    (directory, tag)
}

pub(super) fn render_cases(cases: &mut Vec<(String, Model, [u8; 3])>) {
    gradients::render_cases(cases);
    let mut variants = Vec::new();
    for cube in [false, true] {
        for enabled in [false, true] {
            for alpha in [0.0, 0.5] {
                for displaced in [false, true] {
                    variants.push((cube, 0, 0, enabled, alpha, displaced));
                }
            }
        }
    }
    for face in 1..6 {
        variants.push((true, face, 0, true, 0.0, false));
    }
    for face in 0..6 {
        variants.push((true, face, 1, true, 0.0, false));
    }
    for (cube, face, mip, enabled, alpha, displaced) in variants {
        let (directory, tag) = material(cube, face, mip, enabled, alpha, displaced);
        let manager = PackageManager::new(
            directory.path(),
            tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
            Some(tiger_pkg::PackagePlatform::Win64),
        )
        .unwrap();
        let mut model = Model::default();
        let material =
            crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model).unwrap();
        model.effects.push(material);
        quad(&mut model, 0.0, None, Some([0.0, 0.0, 0.125]));
        quad(&mut model, -0.1, Some(0), None);
        model.colors = vec![[0.5, 0.0, 0.0, 1.0]; model.vertices.len()];
        let expected = if enabled && !displaced {
            let [r, g, b] = [
                [32u8, 64, 96],
                [255, 0, 0],
                [0, 255, 0],
                [0, 0, 255],
                [255, 255, 0],
                [255, 0, 255],
            ][face]
                .map(|v| if mip == 1 { v / 2 } else { v });
            [
                f32::from(b) / 255.0,
                f32::from(r) / 255.0,
                f32::from(g) / 255.0 + 0.125 * (1.0 - alpha),
            ]
        } else {
            [0.0, 0.0, 0.125]
        };
        cases.push((format!("native-effect-cube-{cube}-face-{face}-mip-{mip}-on-{enabled}-alpha-{alpha}-displaced-{displaced}"),model,encoded(expected)));
    }
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES, SUNDIAL_EFFECT_AUDIT and SUNDIAL_TEST_ARTIFACTS"]
fn audited_gear_effects_load_and_render() {
    let packages = crate::test_support::preview_packages();
    let audit = std::env::var_os("SUNDIAL_EFFECT_AUDIT").unwrap();
    let output = crate::test_support::artifact_dir("effects");
    std::fs::create_dir_all(&output).unwrap();
    let audit: serde_json::Value = serde_json::from_slice(&std::fs::read(audit).unwrap()).unwrap();
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let mut receipt = Vec::new();
    let mut failures = Vec::new();
    let mut entities = std::collections::BTreeSet::new();
    for item in audit["cases"].as_array().unwrap() {
        for entity in item["entities"].as_array().unwrap() {
            let tag = u32::from_str_radix(entity["tag"].as_str().unwrap(), 16).unwrap();
            if !entities.insert(tag) {
                continue;
            }
            match load_with_manager(&manager, tag, &Load::default(), None) {
                Ok(model) => {
                    for effect in &model.effects {
                        if effect.kind == Kind::Unavailable
                            || [0.0, 0.5, 2.0].iter().any(|&t| {
                                effect.frame(t).is_none()
                                    || effect
                                        .native
                                        .as_ref()
                                        .is_some_and(|n| n.vertex_frame(t).is_none())
                            })
                        {
                            failures.push(format!("{tag:08X}: unavailable effect or frame"));
                        }
                    }
                    let mut frames = Vec::new();
                    let mut initial = None;
                    for (frame, seconds) in [0.0, 0.5, 2.0].into_iter().enumerate() {
                        let image = render::animated_image(
                            &model,
                            render::Camera::default(),
                            render::Scene::unprocessed(),
                            [320, 240],
                            seconds,
                        );
                        let pixels: Vec<u8> =
                            image.pixels.iter().flat_map(|p| p.to_array()).collect();
                        let changed = initial.as_ref().map_or(0, |old: &Vec<u8>| {
                            old.chunks_exact(4)
                                .zip(pixels.chunks_exact(4))
                                .filter(|(a, b)| a != b)
                                .count()
                        });
                        let file = format!("native-{tag:08X}-{frame}.png");
                        std::fs::write(output.join(&file), export::png(&pixels, 320, 240).unwrap())
                            .unwrap();
                        if initial.is_none() {
                            initial = Some(pixels);
                        }
                        frames.push(json!({"seconds":seconds,"image":file,"changed_pixels_from_zero":changed}));
                    }
                    receipt.push(json!({"entity":entity["tag"],"item":item["name"],"effects":model.effects.len(),"frames":frames,"notices":model.notices}));
                }
                Err(error) => failures.push(format!("{tag:08X}: {error}")),
            }
        }
    }
    std::fs::write(
        output.join("native-effects.json"),
        serde_json::to_vec_pretty(&json!({"entities":receipt,"failures":failures})).unwrap(),
    )
    .unwrap();
    assert!(
        !receipt.is_empty(),
        "Need native entities to verify effects"
    );
    assert!(
        receipt
            .iter()
            .any(|row| row["effects"].as_u64().is_some_and(|count| count > 0)),
        "Need loaded native effects to verify frames"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
