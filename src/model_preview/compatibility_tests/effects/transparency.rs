//! Overlapping package materials with independent alpha-composition expectations.
use super::*;
use native::{instruction, register, shader_stage, source};
use std::sync::Arc;

pub(crate) struct Case {
    pub name: String,
    pub model: Arc<Model>,
    pub camera: render::Camera,
    pub seconds: f32,
    pub expected: [u8; 3],
}

pub(crate) fn scene() -> render::Scene {
    render::Scene {
        background: [0; 3],
        ..render::Scene::unprocessed()
    }
}

fn model(moving: bool, same: bool, coplanar: bool) -> Model {
    let mut package = Package::default();
    let pixel = shader_stage(
        &mut package,
        &[
            instruction(98, &[&register(1, 5, 15)]),
            instruction(101, &[&register(2, 0, 15)]),
            instruction(54, &[&register(2, 0, 15), &source(1, 5, 0xE4)]),
            instruction(62, &[]),
        ]
        .concat(),
        0,
        &[("TEXCOORD", 5)],
        &[("SV_TARGET", 0)],
    );
    let vertex = moving.then(|| {
        shader_stage(
            &mut package,
            &[
                instruction(89, &[&[0x0020_8000, 0, 1]]),
                instruction(95, &[&register(1, 0, 15)]),
                instruction(95, &[&register(1, 1, 15)]),
                instruction(101, &[&register(2, 4, 15)]),
                instruction(101, &[&register(2, 5, 15)]),
                instruction(54, &[&register(2, 4, 15), &source(1, 0, 0xE4)]),
                // position.y += color.r * (4 * time). Only the red panel moves.
                instruction(
                    50,
                    &[
                        &register(2, 4, 2),
                        &source(1, 1, 0),
                        &[0x0020_800A, 0, 0],
                        &source(1, 0, 0x55),
                    ],
                ),
                instruction(54, &[&register(2, 5, 15), &source(1, 1, 0xE4)]),
                instruction(62, &[]),
            ]
            .concat(),
            1,
            &[("POSITION", 0), ("COLOR", 1)],
            &[("TEXCOORD", 4), ("TEXCOORD", 5)],
        )
    });
    let mut materials = Vec::new();
    for _ in 0..2 {
        let mut bytes = vec![0; 0x3A0];
        bytes[0x20] = 0x88;
        put(&mut bytes, 0x2C8, &pixel.to_le_bytes());
        if let Some(vertex) = vertex {
            put(&mut bytes, 0x48, &vertex.to_le_bytes());
            array(&mut bytes, 0x98, 0x8080_0090, &[0; 16], 16);
            let constants: Vec<_> = [4.0_f32; 4]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect();
            array(&mut bytes, 0x78, 0x8080_0090, &constants, 16);
            array(
                &mut bytes,
                0x68,
                0x8080_0009,
                &[0x3C, 1, 0, 0x34, 0, 3, 0x43, 0],
                1,
            );
        }
        materials.push(package.add(0x8080_71E8, bytes));
    }
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    if let Some(output) =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string)
    {
        let output = Path::new(&output).join("transparency-packages");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::copy(
            directory.path().join("w64_preview_0001_0.pkg"),
            output.join(if moving { "animated.pkg" } else { "static.pkg" }),
        )
        .unwrap();
    }
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let mut model = Model::default();
    for tag in materials {
        let material =
            crate::model_preview::effects::load(&manager, tag, Ok(&[]), 0, &mut model).unwrap();
        model.effects.push(material);
    }
    // Stored order is near red, far blue, then middle green. Red and blue share a material.
    for (depth, material, color) in [
        (-0.3, 0, [0.25, 0.0, 0.0, 0.5]),
        (0.3, 0, [0.0, 0.0, 0.125, 0.75]),
        (0.0, usize::from(!same), [0.0, 0.375, 0.0, 0.25]),
    ] {
        quad(
            &mut model,
            if coplanar { 0.0 } else { depth },
            Some(material),
            None,
        );
        model.colors.extend([color; 4]);
    }
    model
}

pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    // Front: red + green*0.5 + blue*0.75*0.5. Reverse: blue + green*0.25 + red*0.75*0.25.
    let front = encoded([0.25, 0.1875, 0.046875]);
    let reverse = encoded([0.046875, 0.09375, 0.125]);
    for same in [false, true] {
        let model = Arc::new(model(false, same, false));
        for (step, reversed) in [false, true, false].into_iter().enumerate() {
            cases.push(Case {
                name: format!("transparent-order-shared-{same}-{step}"),
                model: model.clone(),
                camera: render::Camera {
                    yaw: if reversed { std::f32::consts::PI } else { 0.0 },
                    ..camera
                },
                seconds: 0.0,
                expected: if reversed { reverse } else { front },
            });
        }
    }
    // The same three analytic layers with more than 65535 distinct source vertices.
    // Color, coverage and camera reversal must survive 32-bit indexed GPU submission.
    let dense = Arc::new(tessellated());
    for (step, reversed) in [false, false, true, false].into_iter().enumerate() {
        cases.push(Case {
            name: format!("transparent-order-dense-{step}"),
            model: dense.clone(),
            camera: render::Camera {
                yaw: if reversed { std::f32::consts::PI } else { 0.0 },
                ..camera
            },
            seconds: step as f32 * 0.25,
            expected: if reversed { reverse } else { front },
        });
    }
    // Coplanar triangles keep the stored draw order, even across material boundaries.
    let moved = encoded([0.046875, 0.375, 0.09375]);
    cases.push(Case {
        name: "transparent-order-coplanar".into(),
        model: Arc::new(model(false, false, true)),
        camera,
        seconds: 0.0,
        expected: moved,
    });
    let animated = Arc::new(model(true, false, false));
    for (step, seconds) in [0.0, 1.0, 0.25, 1.0, 0.0].into_iter().enumerate() {
        cases.push(Case {
            name: format!("transparent-order-motion-{step}"),
            model: animated.clone(),
            camera,
            seconds,
            expected: if seconds == 1.0 { moved } else { front },
        });
    }
    // A real package clip moves only the red panel's rig. Its root translates by
    // [1, 1.5, 2] at 1/600 second, putting red behind the other two panels.
    let fixture = crate::model_preview::compatibility_tests::fidelity::build();
    let manager = fixture.manager();
    let tag = fixture
        .clips
        .iter()
        .find(|(name, _)| name == "root-motion")
        .unwrap()
        .1;
    let mut rig = load_with_manager(&manager, fixture.entity, &Load::default(), Some(tag)).unwrap();
    let mut posed = model(false, false, false);
    for point in &mut posed.vertices {
        point[0] *= 3.0;
        point[2] *= 3.0;
    }
    posed.weights = (0..4)
        .map(|_| {
            Some(animation::Weights {
                values: [255, 0, 0, 0],
                bones: [0; 4],
            })
        })
        .collect();
    posed.rigs.push(animation::Rig {
        vertices: 0..4,
        animation: rig.animation.take().unwrap(),
    });
    let posed = Arc::new(posed);
    for (step, seconds) in [0.0, 1.0 / 600.0, 0.0].into_iter().enumerate() {
        cases.push(Case {
            name: format!("transparent-order-pose-{step}"),
            model: posed.clone(),
            camera,
            seconds,
            expected: if seconds == 0.0 { front } else { moved },
        });
    }
    cases
}

fn tessellated() -> Model {
    let mut source = model(false, true, false);
    let mut dense = Model {
        effects: std::mem::take(&mut source.effects),
        ..Default::default()
    };
    for layer in 0..3 {
        let depth = source.vertices[layer * 4][1];
        let color = source.colors[layer * 4];
        for row in 0..72 {
            for column in 0..80 {
                let base = dense.vertices.len() as u32;
                let x = |n| -1.0 + 2.0 * n as f32 / 80.0;
                let z = |n| -1.0 + 2.0 * n as f32 / 72.0;
                dense.vertices.extend([
                    [x(column), depth, z(row)],
                    [x(column + 1), depth, z(row)],
                    [x(column + 1), depth, z(row + 1)],
                    [x(column), depth, z(row + 1)],
                ]);
                dense.normals.extend([[0.0, -1.0, 0.0]; 4]);
                dense.uvs.extend([[0.5, 0.5]; 4]);
                dense.colors.extend([color; 4]);
                dense
                    .triangles
                    .extend([[base, base + 1, base + 2], [base, base + 2, base + 3]]);
                dense.triangle_effects.extend([Some(0); 2]);
                dense.triangle_constant.extend([None; 2]);
            }
        }
    }
    dense
}

#[test]
fn translucent_materials_follow_camera_motion_and_rewind() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("effects").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for case in cases() {
        let image =
            render::animated_image(&case.model, case.camera, scene(), [160, 160], case.seconds);
        let mut error = 0;
        for y in 72..88 {
            for x in 72..88 {
                for (actual, expected) in image.pixels[y * 160 + x].to_array()[..3]
                    .iter()
                    .zip(case.expected)
                {
                    error = error.max(actual.abs_diff(expected));
                }
            }
        }
        let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        std::fs::write(
            output.join(format!("{}.png", case.name)),
            export::png(&rgba, 160, 160).unwrap(),
        )
        .unwrap();
        assert!(error <= 1, "{}: error {error}", case.name);
        receipt.push(json!({"name":case.name,"seconds":case.seconds,"yaw":case.camera.yaw,"expected":case.expected,"maximum_error":error}));
    }
    std::fs::write(
        output.join("transparency.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
