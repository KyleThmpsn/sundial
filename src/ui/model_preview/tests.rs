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
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(820.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| preview.draw_assets(ui, &model));
            },
        );
        crate::test_support::capture::record(&output);
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
    crate::test_support::capture::write(&ctx, &output, "pending-audio-cancel");
    for pressed in [true, false] {
        let _ = frame(
            &mut preview,
            crate::test_support::primary_press(cancel, pressed),
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
