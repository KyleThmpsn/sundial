//! The ordinary shader page's software preview, driven through its real load, render and pause
//! path. Failure boundaries are a missing program, a clock stuck at zero, stale image caching,
//! unintended motion while paused and an unreadable frame artifact.
use super::*;
mod armor;

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn workbench_controls_respond_during_model_loading_and_rendering() {
    assert!(
        !model_preview::gpu::available(),
        "Run the software path in its own process"
    );
    let packages = crate::test_support::preview_packages();
    let output = crate::test_support::artifact_dir("effects");
    std::fs::create_dir_all(&output).unwrap();
    let ctx = egui::Context::default();
    let id = egui::Id::new("responsive-native-preview");
    let edit = egui::Id::new("editable-during-preview");
    let appearance = Appearance {
        arrangement: 900,
        dyes: vec![(4, 3702), (5, 3703), (6, 3704)],
        dye_textures: vec![],
    };
    let started = std::time::Instant::now();
    let mut typed = String::new();
    let mut frames = Vec::new();
    let mut captured = false;
    let mut events = 0;
    while !captured {
        ctx.memory_mut(|m| m.request_focus(edit));
        let inject = events < 24;
        let before = std::time::Instant::now();
        let frame = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(820.0, 720.0),
                )),
                events: if inject {
                    vec![egui::Event::Text("x".into())]
                } else {
                    vec![]
                },
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut typed).id(edit));
                    show(
                        ui,
                        id,
                        &packages,
                        Some(appearance.clone()),
                        &[],
                        egui::vec2(768.0, 576.0),
                    );
                    assert!(
                        ui.is_enabled(),
                        "The preview disabled independent authoring controls"
                    );
                    let _ = ui.button("Save Recipe");
                });
            },
        );
        if inject {
            events += 1;
        }
        let elapsed = before.elapsed().as_secs_f64() * 1000.0;
        frames.push(elapsed);
        assert!(elapsed < 250.0, "UI frame blocked for {elapsed:.1} ms");
        let state = ctx.data(|d| d.get_temp::<Arc<Mutex<Still>>>(id)).unwrap();
        let still = state.lock().unwrap();
        assert!(
            still.error.is_none(),
            "{:?}",
            still.error.as_ref().map(|e| &e.1)
        );
        if let Some(texture) = still.texture.as_ref() {
            for (id, delta) in frame
                .textures_delta
                .set
                .iter()
                .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
            {
                if *id == texture.id() {
                    let egui::ImageData::Color(image) = &delta.image;
                    let rgba = image
                        .pixels
                        .iter()
                        .flat_map(|p| p.to_array())
                        .collect::<Vec<_>>();
                    std::fs::write(
                        output.join("responsive-preview.png"),
                        crate::model_preview::export::png(&rgba, image.width(), image.height())
                            .unwrap(),
                    )
                    .unwrap();
                    captured = true;
                }
            }
        }
        drop(still);
        assert!(started.elapsed().as_secs() < 120, "Preview did not finish");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    assert!(typed.len() >= 20, "Editing was interrupted: {typed:?}");
    std::fs::write(
        output.join("responsive-preview.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"typed":typed,"frame_ms":frames,"completed":true}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn abandoned_preview_results_do_not_replace_the_current_selection() {
    let appearance = |arrangement| {
        Subject::Appearance(Appearance {
            arrangement,
            dyes: vec![],
            dye_textures: vec![],
        })
    };
    let old = (PathBuf::from("temporary-packages"), appearance(1), None);
    let current = (PathBuf::from("temporary-packages"), appearance(2), None);
    let (sender, receiver) = mpsc::channel();
    let mut still = Still {
        wanted: Some(current.clone()),
        pending: Some((old, receiver)),
        ..Default::default()
    };
    sender.send(Ok(Model::default())).unwrap();
    still.receive();
    assert!(still.model.is_none());
    assert!(still.shown.is_none());
    assert!(matches!(
        still.wanted.as_ref().map(|wanted| &wanted.1),
        Some(Subject::Appearance(Appearance { arrangement: 2, .. }))
    ));
}

#[test]
#[ignore = "Requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
#[allow(clippy::cognitive_complexity)]
fn shader_texture_motion_reaches_the_software_page_preview() {
    assert!(
        !model_preview::gpu::available(),
        "Run this headless workflow without a GPU renderer"
    );
    let packages = crate::test_support::preview_packages();
    let output = crate::test_support::artifact_dir("preview-animation");
    std::fs::create_dir_all(&output).unwrap();
    let appearance = Appearance {
        arrangement: 930,
        dye_textures: Vec::new(),
        dyes: vec![(4, 8054), (5, 8054), (6, 8054)],
    };
    let ctx = egui::Context::default();
    let id = egui::Id::new("shader-texture-motion");
    ctx.enable_accesskit();
    let images = std::cell::RefCell::new(std::collections::HashMap::new());
    let frame = |events| {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let preview = show(
                        ui,
                        id,
                        &packages,
                        Some(appearance.clone()),
                        &[],
                        egui::vec2(320.0, 320.0),
                    );
                    super::super::pop_out(
                        ui,
                        id,
                        preview.rect,
                        &packages,
                        (appearance.clone(), "Shader Playback"),
                    );
                });
            },
        );
        for (id, delta) in output
            .textures_delta
            .set
            .iter()
            .flat_map(|(id, deltas)| deltas.iter().map(move |delta| (id, delta)))
        {
            if delta.pos.is_none() {
                let egui::ImageData::Color(image) = &delta.image;
                images.borrow_mut().insert(*id, image.clone());
            }
        }
        crate::test_support::capture::record(&output);
        output
    };
    let draw = || frame(Vec::new());
    let started = std::time::Instant::now();
    let state = loop {
        let _ = draw();
        let state = ctx
            .data_mut(|data| data.get_temp::<Arc<Mutex<Still>>>(id))
            .unwrap();
        {
            let still = state.lock().unwrap();
            assert!(
                still.error.is_none(),
                "{:?}",
                still.error.as_ref().map(|e| &e.1)
            );
            if let (Some(model), Some(_)) = (&still.model, &still.texture) {
                assert!(model.has_shader_animation());
                break state.clone();
            }
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "Preview did not load"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let image = |frame: &egui::FullOutput| {
        let texture = state.lock().unwrap().texture.as_ref().unwrap().id();
        frame
            .textures_delta
            .set
            .iter()
            .find_map(|(id, deltas)| {
                if *id != texture {
                    return None;
                }
                deltas.first().map(|delta| {
                    let egui::ImageData::Color(image) = &delta.image;
                    image.clone()
                })
            })
            .or_else(|| images.borrow().get(&texture).cloned())
    };
    // Rendering now runs in the background. Drive real UI frames until the requested
    // instant is published, ignoring an older animated frame that was already in flight.
    let capture = |seconds: f32| {
        let started = std::time::Instant::now();
        loop {
            let frame = draw();
            let rendered = state.lock().unwrap().rendered_seconds;
            if rendered.is_some_and(|time| time >= seconds && (seconds != 0.0 || time == 0.0))
                && let Some(image) = image(&frame)
            {
                break image;
            }
            assert!(
                started.elapsed().as_secs() < 30,
                "The background preview did not publish a frame"
            );
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    };
    let save = |name: &str, image: &egui::ColorImage| {
        let rgba = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect::<Vec<_>>();
        let path = output.join(name);
        let png = crate::model_preview::export::png(&rgba, image.width(), image.height()).unwrap();
        std::fs::write(&path, png).unwrap();
        let decoded = eframe::icon_data::from_png_bytes(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(decoded.rgba, rgba);
    };
    pause(&ctx, true);
    {
        let mut still = state.lock().unwrap();
        still.seconds = 0.0;
        still.rendered_seconds = None;
    }
    let first = capture(0.0);
    save("shader-motion-0.png", &first);
    pause(&ctx, false);
    let later = [1.137, 7.5, 23.719]
        .into_iter()
        .find_map(|seconds| {
            {
                let mut still = state.lock().unwrap();
                still.seconds = seconds;
                still.last_tick = Some(std::time::Instant::now());
            }
            let later = capture(seconds);
            (later.pixels != first.pixels).then_some(later)
        })
        .expect("Source UV motion must change the page preview's pixels");
    save("shader-motion-later.png", &later);
    pause(&ctx, true);
    let paused_at = state.lock().unwrap().seconds;
    let _ = draw();
    {
        state.lock().unwrap().last_tick =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
    }
    let _ = draw();
    assert_eq!(state.lock().unwrap().seconds, paused_at);

    // The preference and actual transport use the same clock for every animation channel.
    set_options(
        &ctx,
        Options {
            play_animations: false,
            ..Default::default()
        },
    );
    pause(&ctx, false);
    let _ = frame(vec![egui::Event::PointerMoved(egui::pos2(160.0, 160.0))]);
    let held_at = state.lock().unwrap().seconds;
    std::thread::sleep(std::time::Duration::from_millis(60));
    let output_frame = draw();
    assert_eq!(state.lock().unwrap().seconds, held_at);
    let fps_labels = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with(" FPS") => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(fps_labels(&output_frame).is_empty());
    crate::test_support::capture::write(&ctx, &output_frame, "inline-animation-default-off");
    let click = |label: &str| {
        let output = draw();
        let tree = output.platform_output.accesskit_update.as_ref().unwrap();
        let bounds = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(label))
                    .then(|| node.bounds())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("missing {label} transport"));
        let at = egui::pos2(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            ((bounds.y0 + bounds.y1) * 0.5) as f32,
        );
        for pressed in [true, false] {
            frame(crate::test_support::primary_press(at, pressed));
        }
    };
    click("Play");
    draw();
    std::thread::sleep(std::time::Duration::from_millis(60));
    draw();
    assert!(
        state.lock().unwrap().seconds > held_at,
        "Play did not advance the model"
    );
    click("Pause");
    let held_at = state.lock().unwrap().seconds;
    draw();
    std::thread::sleep(std::time::Duration::from_millis(60));
    draw();
    assert_eq!(
        state.lock().unwrap().seconds,
        held_at,
        "Pause did not hold the model"
    );
    // A queued software image settles at the held instant and stays byte-for-byte still.
    let frozen = capture(held_at);
    save("shader-motion-paused.png", &frozen);
    let rendered_at = state.lock().unwrap().rendered_seconds;
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let output = draw();
        if let Some(image) = image(&output) {
            assert_eq!(image.pixels, frozen.pixels);
        }
    }
    assert_eq!(state.lock().unwrap().rendered_seconds, rendered_at);
    set_options(
        &ctx,
        Options {
            show_fps: true,
            play_animations: false,
        },
    );
    let output_frame = draw();
    let labels = fps_labels(&output_frame);
    assert_eq!(labels.len(), 1, "the counter must be one small label");
    assert!(labels[0].pos.x < 90.0 && labels[0].pos.y < 32.0);
    assert!(labels[0].galley.size().y <= 16.0);
    crate::test_support::capture::write(&ctx, &output_frame, "inline-animation-paused-fps");
    click("Play");
    draw();
    let resumed_at = state.lock().unwrap().seconds;
    assert!(
        resumed_at - held_at < 0.15,
        "resume included the paused interval"
    );
    std::thread::sleep(std::time::Duration::from_millis(60));
    draw();
    assert!(state.lock().unwrap().seconds > resumed_at);
    set_options(&ctx, Options::default());
    assert!(fps_labels(&draw()).is_empty());
    std::fs::write(output.join("shader-motion-readback.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "program_loaded": true, "visible_texture_motion": true, "pause_preserved_time": true,
            "default_off_holds_time": true, "play_pause_resume_clicked": true, "fps_opt_in": true,
            "paused": "shader-motion-paused.png",
            "first": "shader-motion-0.png", "later": "shader-motion-later.png",
            "different_pixels": first.pixels.iter().zip(&later.pixels).filter(|(a,b)| a != b).count(),
        })).unwrap()).unwrap();
}
