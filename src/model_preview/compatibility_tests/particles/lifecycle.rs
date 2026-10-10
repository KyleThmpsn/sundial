//! Packaged definition to repeated simulation, rendered motion, retirement and rewind.
use super::*;
use fixtures::{Package, array};

fn definition(package: &mut Package, rate: f32, missing_input: bool) -> u32 {
    let mut bytes = vec![0; 0x150];
    bytes[0x80..0xF0].fill(0xFF);
    bytes[0x115] = 0;
    bytes[0x116] = 0;
    bytes[0x141] = 4;
    bytes[0x142] = 7;
    bytes[0x149] = 1;
    bytes[0x14A] = 1;
    floats(&mut bytes, 0x120, &[1.05]);
    let defaults: [[f32; 4]; 8] = [
        [1.0, rate, 1.0, 2.0],
        [1.0, 1.0, 1.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0; 4],
        [1.0, 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    ];
    for (route, bank, scalar) in [
        (0, 1, 15),
        (2, 0, 1),
        (5, 6, 0),
        (6, 1, 16),
        (7, 1, 12),
        (8, 2, 16),
        (9, 2, 12),
        (10, 6, 1),
        (11, 6, 4),
        (12, 6, 7),
        (15, 6, 8),
        (16, 6, 2),
        (17, 6, 3),
        (18, 6, 16),
        (19, 6, 8),
        (20, 6, 20),
        (21, 6, 9),
        (22, 6, 10),
        (38, 0, 2),
        (42, 0, 0),
        (43, 1, 22),
    ] {
        bytes[0x80 + route * 2..0x82 + route * 2].copy_from_slice(&[bank, scalar]);
    }
    let values: Vec<_> = defaults
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
    array(&mut bytes, 8, 0x8080_0090, &values, 16);
    let mut sections: [Vec<u8>; 8] = std::array::from_fn(|_| Vec::new());
    // Post-placement copies preserve the routed fourth lanes, including age.
    for slot in [3, 4] {
        sections[3].extend([0x3E, 2, slot, 0, 0x3F, 1, slot, 0]);
    }
    for (constant, slot) in [(0, 0), (0, 1), (0, 2), (1, 5), (2, 6)] {
        sections[4].extend([0x34, constant, 0x3F, 1, slot, 0]);
    }
    if missing_input {
        sections[1].extend([0x47, 0, 0x3F, 5, 0, 0]);
    }
    for (index, code) in sections.iter().enumerate() {
        put(
            &mut bytes,
            0x70 + index * 2,
            &(code.len() as u16).to_le_bytes(),
        );
    }
    let code: Vec<_> = sections.into_iter().flatten().collect();
    array(&mut bytes, 0x50, 0x8080_0009, &code, 1);
    let constants: Vec<_> = [
        [0.0_f32, 0.0, 0.5, 0.5],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 30.0, 1.0, 0.5],
    ]
    .into_iter()
    .flatten()
    .flat_map(f32::to_le_bytes)
    .collect();
    array(&mut bytes, 0x60, 0x8080_0090, &constants, 16);
    package.add(0x8080_6E2C, bytes)
}

pub(super) fn load(directory: &Path, rate: f32, missing_input: bool) -> Model {
    let (mut package, _, models) = fixtures::cloth_entity();
    let definition = definition(&mut package, rate, missing_input);
    let material = material::material(
        &mut package,
        material::Input {
            red: 128,
            green: 255,
            brightness: 30.0,
            coverage: 1.0,
            u: 0.5,
        },
    );
    let vertices: Vec<_> = [[-2.0_f32, 0.0, -2.0], [2.0, 0.0, -2.0], [0.0, 0.0, 2.0]]
        .into_iter()
        .flat_map(|p| [p[0], p[1], p[2], 0.5, 0.5, 0.0, 1.0, 0.0])
        .flat_map(f32::to_le_bytes)
        .collect();
    let stream = package.vertex(32, vertices);
    let mesh = package.payload_mut(models[0]);
    let at = 0x28 + i64::from_le_bytes(mesh[0x18..0x20].try_into().unwrap()) as usize;
    put(mesh, at, &stream.to_le_bytes());
    put(mesh, at + 0x58, &13_u16.to_le_bytes());
    floats(mesh, 0x50, &[1.0; 3]);
    floats(mesh, 0x60, &[0.0; 3]);
    floats(mesh, 0x6C, &[1.0]);
    floats(mesh, 0x70, &[1.0, 1.0, 0.0, 0.0]);
    let mut emitter = vec![0; 0x48];
    put(&mut emitter, 0x40, &models[0].to_le_bytes());
    let emitter = package.add(0x8080_6E2E, emitter);
    let mut system = vec![0; 52];
    for (at, tag) in [(0, definition), (0x14, material), (0x18, emitter)] {
        put(&mut system, at, &tag.to_le_bytes());
    }
    let system = package.raw(PARTICLE_SYSTEM, 8, 0, system);
    std::fs::create_dir_all(directory).unwrap();
    package.write(directory);
    let mut model = fixtures::load(directory, system).unwrap();
    // Shader identity has a separate original-bytecode witness. This fixture tests its
    // established instance and pixel contract without redistributing game shader bytes.
    model.assets.particles[0].pixel_kind = Some(assets::PixelKind::DualMaskRamp);
    model
}

