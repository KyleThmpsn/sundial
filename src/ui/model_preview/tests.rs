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
