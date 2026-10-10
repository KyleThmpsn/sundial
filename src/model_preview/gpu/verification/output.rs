//! Package-to-display behavior prepared before output-stage implementation.
use super::*;

fn hdr(attenuated: bool) -> Model {
    let mut model = crate::model_preview::compatibility_tests::effects::hdr::case(10, false);
    if !attenuated {
        model.triangles.truncate(2);
        model.triangle_effects.truncate(2);
        model.triangle_constant.truncate(2);
    }
    model
}

pub(super) fn append(cases: &mut Vec<Case>) {
    for case in &mut *cases {
        case.frame.scene.filmic = false;
        case.frame.scene.bloom = false;
    }
    cases_with_output(cases);
    intensity_cases(cases);
    ambient_cases(cases);
}

fn ambient_cases(cases: &mut Vec<Case>) {
    for power in [0.0_f64, 0.5, 8.0] {
        for visibility in [0.0_f64, 64.0 / 255.0, 1.0] {
            for w in [0.0_f64, 0.3, 1.0] {
                for exponent in [0.0_f64, 0.25, 1.0, 2.0] {
                    ambient_case(cases, [power, visibility, w, exponent], 0);
                }
            }
        }
    }
    for mode in [1, 2, 3, 5, 6] {
        for power in [0.0_f64, 8.0] {
            ambient_case(cases, [power, 1.0, 1.0, 1.0], mode);
        }
    }
    ambient_timeline(cases);
}

fn ambient_timeline(cases: &mut Vec<Case>) {
    let mut shared = None;
    for (step, w) in [0.0_f64, 0.5, 1.0, 0.5, 0.0].into_iter().enumerate() {
        ambient_case(cases, [0.0, 1.0, w, 1.0], 4);
        let case = cases.last_mut().unwrap();
        case.name = format!("output-ambient-time-{step}");
        if let Some(model) = &shared {
            case.frame.model = Arc::clone(model);
        } else {
            shared = Some(case.frame.model.clone());
        }
    }
}

fn ambient_case(cases: &mut Vec<Case>, controls: [f64; 4], mode: u8) {
    let [power, visibility, w, exponent] = controls;
    let mut model = crate::model_preview::compatibility_tests::effects::opaque::ambient_case(
        power as f32,
        visibility as f32,
        w as f32,
        if mode == 3 { -1.0 } else { exponent as f32 },
        mode,
    );
    model.dyes[0] = Some(dye());
    model.triangle_dyes = vec![0; model.triangles.len()];
    let encoded = (((power + 1.0 / 128.0).log2() + 7.0) / 13.0).clamp(0.0, 1.0);
    let y = (((encoded + visibility) * 0.5).clamp(0.0, 1.0) * 255.0).round_ties_even() / 255.0;
    let packed_w = (w.clamp(0.0, 1.0) * 255.0).round_ties_even() / 255.0;
    let occlusion = if mode == 0 || mode == 4 {
        ((2.0 * y).clamp(0.0, 1.0) * packed_w)
            .powi(2)
            .max(0.0001)
            .powf(exponent)
    } else {
        0.0 // The fixture's ordinary gear mask supplies the previous ambient fallback.
    };
    let emission =
        2.0_f64.powf(13.0 * (2.0 * y - (1.0 + 2.0 / 255.0)).clamp(0.0, 1.0) - 7.0) - 1.0 / 128.0;
    // This head-on dielectric reflects 4% of incident light. The zero gear mask
    // suppresses its environment reflection, leaving 96% diffuse fill and full emission.
    let expected = [0.0, 64.0 / 255.0, 0.5].map(|v| reference(v * (0.96 * occlusion + emission)));
    cases.push(Case {
        name: format!("output-ambient-{power}-{visibility}-{w}-{exponent}-{mode}"),
        frame: Frame {
            animate: false,
            dyes: None,
            model: Arc::new(model),
            camera: Camera {
                yaw: 0.0,
                pitch: 0.0,
                zoom: 1.0,
                pan: [0.0; 2],
            },
            scene: Scene {
                light: [0.0, 0.0, 1.0],
                key: 0.0,
                fill: 1.0,
                bloom: false,
                background: [0; 3],
                ..Scene::unit_exposure()
            },
            style: Style::Textured,
            seconds: if mode == 4 { w as f32 } else { 0.0 },
            pose: None,
        },
        expected: Some(expected),
    });
}

