use super::*;
use serde_json::json;

#[test]
fn large_unlock_tables_only_draw_the_viewport() {
    let catalog = Catalog::for_test(Vec::new(), Default::default()).with_test_progression(
        (0..23_000)
            .map(|index| UnlockDefinition {
                hash: index as u64,
                code: 1,
                compact_slot: Some(index as u16),
                name: Some(format!("Unlock {index:05}")),
                ..Default::default()
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    );
    let mut document = json!({});
    let mut state = UiState {
        read_only: true,
        ..Default::default()
    };
    let ctx = egui::Context::default();
    let mut render = |state: &mut UiState| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 760.0),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    draw(ui, &mut document, &UnlockPolicy::default(), &catalog, state);
                });
            },
        )
    };
    for _ in 0..128 {
        render(&mut state);
        if state.unlock_browser.filtered.is_some() {
            break;
        }
    }
    let started = std::time::Instant::now();
    for _ in 0..30 {
        let output = render(&mut state);
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Unlock 000")
        )), "the viewport must paint actual unlock rows");
        assert!(
            output.shapes.len() < 1000,
            "Only visible rows should be painted"
        );
    }
    eprintln!(
        "23,000 unlocks, frame average: {:?}",
        started.elapsed() / 30
    );
    state.query = "Unlock 22999".into();
    render(&mut state);
    assert_eq!(state.unlock_browser.filtered.as_ref().unwrap().1.len(), 1);
}
