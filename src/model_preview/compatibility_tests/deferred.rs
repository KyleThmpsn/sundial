//! Package-to-preview surface witnesses prepared before the native surface implementation.
//! Roles and packing are independently executed in the original Shadowkeep pixel programs.
use super::*;
use effects::native::{instruction, literal, register, shader_stage, source};
use fixtures::{Package, array, floats, put};
mod decal;
mod vehicle;
pub(crate) use decal::cases as decal_cases;
pub(crate) use vehicle::{emission_cases, studio_cases};

pub(crate) fn framing_case() -> Model {
    fixture(None, 0, 64, [128, 128], None, 0, true)
}

pub(super) fn artifact(model: &Model, output: &Path, name: &str) -> usize {
    let size = [720, 540];
    let image = render::styled_image(
        model,
        render::Camera::default(),
        render::Scene::default(),
        size,
        0.0,
        render::Style::Textured,
    );
    let visible = image
        .pixels
        .iter()
        .filter(|&&p| p != eframe::egui::Color32::from_rgb(24, 28, 35))
        .count();
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join(format!("{name}-textured.png")),
        export::png(&rgba, size[0], size[1]).unwrap(),
    )
    .unwrap();
    visible
}

fn constant(row: u32, swizzle: u32) -> [u32; 3] {
    [0x0020_8006 | swizzle << 4, 0, row]
}

pub(crate) fn case(transformed: bool, metal: u8, smooth: u8, normal: [u8; 2]) -> Model {
    packaged(transformed.then_some(1), metal, smooth, normal)
}

fn packaged(input: Option<u8>, metal: u8, smooth: u8, normal: [u8; 2]) -> Model {
    fixture(input, metal, smooth, normal, None, 0, false)
}

fn distant_material(package: &mut Package, material: &[u8], available: bool) -> u32 {
    let mut material = material.to_vec();
    material[0x20] = 0x88;
    if available {
        let code = [
            instruction(101, &[&register(2, 0, 15)]),
            instruction(54, &[&register(2, 0, 7), &literal(0.125)]),
            instruction(54, &[&register(2, 0, 8), &literal(0.0)]),
            instruction(62, &[]),
        ]
        .concat();
        let pixel = shader_stage(package, &code, 0, &[], &[("SV_TARGET", 0)]);
        put(&mut material, 0x2C8, &pixel.to_le_bytes());
        // Default geometry varyings are sufficient for this constant transparent program.
        put(&mut material, 0x48, &u32::MAX.to_le_bytes());
    }
    package.add(0x8080_71E8, material)
}

