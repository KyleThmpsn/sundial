//! Production package loading and rendering, with independent format witnesses.
//! Failure model and native survey procedure are recorded before decoder changes.
use super::*;
use serde_json::json;
pub(crate) mod effects;
pub(crate) mod fidelity;
mod fixtures;
mod plates;

fn artifact(model: &Model, output: &Path, name: &str) -> usize {
    let image = render::styled_image(
        model,
        render::Camera::default(),
        render::Scene::default(),
        [320, 240],
        0.0,
        render::Style::Solid,
    );
    let background = eframe::egui::Color32::from_rgb(24, 28, 35);
    let visible = image
        .pixels
        .iter()
        .filter(|&&pixel| pixel != background)
        .count();
    let rgba = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect::<Vec<_>>();
    std::fs::write(
        output.join(format!("{name}.png")),
        export::png(&rgba, 320, 240).unwrap(),
    )
    .unwrap();
    visible
}

fn validate(model: &Model) {
    assert!(!model.triangles.is_empty());
    assert!(model.vertices.iter().flatten().all(|v| v.is_finite()));
    assert!(model.normals.iter().flatten().all(|v| v.is_finite()));
    assert!(model.uvs.iter().flatten().all(|v| v.is_finite()));
    assert_eq!(model.vertices.len(), model.normals.len());
    assert_eq!(model.vertices.len(), model.uvs.len());
    assert_eq!(model.vertices.len(), model.weights.len());
    assert!(
        model
            .triangles
            .iter()
            .flatten()
            .all(|&v| (v as usize) < model.vertices.len())
    );
    for length in [
        model.triangle_textures.len(),
        model.triangle_dyes.len(),
        model.triangle_clip.len(),
        model.triangle_constant.len(),
        model.triangle_gearstacks.len(),
        model.triangle_normals.len(),
    ] {
        assert_eq!(length, model.triangles.len());
    }
}

