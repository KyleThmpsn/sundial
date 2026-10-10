//! Package-to-simulation acceptance authored before the portable cloth implementation.
//! Synthetic solver bytes and all frame witnesses came from the independent packfile
//! encoder and archived Shadowkeep dispatcher, with explicit 60 Hz scheduling inputs.
use super::*;
use fixtures::{Package, array, floats, layouts, put};
use std::fs;
mod animation;

pub(in crate::model_preview) fn gpu_case(animated: bool) -> Model {
    let solver: &[u8] = include_bytes!("cloth/solver.bin");
    let (directory, tag) = fixture_with_animation(solver, false, animated);
    fixtures::load(directory.path(), tag).unwrap()
}

fn fixture(solver: &[u8], bad_binding: bool) -> (tempfile::TempDir, u32) {
    fixture_with_animation(solver, bad_binding, false)
}

fn fixture_with_animation(
    solver: &[u8],
    bad_binding: bool,
    animated: bool,
) -> (tempfile::TempDir, u32) {
    let directory = tempfile::tempdir().unwrap();
    let mut package = Package::default();
    layouts(&mut package);
    let positions = [[0., 0., 1.], [1., 0., 1.], [0., 0., 0.]];
    let mut stream = vec![0; 3 * 48];
    for (i, position) in positions.iter().enumerate() {
        floats(&mut stream, i * 48, position);
        floats(&mut stream, i * 48 + 16, &[0., -1., 0., 0.]);
        floats(&mut stream, i * 48 + 32, &[1., 0., 0., 1.]);
    }
    let stream = package.vertex(48, stream);
    let uv = package.vertex(4, vec![0; 12]);
    let skin = package.vertex(8, [255, 0, 0, 0, 0, 0, 0, 0].repeat(3));
    let indices = package.indices(false);
    let mut model = vec![0; 0x80];
    floats(&mut model, 0x6C, &[1., 1., 1., 0., 0.]);
    let mesh = array(&mut model, 0x10, 0x8080_7378, &[0; 0x88], 0x88);
    for (slot, tag) in [stream, uv, skin, u32::MAX].into_iter().enumerate() {
        put(&mut model, mesh + slot * 4, &tag.to_le_bytes());
    }
    put(&mut model, mesh + 0x10, &indices.to_le_bytes());
    for stage in 1..24 {
        put(&mut model, mesh + 0x28 + stage * 2, &1u16.to_le_bytes());
    }
    put(&mut model, mesh + 0x58, &18u16.to_le_bytes());
    let mut part = [0; 32];
    put(&mut part, 4, &(-1i16).to_le_bytes());
    put(&mut part, 6, &3u16.to_le_bytes());
    put(&mut part, 12, &3u32.to_le_bytes());
    part[0x1A] = 255;
    array(&mut model, mesh + 0x18, 0x8080_737E, &part, 32);
    let model = package.add(MODEL, model);
    let solver = package.raw(0, 0, 0, solver.to_vec());
    let mut definition = vec![0; 0x6A0];
    for role in 0..11 {
        put(
            &mut definition,
            8 + role * 12,
            &(if role == 1 { 0u32 } else { 1 }).to_le_bytes(),
        );
        put(&mut definition, 12 + role * 12, &u32::MAX.to_le_bytes());
        put(
            &mut definition,
            16 + role * 12,
            &(if role == 1 { 1u32 } else { 0 }).to_le_bytes(),
        );
    }
    let mut binding = [0; 12];
    put(
        &mut binding,
        4,
        &(if bad_binding { 999u32 } else { 3 }).to_le_bytes(),
    );
    put(&mut binding, 8, &3u32.to_le_bytes());
    array(&mut definition, 0x90, 0x8080_7280, &binding, 12);
    for group in 0..2 {
        array(
            &mut definition,
            0xA0 + group * 0x180,
            0x8080_0007,
            &0u32.to_le_bytes(),
            4,
        );
    }
    put(&mut definition, 0x690, &solver.to_le_bytes());
    let definition = package.add(0x8080_727A, definition);
    let mut component = vec![0; 0x400];
    put(&mut component, 0x10, &0x30i64.to_le_bytes());
    put(&mut component, 0x18, &0x68i64.to_le_bytes());
    put(&mut component, 0x3C, &0x8080_7273u32.to_le_bytes());
    put(&mut component, 0x7C, &0x8080_7286u32.to_le_bytes());
    put(&mut component, 0x80 + 0x1DC, &model.to_le_bytes());
    put(&mut component, 0x80 + 0x358, &definition.to_le_bytes());
    let component = package.add(RESOURCE, component);
    let mut entity = vec![0; 0x28];
    let mut components = vec![component];
    if animated {
        components.extend(animation::components(&mut package));
    }
    let rows = components
        .into_iter()
        .flat_map(|tag| [tag, 0, 0])
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    array(&mut entity, 0x10, 0x8080_9C04, &rows, 12);
    let entity = package.add(ENTITY, entity);
    package.write(directory.path());
    (directory, entity)
}

