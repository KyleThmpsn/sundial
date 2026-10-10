//! A rendered viewer obeys its source window's FPS option and does not count cached images.
use super::*;

#[test]
fn viewer_fps_is_minimal_optional_and_counts_completed_model_frames() {
    let mut model = Model::default();
    model.vertices = vec![[-1.0, 0.0, -1.0], [1.0, 0.0, -1.0], [0.0, 0.0, 1.0]];
    model.triangles = vec![[0, 1, 2]];
    let model = Arc::new(model);
    let ctx = egui::Context::default();
    let mut preview = Preview {
        model: Some(model.clone()),
        source_viewport: Some(egui::ViewportId::ROOT),
        ..Default::default()
    };
    let mut now = 0.0;
    let mut frame = |preview: &mut Preview| {
        now += 0.1;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320.0, 240.0),
                )),
                time: Some(now),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| preview.draw_viewport(ui, &model));
            },
        );
        crate::test_support::capture::record(&output);
        output
    };
    let labels = |output: &egui::FullOutput| {
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
    let started = std::time::Instant::now();
    while preview.texture.is_none() {
        assert!(labels(&frame(&mut preview)).is_empty());
        assert!(started.elapsed().as_secs() < 10, "the model never rendered");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    set_options(
        &ctx,
        Options {
            show_fps: true,
            ..Default::default()
        },
    );
    let mut output = frame(&mut preview);
    for _ in 0..20 {
        output = frame(&mut preview);
    }
    let shown = labels(&output);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        shown[0].galley.text(),
        "0 FPS",
        "cached software images are not new renders"
    );
    assert!(shown[0].pos.x < 32.0 && shown[0].pos.y < 32.0);
    assert!(shown[0].galley.size().y <= 16.0);
    crate::test_support::capture::write(&ctx, &output, "viewer-fps-enabled");
    preview.source_viewport = Some(egui::ViewportId::from_hash_of("other-source"));
    assert!(
        labels(&frame(&mut preview)).is_empty(),
        "another source inherited the root preference"
    );
    preview.source_viewport = Some(egui::ViewportId::ROOT);
    set_options(&ctx, Options::default());
    let output = frame(&mut preview);
    assert!(labels(&output).is_empty());
    crate::test_support::capture::write(&ctx, &output, "viewer-fps-disabled");
    crate::test_support::artifact(
        "viewer-fps.json",
        &serde_json::json!({
            "default_hidden": true,
            "source_preference_respected": true,
            "cached_software_rate": 0,
            "frames_driven_after_enable": 21,
        }),
    );
}