/// Uncompressed temporary packages exercise the same dispatch, declarations, buffer reader,
/// transforms, triangle assembly and rasterizer as an installed model. No personal paths.
#[test]
fn declared_meshes_and_terrain_load_and_render_from_packages() {
    let fixture = fixtures::build();
    let manager = fixture.manager();
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_COMPATIBILITY_OUTPUT");
    let output = configured
        .as_ref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let mut receipt = Vec::new();
    for (name, tag) in &fixture.cases {
        let model = load_with_manager(&manager, *tag, &Load::default(), None)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        validate(&model);
        let visible = artifact(&model, output, name);
        assert!(visible > 100, "{name}: only {visible} pixels");
        expectations(name, &model);
        receipt.push(json!({"case":name,"tag":format!("{tag:08X}"),"visible_pixels":visible,
            "vertices":model.vertices.len(),"triangles":model.triangles.len(),"notices":model.notices}));
    }
    for (name, tag) in &fixture.invalid {
        let error = load_with_manager(&manager, *tag, &Load::default(), None)
            .err()
            .unwrap_or_else(|| panic!("{name} was accepted"));
        receipt.push(json!({"case":name,"rejected":error}));
    }
    std::fs::write(
        output.join("fixture-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

fn expectations(name: &str, model: &Model) {
    match name {
        "float-stage" => {
            assert_eq!(
                model.vertices,
                [[10.0, 20.0, 30.0], [12.0, 20.0, 30.0], [10.0, 22.0, 32.0]]
            );
            assert_eq!(model.uvs[0], [0.75, 1.25]);
            assert!((model.normals[0][2] - 1.0).abs() < 1e-6);
        }
        "packed-single" | "static-single" => {
            assert_eq!(model.uvs[0], [0.0, 1.0]);
            assert!((model.normals[0][2] - 1.0).abs() < 1e-6);
        }
        "terrain" => {
            assert_eq!(model.vertices[0], [16.0, 32.0, 9.0]);
            assert_eq!(model.vertices[1], [17.0, 32.0, 9.0]);
            assert_eq!(model.triangles.len(), 1);
            assert_eq!(model.uvs[0], [0.25, 0.75]);
        }
        "two-influence" => {
            let weights = model.weights[0]
                .as_ref()
                .expect("native packed skin weights");
            assert_eq!(weights.bones, [1, 2, 0, 0]);
            assert_eq!(weights.values, [64, 191, 0, 0]);
        }
        _ => {}
    }
}

#[test]
fn regular_and_cloth_components_render_together() {
    let (package, entity, models) = fixtures::cloth_entity();
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let model = load_with_manager(&manager, entity, &Load::default(), None).unwrap();
    validate(&model);
    assert_eq!(model.tags, models);
    assert_eq!(model.triangles.len(), 2);
    // The cloth triangle extends four units below the regular triangle after model scaling.
    assert!(model.vertices.contains(&[10.0, 16.0, 30.0]));
    assert!(model.vertices.contains(&[10.0, 20.0, 30.0]));
    let temporary = tempfile::tempdir().unwrap();
    let configured = std::env::var_os("SUNDIAL_COMPATIBILITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(temporary.path());
    std::fs::create_dir_all(output).unwrap();
    let visible = artifact(&model, output, "regular-and-cloth");
    assert!(visible > 10);
    std::fs::write(
        output.join("regular-and-cloth.json"),
        serde_json::to_vec_pretty(&json!({
            "models": model.tags, "triangles": model.triangles.len(), "visible_pixels": visible,
            "notices": model.notices, "cloth_simulation": false
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn collapsed_marker_parts_do_not_draw_or_change_framing() {
    let (mut package, entity, models) = fixtures::cloth_entity();
    fixtures::floats(package.payload_mut(models[1]), 0x6C, &[0.0]);
    let directory = tempfile::tempdir().unwrap();
    package.write(directory.path());
    let manager = PackageManager::new(
        directory.path(),
        tiger_pkg::GameVersion::Destiny(tiger_pkg::DestinyVersion::Destiny2Shadowkeep),
        Some(tiger_pkg::PackagePlatform::Win64),
    )
    .unwrap();
    let model = load_with_manager(&manager, entity, &Load::default(), None).unwrap();
    validate(&model);
    assert_eq!(model.triangles.len(), 1);
    assert!(model.vertices.iter().all(|v| v[1] >= 20.0));
    let configured = std::env::var_os("SUNDIAL_COMPATIBILITY_OUTPUT");
    let output = configured
        .as_deref()
        .map(Path::new)
        .unwrap_or(directory.path());
    std::fs::create_dir_all(output).unwrap();
    assert!(artifact(&model, output, "collapsed-marker-parts") > 10);
    std::fs::write(
        output.join("collapsed-marker-parts.glb"),
        export::glb(&model, 0.0).unwrap(),
    )
    .unwrap();
}

/// The supplied case list records the full census population and deterministic selection.
/// Every failure is retained in the receipt before any required witness fails the test.
#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES, SUNDIAL_COMPATIBILITY_CASES and SUNDIAL_COMPATIBILITY_OUTPUT"]
fn native_model_compatibility_survey() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let input = std::env::var_os("SUNDIAL_COMPATIBILITY_CASES").expect("case list");
    let output = std::env::var_os("SUNDIAL_COMPATIBILITY_OUTPUT").expect("artifact directory");
    let output = Path::new(&output);
    std::fs::create_dir_all(output).unwrap();
    let manager = crate::investment::discovery::open_packages(Path::new(&packages)).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!(!cases.is_empty());
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for case in cases {
        let tag = u32::from_str_radix(case["tag"].as_str().unwrap(), 16).unwrap();
        let name = format!("{tag:08X}");
        let result = match load_with_manager(&manager, tag, &Load::default(), None) {
            Ok(model) if !model.triangles.is_empty() => {
                validate(&model);
                let visible = artifact(&model, output, &name);
                if visible < 10 && case["required"] == true {
                    failures.push(format!("{name}: only {visible} pixels"));
                }
                json!({"case":case,"loaded":true,"vertices":model.vertices.len(),
                    "triangles":model.triangles.len(),"visible_pixels":visible,
                    "textures":model.textures.len(),"notices":model.notices})
            }
            result => {
                let error = result.err().unwrap_or_else(|| "No surface geometry".into());
                if case["required"] == true {
                    failures.push(format!("{name}: {error}"));
                }
                json!({"case":case,"loaded":false,"error":error})
            }
        };
        eprintln!(
            "{name}: {}",
            if result["loaded"] == true {
                "rendered"
            } else {
                result["error"].as_str().unwrap()
            }
        );
        report.push(result);
        std::fs::write(
            output.join("native-receipt.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
