//! Generated model fixtures through the production software and live OpenGL renderers.
//! Failures and independent expectations are recorded before implementation in the audit note.
use super::*;
use crate::model_preview::{export, render, texture::Texture};
use serde_json::json;
use std::{fs, path::PathBuf};
use winit::platform::windows::EventLoopBuilderExtWindows;

struct Case {
    name: String,
    frame: Frame,
    expected: Option<[u8; 3]>,
}

fn texture(tag: u32, colors: &[[u8; 4]]) -> Texture {
    Texture {
        mips: None,
        linear: None,
        tag,
        size: [colors.len(), 1],
        rgba: colors.iter().flatten().copied().collect(),
    }
}

fn quad(model: &mut Model, depth: f32, albedo: Option<usize>, constant: Option<[f32; 3]>) {
    let start = model.vertices.len() as u32;
    model.vertices.extend([
        [-1.0, depth, -1.0],
        [1.0, depth, -1.0],
        [1.0, depth, 1.0],
        [-1.0, depth, 1.0],
    ]);
    model.normals.extend([[0.0, -1.0, 0.0]; 4]);
    model
        .uvs
        .extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    model
        .triangles
        .extend([[start, start + 1, start + 2], [start, start + 2, start + 3]]);
    model.triangle_textures.extend([albedo; 2]);
    model.triangle_constant.extend([constant; 2]);
    model.triangle_dyes.extend([0; 2]);
}

fn dye() -> shader::Dye {
    shader::Dye {
        surface: crate::dyes::material::Surface {
            albedo: [0.2, 0.3, 0.4],
            worn_albedo: [0.6, 0.1, 0.05],
            params: [1.0, 1.0, 1.0, 0.0],
            worn_params: [1.0, 0.5, 0.5, 0.0],
            roughness: [0.0, 1.0, 0.0, 1.0],
            worn_roughness: [0.0, 1.0, 0.0, 1.0],
            wear: [0.0, 1.0, 0.0, 1.0],
            emissive: [0.0; 3],
            iridescence: -1.0,
        },
        detail: None,
        normal: None,
        transform: [1.0, 1.0, 0.0, 0.0],
        normal_transform: [1.0, 1.0, 0.0, 0.0],
        vectors: [[0.0; 4]; 27],
    }
}