#[test]
fn native_cloth_plays_seeks_composes_and_exports_archived_frame_witnesses() {
    let bytes = include_bytes!("cloth/solver.bin");
    let witness: serde_json::Value =
        serde_json::from_str(include_str!("cloth/witness.json")).unwrap();
    let (directory, tag) = fixture(bytes, false);
    let model = fixtures::load(directory.path(), tag).unwrap();
    assert!(model.has_cloth(), "{:?}", model.notices);
    let temporary = tempfile::tempdir().unwrap();
    let output = crate::test_support::artifacts("fidelity")
        .unwrap_or_else(|| temporary.path().to_owned())
        .join("cloth-playback");
    fs::create_dir_all(&output).unwrap();
    let mut maximum = 0.0f32;
    for (frame, expected) in witness["frames"].as_array().unwrap().iter().enumerate() {
        let pose = model.pose(frame as f32 / 60.).expect("Cloth pose");
        for (actual, expected) in pose.positions.iter().zip(expected.as_array().unwrap()) {
            for axis in 0..3 {
                maximum =
                    maximum.max((actual[axis] - expected[axis].as_f64().unwrap() as f32).abs());
            }
        }
    }
    assert!(maximum < 0.0005, "Native trajectory differs by {maximum}");
    let initial = model.pose(0.).unwrap().positions;
    let mut images = Vec::new();
    for frame in [0, 30, 60, 120] {
        let seconds = frame as f32 / 60.;
        let glb = export::glb(&model, seconds).unwrap();
        let expected = witness["frames"][frame].as_array().unwrap();
        for (point, expected) in fidelity::glb_vectors(&glb, "POSITION").iter().zip(expected) {
            let expected = [
                expected[0].as_f64().unwrap() as f32,
                expected[2].as_f64().unwrap() as f32,
                -expected[1].as_f64().unwrap() as f32,
            ];
            assert!((0..3).all(|axis| (point[axis] - expected[axis]).abs() < 0.0005));
        }
        fs::write(output.join(format!("frame-{frame}.glb")), glb).unwrap();
        let image = render::styled_image(
            &model,
            render::Camera::default(),
            render::Scene::default(),
            [256, 256],
            seconds,
            render::Style::Solid,
        );
        let rgba = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        fs::write(
            output.join(format!("frame-{frame}.png")),
            export::png(&rgba, 256, 256).unwrap(),
        )
        .unwrap();
        images.push(rgba);
    }
    assert!(
        images[0]
            .iter()
            .zip(&images[2])
            .filter(|(a, b)| a != b)
            .count()
            > 100
    );
    assert_eq!(
        model.pose(0.).unwrap().positions,
        initial,
        "Seeking must reset the simulation"
    );
    let mut composed = Model::default();
    appearance::append(&mut composed, model, "First").unwrap();
    let second = fixtures::load(directory.path(), tag).unwrap();
    appearance::append(&mut composed, second, "Second").unwrap();
    let pose = composed.pose(1.).unwrap();
    assert_eq!(pose.positions[..3], pose.positions[3..]);
    assert_ne!(pose.positions[..3], initial);
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
        "witness":witness,"maximum_position_error":maximum,"rewind":"exact",
        "independent_owners":2,"gameplay_verified":false}))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn invalid_cloth_keeps_stored_geometry_and_reports_the_missing_contract() {
    let bytes = include_bytes!("cloth/solver.bin");
    for (solver, invalid_binding) in [(&bytes[..bytes.len() - 1], false), (bytes.as_slice(), true)]
    {
        let (directory, tag) = fixture(solver, invalid_binding);
        let model = fixtures::load(directory.path(), tag).unwrap();
        assert!(!model.has_cloth());
        assert_eq!(model.vertices, [[0., 0., 1.], [1., 0., 1.], [0., 0., 0.]]);
        assert!(model.notices.iter().any(|n| n.contains("Cloth")));
        assert_eq!(model.triangles.len(), 1);
    }
}

