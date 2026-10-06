//! Package-to-pose, image and GLB checks authored before the expanded clip decoder.
use super::*;
mod fixtures;
pub(crate) use fixtures::build;

fn close(actual: [f32; 3], expected: [f32; 3]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.003),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn composed_owners_keep_independent_rigs_and_static_parts_through_pose_and_export() {
    let fixture = build();
    let manager = fixture.manager();
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut model = Model::default();
    let tag = fixture
        .clips
        .iter()
        .find(|(name, _)| name == "float")
        .unwrap()
        .1;
    let moving = load_with_manager(&manager, fixture.entity, &Load::default(), Some(tag)).unwrap();
    appearance::append(&mut model, moving, "Moving").unwrap();
    let idle = load_with_manager(&manager, fixture.entity, &Load::default(), None).unwrap();
    appearance::append(&mut model, idle, "Idle").unwrap();
    let mut fixed = load_with_manager(&manager, fixture.entity, &Load::default(), None).unwrap();
    fixed.animation = None;
    fixed.clips.clear();
    fixed.animation_notice = Some("Static companion".into());
    let fixed_positions = fixed.vertices.clone();
    appearance::append(&mut model, fixed, "Fixed").unwrap();
    let seconds = 1.0 / 30.0;
    let initial = model.pose(0.0).unwrap().positions;
    let pose = model
        .pose(seconds)
        .expect("Composed appearances must retain their rigs");
    for (offset, name) in [(0, "float"), (3, "static")] {
        for (actual, expected) in pose.positions[offset..offset + 3]
            .iter()
            .zip(expectation(name, false).0)
        {
            close(*actual, expected);
        }
    }
    assert_eq!(pose.positions[6..], fixed_positions);
    assert!(model.notices.iter().any(|n| n.contains("Static companion")));
    let glb = export::glb(&model, seconds).unwrap();
    for (actual, expected) in glb_vectors(&glb, "POSITION")
        .into_iter()
        .zip(&pose.positions)
    {
        close(actual, [expected[0], expected[2], -expected[1]]);
    }
    std::fs::write(output.join("composed-rigs.glb"), glb).unwrap();
    assert!(artifact(&model, output, "composed-rigs") > 100);
    assert_eq!(model.pose(0.0).unwrap().positions, initial);
    std::fs::write(output.join("composed-rigs-receipt.json"), serde_json::to_vec_pretty(
        &json!({"positions":pose.positions,"clips":model.clips.iter().map(|c| (&c.name,c.tag)).collect::<Vec<_>>(),"notices":model.notices})).unwrap()).unwrap();
}