fn normal_cases(add: &mut impl FnMut(String, Model, Scene, Option<[u8; 3]>)) {
    for flat in [false, true] {
        for mirrored in [false, true] {
            let mut model = Model::default();
            model.textures.push(texture(1, &[[150, 180, 220, 255]]));
            model.textures.push(texture(2, &[[180, 90, 230, 255]]));
            quad(&mut model, 0.0, Some(0), None);
            model.triangle_normals = vec![Some(1); 2];
            if flat {
                model.normals.clear();
            }
            if mirrored {
                for uv in &mut model.uvs {
                    uv[0] = -uv[0];
                }
            }
            add(
                format!("tilted-normal-flat-{flat}-mirrored-{mirrored}"),
                model,
                Scene::default(),
                None,
            );
        }
    }
    for mirrored in [false, true] {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[150, 180, 220, 255]]));
        model.textures.push(texture(2, &[[180, 90, 230, 255]]));
        quad(&mut model, 0.0, Some(0), None);
        model.normals.fill([0.0; 3]);
        model.triangle_normals = vec![Some(1); 2];
        if mirrored {
            for uv in &mut model.uvs {
                uv[0] = -uv[0];
            }
        }
        add(
            format!("missing-normal-mirrored-{mirrored}"),
            model,
            Scene::default(),
            None,
        );
    }
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let camera = Camera {
        yaw: 0.0,
        pitch: 0.0,
        zoom: 1.0,
        pan: [0.0; 2],
    };
    let mut add = |name: String, model: Model, scene: Scene, expected| {
        cases.push(Case {
            name,
            frame: Frame {
                model: Arc::new(model),
                camera,
                scene,
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected,
        });
    };
    packaged_cases(&mut add);
    legacy_color_cases(&mut add);
    for exposure in [0.0, 0.25, 1.0, 2.0] {
        let mut model = Model::default();
        model
            .textures
            .push(texture(1, &[[0, 0, 0, 255], [255, 255, 255, 255]]));
        quad(&mut model, 0.0, Some(0), Some([0.5; 3]));
        model.uvs.fill([0.5, 0.5]);
        // The center is 0.5 linear light, multiplied by a 0.5 emissive constant.
        let expected = match exposure {
            0.0 => 0,
            0.25 => 71,
            1.0 => 137,
            _ => 188,
        };
        add(
            format!("filtered-emission-{exposure}"),
            model,
            Scene {
                exposure,
                ..Scene::default()
            },
            Some([expected; 3]),
        );
    }
    for reverse in [false, true] {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[255, 255, 255, 0]]));
        if reverse {
            quad(&mut model, 0.1, None, Some([0.0, 1.0, 0.0]));
            quad(&mut model, -0.1, Some(0), Some([1.0, 0.0, 0.0]));
        } else {
            quad(&mut model, -0.1, Some(0), Some([1.0, 0.0, 0.0]));
            quad(&mut model, 0.1, None, Some([0.0, 1.0, 0.0]));
        }
        add(
            format!("transparent-depth-{reverse}"),
            model,
            Scene::default(),
            Some([0, 255, 0]),
        );
    }
    // Alpha remains linear while RGB is decoded before filtering. Exactly 0.5 is covered.
    let mut model = Model::default();
    model
        .textures
        .push(texture(1, &[[255, 255, 255, 0], [255, 255, 255, 255]]));
    quad(&mut model, 0.0, Some(0), Some([1.0; 3]));
    model.uvs.fill([0.5, 0.5]);
    add(
        "alpha-boundary".into(),
        model,
        Scene::default(),
        Some([255; 3]),
    );
    for shared in [false, true] {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[128, 128, 255, 255]]));
        model.textures.push(texture(2, &[[128, 128, 255, 255]]));
        quad(&mut model, 0.0, Some(0), None);
        model.triangle_normals = vec![Some(if shared { 0 } else { 1 }); 2];
        add(
            format!("color-and-normal-{shared}"),
            model,
            Scene::default(),
            None,
        );
    }
    for alpha in [0, 32, 39, 40, 48, 128, 255] {
        for detail in [false, true] {
            let mut model = Model::default();
            model.textures.push(texture(1, &[[160, 100, 40, 255]]));
            model.textures.push(texture(2, &[[220, 100, 20, alpha]]));
            model.textures.push(texture(3, &[[130, 125, 230, 255]]));
            model.textures.push(texture(4, &[[64, 128, 220, 192]]));
            quad(&mut model, 0.0, Some(0), None);
            model.triangle_gearstacks = vec![Some(1); 2];
            model.triangle_normals = vec![Some(2); 2];
            let mut material = dye();
            if detail {
                material.detail = Some(3);
                material.normal = Some(2);
            }
            model.dyes[0] = Some(material);
            add(
                format!("mask-{alpha}-detail-{detail}"),
                model,
                Scene::default(),
                None,
            );
        }
    }
    for row in [0.0, 1.0, 2.0, 3.0] {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[128, 128, 128, 255]]));
        model.textures.push(texture(2, &[[255, 128, 0, 255]]));
        quad(&mut model, 0.0, Some(0), None);
        model.triangle_gearstacks = vec![Some(1); 2];
        let mut material = dye();
        material.surface.iridescence = row;
        model.dyes[0] = Some(material);
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 0, 255, 255],
        ];
        model.iridescence = Some(Texture {
            mips: None,
            linear: None,
            tag: 5,
            size: [1, 4],
            rgba: colors.into_iter().flatten().collect(),
        });
        add(
            format!("iridescence-row-{row}"),
            model,
            Scene::default(),
            None,
        );
    }
    for has_texture in [false, true] {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[120, 180, 220, 255]]));
        quad(&mut model, 0.0, has_texture.then_some(0), None);
        model.dyes[0] = Some(dye());
        add(
            format!("dye-without-mask-{has_texture}"),
            model,
            Scene::default(),
            None,
        );
    }
    normal_cases(&mut add);
    let mut model = Model::default();
    quad(&mut model, 0.0, None, None);
    model.dyes[0] = Some(dye());
    model.triangle_dyes.clear();
    add(
        "missing-dye-assignment".into(),
        model,
        Scene::default(),
        None,
    );
    for (step, seconds) in [0.0, 0.25, 0.75, 1.25, 1.75, 0.25].into_iter().enumerate() {
        let mut model = Model::default();
        model.textures.push(texture(1, &[[160, 120, 80, 255]]));
        model.textures.push(texture(2, &[[255, 128, 0, 255]]));
        quad(&mut model, 0.0, Some(0), None);
        model.triangle_gearstacks = vec![Some(1); 2];
        let (animation, vectors) = crate::dyes::material::verification::timeline();
        model.dye_animations.push((0, animation));
        model.dyes[0] = Some(shader::Dye {
            surface: crate::dyes::material::properties(&vectors).surfaces[0],
            vectors,
            ..dye()
        });
        add(
            format!("native-cubic-timeline-{step}-{seconds}"),
            model,
            Scene::default(),
            None,
        );
    }
    let length = cases.len();
    for (case, seconds) in cases[length - 6..]
        .iter_mut()
        .zip([0.0, 0.25, 0.75, 1.25, 1.75, 0.25])
    {
        case.frame.seconds = seconds;
    }
    gain_cases(&mut cases, camera);
    paint_cases(&mut cases, camera);
    native_normal_cases(&mut cases, camera);
    normal_blue_cases(&mut cases, camera);
    metal_cases(&mut cases, camera);
    tangent_cases(&mut cases);
    canvas_cases(&mut cases);
    skeletal_cases(&mut cases);
    for (name, model, expected) in
        crate::model_preview::compatibility_tests::effects::render_cases()
    {
        cases.push(Case {
            name,
            frame: Frame {
                model: Arc::new(model),
                camera: Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    ..Default::default()
                },
                scene: Scene::default(),
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected: Some(expected),
        });
    }
    cases.push(Case {
        name: "packaged-particle-inspection-Solid".into(),
        frame: Frame {
            model: Arc::new(crate::model_preview::compatibility_tests::particles::case(
                true,
            )),
            camera,
            scene: Scene::default(),
            style: Style::Solid,
            seconds: 0.0,
            pose: None,
        },
        expected: None,
    });
    installed_gear_cases(&mut cases);
    cases
}

