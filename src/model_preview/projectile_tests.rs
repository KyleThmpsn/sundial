//! Opt-in coverage survey against an external projectile case list.
use super::*;
use serde_json::json;

#[test]
#[ignore = "Requires installed packages, SUNDIAL_PROJECTILE_CASES and SUNDIAL_PROJECTILE_OUTPUT"]
fn projectile_preview_from_installed_packages() {
    let packages = std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").expect("package directory");
    let input = std::env::var_os("SUNDIAL_PROJECTILE_CASES").expect("projectile case JSON");
    let output = std::env::var_os("SUNDIAL_PROJECTILE_OUTPUT").expect("output directory");
    let output = Path::new(&output);
    std::fs::create_dir_all(output).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!(!cases.is_empty());
    let mut report = Vec::new();
    for case in cases {
        let tag = u32::try_from(case["asset"].as_u64().unwrap()).unwrap();
        let name = case["name"].as_str().unwrap();
        let result = match load(Path::new(&packages), tag) {
            Ok(model) => {
                assert!(model.vertices.iter().flatten().all(|v| v.is_finite()));
                assert!(
                    model
                        .triangles
                        .iter()
                        .flatten()
                        .all(|&v| (v as usize) < model.vertices.len())
                );
                let image =
                    render::animated_image(&model, render::Camera::default(), [320, 320], 0.0);
                let visible = image
                    .pixels
                    .iter()
                    .filter(|&&p| p != eframe::egui::Color32::from_rgb(24, 28, 35))
                    .count();
                let mut bytes = b"P6\n320 320\n255\n".to_vec();
                bytes.extend(image.pixels.iter().flat_map(|p| [p.r(), p.g(), p.b()]));
                std::fs::write(output.join(format!("{tag:08X}.ppm")), bytes).unwrap();
                let animation = model.animation.as_ref().map(|a| {
                    let other = render::animated_image(
                        &model,
                        render::Camera::default(),
                        [320, 320],
                        a.duration() * 0.5,
                    );
                    json!({"clip":a.tag,"duration":a.duration(),"frame_changes":other!=image})
                });
                json!({"asset":tag,"name":name,"loaded":true,"visible_pixels":visible,
                    "vertices":model.vertices.len(),"triangles":model.triangles.len(),
                    "textures":model.textures.len(),"textured_triangles":model.triangle_textures.iter().filter(|v|v.is_some()).count(),
                    "animation":animation,"animation_notice":model.animation_notice,"texture_notices":model.notices})
            }
            Err(error) => json!({"asset":tag,"name":name,"loaded":false,"error":error}),
        };
        eprintln!(
            "{name} 0x{tag:08X}: {}",
            if result["loaded"] == true {
                "rendered"
            } else {
                result["error"].as_str().unwrap()
            }
        );
        report.push(result);
        std::fs::write(
            output.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    eprintln!(
        "Projectile survey: {} of {} loaded",
        report.iter().filter(|r| r["loaded"] == true).count(),
        report.len()
    );
}