fn cases_with_output(cases: &mut Vec<Case>) {
    let timeline: Vec<_> = cases
        .iter()
        .filter(|case| case.name.starts_with("native-cubic-timeline-"))
        .map(|case| Frame {
            animate: false,
            dyes: None,
            model: case.frame.model.clone(),
            camera: case.frame.camera,
            scene: Scene {
                background: [0; 3],
                ..Scene::unit_exposure()
            },
            style: Style::Textured,
            seconds: case.frame.seconds,
            pose: case.frame.pose.clone(),
        })
        .collect();
    let mut add = |name: &str, model: Model, scene: Scene, zoom| {
        cases.push(Case {
            name: format!("output-{name}"),
            frame: Frame {
                animate: false,
                dyes: None,
                model: Arc::new(model),
                camera: Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    zoom,
                    pan: [0.0; 2],
                },
                scene,
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected: None,
        });
    };
    for (name, attenuated, exposure) in [
        ("hdr", false, 1.0),
        ("attenuated", true, 1.0),
        ("exposure", false, 0.25),
        ("black", false, 0.0),
    ] {
        add(
            name,
            hdr(attenuated),
            Scene {
                bloom: false,
                filmic: true,
                exposure,
                background: [0; 3],
                ..Scene::unit_exposure()
            },
            3.0,
        );
    }
    add(
        "uniform-bloom",
        hdr(false),
        Scene {
            background: [0; 3],
            ..Scene::unit_exposure()
        },
        3.0,
    );
    for (step, zoom) in [
        ("halo", 0.35),
        ("halo-wide", 0.35),
        ("halo-small", 0.35),
        ("halo-rewind", 0.35),
    ] {
        add(
            step,
            hdr(false),
            Scene {
                background: [0; 3],
                ..Scene::unit_exposure()
            },
            zoom,
        );
    }
    add(
        "halo-attenuated",
        hdr(true),
        Scene {
            background: [0; 3],
            ..Scene::unit_exposure()
        },
        0.35,
    );
    add(
        "halo-off",
        hdr(false),
        Scene {
            bloom: false,
            background: [0; 3],
            ..Scene::unit_exposure()
        },
        0.35,
    );
    add("background", Model::default(), Scene::unit_exposure(), 1.0);
    let mut hidden = hdr(false);
    // A larger opaque front surface must hide the entire bright source, including its halo.
    quad(&mut hidden, -0.5, None, Some([0.0; 3]));
    for vertex in &mut hidden.vertices[8..] {
        vertex[0] *= 1.5;
        vertex[2] *= 1.5;
    }
    add(
        "hidden",
        hidden,
        Scene {
            background: [0; 3],
            ..Scene::unit_exposure()
        },
        0.35,
    );
    for (step, frame) in timeline.into_iter().enumerate() {
        cases.push(Case {
            name: format!("output-time-{step}"),
            frame,
            expected: None,
        });
    }
}

fn intensity_cases(cases: &mut Vec<Case>) {
    let parameters = [0.0, 0.125, 0.5, 2.0, 8.0]
        .into_iter()
        .flat_map(|power| {
            [0.0, 64.0 / 255.0, 1.0]
                .into_iter()
                .map(move |visibility| (power, visibility, false))
        })
        .chain([(8.0, 1.0, true)]);
    for (power, visibility, malformed) in parameters {
        let mut model = crate::model_preview::compatibility_tests::effects::opaque::intensity_case(
            power, visibility, malformed,
        );
        // A selected dye enables the fixture's zero ambient mask. Keep the studio's
        // reflection separate from the independently expected emission value.
        model.dyes[0] = Some(dye());
        model.triangle_dyes = vec![0; model.triangles.len()];
        cases.push(Case {
            name: if malformed {
                "output-unsupported-intensity".into()
            } else {
                format!("output-intensity-{power}-{visibility}")
            },
            frame: Frame {
                animate: false,
                dyes: None,
                model: Arc::new(model),
                camera: Camera {
                    yaw: 0.0,
                    pitch: 0.0,
                    zoom: 1.0,
                    pan: [0.0; 2],
                },
                scene: Scene {
                    light: [0.0, 0.0, 1.0],
                    key: 0.0,
                    fill: 0.0,
                    bloom: false,
                    background: [0; 3],
                    ..Scene::unit_exposure()
                },
                style: Style::Textured,
                seconds: 0.0,
                pose: None,
            },
            expected: None,
        });
    }
}

