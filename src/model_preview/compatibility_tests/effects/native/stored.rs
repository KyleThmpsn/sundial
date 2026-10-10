//! Package-to-preview numeric attribute witnesses, prepared before recovery code.
use super::*;

fn table(package: &mut Package, format: u32, width: u16, data: Vec<u8>) -> u32 {
    let payload = package.raw(0, 0, 0, data);
    let mut header = vec![0; 0x28];
    put(&mut header, 4, &format.to_le_bytes());
    for (at, value) in [(14, width), (16, 1), (18, 1), (20, 1)] {
        put(&mut header, at, &value.to_le_bytes());
    }
    package.raw(payload, 32, 1, header)
}

fn fixture(invalid: bool) -> (tempfile::TempDir, u32) {
    fixture_at(invalid, 0)
}

fn fixture_at(invalid: bool, stage: usize) -> (tempfile::TempDir, u32) {
    fixture_parts(invalid, stage, 1, 3, 0)
}

fn fixture_parts(
    invalid: bool,
    stage: usize,
    copies: usize,
    count: usize,
    first: usize,
) -> (tempfile::TempDir, u32) {
    fixture_inputs(invalid, stage, copies, count, first, 0)
}

fn address(code: &mut Vec<u32>, variant: u8) {
    code.extend(instruction(54, &[&register(0, 0, 1), &source(1, 4, 0)]));
    if variant != 2 {
        return;
    }
    code.extend(instruction(
        35,
        &[
            &register(0, 0, 1),
            &source(0, 0, 0),
            &[0x4001, 29],
            &[0x4001, 17],
        ],
    ));
    code.extend(instruction(
        78,
        &[
            &register(0, 0, 1),
            &register(0, 1, 1),
            &source(0, 0, 0),
            &[0x4001, 29],
        ],
    ));
    // Both outputs contribute to the address, with the source aliased by the quotient.
    code.extend(instruction(
        30,
        &[&register(0, 0, 1), &source(0, 0, 0), &source(0, 1, 0)],
    ));
    code.extend(instruction(
        30,
        &[
            &register(0, 0, 1),
            &source(0, 0, 0),
            &[0x4001, (-17i32) as u32],
        ],
    ));
}

