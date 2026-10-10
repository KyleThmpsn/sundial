//! Read the staged output of the configured cloth importer as ordinary preview content.
use super::*;
use std::{collections::BTreeSet, fs, path::PathBuf};

#[test]
#[ignore = "Requires SUNDIAL_CLOTH_PREVIEW_CASES from the cloth import E2E and fresh SUNDIAL_TEST_ARTIFACTS"]
fn staged_cloth_keeps_complete_weights_and_renders_from_multiple_views() {
    let cases =
        PathBuf::from(std::env::var_os("SUNDIAL_CLOTH_PREVIEW_CASES").expect("Preview cases"));
    let output = crate::test_support::artifact_dir("cloth-preview");
    assert!(!output.exists(), "Use a fresh preview artifact directory");
    let cases: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(cases).unwrap()).unwrap();
    assert!(!cases.is_empty());
    fs::create_dir_all(&output).unwrap();
    let mut receipt = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let model = load(
            Path::new(case["packages"].as_str().unwrap()),
            case["tag"].as_u64().unwrap() as u32,
        )
        .unwrap();
        validate(&model);
        if let Some(count) = case["stored_triangles"].as_u64() {
            let stored = (0..model.triangles.len())
                .filter(|&i| model.triangle_dye_maps[i].is_none() || model.triangle_dyes[i] == 0)
                .count();
            assert_eq!(
                stored as u64, count,
                "The stored cloth view draws simulation or lower-detail copies"
            );
        }
        let blended = model
            .weights
            .iter()
            .flatten()
            .filter(|w| w.values.iter().filter(|v| **v != 0).count() > 1)
            .count();
        if case["weighted"] == true {
            assert!(blended > 0, "The staged preview lost blended skinning");
            assert_eq!(model.weights.len(), model.vertices.len());
        }
        let placement_error = verify_placement(&model, case);
        let mut images = Vec::new();
        for (view, yaw) in [0., std::f32::consts::FRAC_PI_2, std::f32::consts::PI, -0.65]
            .into_iter()
            .enumerate()
        {
            for (mode, style) in [
                ("textured", render::Style::Textured),
                ("solid", render::Style::Solid),
            ] {
                let image = render::styled_image(
                    &model,
                    render::Camera {
                        yaw,
                        pitch: 0.15,
                        ..Default::default()
                    },
                    render::Scene::default(),
                    [640, 640],
                    0.,
                    style,
                );
                let distinct = image
                    .pixels
                    .iter()
                    .map(|p| p.to_array())
                    .collect::<BTreeSet<_>>()
                    .len();
                assert!(
                    distinct > 8,
                    "A cloth view is blank or flat: {index} {view} {mode}"
                );
                let file = format!("model-{index}-view-{view}-{mode}.png");
                let rgba = image
                    .pixels
                    .iter()
                    .flat_map(|p| p.to_array())
                    .collect::<Vec<_>>();
                fs::write(output.join(&file), export::png(&rgba, 640, 640).unwrap()).unwrap();
                images.push(json!({"file":file,"distinct_colors":distinct}));
            }
        }
        fs::write(
            output.join(format!("model-{index}.glb")),
            export::glb(&model, 0.).unwrap(),
        )
        .unwrap();
        receipt.push(json!({"case":case,"vertices":model.vertices.len(),"triangles":model.triangles.len(),"blended_vertices":blended,"maximum_uv_error":placement_error,"notices":model.notices,"images":images}));
        fs::write(output.join("preview.json"), serde_json::to_vec_pretty(&json!({"cases":receipt,"cloth_simulation_verified":false,"gameplay_verified":false})).unwrap()).unwrap();
    }
}

/// Configured witnesses come from the original shader's GPU stream output and the
/// emitted texture's atlas rectangle, independently of preview UV recovery.
fn verify_placement(model: &Model, case: &serde_json::Value) -> Option<f32> {
    #[derive(serde::Deserialize)]
    struct Witness {
        position: [f32; 3],
        uv: [f32; 2],
    }
    let path = case["uv_witnesses"].as_str()?;
    let witnesses: Vec<Witness> = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(!witnesses.is_empty());
    let mut maximum = 0.0f32;
    for (position, uv) in model.vertices.iter().zip(&model.uvs) {
        let error = witnesses
            .iter()
            .filter(|w| (0..3).all(|axis| (w.position[axis] - position[axis]).abs() < 0.00002))
            .map(|w| {
                (0..2)
                    .map(|axis| (w.uv[axis] - uv[axis]).abs())
                    .fold(0.0f32, f32::max)
            })
            .reduce(f32::min)
            .expect("The preview vertex is absent from the original GPU output");
        assert!(
            error < 0.00002,
            "The preview samples the wrong atlas location: {error}"
        );
        maximum = maximum.max(error);
    }
    Some(maximum)
}