#[test]
#[ignore = "Requires configured package inputs, independent native draw witnesses and a fresh output directory"]
fn configured_cloth_playback_matches_native_draw_witnesses() {
    let cases = std::env::var_os("SUNDIAL_CLOTH_PLAYBACK_CASES").expect("Cloth cases");
    let output = crate::test_support::artifact_dir("cloth-playback");
    assert!(!output.exists(), "Use fresh artifacts");
    let cases: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(cases).unwrap()).unwrap();
    assert!(!cases.is_empty(), "A configured corpus cannot be empty");
    fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let model = load(
            Path::new(case["packages"].as_str().unwrap()),
            case["tag"].as_u64().unwrap() as u32,
        )
        .unwrap();
        assert!(model.has_cloth(), "{:?}", model.notices);
        let witnesses: serde_json::Value =
            serde_json::from_slice(&fs::read(case["witnesses"].as_str().unwrap()).unwrap())
                .unwrap();
        let frames = witnesses["frames"].as_array().unwrap();
        assert!(frames.len() >= 2);
        assert!(
            witnesses["image_sha256"]
                .as_str()
                .is_some_and(|s| s.len() == 64)
        );
        let mut maximum = 0.0f32;
        for (frame, witness) in frames.iter().enumerate() {
            let seconds = witness["seconds"].as_f64().unwrap() as f32;
            let glb = export::glb(&model, seconds).unwrap();
            fs::write(output.join(format!("case-{index}-frame-{frame}.glb")), &glb).unwrap();
            maximum = maximum.max(native_draw_error(&glb, witness));
            let image = render::styled_image(
                &model,
                render::Camera::default(),
                render::Scene::default(),
                [512, 512],
                seconds,
                render::Style::Solid,
            );
            let rgba = image
                .pixels
                .iter()
                .flat_map(|p| p.to_array())
                .collect::<Vec<_>>();
            fs::write(
                output.join(format!("case-{index}-frame-{frame}.png")),
                export::png(&rgba, 512, 512).unwrap(),
            )
            .unwrap();
        }
        assert!(
            maximum < case["tolerance"].as_f64().unwrap() as f32,
            "Native draw error {maximum}"
        );
        let initial = model.pose(0.).unwrap().positions;
        assert_ne!(
            initial,
            model
                .pose(frames.last().unwrap()["seconds"].as_f64().unwrap() as f32)
                .unwrap()
                .positions
        );
        assert_eq!(initial, model.pose(0.).unwrap().positions);
        receipt.push(json!({"case":case,"witnesses":witnesses,"maximum_draw_error":maximum,"notices":model.notices}));
    }
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({"cases":receipt,"gameplay_verified":false})).unwrap(),
    )
    .unwrap();
}

fn native_draw_error(glb: &[u8], witness: &serde_json::Value) -> f32 {
    // Exported normal-map charts split native vertices. Follow every primitive's actual
    // indices and compare paired position/normal values in both directions instead of
    // assuming that the first accessor retains the source's allocation or ordering.
    let actual = exported_draws(glb);
    let positions = witness["POSITION"].as_array().unwrap();
    let normals = witness["NORMAL"].as_array().unwrap();
    assert_eq!(positions.len(), normals.len());
    let expected: Vec<[f32; 6]> = positions
        .iter()
        .zip(normals)
        .map(|(p, n)| {
            std::array::from_fn(|axis| {
                if axis < 3 { &p[axis] } else { &n[axis - 3] }
                    .as_f64()
                    .unwrap() as f32
            })
        })
        .collect();
    assert!(!actual.is_empty() && !expected.is_empty());
    let directed = |a: &[[f32; 6]], b: &[[f32; 6]]| {
        a.iter()
            .map(|a| {
                b.iter()
                    .map(|b| {
                        a.iter()
                            .zip(b)
                            .map(|(a, b)| (a - b).abs())
                            .fold(0.0f32, f32::max)
                    })
                    .fold(f32::INFINITY, f32::min)
            })
            .fold(0.0f32, f32::max)
    };
    directed(&actual, &expected).max(directed(&expected, &actual))
}

fn exported_draws(bytes: &[u8]) -> Vec<[f32; 6]> {
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let doc: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length]).unwrap();
    let binary = &bytes[28 + length..];
    let accessor = |index: usize| {
        let a = &doc["accessors"][index];
        let v = &doc["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let offset = v["byteOffset"].as_u64().unwrap_or(0) as usize
            + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        (a, v, offset)
    };
    let mut result = Vec::new();
    for primitive in doc["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
    {
        let (indices, _, start) = accessor(primitive["indices"].as_u64().unwrap() as usize);
        let width = match indices["componentType"].as_u64().unwrap() {
            5123 => 2,
            5125 => 4,
            _ => panic!("Unexpected index width"),
        };
        let count = indices["count"].as_u64().unwrap() as usize;
        for i in 0..count {
            let at = start + i * width;
            let index = if width == 2 {
                u16::from_le_bytes(binary[at..at + 2].try_into().unwrap()) as usize
            } else {
                u32::from_le_bytes(binary[at..at + 4].try_into().unwrap()) as usize
            };
            let mut point = [0.0; 6];
            for (half, semantic) in ["POSITION", "NORMAL"].into_iter().enumerate() {
                let (a, v, start) =
                    accessor(primitive["attributes"][semantic].as_u64().unwrap() as usize);
                assert_eq!(a["componentType"], 5126);
                assert_eq!(a["type"], "VEC3");
                assert!(index < a["count"].as_u64().unwrap() as usize);
                let stride = v["byteStride"].as_u64().unwrap_or(12) as usize;
                for axis in 0..3 {
                    let at = start + index * stride + axis * 4;
                    point[half * 3 + axis] =
                        f32::from_le_bytes(binary[at..at + 4].try_into().unwrap());
                }
            }
            assert!(point.iter().all(|v| v.is_finite()));
            result.push(point);
        }
    }
    result
}
