//! Package-to-render and GLB witnesses prepared before stored tangent rendering.
use super::*;
use eframe::egui::ColorImage;

pub(crate) fn cameras() -> [render::Camera; 3] {
    [(-0.55, 0.0), (0.4, 0.25), (-0.7, -0.3)].map(|(yaw, pitch)| render::Camera {
        yaw,
        pitch,
        ..Default::default()
    })
}

pub(crate) fn scene() -> render::Scene {
    render::Scene {
        filmic: false,
        bloom: false,
        light: [-0.6, 0.3, -0.74],
        background: [0; 3],
        ..render::Scene::unit_exposure()
    }
}

pub(crate) fn case(kind: &str) -> Model {
    let mut model = effects::opaque_detail_case();
    model.effects.clear();
    model.triangle_effects.clear();
    model.motions.clear();
    model.animation = None;
    model.rigs.clear();
    model.vertices = vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, 1.0]];
    model.normals = vec![[0.0, -1.0, 0.0]; 3];
    model.uvs = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    model.triangles = vec![[0, 1, 2]];
    model.triangle_textures = vec![Some(0)];
    model.triangle_gearstacks = vec![Some(1)];
    model.triangle_normals = vec![Some(2)];
    model.triangle_dyes = vec![0];
    model.textures[0].rgba = vec![150, 180, 220, 255];
    model.textures[1].rgba = vec![255, 102, 0, 0];
    model.textures[2].size = [1, 1];
    model.textures[2].rgba = vec![191, 159, 255, 255];
    model.dyes[0].as_mut().unwrap().detail = None;
    model.tangents = vec![
        match kind {
            "mirrored" => [0.0, 0.0, 1.0, -1.0],
            "skewed" => [0.0, -4.0, 2.0, 1.0],
            "zero" => [0.0; 4],
            "parallel" => [0.0, -1.0, 0.0, 1.0],
            "nonfinite" => [f32::NAN, 0.0, 1.0, 1.0],
            "invalid-hand" => [0.0, 0.0, 1.0, 0.0],
            _ => [0.0, 0.0, 1.0, 1.0],
        };
        3
    ];
    if kind == "missing" || kind == "unavailable" {
        model.tangents.clear();
    }
    if kind == "one-invalid" {
        model.tangents[0] = [0.0; 4];
    }
    if kind == "collapsed" || kind == "unavailable" {
        model.uvs.fill([0.25, 0.5]);
    }
    model
}

pub(crate) const KINDS: [&str; 11] = [
    "authored",
    "mirrored",
    "skewed",
    "collapsed",
    "missing",
    "zero",
    "parallel",
    "nonfinite",
    "invalid-hand",
    "one-invalid",
    "unavailable",
];

fn expected(kind: &str) -> [f32; 3] {
    let x = 191.0 / 255.0 * 2.0 - 1.0;
    let y = 159.0 / 255.0 * 2.0 - 1.0;
    let z = (1.0_f32 - x * x - y * y).sqrt();
    match kind {
        "unavailable" => [0.0, -1.0, 0.0],
        "mirrored" => [y, -z, x],
        "authored" | "skewed" | "collapsed" => [-y, -z, x],
        _ => [x, -z, y],
    }
}

pub(crate) fn animated(mirrored: bool) -> Model {
    let fixture = fidelity::build();
    let clip = fixture
        .clips
        .iter()
        .find(|(name, _)| name == "float")
        .unwrap()
        .1;
    let mut model = load_with_manager(
        &fixture.manager(),
        fixture.entity,
        &Load::default(),
        Some(clip),
    )
    .unwrap();
    let material = case("authored");
    model.textures = material.textures;
    model.dyes = material.dyes;
    model.triangle_textures.fill(Some(0));
    model.triangle_gearstacks = vec![Some(1)];
    model.triangle_normals = vec![Some(2)];
    model.triangle_dyes.fill(0);
    model.uvs = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    model.tangents = vec![[0.0, 0.8, 0.6, if mirrored { -1.0 } else { 1.0 }]; 3];
    model
}

fn animated_normal(mirrored: bool, step: usize) -> [f32; 3] {
    let x = 191.0 / 255.0 * 2.0 - 1.0;
    let y = 159.0 / 255.0 * 2.0 - 1.0;
    let z = (1.0_f32 - x * x - y * y).sqrt();
    let hand = if mirrored { -1.0 } else { 1.0 };
    let base = [-hand * y, 0.8 * x - 0.6 * z, 0.6 * x + 0.8 * z];
    match step {
        1 => [-base[1], base[0], base[2]],
        2 => [-base[0], -base[1], base[2]],
        _ => base,
    }
}

