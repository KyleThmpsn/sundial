//! Live renderer acceptance for GPU deformation and both particle study contracts.
//! CPU evaluation and generated package witnesses remain the independent reference.
use super::*;
use crate::model_preview::compatibility_tests;

pub(super) fn cases(cases: &mut Vec<Case>) {
    let camera = Camera {
        yaw: 0.0,
        pitch: 0.0,
        zoom: 0.65,
        pan: [0.0; 2],
    };
    let scene = Scene::unprocessed();
    for animated in [false, true] {
        let model = Arc::new(compatibility_tests::cloth_gpu_case(animated));
        assert!(model.has_cloth(), "Cloth fixture must load its timeline");
        for (step, seconds) in [0.0, 0.5, 0.508_333_3, 0.508_333_3, 1.0, 0.0]
            .into_iter()
            .enumerate()
        {
            cases.push(Case {
                name: format!("offload-cloth-{animated}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene,
                    style: Style::Solid,
                    seconds,
                    pose: None,
                    animate: true,
                    dyes: None,
                },
                expected: None,
            });
        }
    }
    for stage in [0, 2, 7] {
        let model = Arc::new(compatibility_tests::effects::motion_case(stage));
        for (step, seconds) in [0.0, 0.25, 0.5, 0.5, 0.0].into_iter().enumerate() {
            cases.push(Case {
                name: format!("offload-motion-{stage}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene,
                    style: Style::Solid,
                    seconds,
                    pose: None,
                    animate: true,
                    dyes: None,
                },
                expected: None,
            });
        }
    }
    for (name, rate, missing) in [
        ("burst", 0.0, false),
        ("continuous", 2.0, false),
        ("material", 0.0, true),
    ] {
        let model = Arc::new(compatibility_tests::particles::lifecycle_case(
            rate, missing,
        ));
        for (step, seconds) in [0.0, 0.5, 0.75, 1.05, 0.0, 0.0].into_iter().enumerate() {
            cases.push(Case {
                name: format!("offload-particle-{name}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera: Camera {
                        zoom: 3.0,
                        ..camera
                    },
                    scene: Scene {
                        particle_study: step != 5,
                        ..scene
                    },
                    style: Style::Textured,
                    seconds,
                    pose: None,
                    animate: true,
                    dyes: None,
                },
                expected: None,
            });
        }
    }
    let mut model = Model::default();
    model.textures.push(texture(1, &[[80, 140, 220, 128]; 2]));
    model.particle_sources = (0..2)
        .map(|_| crate::model_preview::particles::Source {
            position: [0.0; 3],
            drift: [0.15, 0.0, 0.3],
            width: 0.25,
            phase: 0.0,
            period: 2.0,
            texture: 0,
            gradient: None,
        })
        .collect();
    let model = Arc::new(model);
    for (step, seconds) in [0.0, 0.5, 0.5, 0.0].into_iter().enumerate() {
        cases.push(Case {
            name: format!("offload-sprite-{step}"),
            frame: Frame {
                model: model.clone(),
                camera: Camera {
                    zoom: 1.0,
                    ..camera
                },
                scene: Scene {
                    particle_study: true,
                    ..scene
                },
                style: Style::Textured,
                seconds,
                pose: None,
                animate: true,
                dyes: None,
            },
            expected: None,
        });
    }
}

pub(super) fn result(
    case: &Case,
    cpu: &egui::ColorImage,
    gpu: &egui::ColorImage,
) -> serde_json::Value {
    let background = egui::Color32::from_rgb(
        case.frame.scene.background[0],
        case.frame.scene.background[1],
        case.frame.scene.background[2],
    );
    let retired = matches!(
        case.name.as_str(),
        "offload-particle-burst-3" | "offload-particle-material-3"
    );
    let mut compared = 0;
    let mut mismatch = 0;
    let mut error = 0;
    for y in 3..gpu.height() - 3 {
        for x in 3..gpu.width() - 3 {
            let index = y * gpu.width() + x;
            if !retired && !interior(cpu, x, y, background) && !interior(gpu, x, y, background) {
                continue;
            }
            compared += 1;
            mismatch +=
                usize::from((cpu.pixels[index] == background) != (gpu.pixels[index] == background));
            for (a, b) in cpu.pixels[index]
                .to_array()
                .into_iter()
                .zip(gpu.pixels[index].to_array())
            {
                error = error.max(a.abs_diff(b));
            }
        }
    }
    let empty = |image: &egui::ColorImage| image.pixels.iter().all(|&pixel| pixel == background);
    json!({"name":case.name,"max_channel_difference":error,"coverage_mismatches":mismatch,
        "compared_pixels":compared,"cpu_empty":empty(cpu),"gpu_empty":empty(gpu),
        "passed":error<=3 && mismatch==0 && compared>20 && (!retired || empty(cpu) && empty(gpu))})
}
