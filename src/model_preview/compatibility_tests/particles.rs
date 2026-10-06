//! Package-loaded particle meshes stay inspectable without becoming object surfaces.
use super::*;
use fixtures::{floats, put};

pub(crate) fn case(composed: bool) -> Model {
    let (mut package, _, models) = fixtures::cloth_entity();
    for (tag, offset) in [(models[0], 0.0), (models[1], 3.0)] {
        let bytes = package.payload_mut(tag);
        floats(bytes, 0x50, &[1.0; 3]);
        floats(bytes, 0x60, &[offset, 0.0, 0.0]);
        floats(bytes, 0x6C, &[1.0]);
    }
    let mut emitter = vec![0; 0x48];
    put(&mut emitter, 0x40, &models[1].to_le_bytes());
    let emitter = package.add(0x8080_6E2E, emitter);
    let mut system = vec![0; 0x20];
    put(&mut system, 0x18, &emitter.to_le_bytes());
    let system = package.add(PARTICLE_SYSTEM, system);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let particle = load_with_manager(&manager, system, &Load::default(), None).unwrap();
    if !composed {
        return particle;
    }
    let mut model = load_with_manager(&manager, models[0], &Load::default(), None).unwrap();
    appearance::append(&mut model, particle, "Particle Mesh").unwrap();
    model
}

#[test]
fn composed_particle_meshes_remain_inspectable_without_changing_object_preview() {
    let mut model = case(true);
    let particle = case(false);
    // The known package positions put the particle triangle to the object's right.
    assert_eq!(
        model.vertices[..3],
        [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]]
    );
    assert_eq!(
        model.vertices[3..],
        [[3.0, -2.0, 0.0], [4.0, -2.0, 0.0], [3.0, -1.0, 1.0]]
    );
    assert!(model.has_object_mesh());
    assert!(!particle.has_object_mesh());
    let camera = render::Camera {
        yaw: 0.0,
        pitch: 0.0,
        ..Default::default()
    };
    let scene = render::Scene::default();
    let draw =
        |model: &Model, style| render::styled_image(model, camera, scene, [192, 192], 0.0, style);
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let background = eframe::egui::Color32::from_rgb(24, 28, 35);
    let mut receipt = Vec::new();
    for style in [
        render::Style::Textured,
        render::Style::Solid,
        render::Style::Wireframe,
    ] {
        let image = draw(&model, style);
        let visible = image.pixels.iter().filter(|&&p| p != background).count();
        assert!(
            visible
                > if style == render::Style::Textured {
                    1000
                } else {
                    20
                }
        );
        // Both inspection styles must show the native mesh's right-hand screen region.
        if style != render::Style::Textured {
            assert!((128..164).any(|x| (78..116).any(|y| image.pixels[y * 192 + x] != background)));
        }
        let bytes = image
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>();
        std::fs::write(
            output.join(format!("composed-particle-{}.png", style.label())),
            export::png(&bytes, 192, 192).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"style":style.label(),"visible_pixels":visible}));
    }
    let composed = draw(&model, render::Style::Textured);
    // Keep all source vertices and material assignments in this object-only control.
    // Only the extra particle draw is removed, so a camera polluted by unused vertices fails.
    model.triangles.truncate(1);
    assert_eq!(
        composed.pixels,
        draw(&model, render::Style::Textured).pixels
    );
    let standalone = draw(&particle, render::Style::Textured);
    assert!(
        standalone
            .pixels
            .iter()
            .filter(|&&p| p != background)
            .count()
            > 1000
    );
    let bytes = standalone
        .pixels
        .iter()
        .flat_map(|p| p.to_array())
        .collect::<Vec<_>>();
    std::fs::write(
        output.join("standalone-particle-mesh.png"),
        export::png(&bytes, 192, 192).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output.join("particle-inspection-receipt.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":receipt,"standalone_visible":true,"native_particle_playback":false}),
        )
        .unwrap(),
    )
    .unwrap();
}