pub(super) fn size(name: &str) -> [f32; 2] {
    match name {
        "output-halo-wide" => [208.0, 144.0],
        "output-halo-small" => [96.0, 128.0],
        _ => [160.0, 160.0],
    }
}

fn reference(value: f64) -> u8 {
    // Independent target film curve, verified against unmodified original bytecode.
    let a = ((4.016 * value + 0.030) * 1.6 * value
        / ((3.888 * value + 0.590) * 1.6 * value + 0.140))
        .clamp(0.0, 1.0);
    let b = (value * (1.048747 * value + 3.134397)
        / (value * (0.990440 * value + 3.240450) + 0.651790))
        .clamp(0.0, 1.0);
    let linear = 0.4 * a + 0.6 * b;
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

fn statistics(
    case: &Case,
    cpu: &egui::ColorImage,
    gpu: &egui::ColorImage,
) -> (u8, [usize; 2], usize) {
    let mut error = 0;
    let mut halo = [0_usize; 2];
    let mut compared = 0;
    let [w, h] = cpu.size;
    let radius = (w.min(h) as f32 * render::RADIUS_SCALE * case.frame.camera.zoom / 2_f32.sqrt())
        .round() as usize;
    for index in 0..w * h {
        let (x, y) = (index % w, index / w);
        let edge = x
            .abs_diff(w / 2)
            .abs_diff(radius)
            .min(y.abs_diff(h / 2).abs_diff(radius));
        if edge < 5 {
            continue;
        }
        for channel in 0..3 {
            error = error.max(
                cpu.pixels[index].to_array()[channel]
                    .abs_diff(gpu.pixels[index].to_array()[channel]),
            );
        }
        compared += 1;
        if x.abs_diff(w / 2) > radius + 5 || y.abs_diff(h / 2) > radius + 5 {
            for (count, image) in halo.iter_mut().zip([cpu, gpu]) {
                if image.pixels[index].r() > 1 {
                    *count += 1;
                }
            }
        }
    }
    (error, halo, compared)
}

fn expected(case: &Case) -> Option<[u8; 3]> {
    if case.expected.is_some() {
        return case.expected;
    }
    let expected = if let Some(parameters) = case.name.strip_prefix("output-intensity-") {
        let values: Vec<f64> = parameters.split('-').map(|v| v.parse().unwrap()).collect();
        let packed = (((values[0] + 1.0 / 128.0).log2() + 7.0) / 13.0).clamp(0.0, 1.0) * 0.5
            + values[1] * 0.5;
        let y = (packed.clamp(0.0, 1.0) * 255.0).round() / 255.0;
        let decoded = 2.0_f64.powf(13.0 * (2.0 * y - (1.0 + 2.0 / 255.0)).clamp(0.0, 1.0) - 7.0)
            - 1.0 / 128.0;
        Some([0.0, 64.0 / 255.0, 0.5].map(|v| v * decoded))
    } else {
        match case.name.as_str() {
            "output-hdr" => Some([4.0, 0.25, 1.0]),
            "output-attenuated" => Some([0.5, 0.03125, 0.125]),
            "output-exposure" => Some([1.0, 0.0625, 0.25]),
            "output-black" | "output-hidden" | "output-unsupported-intensity" => Some([0.0; 3]),
            "output-uniform-bloom" => {
                let luminance = 4.0 * 0.3 + 0.25 * 0.59 + 0.11;
                let gain = (0.016 + luminance * 0.0005)
                    * (2.0_f64 * 0.05882 + 2.0 * 0.17647 + 0.52941).powi(2);
                Some([4.0, 0.25, 1.0].map(|v| v * (1.0 + gain)))
            }
            _ => None,
        }
    };
    expected.map(|rgb| rgb.map(reference))
}

fn expectation(expected: Option<[u8; 3]>, images: [&egui::ColorImage; 2]) -> u8 {
    let mut difference = 0;
    if let Some(expected) = expected {
        for image in images {
            let center = image.height() / 2 * image.width() + image.width() / 2;
            for (actual, expected) in image.pixels[center].to_array().into_iter().zip(expected) {
                difference = difference.max(actual.abs_diff(expected));
            }
        }
    }
    difference
}

fn halo_ok(case: &Case, halo: [usize; 2]) -> bool {
    let wants_halo = matches!(
        case.name.as_str(),
        "output-halo"
            | "output-halo-wide"
            | "output-halo-small"
            | "output-halo-rewind"
            | "output-halo-attenuated"
    );
    if wants_halo {
        halo.iter().all(|&n| n > 20)
    } else if matches!(
        case.name.as_str(),
        "output-hidden" | "output-halo-off" | "output-black"
    ) {
        halo == [0, 0]
    } else {
        true
    }
}

fn timeline(case: &Case, sums: &[u64], prior: &[serde_json::Value]) -> bool {
    match case.name.as_str() {
        "output-time-1" | "output-time-2" => prior
            .iter()
            .find(|row| row["name"] == "output-time-0")
            .is_some_and(|row| row["pixel_sums"] != json!(sums)),
        "output-time-3" | "output-time-5" => prior
            .iter()
            .find(|row| row["name"] == "output-time-1")
            .is_some_and(|row| row["pixel_sums"] == json!(sums)),
        "output-time-4" => prior
            .iter()
            .find(|row| row["name"] == "output-time-0")
            .is_some_and(|row| row["pixel_sums"] == json!(sums)),
        "output-ambient-time-1" | "output-ambient-time-2" => prior
            .iter()
            .find(|row| row["name"] == "output-ambient-time-0")
            .is_some_and(|row| row["pixel_sums"] != json!(sums)),
        "output-ambient-time-3" => prior
            .iter()
            .find(|row| row["name"] == "output-ambient-time-1")
            .is_some_and(|row| row["pixel_sums"] == json!(sums)),
        "output-ambient-time-4" => prior
            .iter()
            .find(|row| row["name"] == "output-ambient-time-0")
            .is_some_and(|row| row["pixel_sums"] == json!(sums)),
        _ => true,
    }
}

pub(super) fn result(
    case: &Case,
    cpu: &egui::ColorImage,
    gpu: &egui::ColorImage,
    prior: &[serde_json::Value],
) -> serde_json::Value {
    let (error, halo, compared) = statistics(case, cpu, gpu);
    let expected = expected(case);
    let expectation = expectation(expected, [cpu, gpu]);
    let halo_ok = halo_ok(case, halo);
    let background_ok = case.name != "output-background"
        || [cpu, gpu].into_iter().all(|image| {
            image
                .pixels
                .iter()
                .all(|p| p.to_array()[..3] == case.frame.scene.background)
        });
    let sums: Vec<_> = [cpu, gpu]
        .into_iter()
        .map(|image| {
            image
                .pixels
                .iter()
                .map(|p| p.to_array().into_iter().map(u64::from).sum::<u64>())
                .sum::<u64>()
        })
        .collect();
    let rewind_ok = case.name != "output-halo-rewind"
        || prior
            .iter()
            .find(|row| row["name"] == "output-halo")
            .is_some_and(|row| row["pixel_sums"] == json!(sums));
    let attenuation_ok = case.name != "output-halo-attenuated"
        || prior
            .iter()
            .find(|row| row["name"] == "output-halo")
            .is_some_and(|row| {
                sums.iter()
                    .enumerate()
                    .all(|(i, sum)| *sum < row["pixel_sums"][i].as_u64().unwrap())
            });
    let time_ok = timeline(case, &sums, prior);
    json!({"name":case.name,"max_channel_difference":error,"independent_expectation_error":expectation,"compared_pixels":compared,
        "halo_pixels":halo,"pixel_sums":sums,"expected_center":expected,"rewind_verified":rewind_ok,"attenuation_verified":attenuation_ok,
        "timeline_verified":time_ok,"notices":case.frame.model.notices,"passed":error <= 3 && expectation <= 2 && halo_ok && background_ok && rewind_ok && attenuation_ok && time_ok && compared > 30})
}
