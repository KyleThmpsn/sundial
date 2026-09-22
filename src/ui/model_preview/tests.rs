use super::*;

#[test]
fn comparison_keeps_the_camera_when_the_candidate_model_changes() {
    let target = |arrangement| {
        (
            PathBuf::from("packages"),
            Target::Weapon(
                Appearance {
                    arrangement,
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
                zoom: 1.7
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
        dyes: vec![(4, 8054), (5, 8054), (6, 8054)],
    };
    let model = model_preview::weapon::load(&packages, &appearance).unwrap();
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
    let paused = preview.rendered.unwrap().1;
    preview.playing = true;
    draw(&mut preview);
    let playing = preview.rendered.unwrap().1;
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