#[test]
fn packaged_particle_lifecycle_moves_retires_rewinds_and_keeps_inputs_explicit() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path())
        .join("particle-lifecycle");
    let model = load(&output.join("burst"), 0.0, false);
    let particle = &model.assets.particles[0];
    let simulation = particle.simulation.as_ref().expect("stored-only lifecycle");
    assert_eq!(simulation.at(0.0).len(), 1);
    assert_eq!(simulation.at(0.5).len(), 1);
    // Semi-implicit integration: v_n = 1 + n/60, p_n = n/60 + n(n+1)/(2*60^2).
    let position = simulation.at(0.5)[0].attributes[4];
    let expected = 0.5 + 30.0 * 31.0 / (2.0 * 60.0 * 60.0);
    assert!((position[0] - expected).abs() < 0.00001, "{position:?}");
    assert!(simulation.at(1.05).is_empty(), "completed particles retire");
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        zoom: 3.0,
        ..Default::default()
    };
    let scene = render::Scene {
        particle_study: true,
        ..render::Scene::unprocessed()
    };
    let draw = |seconds| {
        render::styled_image(
            &model,
            camera,
            scene,
            [192, 192],
            seconds,
            render::Style::Textured,
        )
    };
    let start = draw(0.0);
    let moved = draw(0.5);
    let end = draw(1.05);
    let background = eframe::egui::Color32::from_rgb(24, 28, 35);
    let center = |image: &eframe::egui::ColorImage| {
        let visible: Vec<_> = image
            .pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| **p != background)
            .map(|(i, _)| i % 192)
            .collect();
        assert!(visible.len() > 20);
        visible.iter().sum::<usize>() as f32 / visible.len() as f32
    };
    assert!(
        center(&moved) - center(&start) > 15.0,
        "native position must reach the draw"
    );
    assert!(end.pixels.iter().all(|&pixel| pixel == background));
    assert_eq!(
        start.pixels,
        draw(0.0).pixels,
        "scrubbing backward restarts the same seed and burst"
    );
    let continuous = load(&output.join("continuous"), 2.0, false);
    let counts: Vec<_> = [0.0, 0.5, 0.75, 1.25]
        .into_iter()
        .map(|t| {
            continuous.assets.particles[0]
                .simulation
                .as_ref()
                .unwrap()
                .at(t)
                .len()
        })
        .collect();
    assert_eq!(
        counts,
        [1, 2, 2, 1],
        "rate, capacity, age and retirement have independent effects"
    );
    let missing = load(&output.join("missing-input"), 0.0, true);
    assert!(missing.assets.particles[0].simulation.is_none());
    assert!(
        missing.assets.particles[0]
            .simulation_notice
            .as_deref()
            .unwrap()
            .contains("input")
    );
    assert_eq!(
        start.pixels,
        draw(0.0).pixels,
        "a failed definition cannot mutate another preview"
    );
    for (name, image) in [("start", start), ("moving", moved), ("retired", end)] {
        let rgba: Vec<_> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect();
        std::fs::write(
            output.join(format!("{name}.png")),
            export::png(&rgba, 192, 192).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(output.join("receipt.json"), serde_json::to_vec_pretty(&json!({
        "fixture": "packaged native definitions with analytic constant acceleration",
        "step_seconds": 1.0 / 60.0, "position_at_half_second": position,
        "expected_x": expected, "continuous_counts": counts, "rewind_exact": true,
        "missing_inputs_rejected": true, "live_engine_scheduling_verified": false,
        "limits": "Studio host, identity attachment, bounded ordinary geometry and supplied shader contract"
    })).unwrap()).unwrap();
}