fn fixture_inputs(
    invalid: bool,
    stage: usize,
    copies: usize,
    count: usize,
    first: usize,
    variant: u8,
) -> (tempfile::TempDir, u32) {
    let mut package = Package::default();
    fixtures::layouts(&mut package);
    let auxiliary = table(
        &mut package,
        30,
        3,
        vec![0, 60, 0, 64, 0, 64, 0, 60, 0, 56, 0, 68],
    );
    let mut metadata_rows = vec![0u32; count];
    metadata_rows[first..first + 3].copy_from_slice(&[0, 1, 2]);
    let metadata = table(
        &mut package,
        42,
        count as u16,
        metadata_rows
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    );
    let mut color_rows = vec![0; count * 4];
    color_rows[first * 4..(first + 3) * 4]
        .copy_from_slice(&[255, 0, 0, 255, 0, 128, 255, 255, 64, 255, 0, 255]);
    let colors = table(
        &mut package,
        28,
        count as u16,
        if invalid { vec![255; 4] } else { color_rows },
    );
    let mut code = Vec::new();
    code.extend(instruction(104, &[&[7]]));
    code.extend(instruction(89, &[&[0x0020_8000, 0, 1]]));
    code.extend(instruction(89, &[&[0x0020_8000, 11, 24]]));
    code.extend(instruction(89, &[&[0x0020_8000, 12, 14]]));
    for (slot, kind) in [(0, 0x5555), (1, 0x4444), (3, 0x4444)] {
        code.extend(instruction(88 | 3 << 11, &[&[0x0010_7000, slot], &[kind]]));
    }
    // A time-driven position before the omitted matrix palette, with distinct row
    // consumers and separate normal/tangent captures like the imported native envelope.
    code.extend(instruction(
        0,
        &[
            &register(0, 2, 7),
            &source(1, 0, 0xE4),
            &[0x0020_8006, 0, 0],
        ],
    ));
    code.extend(instruction(54, &[&register(0, 2, 8), &literal(1.0)]));
    code.extend(instruction(67, &[&register(0, 0, 8), &source(1, 0, 0xFF)]));
    code.extend(instruction(31 | 1 << 18, &[&source(0, 0, 0xFF)]));
    code.extend(instruction(54, &[&register(0, 4, 15), &literal(0.0)]));
    code.extend(instruction(21, &[]));
    code.extend(instruction(
        0,
        &[
            &register(0, 0, 7),
            &source(1, 0, 0xE4),
            &[0x0020_8006, 12, 7],
        ],
    ));
    for (axis, row) in [(1, 4), (2, 5), (4, 6)] {
        code.extend(instruction(
            17,
            &[
                &register(0, 3, axis),
                &source(0, row, 0xE4),
                &source(0, 2, 0xE4),
            ],
        ));
    }
    code.extend(instruction(54, &[&register(0, 2, 7), &source(1, 2, 0xE4)]));
    code.extend(instruction(
        16,
        &[&register(0, 3, 1), &source(0, 4, 0xE4), &source(0, 2, 0xE4)],
    ));
    code.extend(instruction(54, &[&register(0, 2, 7), &source(1, 3, 0xE4)]));
    code.extend(instruction(
        16,
        &[&register(0, 3, 1), &source(0, 4, 0xE4), &source(0, 2, 0xE4)],
    ));
    // The low metadata word chooses the auxiliary row. Every vertex has a different scale.
    code.extend(instruction(
        54,
        &[&register(0, 0, 15), &[0x4002, 0, 0, 0, 0]],
    ));
    address(&mut code, variant);
    code.extend(instruction(
        45,
        &[&register(0, 1, 1), &source(0, 0, 0xE4), &source(7, 3, 0xE4)],
    ));
    code.extend(instruction(54, &[&register(0, 0, 1), &source(0, 1, 0)]));
    code.extend(instruction(
        45,
        &[
            &register(0, 1, 15),
            &source(0, 0, 0xE4),
            &source(7, 1, 0xE4),
        ],
    ));
    code.extend(instruction(
        41,
        &[&register(0, 1, 10), &source(0, 1, 0xE4), &[0x4001, 8]],
    ));
    code.extend(instruction(
        60,
        &[&register(0, 2, 3), &source(0, 1, 0x08), &source(0, 1, 0xDD)],
    ));
    code.extend(instruction(131, &[&register(0, 2, 3), &source(0, 2, 0xE4)]));
    // Swizzles deliberately route raw UV x/y into output z/w.
    code.extend(instruction(
        56,
        &[
            &register(2, 3, 12),
            &source(1, 1, 0x44),
            &source(0, 2, 0x44),
        ],
    ));
    code.extend(instruction(
        50,
        &[
            &register(2, 3, 3),
            &source(1, 1, 0xE4),
            &literal(2.0),
            &literal(0.25),
        ],
    ));
    code.extend(instruction(31 | 1 << 18, &[&source(1, 4, 0)]));
    address(&mut code, variant);
    code.extend(instruction(
        45,
        &[
            &register(2, 8, 15),
            &source(0, 0, 0xE4),
            &source(7, 0, 0xE4),
        ],
    ));
    code.extend(instruction(18, &[]));
    code.extend(instruction(54, &[&register(2, 8, 15), &literal(1.0)]));
    code.extend(instruction(21, &[]));
    if variant == 1 {
        // Normal W was not retained by the geometry decoder. A numeric output must not
        // silently use zero or reinterpret a NaN placeholder as an integer lookup.
        code.extend(instruction(54, &[&register(2, 8, 15), &source(1, 2, 0xFF)]));
    }
    code.extend(instruction(62, &[]));
    let shader = shader_stage(
        &mut package,
        &code,
        1,
        &[
            ("POSITION", 0),
            ("TEXCOORD", 1),
            ("NORMAL", 2),
            ("TANGENT", 3),
            ("SV_VERTEXID", 4),
        ],
        &[("TEXCOORD", 3), ("TEXCOORD", 8)],
    );
    let mut material = vec![0; 0x400];
    material[0x20] = 0x88;
    put(&mut material, 0x48, &shader.to_le_bytes());
    let pixel_code = [
        instruction(54, &[&register(2, 0, 15), &source(1, 8, 0xE4)]),
        instruction(62, &[]),
    ]
    .concat();
    let pixel = shader_stage(
        &mut package,
        &pixel_code,
        0,
        &[("TEXCOORD", 8)],
        &[("SV_TARGET", 0)],
    );
    put(&mut material, 0x2C8, &pixel.to_le_bytes());
    array(&mut material, 0x68, 0x8080_0009, &[0x3C, 1, 0, 0x43, 0], 1);
    array(&mut material, 0x98, 0x8080_0090, &[0; 16], 16);
    let bindings: Vec<_> = [(0u32, colors), (1, auxiliary), (3, metadata)]
        .into_iter()
        .flat_map(|(slot, tag)| [slot, tag])
        .flat_map(u32::to_le_bytes)
        .collect();
    array(&mut material, 0x50, 0x8080_7211, &bindings, 8);
    let materials: Vec<_> = (0..copies)
        .map(|_| package.add(0x8080_71E8, material.clone()))
        .collect();
    let data: Vec<_> = [
        [-1.0f32, 0.0, -1.0, 0.0, 0.5, 0.0, -1.0, 0.0],
        [1.0, 0.0, -1.0, 0.25, 0.0, 0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0, 0.5, 0.75, 0.0, -1.0, 0.0],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect();
    let mut sparse = vec![0; count * 32];
    sparse[first * 32..(first + 3) * 32].copy_from_slice(&data);
    let vertices = package.vertex(32, sparse);
    let indices = package.indices(false);
    if first != 0 {
        let payload = package.reference(indices);
        *package.payload_mut(payload) = [first as u16 + 2, first as u16, first as u16 + 1]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
    }
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x50, &[1.0; 3]);
    floats(&mut model, 0x6C, &[1.0]);
    floats(&mut model, 0x70, &[0.5, 0.25, 0.125, 0.375]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    put(&mut model, mesh, &vertices.to_le_bytes());
    put(&mut model, mesh + 0x10, &indices.to_le_bytes());
    for s in 0..24 {
        put(
            &mut model,
            mesh + 0x28 + s * 2,
            &(if s <= stage { 0i16 } else { copies as i16 }).to_le_bytes(),
        );
    }
    put(&mut model, mesh + 0x58 + stage * 2, &13u16.to_le_bytes());
    let mut part = [0; 0x20];
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    part[0x1A] = 255;
    let parts: Vec<_> = materials
        .into_iter()
        .flat_map(|material| {
            let mut row = part;
            put(&mut row, 0, &material.to_le_bytes());
            row
        })
        .collect();
    array(&mut model, mesh + 0x18, 0x8080_737E, &parts, 0x20);
    let tag = package.add(MODEL, model);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    (directory, tag)
}

#[test]
fn many_materials_keep_sparse_geometry_and_original_vertex_identity() {
    let (directory, tag) = fixture_parts(false, 0, 64, 8192, 4);
    let model = fixtures::load(directory.path(), tag)
        .expect("A small drawn model must not duplicate every unused buffer vertex per material");
    assert_eq!(model.triangles.len(), 64);
    assert!(model.vertices.len() <= 64 * 3);
    for corners in &model.triangles {
        let expected = [
            ([0.0, 0.0, 1.0], [0.25, 3.0], [64.0 / 255.0, 1.0, 0.0, 1.0]),
            ([-1.0, 0.0, -1.0], [0.0, 1.0], [1.0, 0.0, 0.0, 1.0]),
            ([1.0, 0.0, -1.0], [0.5, 0.0], [0.0, 128.0 / 255.0, 1.0, 1.0]),
        ];
        for (&vertex, (position, detail, color)) in corners.iter().zip(expected) {
            assert_eq!(model.vertices[vertex as usize], position);
            assert_eq!(model.detail_uvs[vertex as usize], detail);
            assert_eq!(model.colors[vertex as usize], color);
        }
    }
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let visible = artifact(&model, output, "sparse-material-geometry");
    assert!(visible > 100);
    std::fs::write(output.join("sparse-material-receipt.json"), serde_json::to_vec_pretty(&json!({
        "source_vertices":8192,"materials":64,"drawn_vertices":model.vertices.len(),"triangles":model.triangles.len(),"visible_pixels":visible,"notices":model.notices
    })).unwrap()).unwrap();
}

#[test]
fn divided_vertex_addresses_reach_their_numeric_detail_and_color_tables() {
    let (directory, tag) = fixture_inputs(false, 0, 1, 3, 0, 2);
    let model = fixtures::load(directory.path(), tag).unwrap();
    assert_eq!(model.detail_uvs, [[0.0, 1.0], [0.5, 0.0], [0.25, 3.0]]);
    assert_eq!(
        model.colors,
        [
            [1.0; 4],
            [0.0, 128.0 / 255.0, 1.0, 1.0],
            [64.0 / 255.0, 1.0, 0.0, 1.0]
        ]
    );
    assert!(
        !model
            .notices
            .iter()
            .any(|notice| notice.contains("stored attribute")),
        "{:?}",
        model.notices
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let visible = artifact(&model, output, "divided-vertex-addresses");
    assert!(visible > 100);
    std::fs::write(
        output.join("divided-vertex-addresses.glb"),
        export::glb(&model, 0.0).unwrap(),
    )
    .unwrap();
    std::fs::write(output.join("divided-vertex-addresses-receipt.json"), serde_json::to_vec_pretty(&json!({"detail_uvs":model.detail_uvs,"colors":model.colors,"visible_pixels":visible,"notices":model.notices})).unwrap()).unwrap();
}

#[test]
fn body_and_decal_stages_keep_their_native_material_motion() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for stage in [0usize, 1, 2, 6, 7] {
        let (directory, tag) = fixture_at(false, stage);
        let model = fixtures::load(directory.path(), tag).unwrap();
        let initial = model.pose(0.0).expect("Missing native motion").positions;
        for seconds in [0.5f32, 1.0, 0.0] {
            let pose = model.pose(seconds).unwrap();
            for (index, (&point, &stored)) in pose.positions.iter().zip(&initial).enumerate() {
                for axis in 0..3 {
                    assert!(
                        (point[axis] - stored[axis] - seconds).abs() < 0.0001,
                        "Stage {stage}, vertex {index}, axis {axis}: {point:?} != {stored:?} + {seconds}"
                    );
                }
            }
            let name = format!("moving-stage-{stage}-{}", (seconds * 100.0) as u32);
            // Standard GLB stores the opaque and decal passes. A standalone transparent
            // stage retains its posed image without pretending to export its effect.
            if stage != 7 {
                std::fs::write(
                    output.join(format!("{name}.glb")),
                    export::glb(&model, seconds).unwrap(),
                )
                .unwrap();
            }
            let image = render::styled_image(
                &model,
                render::Camera::default(),
                render::Scene::unprocessed(),
                [320, 240],
                seconds,
                render::Style::Solid,
            );
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            std::fs::write(
                output.join(format!("{name}.png")),
                export::png(&rgba, 320, 240).unwrap(),
            )
            .unwrap();
            receipt.push(json!({"stage":stage,"seconds":seconds,"positions":pose.positions,"notices":model.notices,"glb":stage != 7}));
        }
    }
    std::fs::write(
        output.join("moving-stages-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn indexed_numeric_attributes_survive_package_loading_and_composition() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let (directory, tag) = fixture(false);
    let first = fixtures::load(directory.path(), tag).unwrap();
    assert_eq!(first.detail_uvs, [[0.0, 1.0], [0.5, 0.0], [0.25, 3.0]]);
    let expected = [
        [1.0; 4],
        [0.0, 128.0 / 255.0, 1.0, 1.0],
        [64.0 / 255.0, 1.0, 0.0, 1.0],
    ];
    assert_eq!(first.colors, expected);
    let second = fixtures::load(directory.path(), tag).unwrap();
    let mut composed = Model::default();
    appearance::append(&mut composed, first, "First").unwrap();
    appearance::append(&mut composed, second, "Second").unwrap();
    assert_eq!(composed.colors[3..], expected);
    let visible = artifact(&composed, output, "stored-attributes");
    assert!(visible > 100);
    std::fs::write(output.join("stored-attributes-receipt.json"), serde_json::to_vec_pretty(
        &json!({"detail_uvs":composed.detail_uvs,"colors":composed.colors,"visible_pixels":visible})).unwrap()).unwrap();
    let (directory, tag) = fixture(true);
    let unavailable = fixtures::load(directory.path(), tag).unwrap();
    assert!(
        unavailable
            .notices
            .iter()
            .any(|n| n.contains("attribute") && n.contains("truncated")),
        "{:?}",
        unavailable.notices
    );
    assert!(
        unavailable.colors.iter().all(|c| *c == [1.0; 4]),
        "Failed reads must not apply a partial color table"
    );
    let (directory, tag) = fixture_inputs(false, 0, 1, 3, 0, 1);
    let missing_input = fixtures::load(directory.path(), tag).unwrap();
    assert!(
        missing_input
            .notices
            .iter()
            .any(|n| n.contains("attribute input lane is unavailable")),
        "{:?}",
        missing_input.notices
    );
    assert!(missing_input.colors.iter().all(|c| *c == [1.0; 4]));
    std::fs::write(output.join("unavailable-attributes-receipt.json"), serde_json::to_vec_pretty(&json!({"truncated_table":unavailable.notices,"missing_input":missing_input.notices,"colors":missing_input.colors})).unwrap()).unwrap();
}

#[test]
fn high_resolution_native_texture_keeps_single_pixel_landmarks() {
    let directory = tempfile::tempdir().unwrap();
    let mut package = Package::default();
    let mut data = vec![0; 4096 * 4];
    for x in [0usize, 2047, 2048, 3001, 4095] {
        data[x * 4..x * 4 + 4].copy_from_slice(&[17, 193, 251, 255]);
    }
    // A distinct lower mip makes accidental top-mip skipping observable.
    data.extend(std::iter::repeat_n([240, 0, 0, 255], 2048).flatten());
    let tag = table(&mut package, 28, 4096, data);
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let texture = texture::load(&manager, tag).unwrap();
    assert_eq!(texture.size, [4096, 1]);
    for x in [0usize, 2047, 2048, 3001, 4095] {
        assert_eq!(&texture.rgba[x * 4..x * 4 + 4], &[17, 193, 251, 255]);
    }
    assert_eq!(&texture.rgba[3000 * 4..3000 * 4 + 4], &[0; 4]);
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    std::fs::write(
        output.join("high-resolution-landmarks.png"),
        export::png(&texture.rgba, 4096, 1).unwrap(),
    )
    .unwrap();
    std::fs::write(output.join("high-resolution-receipt.json"), serde_json::to_vec_pretty(
        &json!({"size":texture.size,"landmarks":[0,2047,2048,3001,4095],"rgba":[17,193,251,255]})).unwrap()).unwrap();
}

pub(in crate::model_preview) fn motion_case(stage: usize) -> Model {
    let (directory, tag) = fixture_at(false, stage);
    fixtures::load(directory.path(), tag).unwrap()
}

pub(crate) fn opaque_detail_case() -> Model {
    let (directory, tag) = fixture(false);
    let mut model = fixtures::load(directory.path(), tag).unwrap();
    model.uvs.fill([0.25, 0.5]);
    model.detail_uvs.fill([0.75, 0.5]);
    model.triangle_dyes.fill(0);
    model.textures.extend([
        texture::Texture {
            mips: None,
            linear: None,
            tag: 1,
            size: [1, 1],
            // Native plate overlay is neutral at linear 0.25, approximately sRGB 137.
            // Keep these coordinate landmarks independent of the plate's color bias.
            rgba: vec![137, 137, 137, 255],
        },
        texture::Texture {
            mips: None,
            linear: None,
            tag: 2,
            size: [1, 1],
            rgba: vec![255, 255, 0, 255],
        },
        texture::Texture {
            mips: None,
            linear: None,
            tag: 3,
            size: [2, 1],
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        },
    ]);
    model.triangle_textures.fill(Some(0));
    model.triangle_gearstacks.fill(Some(1));
    model.dyes[0] = Some(shader::Dye {
        surface: crate::dyes::material::Surface {
            albedo: [0.5; 3],
            worn_albedo: [0.5; 3],
            params: [1.0, 0.0, 0.0, 0.0],
            worn_params: [1.0, 0.0, 0.0, 0.0],
            roughness: [0.0, 1.0, 0.0, 1.0],
            worn_roughness: [0.0, 1.0, 0.0, 1.0],
            wear: [0.0, 1.0, 0.0, 1.0],
            emissive: [0.0; 3],
            iridescence: -1.0,
        },
        detail: Some(2),
        normal: None,
        transform: [1.0, 1.0, 0.0, 0.0],
        normal_transform: [1.0, 1.0, 0.0, 0.0],
        vectors: [[0.0; 4]; 27],
    });
    model
}

#[test]
fn recovered_detail_coordinates_drive_the_opaque_color_pattern() {
    let model = opaque_detail_case();
    let image = render::animated_image(
        &model,
        render::Camera {
            yaw: 0.0,
            pitch: 0.0,
            zoom: 1.0,
            pan: [0.0; 2],
        },
        render::Scene {
            filmic: false,
            bloom: false,
            key: 0.0,
            fill: 1.0,
            ..render::Scene::unit_exposure()
        },
        [320, 240],
        0.0,
    );
    let center = image.pixels[120 * 320 + 160].to_array();
    assert!(
        center[2] > center[0].saturating_add(150),
        "The native detail coordinate selects blue, the atlas coordinate selects red: {center:?}"
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        output.join("opaque-detail-pattern.png"),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
    std::fs::write(output.join("opaque-detail-receipt.json"), serde_json::to_vec_pretty(
        &json!({"center":center,"expected_dominant_channel":"blue","primary_uv":[0.25,0.5],"detail_uv":[0.75,0.5]})).unwrap()).unwrap();
}

#[test]
fn recovered_detail_patterns_survive_overlapping_uvs_in_export() {
    let mut model = opaque_detail_case();
    let count = model.vertices.len();
    model.vertices.extend(model.vertices.clone());
    model.normals.extend(model.normals.clone());
    model.uvs.extend(model.uvs.clone());
    model.detail_uvs.extend(vec![[0.25, 0.5]; count]);
    model
        .triangles
        .push(model.triangles[0].map(|v| v + count as u32));
    model.triangle_textures.push(Some(0));
    model.triangle_gearstacks.push(Some(1));
    model.triangle_dyes.push(0);
    model.triangle_detail_uv.push(true);
    let glb = export::glb(&model, 0.0).unwrap();
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
    let mut colors = Vec::new();
    for primitive in document["meshes"][0]["primitives"].as_array().unwrap() {
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
        let mut compressed = Vec::new();
        let mut cursor = 8;
        while cursor + 12 <= png.len() {
            let size = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
            if &png[cursor + 4..cursor + 8] == b"IDAT" {
                compressed.extend(&png[cursor + 8..cursor + 8 + size]);
            }
            cursor += size + 12;
        }
        let mut raw = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(compressed.as_slice()),
            &mut raw,
        )
        .unwrap();
        assert_eq!(raw[0], 0);
        colors.push([raw[1], raw[2], raw[3]]);
    }
    assert!(
        colors.iter().any(|c| c[2] > 200 && c[0] < 20),
        "Missing blue detail: {colors:?}"
    );
    assert!(
        colors.iter().any(|c| c[0] > 200 && c[2] < 20),
        "Missing red detail: {colors:?}"
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    std::fs::write(output.join("overlapping-native-detail.glb"), glb).unwrap();
    std::fs::write(output.join("overlapping-native-detail-receipt.json"), serde_json::to_vec_pretty(&json!({"exported_colors":colors,"primary_uv":[0.25,0.5],"detail_uvs":[[0.75,0.5],[0.25,0.5]]})).unwrap()).unwrap();
}
