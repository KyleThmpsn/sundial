use super::*;

#[test]
fn pending_audio_can_be_canceled_without_playing_a_late_result() {
    let ctx = egui::Context::default();
    let (sender, receiver) = mpsc::channel();
    let mut preview = Preview {
        audio_pending: Some(receiver),
        audio_tag: Some(42),
        audio_status: Some("Decoding audio".into()),
        ..Default::default()
    };
    let mut model = Model::default();
    model.assets.sounds.push(model_preview::assets::Sound {
        tag: 10,
        name: Some("Fixture Sound".into()),
        notice: None,
        clips: vec![model_preview::assets::AudioClip {
            tag: 42,
            name: Some("Fixture Clip".into()),
            size: 44,
            codec: 1,
            channels: 1,
            sample_rate: 8_000,
        }],
    });
    let frame = |preview: &mut Preview, events| {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(820.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| preview.draw_assets(ui, &model));
            },
        );
        crate::app::tests::capture::record(&output);
        output
    };
    let _ = frame(&mut preview, Vec::new());
    let output = frame(&mut preview, Vec::new());
    let cancel = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == "Cancel" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("pending clips need an available cancel action");
    crate::app::tests::capture::write(&ctx, &output, "pending-audio-cancel");
    for pressed in [true, false] {
        let _ = frame(
            &mut preview,
            vec![
                egui::Event::PointerMoved(cancel),
                egui::Event::PointerButton {
                    pos: cancel,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert!(preview.audio_pending.is_none());
    assert!(preview.audio_tag.is_none());
    assert!(preview.audio_status.is_none());
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    let late = sender.send(Ok((file, std::time::Duration::from_secs(1))));
    assert!(late.is_err(), "a canceled decode has no playback receiver");
    drop(late);
    assert!(!path.exists());
    preview.poll_audio(&ctx);
    assert!(preview.playback.is_none());
    let (next, receiver) = mpsc::channel();
    preview.audio_pending = Some(receiver);
    preview.audio_tag = Some(43);
    next.send(Err("A later decode completed".into())).unwrap();
    preview.poll_audio(&ctx);
    assert_eq!(
        preview.audio_status.as_deref(),
        Some("A later decode completed")
    );
    crate::test_support::artifact(
        "audio-cancellation.json",
        &serde_json::json!({
            "late_result_discarded": true, "temporary_file_removed": true, "later_result_received": true,
        }),
    );
}

#[test]
fn comparison_keeps_the_camera_when_the_candidate_model_changes() {
    let target = |arrangement| {
        (
            PathBuf::from("packages"),
            Target::Weapon(
                Appearance {
                    arrangement,
                    dye_textures: Vec::new(),
                    dyes: vec![],
                },
                None,
            ),
        )
    };
    let (_, receiver) = mpsc::channel();
    let mut preview = Preview {
        selection: Some(target(1)),
        pending: Some((target(1), receiver)),
        preserve_camera: true,
        camera: Camera {
            yaw: 1.3,
            pitch: 0.4,
            zoom: 1.7,
            pan: [0.0, 0.0],
        },
        model: Some(Arc::new(Model::default())),
        ..Default::default()
    };
    preview.sync(&egui::Context::default(), target(2));
    assert!(preview.model.is_none());
    assert!(
        preview.camera
            == Camera {
                yaw: 1.3,
                pitch: 0.4,
                zoom: 1.7,
                pan: [0.0, 0.0]
            }
    );
}

#[test]
fn package_generation_change_discards_the_previous_model_and_pending_result() {
    let target = |generation| {
        (
            PathBuf::from("packages"),
            Target::Weapon(
                Appearance {
                    arrangement: 12,
                    dye_textures: Vec::new(),
                    dyes: vec![],
                },
                Some(generation),
            ),
        )
    };
    let (sender, receiver) = mpsc::channel();
    let mut preview = Preview {
        selection: Some(target(0)),
        pending: Some((target(0), receiver)),
        model: Some(Arc::new(Model::default())),
        camera: Camera {
            yaw: 1.2,
            ..Default::default()
        },
        ..Default::default()
    };
    let ctx = egui::Context::default();
    preview.sync(&ctx, target(1));
    assert!(preview.model.is_none());
    assert_eq!(preview.camera.yaw, 1.2);
    sender.send(Ok(Model::default())).unwrap();
    preview.sync(&ctx, target(1));
    assert!(
        preview.model.is_none(),
        "a result from the previous package generation is stale"
    );
    assert_eq!(preview.pending.as_ref().unwrap().0, target(1));
}

#[test]
#[ignore = "requires installed Shadowkeep packages"]
fn native_shader_playback_starts_and_pause_preserves_time() {
    let packages = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    // A native UV-animation dye in all three channels isolates the playback UI.
    let appearance = Appearance {
        arrangement: 930,
        dye_textures: Vec::new(),
        dyes: vec![(4, 8054), (5, 8054), (6, 8054)],
    };
    let model = model_preview::appearance::load_reported(
        &packages,
        &appearance,
        &crate::model_preview::Load::default(),
    )
    .unwrap();
    assert!(model.has_shader_animation());
    let selection = (packages, Target::Weapon(appearance, None));
    let (sender, receiver) = mpsc::channel();
    sender.send(Ok(model)).unwrap();
    let mut preview = Preview {
        selection: Some(selection.clone()),
        pending: Some((selection.clone(), receiver)),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    preview.sync(&ctx, selection);
    assert!(preview.playing);
    preview.playing = false;
    preview.seconds = 2.5;
    preview.last_tick = Some(std::time::Instant::now() - std::time::Duration::from_secs(5));
    let draw = |preview: &mut Preview| {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| preview.draw(ui, "Shader Test"));
        });
    };
    draw(&mut preview);
    assert_eq!(preview.seconds, 2.5);
    assert_eq!(preview.rendered_seconds, Some(2.5));
    preview.seconds = 0.0;
    draw(&mut preview);
    assert_eq!(preview.rendered_seconds, Some(0.0));
    let paused = preview.rendered.unwrap().2;
    preview.playing = true;
    draw(&mut preview);
    let playing = preview.rendered.unwrap().2;
    assert!(playing.into_iter().max().unwrap() <= 320);
    assert!(paused.into_iter().max().unwrap() > playing.into_iter().max().unwrap());
}

#[test]
fn shader_change_invalidates_image_and_preserves_view() {
    let ctx = egui::Context::default();
    let target = |dye| {
        (
            PathBuf::from("packages"),
            Target::Weapon(
                Appearance {
                    arrangement: 12,
                    dye_textures: Vec::new(),
                    dyes: vec![(4, dye)],
                },
                None,
            ),
        )
    };
    let (_, receiver) = mpsc::channel();
    let mut preview = Preview {
        selection: Some(target(1)),
        pending: Some((target(1), receiver)),
        camera: Camera {
            yaw: 1.2,
            pitch: 0.3,
            zoom: 2.0,
            pan: [0.0, 0.0],
        },
        model: Some(Arc::new(Model::default())),
        ..Default::default()
    };
    preview.sync(&ctx, target(2));
    assert!(preview.model.is_none());
    assert_eq!(preview.camera.yaw, 1.2);
    assert_eq!(preview.camera.zoom, 2.0);
    assert_eq!(preview.selection, Some(target(2)));
}

#[test]
fn switching_selection_discards_stale_geometry_without_starting_parallel_reads() {
    let ctx = egui::Context::default();
    let first = (PathBuf::from("first"), Target::Object(1));
    let next = (PathBuf::from("second"), Target::Object(2));
    let (sender, receiver) = mpsc::channel();
    let mut preview = Preview {
        selection: Some(first.clone()),
        pending: Some((first, receiver)),
        model: Some(Arc::new(Model::default())),
        playing: true,
        seconds: 2.0,
        last_tick: Some(std::time::Instant::now()),
        rendered_seconds: Some(2.0),
        ..Default::default()
    };
    preview.sync(&ctx, next.clone());
    assert!(preview.model.is_none());
    assert!(!preview.playing);
    assert_eq!(preview.seconds, 0.0);
    assert!(preview.last_tick.is_none());
    assert!(preview.rendered_seconds.is_none());
    assert_eq!(preview.pending.as_ref().unwrap().0.1, Target::Object(1));
    // A failure for the old selection must not become the new selection's error.
    sender.send(Err("old selection".into())).unwrap();
    preview.sync(&ctx, next.clone());
    assert!(preview.error.is_none());
    assert_eq!(preview.pending.as_ref().unwrap().0, next);
}

/// The viewer on Age-Old Bond, headless: captures of the loading and loaded screens under
/// `PARHELION_UI_CAPTURE_DIR`, and under `SUNDIAL_PROBE_OUT` a textured frame of the model with
/// the dye each slot resolved to. The weapon's cream body and its dark metal panels must both
/// reach the frame: its hologram shell in the transparent stage, drawn opaque in the body's
/// slot, once covered the panels and turned the whole gun cream.
#[test]
#[ignore = "requires SUNDIAL_PREVIEW_PACKAGES and SUNDIAL_PROBE_OUT, and builds a catalog"]
fn age_old_bond_viewer_draws_every_dye_slot() {
    let packages = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_PACKAGES").unwrap());
    let out = PathBuf::from(std::env::var_os("SUNDIAL_PROBE_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let loadout = catalog.preview_loadout(0x23DB_942F).expect("Age-Old Bond");
    let appearance = catalog.preview_appearance(&loadout);
    let mut report = format!(
        "arrangement {} dyes {:?}\n",
        appearance.arrangement, appearance.dyes
    );
    let selection: Selection = (packages.clone(), Target::Weapon(appearance, None));
    let mut preview = Preview {
        selection: Some(selection.clone()),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let frame = |preview: &mut Preview, name: Option<&str>| {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(820.0, 640.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    preview.sync(ctx, selection.clone());
                    preview.draw(ui, "Age-Old Bond");
                });
            },
        );
        crate::app::tests::capture::record(&output);
        if let Some(name) = name {
            crate::app::tests::capture::write(&ctx, &output, name);
        }
    };
    frame(&mut preview, Some("preview-loading"));
    let started = std::time::Instant::now();
    while preview.model.is_none() {
        assert!(
            started.elapsed().as_secs() < 180,
            "the model did not arrive: {:?}",
            preview.error
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        frame(&mut preview, None);
    }
    frame(&mut preview, None);
    frame(&mut preview, Some("preview-loaded"));
    let model = preview.model.clone().unwrap();
    let mut counts = [0usize; 6];
    for &slot in &model.triangle_dyes {
        if let Some(count) = counts.get_mut(usize::from(slot)) {
            *count += 1;
        }
    }
    for (slot, dye) in model.dyes.iter().enumerate() {
        report += &format!(
            "slot {slot}: {} triangles, {}\n",
            counts[slot],
            dye.as_ref().map_or("no dye".to_owned(), |dye| format!(
                "albedo {:?} worn {:?} iridescence {}",
                dye.surface.albedo, dye.surface.worn_albedo, dye.surface.iridescence
            ))
        );
    }
    let image = render::styled_image(
        &model,
        Camera::default(),
        Scene::default(),
        [720, 450],
        0.0,
        render::Style::Textured,
    );
    let mut ppm = format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
    for pixel in &image.pixels {
        ppm.extend([pixel.r(), pixel.g(), pixel.b()]);
    }
    std::fs::write(out.join("age-old-bond.ppm"), ppm).unwrap();
    // Each channel in a flat colour of its own, the body's cream red and its panels' dark
    // metal green, written where a build writes a dye's intact and worn albedos. The panels
    // must reach the frame, so a shell drawn over the body cannot pass.
    let overrides: Vec<crate::model_preview::SurfaceOverride> = (0..3)
        .map(|channel| crate::model_preview::SurfaceOverride {
            slot: channel * 2,
            writes: [9, 13, 17, 21]
                .into_iter()
                .flat_map(|vector| {
                    (0..3).map(move |lane| (vector, lane, f32::from(u8::from(lane == channel))))
                })
                .collect(),
        })
        .collect();
    model.set_surface_overrides(&overrides);
    let flat = render::styled_image(
        &model,
        Camera::default(),
        Scene::default(),
        [720, 450],
        0.0,
        render::Style::Textured,
    );
    model.set_surface_overrides(&[]);
    let mut ppm = format!("P6\n{} {}\n255\n", flat.size[0], flat.size[1]).into_bytes();
    for pixel in &flat.pixels {
        ppm.extend([pixel.r(), pixel.g(), pixel.b()]);
    }
    std::fs::write(out.join("age-old-bond-channels.ppm"), ppm).unwrap();
    let [red, green, blue] = Scene::default().background;
    let background = egui::Color32::from_rgb(red, green, blue);
    let mut drawn = 0usize;
    let mut hues = [0usize; 3];
    for pixel in flat.pixels.iter().filter(|pixel| **pixel != background) {
        drawn += 1;
        let rgb = [pixel.r(), pixel.g(), pixel.b()];
        let lead = (0..3).max_by_key(|&i| rgb[i]).unwrap();
        let peak = u32::from(rgb[lead]);
        if peak > 40 && (0..3).all(|i| i == lead || peak * 3 >= u32::from(rgb[i]) * 4) {
            hues[lead] += 1;
        }
    }
    report += &format!(
        "drawn {drawn} pixels, red {} green {} blue {}\n",
        hues[0], hues[1], hues[2]
    );
    std::fs::write(out.join("age-old-bond-slots.txt"), &report).unwrap();
    eprintln!("{report}");
    assert!(drawn > 0, "the model must reach the frame:\n{report}");
    assert!(
        hues[0] >= drawn / 20 && hues[1] >= drawn / 20,
        "the body and its panels must both show:\n{report}"
    );
}