fn canvas_cases(cases: &mut Vec<Case>) {
    for (name, model) in crate::model_preview::compatibility_tests::canvas_cases() {
        cases.push(Case {
            name,
            frame: Frame {
                model: Arc::new(model),
                camera: Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    ..Default::default()
                },
                scene: Scene::default(),
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected: None,
        });
    }
}

fn gain_cases(cases: &mut Vec<Case>, camera: Camera) {
    for painted in [false, true] {
        let model = Arc::new(
            crate::model_preview::compatibility_tests::effects::opaque::gain_case(painted, false)
                .unwrap(),
        );
        for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
            cases.push(Case {
                name: format!("native-opaque-gain-{painted}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene: Scene {
                        key: 0.0,
                        fill: 1.0,
                        background: [0; 3],
                        ..Default::default()
                    },
                    style: Style::Textured,
                    seconds,
                    pose: None,
                },
                expected: None,
            });
        }
    }
}

fn tangent_cases(cases: &mut Vec<Case>) {
    use crate::model_preview::compatibility_tests::tangents;
    let mut add = |name: String, model: Arc<Model>, seconds| {
        for (view, camera) in tangents::cameras().into_iter().enumerate() {
            let camera = if model.animation.is_some() {
                Camera {
                    zoom: 0.55,
                    ..camera
                }
            } else {
                camera
            };
            cases.push(Case {
                name: format!("{name}-{view}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene: tangents::scene(),
                    style: Style::Textured,
                    seconds,
                    pose: model.pose(seconds).map(Arc::new),
                },
                expected: None,
            });
        }
    };
    for kind in tangents::KINDS {
        add(
            format!("tangent-{kind}"),
            Arc::new(tangents::case(kind)),
            0.0,
        );
    }
    for mirrored in [false, true] {
        let model = Arc::new(tangents::animated(mirrored));
        for (step, seconds) in tangents::TIMES.into_iter().enumerate() {
            add(
                format!("tangent-animated-{mirrored}-{step}"),
                model.clone(),
                seconds,
            );
        }
    }
}

fn paint_cases(cases: &mut Vec<Case>, camera: Camera) {
    for alpha in [0, 39, 40, 96, 180, 255] {
        let model = Arc::new(
            crate::model_preview::compatibility_tests::effects::paint::case(alpha, 0).unwrap(),
        );
        model
            .surface_overrides
            .lock()
            .unwrap()
            .push(crate::model_preview::SurfaceOverride {
                slot: 0,
                writes: vec![(9, 0, 0.05)],
            });
        for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
            cases.push(Case {
                name: format!("native-paint-{alpha}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene: Scene {
                        key: 0.0,
                        fill: 1.0,
                        background: [0; 3],
                        ..Default::default()
                    },
                    style: Style::Textured,
                    seconds,
                    pose: None,
                },
                expected: None,
            });
        }
    }
}