fn glb_vectors(bytes: &[u8], semantic: &str) -> Vec<[f32; 3]> {
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
    let accessor = document["meshes"][0]["primitives"][0]["attributes"][semantic]
        .as_u64()
        .unwrap_or_else(|| panic!("Missing {semantic} in posed GLB")) as usize;
    let accessor = &document["accessors"][accessor];
    let view = &document["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
    let offset = 28 + json_length + view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = accessor["count"].as_u64().unwrap() as usize;
    bytes[offset..offset + count * 12]
        .chunks_exact(12)
        .map(|point| {
            std::array::from_fn(|axis| {
                f32::from_le_bytes(point[axis * 4..axis * 4 + 4].try_into().unwrap())
            })
        })
        .collect()
}

#[test]
fn native_clip_families_preserve_motion_normals_and_materials_through_export() {
    let fixture = build();
    let manager = fixture.manager();
    let default = load_with_manager(&manager, fixture.entity, &Load::default(), None).unwrap();
    assert_eq!(
        default
            .animation
            .as_ref()
            .expect("Missing clips must not hide the idle")
            .tag,
        fixture.clips[0].1
    );
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT");
    let output = configured
        .as_ref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (name, tag) in &fixture.clips {
        let mut model =
            load_with_manager(&manager, fixture.entity, &Load::default(), Some(*tag)).unwrap();
        if name == "root-turn" {
            let mut composed = Model::default();
            appearance::append(&mut composed, model, "Turning").unwrap();
            model = composed;
        }
        receipt.push(verify_clip(name, &model, fixture.clips.len(), output));
    }
    for (name, tag) in &fixture.invalid {
        let model =
            load_with_manager(&manager, fixture.entity, &Load::default(), Some(*tag)).unwrap();
        assert!(model.animation.is_none(), "{name} animated invalid data");
        assert!(
            model.animation_notice.is_some(),
            "{name} lost its failure reason"
        );
        assert_eq!(
            model.vertices,
            [[1.0, 0.0, 0.0], [3.0, 0.0, 0.0], [1.0, 0.0, 2.0]]
        );
        receipt.push(json!({"case":name,"rejected":model.animation_notice}));
    }
    std::fs::write(
        output.join("fidelity-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

fn verify_clip(name: &str, model: &Model, clip_count: usize, output: &Path) -> serde_json::Value {
    validate(model);
    let animation = model
        .active_clip()
        .unwrap_or_else(|| panic!("{name}: {:?}", model.animation_notice));
    assert_eq!(
        animation.fps, 30.0,
        "Bone-index bounds must not become playback rate"
    );
    assert_eq!(
        model.clips.len(),
        clip_count,
        "Unreadable clips must not hide playable clips"
    );
    assert_eq!(
        model.textures[model.triangle_textures[0].unwrap()].rgba,
        [64, 192, 96, 255]
    );
    let seconds = 1.0 / 30.0;
    let pose = animation.sample(model, seconds);
    assert_eq!(pose.positions.len(), model.vertices.len());
    assert_eq!(pose.normals.len(), model.normals.len());
    let (expected, normal, translation) = expectation(name, false);
    close(pose.root_translation, translation);
    for (&actual, expected) in pose.positions.iter().zip(expected) {
        close(actual, expected);
    }
    for &actual in &pose.normals {
        close(actual, normal);
    }
    for time in [f32::NAN, f32::INFINITY, -10.0, 100.0, 0.0] {
        let pose = animation.sample(model, time);
        assert!(
            pose.positions
                .iter()
                .chain(&pose.normals)
                .flatten()
                .all(|v| v.is_finite())
        );
    }
    verify_export(
        model,
        seconds,
        expected,
        normal,
        &output.join(format!("{name}.glb")),
    );
    let (last, last_normal, _) = expectation(name, true);
    verify_export(
        model,
        animation.duration(),
        last,
        last_normal,
        &output.join(format!("{name}-end.glb")),
    );
    let image = render::animated_image(
        model,
        render::Camera::default(),
        render::Scene::default(),
        [320, 240],
        seconds,
    );
    let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    let visible = visible_pixels(&image);
    assert!(visible > 100, "{name}: only {visible} visible pixels");
    assert!(
        name != "root-turn" || edge_pixels(&image) == 0,
        "Turning roots must remain framed"
    );
    std::fs::write(
        output.join(format!("{name}.png")),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
    json!({"case":name,"positions":pose.positions,"normals":pose.normals,
        "root_translation":pose.root_translation,"visible_pixels":visible,"notices":model.notices})
}

fn visible_pixels(image: &eframe::egui::ColorImage) -> usize {
    image
        .pixels
        .iter()
        .filter(|pixel| **pixel != eframe::egui::Color32::from_rgb(24, 28, 35))
        .count()
}

fn edge_pixels(image: &eframe::egui::ColorImage) -> usize {
    let [width, height] = image.size;
    image
        .pixels
        .iter()
        .enumerate()
        .filter(|(i, pixel)| {
            let (x, y) = (i % width, i / width);
            (x == 0 || y == 0 || x + 1 == width || y + 1 == height)
                && **pixel != eframe::egui::Color32::from_rgb(24, 28, 35)
        })
        .count()
}

fn expectation(name: &str, last: bool) -> ([[f32; 3]; 3], [f32; 3], [f32; 3]) {
    match name {
        "static" | "static-single" => (
            [[1.0, 0.0, 0.0], [3.0, 0.0, 0.0], [1.0, 0.0, 2.0]],
            [0.0, -0.6, 0.8],
            [0.0; 3],
        ),
        "root-motion" if last => (
            [[41.0, 60.0, 80.0], [37.0, 60.0, 80.0], [41.0, 60.0, 84.0]],
            [0.0, 0.6, 0.8],
            [40.0, 60.0, 80.0],
        ),
        "root-motion" => (
            [[21.0, 30.0, 40.0], [21.0, 33.0, 40.0], [21.0, 30.0, 43.0]],
            [0.6, 0.0, 0.8],
            [20.0, 30.0, 40.0],
        ),
        "root-turn" if last => (
            [[39.0, 60.0, 80.0], [37.0, 60.0, 80.0], [39.0, 60.0, 78.0]],
            [0.0, -0.6, -0.8],
            [40.0, 60.0, 80.0],
        ),
        "root-turn" => (
            [[20.0, 30.0, 39.0], [20.0, 30.0, 37.0], [22.0, 30.0, 39.0]],
            [0.8, -0.6, 0.0],
            [20.0, 30.0, 40.0],
        ),
        _ if last => (
            [[1.0, 2.0, 0.0], [-3.0, 2.0, 0.0], [1.0, 2.0, 4.0]],
            [0.0, 0.6, 0.8],
            [0.0; 3],
        ),
        _ => {
            let y = if name == "curve" { 1.15 } else { 1.0 };
            (
                [[1.0, y, 0.0], [1.0, y + 3.0, 0.0], [1.0, y, 3.0]],
                [0.6, 0.0, 0.8],
                [0.0; 3],
            )
        }
    }
}

fn verify_export(
    model: &Model,
    seconds: f32,
    expected: [[f32; 3]; 3],
    normal: [f32; 3],
    path: &Path,
) {
    let glb = export::glb(model, seconds).unwrap();
    for (actual, [x, y, z]) in glb_vectors(&glb, "POSITION").into_iter().zip(expected) {
        close(actual, [x, z, -y]);
    }
    for actual in glb_vectors(&glb, "NORMAL") {
        close(actual, [normal[0], normal[2], -normal[1]]);
    }
    std::fs::write(path, glb).unwrap();
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_FIDELITY_OUTPUT"]
fn native_chicken_bank_renders_and_exports_every_clip() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let output = std::env::var_os("SUNDIAL_FIDELITY_OUTPUT").expect("artifact directory");
    let output = Path::new(&output).join("chicken");
    std::fs::create_dir_all(&output).unwrap();
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let initial = load_with_manager(&manager, 0x80BC90E3, &Load::default(), None).unwrap();
    assert_eq!(
        initial.clips.len(),
        9,
        "All nine native clips should be available"
    );
    let mut receipt = Vec::new();
    for clip in &initial.clips {
        let model =
            load_with_manager(&manager, 0x80BC90E3, &Load::default(), Some(clip.tag)).unwrap();
        let animation = model.animation.as_ref().expect("selected native clip");
        let mut frames = Vec::new();
        for step in 0..=5 {
            let seconds = animation.duration() * step as f32 / 5.0;
            let pose = animation.sample(&model, seconds);
            assert!(
                pose.positions
                    .iter()
                    .chain(&pose.normals)
                    .flatten()
                    .all(|v| v.is_finite())
            );
            assert!(
                pose.normals
                    .iter()
                    .any(|n| n.iter().map(|v| v * v).sum::<f32>() > 0.9)
            );
            let image = render::animated_image(
                &model,
                render::Camera::default(),
                render::Scene::default(),
                [320, 240],
                seconds,
            );
            let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let visible = visible_pixels(&image);
            let at_edge = edge_pixels(&image);
            assert!(
                visible > 100,
                "{:08X} sample {step}: only {visible} visible pixels",
                clip.tag
            );
            assert_eq!(
                at_edge, 0,
                "{:08X} sample {step} touches the frame edge",
                clip.tag
            );
            let stem = format!("{:08X}-{step}", clip.tag);
            std::fs::write(
                output.join(format!("{stem}.png")),
                export::png(&rgba, 320, 240).unwrap(),
            )
            .unwrap();
            let glb = export::glb(&model, seconds).unwrap();
            assert_eq!(glb_vectors(&glb, "NORMAL").len(), model.vertices.len());
            std::fs::write(output.join(format!("{stem}.glb")), glb).unwrap();
            frames.push(json!({"seconds":seconds,"vertices":pose.positions.len(),"normals":pose.normals.len(),
                "root_translation":pose.root_translation,"visible_pixels":visible,"edge_pixels":at_edge}));
        }
        receipt.push(json!({"clip":format!("{:08X}",clip.tag),"name":clip.name,"frames":frames}));
    }
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