pub(crate) const TIMES: [f32; 4] = [0.0, 1.0 / 30.0, 2.0 / 30.0, 0.0];

fn animated_frames(out: &Path, receipt: &mut Vec<serde_json::Value>, mirrored: bool) {
    let model = animated(mirrored);
    let mut reference = animated(mirrored);
    reference.triangle_normals.fill(None);
    reference.normals.fill(animated_normal(mirrored, 0));
    for (step, seconds) in TIMES.into_iter().enumerate() {
        let expected = animated_normal(mirrored, step);
        for normal in reference.pose(seconds).unwrap().normals {
            assert!(
                normal
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() < 0.003)
            );
        }
        for (view, camera) in cameras().into_iter().enumerate() {
            let actual = render::animated_image(&model, camera, scene(), [320, 240], seconds);
            let witness = render::animated_image(&reference, camera, scene(), [320, 240], seconds);
            let name = format!("tangent-animated-{mirrored}-{step}-{view}");
            save(out, &name, &actual);
            save(out, &format!("{name}-reference"), &witness);
            let (count, maximum) = compare(&actual, &witness);
            assert!(
                count > 500 && maximum <= 1,
                "{name}: {count} pixels, difference {maximum}"
            );
            receipt.push(
                json!({"name":name,"expected_world_normal":expected,"seconds":seconds,
                "pixels":count,"maximum":maximum,"camera":[camera.yaw,camera.pitch]}),
            );
        }
        std::fs::write(
            out.join(format!("tangent-animated-{mirrored}-{step}.glb")),
            export::glb(&model, seconds).unwrap(),
        )
        .unwrap();
    }
}

fn image(model: &Model, camera: render::Camera) -> ColorImage {
    render::animated_image(model, camera, scene(), [320, 240], 0.0)
}

fn save(out: &Path, name: &str, image: &ColorImage) {
    let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
    std::fs::write(
        out.join(format!("{name}.png")),
        export::png(&bytes, 320, 240).unwrap(),
    )
    .unwrap();
}

fn frame(out: &Path, kind: &str, view: usize, camera: render::Camera) -> serde_json::Value {
    let model = case(kind);
    let actual = image(&model, camera);
    let mut reference = case(kind);
    reference.triangle_normals.fill(None);
    reference.normals.fill(expected(kind));
    let witness = image(&reference, camera);
    let (count, maximum) = compare(&actual, &witness);
    let name = format!("tangent-{kind}-{view}");
    save(out, &name, &actual);
    save(out, &format!("{name}-reference"), &witness);
    assert!(
        count > 500 && maximum <= 1,
        "{name}: {count} pixels, difference {maximum}"
    );
    json!({"name":name,"expected_world_normal":expected(kind),"pixels":count,"maximum":maximum,
        "camera":[camera.yaw,camera.pitch]})
}

fn compare(actual: &ColorImage, witness: &ColorImage) -> (usize, u8) {
    let mut maximum = 0;
    let mut count = 0;
    for (a, b) in actual.pixels.iter().zip(&witness.pixels) {
        if *a != eframe::egui::Color32::BLACK && *b != eframe::egui::Color32::BLACK {
            count += 1;
            maximum = maximum.max(
                a.to_array()
                    .into_iter()
                    .zip(b.to_array())
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap(),
            );
        }
    }
    (count, maximum)
}

#[test]
fn authored_tangents_and_handedness_match_independent_world_normals() {
    let temporary = tempfile::tempdir().unwrap();
    let configured =
        crate::test_support::artifacts("fidelity").map(std::path::PathBuf::into_os_string);
    let out = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(out).unwrap();
    let mut receipt = Vec::new();
    for kind in KINDS {
        for (view, camera) in cameras().into_iter().enumerate() {
            receipt.push(frame(out, kind, view, camera));
        }
        let glb = export::glb(&case(kind), 0.0).unwrap();
        std::fs::write(out.join(format!("tangent-{kind}.glb")), glb).unwrap();
    }
    for mirrored in [false, true] {
        animated_frames(out, &mut receipt, mirrored);
    }
    std::fs::write(
        out.join("tangent-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