fn native_normal_cases(cases: &mut Vec<Case>, camera: Camera) {
    for channel in 0..3 {
        for alpha in [0, 39, 40, 96, 180, 255] {
            for primary in [false, true] {
                let model = crate::model_preview::compatibility_tests::effects::normals::case(
                    alpha,
                    channel,
                    primary,
                    [220, 80, 255, 255],
                    [255, 0, 255, 255],
                );
                cases.push(Case {
                    name: format!("native-normal-{channel}-{alpha}-{primary}"),
                    frame: Frame {
                        model: Arc::new(model),
                        camera,
                        scene: Scene {
                            background: [0; 3],
                            ..Default::default()
                        },
                        style: Style::Textured,
                        seconds: 0.0,
                        pose: None,
                    },
                    expected: None,
                });
            }
        }
    }
}

fn normal_blue_cases(cases: &mut Vec<Case>, camera: Camera) {
    for channel in 0..3 {
        cases.push(Case {
            name: format!("native-normal-blue-spatial-{channel}"),
            frame: Frame {
                model: Arc::new(
                    crate::model_preview::compatibility_tests::effects::normal_blue::spatial_case(
                        channel,
                    ),
                ),
                camera,
                scene: Scene {
                    background: [0; 3],
                    ..Default::default()
                },
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected: None,
        });
    }
    for channel in 0..3 {
        for alpha in [39, 40, 180, 255] {
            for primary in [false, true] {
                let model = crate::model_preview::compatibility_tests::effects::normal_blue::case(
                    alpha, channel, primary, 0, true,
                );
                cases.push(Case {
                    name: format!("native-normal-blue-{channel}-{alpha}-{primary}"),
                    frame: Frame {
                        model: Arc::new(model),
                        camera,
                        scene: Scene {
                            background: [0; 3],
                            ..Default::default()
                        },
                        style: Style::Textured,
                        seconds: 0.0,
                        pose: None,
                    },
                    expected: None,
                });
            }
        }
    }
    for (step, seconds) in [0.0, 0.5, 1.0, 0.0].into_iter().enumerate() {
        let model = crate::model_preview::compatibility_tests::effects::normal_blue::case(
            180, 1, false, 0, false,
        );
        cases.push(Case {
            name: format!("native-normal-blue-no-basis-{step}"),
            frame: Frame {
                model: Arc::new(model),
                camera,
                scene: Scene {
                    background: [0; 3],
                    ..Default::default()
                },
                style: Style::Textured,
                seconds,
                pose: None,
            },
            expected: None,
        });
    }
}

fn metal_cases(cases: &mut Vec<Case>, camera: Camera) {
    for alpha in [0, 39, 40, 180, 255] {
        let model = Arc::new(
            crate::model_preview::compatibility_tests::effects::metal::case(alpha, 10).unwrap(),
        );
        for (step, seconds) in [0.0, 0.5, 5.0, 0.0].into_iter().enumerate() {
            cases.push(Case {
                name: format!("native-metal-{alpha}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera,
                    scene: Scene {
                        key: 0.0,
                        fill: 1.0,
                        background: [0; 3],
                        ..Default::default()
                    },
                    style: Style::Textured,
                    seconds,
                    pose: None,
                },
                expected: None,
            });
        }
    }
    for alpha in [0, 39] {
        for normal in [false, true] {
            let model = Arc::new(
                crate::model_preview::compatibility_tests::effects::metal::without_dye(
                    alpha, normal,
                ),
            );
            for (step, seconds) in [0.0, 0.5, 5.0, 0.0].into_iter().enumerate() {
                cases.push(Case {
                    name: format!("native-metal-without-dye-{alpha}-{normal}-{step}"),
                    frame: Frame {
                        model: model.clone(),
                        camera,
                        scene: Scene {
                            key: 0.0,
                            fill: 1.0,
                            background: [0; 3],
                            ..Default::default()
                        },
                        style: Style::Textured,
                        seconds,
                        pose: None,
                    },
                    expected: None,
                });
            }
        }
    }
}

fn legacy_color_cases(add: &mut impl FnMut(String, Model, Scene, Option<[u8; 3]>)) {
    for (name, model) in crate::model_preview::compatibility_tests::legacy_color::cases()
        .into_iter()
        .chain(crate::model_preview::compatibility_tests::legacy_color::composition::cases())
        .chain(crate::model_preview::compatibility_tests::legacy_normal::cases())
        .chain(crate::model_preview::compatibility_tests::native_sampling::cases())
    {
        add(
            name,
            model,
            Scene {
                background: [0; 3],
                ..Default::default()
            },
            None,
        );
    }
}

fn packaged_cases(add: &mut impl FnMut(String, Model, Scene, Option<[u8; 3]>)) {
    add(
        "native-opaque-color-and-depth".into(),
        crate::model_preview::compatibility_tests::effects::opaque::case(false).unwrap(),
        Scene {
            key: 0.0,
            fill: 1.0,
            background: [0; 3],
            ..Default::default()
        },
        None,
    );
    for mode in 0..3 {
        add(
            format!("native-unsigned-arithmetic-{mode}"),
            crate::model_preview::compatibility_tests::effects::integer::case(mode),
            Scene {
                background: [0; 3],
                ..Default::default()
            },
            Some([99, 137, 225]),
        );
    }
    add(
        "native-affine-derivative".into(),
        crate::model_preview::compatibility_tests::effects::derivative::case(0).unwrap(),
        Scene {
            background: [0; 3],
            ..Default::default()
        },
        None,
    );
    for composed in [false, true] {
        add(
            format!("packaged-particle-mesh-composed-{composed}"),
            crate::model_preview::compatibility_tests::particles::case(composed),
            Scene::default(),
            None,
        );
    }
    for (format, cube) in [10, 26]
        .into_iter()
        .flat_map(|f| [false, true].map(|c| (f, c)))
        .chain([(29, false)])
    {
        use crate::model_preview::compatibility_tests::effects::hdr;
        add(
            format!("native-hdr-{format}-{cube}"),
            hdr::case(format, cube),
            Scene {
                background: [0; 3],
                ..Default::default()
            },
            Some(hdr::expected(format, cube)),
        );
    }
    add(
        "native-opaque-detail-pattern".into(),
        crate::model_preview::compatibility_tests::effects::opaque_detail_case(),
        Scene {
            key: 0.0,
            fill: 1.0,
            ..Default::default()
        },
        None,
    );
    for (name, model) in crate::model_preview::compatibility_tests::native_detail::cases() {
        add(name, model, Scene::default(), None);
    }
    for cutoff in [0.3f32, 0.5, 0.7] {
        add(
            format!("native-body-cutoff-{cutoff}"),
            crate::model_preview::compatibility_tests::decals::case(cutoff),
            Scene {
                key: 0.0,
                fill: 1.0,
                ..Default::default()
            },
            (cutoff > 0.5).then_some([24, 28, 35]),
        );
    }
}

fn installed_gear_cases(cases: &mut Vec<Case>) {
    let (Some(packages), Some(input)) = (
        std::env::var_os("SUNDIAL_PREVIEW_PACKAGES"),
        std::env::var_os("SUNDIAL_PLATE_CASES"),
    ) else {
        return;
    };
    let packages = PathBuf::from(packages);
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let input: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
    for case in input {
        let hash = u32::from_str_radix(case["hash"].as_str().unwrap(), 16).unwrap();
        let appearance = catalog
            .shader_preview_appearance(hash, &Default::default())
            .unwrap();
        let model =
            Arc::new(crate::model_preview::appearance::load(&packages, &appearance).unwrap());
        for seconds in [0.0, 0.5, 1.0, 0.0] {
            cases.push(Case {
                name: format!(
                    "gear-{}-{hash:08X}-{seconds:.1}-{}",
                    if case["minimum_blue_pixels"].as_u64().is_some() {
                        "map"
                    } else {
                        "control"
                    },
                    cases.len()
                ),
                frame: Frame {
                    model: model.clone(),
                    camera: crate::model_preview::compatibility_tests::installed_camera(&case),
                    scene: Scene::default(),
                    style: Style::Textured,
                    seconds,
                    pose: model.pose(seconds).map(Arc::new),
                },
                expected: None,
            });
        }
    }
}

fn skeletal_cases(cases: &mut Vec<Case>) {
    let fixture = crate::model_preview::compatibility_tests::fidelity::build();
    let manager = fixture.manager();
    for (name, tag) in &fixture.clips {
        let model = Arc::new(
            crate::model_preview::load_with_manager(
                &manager,
                fixture.entity,
                &crate::model_preview::Load::default(),
                Some(*tag),
            )
            .unwrap(),
        );
        let animation = model.animation.as_ref().expect("fixture animation");
        for (step, seconds) in [0.0, 1.0 / 30.0, 0.0, 1.0 / 30.0, animation.duration(), 0.0]
            .into_iter()
            .enumerate()
        {
            cases.push(Case {
                name: format!("skeletal-{name}-{step}"),
                frame: Frame {
                    model: model.clone(),
                    camera: Camera {
                        yaw: -std::f32::consts::FRAC_PI_4,
                        pitch: 0.2,
                        zoom: 0.55,
                        pan: [0.0; 2],
                    },
                    scene: Scene::default(),
                    style: Style::Textured,
                    seconds,
                    pose: (step != 5).then(|| {
                        Arc::new(animation.sample(&model, animation.looped_seconds(seconds)))
                    }),
                },
                expected: None,
            });
        }
    }
}

#[test]
#[ignore = "Opens a native OpenGL viewport, requires SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn generated_materials_render_consistently() {
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("output"));
    fs::create_dir_all(&output).unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let done = completed.clone();
    eframe::run_native(
        "Preview Rendering Verification",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([200.0, 200.0])
                .with_active(false),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |creation| {
            assert!(creation.gl.is_some(), "OpenGL context is required");
            Ok(Box::new(Check {
                cases: cases(),
                at: 0,
                output,
                completed: done,
                pending: Arc::new(Mutex::new(None)),
                state: Arc::new(Mutex::new(State::default())),
                results: Vec::new(),
                timeline: Vec::new(),
                started: std::time::Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(
        completed.load(Ordering::SeqCst),
        "rendering verification did not complete"
    );
}

struct Check {
    cases: Vec<Case>,
    at: usize,
    output: PathBuf,
    completed: Arc<AtomicBool>,
    pending: Arc<Mutex<Option<egui::ColorImage>>>,
    state: Arc<Mutex<State>>,
    results: Vec<serde_json::Value>,
    timeline: Vec<([u8; 4], [u8; 4])>,
    started: std::time::Instant,
}

fn interior(image: &egui::ColorImage, x: usize, y: usize, background: egui::Color32) -> bool {
    (-2isize..=2).all(|dy| {
        (-2isize..=2).all(|dx| {
            let index = (y as isize + dy) as usize * image.width() + (x as isize + dx) as usize;
            image.pixels[index] != background
        })
    })
}

impl eframe::App for Check {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        assert!(
            self.started.elapsed().as_secs() < 90,
            "rendering verification timed out"
        );
        let captured = self.pending.lock().unwrap().take();
        if let Some(gpu) = captured {
            let case = &self.cases[self.at];
            let cpu = render::styled_image(
                &case.frame.model,
                case.frame.camera,
                case.frame.scene,
                gpu.size,
                case.frame.seconds,
                case.frame.style,
            );
            for (name, image) in [("cpu", &cpu), ("gpu", &gpu)] {
                let rgba: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                fs::write(
                    self.output.join(format!("{}-{name}.png", case.name)),
                    export::png(&rgba, image.width(), image.height()).unwrap(),
                )
                .unwrap();
            }
            if case.name.starts_with("native-cubic-timeline-") {
                let index = gpu.height() / 2 * gpu.width() + gpu.width() / 2;
                self.timeline
                    .push((cpu.pixels[index].to_array(), gpu.pixels[index].to_array()));
            }
            // Material cases use a central square. Geometry cases compare coverage
            // throughout either renderer's interior, so missing parts cannot hide.
            let mut error = 0_u8;
            let mut expectation = 0_u8;
            let geometry = case.name.starts_with("skeletal-")
                || case.name.starts_with("packaged-particle-")
                || case.name.starts_with("tangent-")
                || case.name.starts_with("native-canvas-");
            let mut compared = 0;
            let mut coverage_mismatches = 0;
            let [r, g, b] = case.frame.scene.background;
            let background = egui::Color32::from_rgb(r, g, b);
            let x_range = if geometry {
                3..gpu.width() - 3
            } else {
                gpu.width() / 2 - 8..gpu.width() / 2 + 8
            };
            let y_range = if geometry {
                3..gpu.height() - 3
            } else {
                gpu.height() / 2 - 8..gpu.height() / 2 + 8
            };
            for y in y_range {
                for x in x_range.clone() {
                    let index = y * gpu.width() + x;
                    if geometry
                        && !interior(&cpu, x, y, background)
                        && !interior(&gpu, x, y, background)
                    {
                        continue;
                    }
                    if (cpu.pixels[index] == background) != (gpu.pixels[index] == background) {
                        coverage_mismatches += 1;
                    }
                    compared += 1;
                    for channel in 0..3 {
                        let a = cpu.pixels[index].to_array()[channel];
                        let b = gpu.pixels[index].to_array()[channel];
                        error = error.max(a.abs_diff(b));
                        if let Some(expected) = case.expected {
                            expectation = expectation
                                .max(a.abs_diff(expected[channel]))
                                .max(b.abs_diff(expected[channel]));
                        }
                    }
                }
            }
            let gear = case.name.starts_with("gear-");
            let blue = |image: &egui::ColorImage| {
                image
                    .pixels
                    .iter()
                    .filter(|p| p.b() > 60 && p.b() > p.r().saturating_mul(2))
                    .count()
            };
            let visible = |image: &egui::ColorImage| {
                image.pixels.iter().filter(|&&p| p != background).count()
            };
            let gear_passed = visible(&cpu) > 100
                && visible(&gpu) > 100
                && (!case.name.starts_with("gear-map-") || (blue(&cpu) > 10 && blue(&gpu) > 10));
            self.results
                .push(json!({"name":case.name,"max_channel_difference":error,
                "independent_expectation_error":expectation,"compared_pixels":compared,
                "coverage_mismatches":coverage_mismatches,
                "cpu_blue_pixels":blue(&cpu),"gpu_blue_pixels":blue(&gpu),
                "passed":if gear { gear_passed } else { error<=3 && expectation<=3 && compared>30 && coverage_mismatches==0 }}));
            self.at += 1;
        }
        if self.at == self.cases.len() {
            let timeline = self.timeline.len() == 6
                && self.timeline[0] != self.timeline[1]
                && self.timeline[1] != self.timeline[2]
                && self.timeline[3] == self.timeline[1]
                && self.timeline[4] == self.timeline[0]
                && self.timeline[5] == self.timeline[1];
            let passed = self.results.iter().all(|row| row["passed"] == true) && timeline;
            fs::write(self.output.join("rendering.json"),serde_json::to_vec_pretty(&json!({
                "cases":self.results.len(),"passed":self.results.iter().filter(|r|r["passed"]==true).count(),
                "scope":"Generated model fixtures through production CPU and OpenGL rendering",
                "gameplay_verified":false,"results":self.results,
                "timeline_verified":timeline,"timeline_centers":self.timeline})).unwrap()).unwrap();
            self.completed.store(passed, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let case = &self.cases[self.at];
        let frame = Frame {
            model: case.frame.model.clone(),
            camera: case.frame.camera,
            scene: case.frame.scene,
            style: case.frame.style,
            seconds: case.frame.seconds,
            pose: case.frame.pose.clone(),
        };
        let pending = self.pending.clone();
        let state = self.state.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 160.0), egui::Sense::hover());
            let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let gl = painter.gl();
                // SAFETY: the callback owns the live context. The readback buffer has exactly
                // four bytes per viewport pixel and is read before the next painter command.
                unsafe {
                    if !state.lock().unwrap().draw(gl, &info, &frame) {
                        return;
                    }
                    let viewport = info.viewport_in_pixels();
                    let (w, h) = (viewport.width_px as usize, viewport.height_px as usize);
                    let mut rgba = vec![0; w * h * 4];
                    gl.read_pixels(
                        viewport.left_px,
                        viewport.from_bottom_px,
                        viewport.width_px,
                        viewport.height_px,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    let mut flipped = vec![0; rgba.len()];
                    for y in 0..h {
                        flipped[y * w * 4..(y + 1) * w * 4]
                            .copy_from_slice(&rgba[(h - y - 1) * w * 4..(h - y) * w * 4]);
                    }
                    *pending.lock().unwrap() =
                        Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &flipped));
                }
            });
            ui.painter().add(egui::Shape::Callback(egui::PaintCallback {
                rect,
                callback: Arc::new(callback),
            }));
        });
        ctx.request_repaint();
    }
}
