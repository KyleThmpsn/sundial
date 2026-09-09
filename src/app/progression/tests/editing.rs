use super::*;

fn frame(
    ctx: &egui::Context,
    state: &mut UiState,
    document: &mut Value,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let catalog = Catalog::for_test(Vec::new(), HashMap::new());
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_content(ui, document, &catalog, None, state, View::Unlocks);
            });
        },
    )
}

fn button_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {label}"))
}

#[test]
fn progression_read_only_closes_add_dialog_and_blocks_undo_until_enabled() {
    let mut document = json!({"state": {"unlocks": {
        "account_progressions": [[23, 9, 8, 7]]
    }}});
    let before = document.clone();
    let mut state = UiState {
        read_only: true,
        unlock_table: UnlockTable::AccountProgressions,
        add_open: true,
        edit_progression_lanes: true,
        ..Default::default()
    };
    state.record_progression_change("account_progressions", 23, Some([1, 2, 3]), Some([9, 8, 7]));
    let ctx = egui::Context::default();
    for read_only in [true, false] {
        state.read_only = read_only;
        let output = frame(&ctx, &mut state, &mut document, vec![]);
        assert!(!state.add_open);
        assert!(!state.edit_progression_lanes);
        let pos = button_position(&output, "Undo Progression Change");
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut state,
                &mut document,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        if read_only {
            assert_eq!(document, before);
        } else {
            assert_eq!(
                saved_progression_lanes(&document, ProgressionScope::Account, 23),
                Some([1, 2, 3])
            );
        }
    }
}