fn fixture(
    input: Option<u8>,
    metal: u8,
    smooth: u8,
    normal: [u8; 2],
    emission: Option<(f32, bool)>,
    prefix: usize,
    far_effect: bool,
) -> Model {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    let mut code = instruction(104, &[&[12]]);
    code.extend(instruction(89, &[&[0x0020_8000, 0, 3]]));
    for input in 0..4 {
        code.extend(instruction(98, &[&register(1, input, 15)]));
    }
    for output in 0..3 {
        code.extend(instruction(101, &[&register(2, output, 15)]));
    }
    code.extend(instruction(90, &[&[0x0010_6000, 1]]));
    for slot in 0..3 {
        code.extend(instruction(
            88 | 3 << 11,
            &[&[0x0010_7000, slot], &[0x5555]],
        ));
    }
    code.extend(instruction(
        50,
        &[
            &register(0, 10, 3),
            &source(1, 3, 0x44),
            &constant(1, 0x44),
            &constant(1, 0xEE),
        ],
    ));
    for (temp, slot) in [(4, 0), (5, 1), (6, 2)] {
        code.extend(instruction(
            69,
            &[
                &register(0, temp, 15),
                &source(0, 10, 0x44),
                &source(7, slot, 0xE4),
                &[0x0010_6000, 1],
            ],
        ));
    }
    code.extend(instruction(54, &[&register(2, 0, 7), &source(0, 4, 0xE4)]));
    code.extend(instruction(
        50,
        &[
            &register(0, 6, 3),
            &source(0, 6, 0x44),
            &constant(2, 0),
            &constant(2, 0x55),
        ],
    ));
    code.extend(instruction(
        15,
        &[&register(0, 7, 1), &source(0, 6, 0x44), &source(0, 6, 0x44)],
    ));
    let minus = [0x8010_0006, 0x41, 7];
    code.extend(instruction(0, &[&register(0, 7, 1), &minus, &literal(1.0)]));
    code.extend(instruction(
        52,
        &[&register(0, 7, 1), &source(0, 7, 0), &literal(0.0)],
    ));
    code.extend(instruction(75, &[&register(0, 7, 1), &source(0, 7, 0)]));
    code.extend(instruction(
        56,
        &[&register(0, 8, 7), &source(1, 1, 0xE4), &source(0, 6, 0)],
    ));
    code.extend(instruction(
        50,
        &[
            &register(0, 8, 7),
            &source(1, 2, 0xE4),
            &source(0, 6, 0x55),
            &source(0, 8, 0xE4),
        ],
    ));
    code.extend(instruction(
        50,
        &[
            &register(0, 8, 7),
            &source(1, 0, 0xE4),
            &source(0, 7, 0),
            &source(0, 8, 0xE4),
        ],
    ));
    code.extend(instruction(
        16,
        &[&register(0, 7, 1), &source(0, 8, 0xE4), &source(0, 8, 0xE4)],
    ));
    code.extend(instruction(68, &[&register(0, 7, 1), &source(0, 7, 0)]));
    code.extend(instruction(
        56,
        &[&register(0, 8, 7), &source(0, 8, 0xE4), &source(0, 7, 0)],
    ));
    code.extend(instruction(
        1 << 13,
        &[&register(0, 6, 4), &source(0, 6, 0xAA), &constant(2, 0xAA)],
    ));
    code.extend(instruction(
        51,
        &[&register(0, 7, 1), &source(0, 5, 0x55), &source(0, 6, 0xAA)],
    ));
    code.extend(instruction(
        50,
        &[
            &register(0, 7, 1),
            &source(0, 7, 0),
            &literal(0.125),
            &literal(0.375),
        ],
    ));
    code.extend(instruction(
        50 | 1 << 13,
        &[
            &register(2, 1, 7),
            &source(0, 8, 0xE4),
            &source(0, 7, 0),
            &literal(0.5),
        ],
    ));
    code.extend(instruction(54, &[&register(2, 1, 8), &literal(0.0)]));
    code.extend(instruction(54, &[&register(2, 0, 8), &literal(0.0)]));
    code.extend(instruction(54, &[&register(2, 2, 1), &source(0, 5, 0)]));
    if let Some((_, valid)) = emission {
        vehicle::writer(&mut code, valid);
    } else {
        code.extend(instruction(
            56,
            &[&register(2, 2, 2), &source(0, 5, 0xAA), &literal(0.5)],
        ));
    }
    code.extend(instruction(54, &[&register(2, 2, 4), &literal(0.0)]));
    code.extend(instruction(54, &[&register(2, 2, 8), &source(1, 0, 0xFF)]));
    code.extend(instruction(62, &[]));
    let pixel = shader_stage(
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
    let mut vertex = instruction(89, &[&[0x0020_8000, 11, 7]]);
    vertex.extend(instruction(95, &[&register(1, 0, 3)]));
    vertex.extend(instruction(101, &[&register(2, 3, 3)]));
    vertex.extend(instruction(
        50,
        &[
            &register(2, 3, 3),
            &source(1, 0, 0x44),
            &[0x0020_8006 | 0x44 << 4, 11, 6],
            &[0x0020_8006 | 0xEE << 4, 11, 6],
        ],
    ));
    vertex.extend(instruction(62, &[]));
    let vertex = shader_stage(
        &mut package,
        &vertex,
        1,
        &[("TEXCOORD", 0)],
        &[("TEXCOORD", 3)],
    );
    let mut material = vec![0; 0x360];
    put(&mut material, 0x48, &vertex.to_le_bytes());
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    let values = [
        [emission.map_or(0.0, |e| e.0), 0.0, 0.0, 0.0],
        [1.0, 1.0, 0.0, 0.0],
        [2.0, -1.0, -0.1, 0.0],
    ];
    let constants: Vec<_> = values
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
    array(&mut material, 0x318, 0x8080_0090, &constants, 16);
    if let Some(input) = input {
        array(
            &mut material,
            0x2E8,
            0x8080_0009,
            &[0x4D, input, 0x43, 1],
            1,
        );
    }
    let mut bindings = Vec::new();
    for (slot, rgba) in [
        (0u32, vec![64, 128, 192, 255, 192, 64, 32, 255]),
        (1, vec![metal, smooth, 255, 255]),
        (2, vec![normal[0], normal[1], 230, 255]),
    ] {
        let width = rgba.len() / 4;
        let data = package.raw(0, 0, 0, rgba);
        let mut header = vec![0; 40];
        put(&mut header, 0, &((width * 4) as u32).to_le_bytes());
        put(
            &mut header,
            4,
            &(if slot == 0 { 29u32 } else { 28 }).to_le_bytes(),
        );
        put(&mut header, 14, &(width as u16).to_le_bytes());
        for at in [16, 18, 20] {
            put(&mut header, at, &1u16.to_le_bytes());
        }
        put(&mut header, 36, &u32::MAX.to_le_bytes());
        let texture = package.raw(data, 32, 1, header);
        bindings.extend(slot.to_le_bytes());
        bindings.extend(texture.to_le_bytes());
    }
    array(&mut material, 0x2D0, 0x8080_7211, &bindings, 8);
    let sampler = package.raw(0, 34, 1, vec![0; 16]);
    let mut descriptor = vec![0; 52];
    for at in [4, 8, 12] {
        put(&mut descriptor, at, &1u32.to_le_bytes());
    }
    put(&mut descriptor, 0, &0x15u32.to_le_bytes());
    put(&mut descriptor, 20, &1u32.to_le_bytes());
    put(&mut descriptor, 48, &f32::MAX.to_le_bytes());
    let data = package.raw(sampler, 42, 1, descriptor);
    package.set_reference(sampler, data);
    let mut row = [0; 16];
    put(&mut row, 0, &sampler.to_le_bytes());
    array(&mut material, 0x308, 0x8080_73F3, &row, 16);
    let unsupported = distant_material(&mut package, &material, far_effect);
    let material = package.add(0x8080_71E8, material);
    let vertices: Vec<_> = [
        [-1.0f32, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [20.0, 0.0, -10.0],
        [30.0, 0.0, -10.0],
        [25.0, 0.0, 10.0],
    ]
    .into_iter()
    .flat_map(|position| {
        [
            position[0],
            position[1],
            position[2],
            (0.75 - 0.125) / 1.5,
            (0.5 - 0.0625) / 0.75,
            0.0,
            -1.0,
            0.0,
        ]
    })
    .flat_map(f32::to_le_bytes)
    .collect();
    let vertices = package.vertex(32, vertices);
    let indices: Vec<_> = [0u16, 1, 2, 3, 4, 5]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    let data = package.raw(0, 0, 0, indices);
    let mut header = vec![0; 16];
    put(&mut header, 8, &12u64.to_le_bytes());
    let indices = package.raw(data, 32, 6, header);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[1.5, 0.75, 0.125, 0.0625]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 136], 136);
    put(&mut model, mesh, &vertices.to_le_bytes());
    put(&mut model, mesh + 16, &indices.to_le_bytes());
    for stage in 1..24 {
        put(
            &mut model,
            mesh + 40 + stage * 2,
            &((if stage <= 7 { 1 } else { 2 }) + prefix as i16).to_le_bytes(),
        );
    }
    put(&mut model, mesh + 88, &13u16.to_le_bytes());
    put(&mut model, mesh + 88 + 7 * 2, &13u16.to_le_bytes());
    let mut part = [0; 32];
    put(&mut part, 0, &material.to_le_bytes());
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    let mut parts = Vec::new();
    for _ in 0..prefix {
        let material = vehicle::prefix_material(&mut package);
        let mut before = part;
        put(&mut before, 0, &material.to_le_bytes());
        parts.extend(before);
    }
    parts.extend(part);
    put(&mut part, 0, &unsupported.to_le_bytes());
    put(&mut part, 8, &3u32.to_le_bytes());
    part[26] = 255;
    parts.extend(part);
    array(&mut model, mesh + 24, 0x8080_737E, &parts, 32);
    let tag = package.add(MODEL, model);
    // Slot zero is structurally valid but has no initial channel. Only slot one
    // supplies the UV equation, so unrelated missing state must not block this draw.
    let mut bank = vec![0; 0x400];
    put(&mut bank, 0x10, &0x30i64.to_le_bytes());
    put(&mut bank, 0x18, &0x1E8i64.to_le_bytes());
    put(&mut bank, 0x3C, &0x8080_979Fu32.to_le_bytes());
    put(&mut bank, 0x1FC, &0x8080_9790u32.to_le_bytes());
    let values: Vec<_> = [1.0f32 / 3.0, 1.0, 0.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    array(&mut bank, 0x90, 0x8080_0090, &values, 16);
    let mut declaration = [0; 112];
    put(&mut declaration, 0, &22u32.to_le_bytes());
    array(&mut bank, 0x2D8, 0x8080_97A1, &declaration, 112);
    let bank = package.add(RESOURCE, bank);
    let mut owner = vec![0; 0x600];
    put(&mut owner, 0x10, &0x30i64.to_le_bytes());
    put(&mut owner, 0x18, &0x1E8i64.to_le_bytes());
    put(&mut owner, 0x3C, &0x8080_72B8u32.to_le_bytes());
    put(&mut owner, 0x1FC, &0x8080_72BDu32.to_le_bytes());
    put(&mut owner, 0x3DC, &tag.to_le_bytes());
    let rows = array(&mut owner, 0x160, 0x8080_9788, &[0; 192], 96);
    for (index, name) in [11u32, 22].into_iter().enumerate() {
        let row = rows + index * 96;
        let link = owner.len();
        owner.resize(link + 40, 0);
        put(&mut owner, row + 4, &0x8080_9789u32.to_le_bytes());
        put(&mut owner, row + 8, &(link as u64).to_le_bytes());
        put(&mut owner, link + 4, &0x8080_9788u32.to_le_bytes());
        put(&mut owner, link + 8, &(row as u64).to_le_bytes());
        put(&mut owner, link + 24, &0x8080_97C1u64.to_le_bytes());
        put(&mut owner, link + 32, &name.to_le_bytes());
    }
    let owner = package.add(RESOURCE, owner);
    let mut entity = vec![0; 0x28];
    let mut components = [0; 24];
    put(&mut components, 0, &owner.to_le_bytes());
    put(&mut components, 12, &bank.to_le_bytes());
    array(&mut entity, 0x10, 0x8080_9C04, &components, 12);
    let tag = package.add(ENTITY, entity);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    fixtures::load(directory.path(), tag).unwrap()
}

#[test]
fn native_surface_roles_and_texture_transforms_survive_package_loading() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("deferred");
    std::fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for transformed in [false, true] {
        for (metal, smooth, normal) in [
            (0, 64, [128, 128]),
            (180, 240, [192, 96]),
            (0, 128, [255, 255]),
        ] {
            let model = case(transformed, metal, smooth, normal);
            validate(&model);
            assert_eq!(model.triangles.len(), 2);
            assert_eq!(
                render::drawn_bounds(&model, render::Style::Textured).1,
                [1.0, 0.0, 1.0]
            );
            assert_eq!(
                render::drawn_bounds(&model, render::Style::Solid).1,
                [30.0, 0.0, 10.0]
            );
            let effect = model.triangle_effects[0].unwrap();
            let material = &model.effects[effect];
            assert!(material.native.as_ref().unwrap().deferred);
            let input = crate::model_preview::effects::native::Input {
                position: model.vertices[0],
                normal: [0.0, -1.0, 0.0],
                tangent: [1.0, 0.0, 0.0, 1.0],
                uv: model.uvs[0],
                detail_uv: [0.0; 2],
                color: [1.0; 4],
            };
            let native = material.native.as_ref().unwrap();
            let varyings = native
                .vertex(&model, &input, &native.vertex_frame(0.0).unwrap())
                .unwrap();
            let pixel = crate::model_preview::effects::native::Pixel {
                varyings,
                dx: [[0.0; 4]; 9],
                dy: [[0.0; 4]; 9],
                screen: [0.0; 3],
                direction: [0.0, -1.0, 0.0],
                distance: 1.0,
                front: true,
                exposure: 1.0,
                depth: crate::model_preview::effects::native::Depth {
                    values: &[],
                    size: [1, 1],
                    top: 0,
                    scale: 1.0,
                },
            };
            let sampled = crate::model_preview::effects::native::sample_surface(
                &model,
                material,
                &material.frame(0.0).unwrap(),
                &shader::Bindings::new(&model, 0, &model.dyes),
                pixel,
            )
            .unwrap();
            let rgb = if transformed {
                [64u8, 128, 192]
            } else {
                [192, 64, 32]
            };
            let expected = rgb.map(|v| shader::linear(f32::from(v) / 255.0));
            assert!(
                sampled
                    .surface
                    .albedo
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 1e-6),
                "Albedo {:?}, expected {expected:?}, UV {:?}, varyings {varyings:?}",
                sampled.surface.albedo,
                input.uv
            );
            assert!((sampled.surface.metal - f32::from(metal) / 255.0).abs() < 1e-6);
            let rough = 1.0 - (f32::from(smooth) / 255.0).min(230.0 / 255.0 - 0.1);
            assert!((sampled.surface.roughness - rough).abs() < 1e-5);
            let xy = normal.map(|v| f32::from(v) * 2.0 / 255.0 - 1.0);
            let z = (1.0 - xy[0] * xy[0] - xy[1] * xy[1]).max(0.0).sqrt();
            let length = (xy[0] * xy[0] + xy[1] * xy[1] + z * z).sqrt();
            let expected_normal = [xy[0] / length, -z / length, xy[1] / length];
            assert!(
                sampled
                    .normal
                    .into_iter()
                    .zip(expected_normal)
                    .all(|(a, b)| (a - b).abs() < 1e-5)
            );
            let name = format!("surface-{transformed}-{metal}-{smooth}");
            let image = render::styled_image(
                &model,
                render::Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    ..Default::default()
                },
                render::Scene::default(),
                [240, 180],
                0.0,
                render::Style::Textured,
            );
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            std::fs::write(
                output.join(format!("{name}.png")),
                export::png(&rgba, 240, 180).unwrap(),
            )
            .unwrap();
            let dark = render::styled_image(
                &model,
                render::Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    ..Default::default()
                },
                render::Scene {
                    key: 0.0,
                    fill: 0.0,
                    filmic: false,
                    bloom: false,
                    ..render::Scene::unit_exposure()
                },
                [240, 180],
                0.0,
                render::Style::Textured,
            );
            assert_eq!(
                dark.pixels[90 * 240 + 120],
                eframe::egui::Color32::BLACK,
                "Disabled studio lights must not leave white edge or environment reflections"
            );
            receipt.push(json!({"case":name,"albedo":sampled.surface.albedo,"normal":sampled.normal,"roughness":sampled.surface.roughness,"metal":sampled.surface.metal,"notices":model.notices}));
        }
    }
    let missing = packaged(Some(0), 0, 64, [128, 128]);
    assert!(missing.triangle_effects[0].is_none());
    assert!(missing.notices.iter().any(|n| n.contains("0000000B")));
    artifact(&missing, &output, "missing-read");
    receipt.push(json!({"case":"missing-read","notices":missing.notices}));
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn distant_transparent_volume_keeps_the_vehicle_surface_in_frame() {
    let model = framing_case();
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("vehicle-framing");
    std::fs::create_dir_all(&output).unwrap();
    let image = render::styled_image(
        &model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        },
        render::Scene::default(),
        [240, 180],
        0.0,
        render::Style::Textured,
    );
    let color_pixels = image
        .pixels
        .iter()
        .filter(|p| p.r() > p.b().saturating_mul(2) && p.r() > 60)
        .count();
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("surface.png"),
        export::png(&rgba, 240, 180).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(
            &json!({"colored_surface_pixels":color_pixels,"notices":model.notices}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        color_pixels > 4000,
        "Only {color_pixels} surface pixels, {:?}",
        model.notices
    );
    let wide = render::styled_image(
        &model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            zoom: 0.06,
            ..Default::default()
        },
        render::Scene::default(),
        [240, 180],
        0.0,
        render::Style::Textured,
    );
    let effect_pixels = wide
        .pixels
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            i % 240 > 180 && p.r() > 80 && p.r().abs_diff(p.g()) < 8 && p.g().abs_diff(p.b()) < 8
        })
        .count();
    let rgba: Vec<_> = wide.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("wide-effect.png"),
        export::png(&rgba, 240, 180).unwrap(),
    )
    .unwrap();
    assert!(
        effect_pixels > 300,
        "The distant effect must still render when zoomed out, found {effect_pixels} pixels"
    );
}
