use super::*;

#[test]
fn progression_rows_stack_vertically_and_keep_name_columns_aligned() {
    let catalog = Catalog::for_test(Vec::new(),Default::default()).with_test_progression(
        (0..3).map(|index| UnlockDefinition {hash:100+index as u64,name:Some(format!("Entry {index}")),code:1,compact_slot:Some(index),..Default::default()}).collect(),
        Vec::new(),
        (0..3).map(|index|serde_json::from_value(json!({"definition_index":index,"hash":200+index,"name":format!("Entry {index} Rank"),"scope":"Account","scope_slot":index,"repeat_last_step":false})).unwrap()).collect(),
    ).with_test_records((0..3).map(|index|crate::catalog::RecordDefinition {index,hash:300+index as u64,name:format!("Entry {index} Triumph"),completion_flag:Some(index as u16),..Default::default()}).collect());
    for width in [640.0, 1100.0] {
        for dark in [false, true] {
            for mode in ["ranks", "saved", "triumphs"] {
                let mut document = json!({"state":{"unlocks":{"account_flag_runs":[[0,3]],"account_progressions":[[0,1,2,3],[1,4,5,6],[2,7,8,9]]}}});
                let mut state = UiState::default();
                state.unlock_browser.tab = if mode == "ranks" {
                    unlocks::Tab::Ranks
                } else {
                    unlocks::Tab::Storage
                };
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 760.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                assert!(!draw_content(
                                    ui,
                                    &mut document,
                                    &catalog,
                                    None,
                                    &mut state,
                                    if mode == "triumphs" {
                                        View::Triumphs
                                    } else {
                                        View::Unlocks
                                    }
                                ));
                            });
                        },
                    );
                }
                let positions = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text.starts_with("Entry ") => {
                            Some(text.pos)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(
                    positions.len() >= 3,
                    "{mode}: all three entries must be visible"
                );
                for pair in positions.windows(2) {
                    assert!(
                        pair[1].y > pair[0].y + 15.0,
                        "{mode}: rows must stack vertically: {pair:?}"
                    );
                    assert!(
                        (pair[1].x - pair[0].x).abs() < 1.0,
                        "{mode}: names must share a column: {pair:?}"
                    );
                }
                crate::app::tests::capture::write(
                    &ctx,
                    &output,
                    &format!("rows-{mode}-{dark}-{width}"),
                );
            }
        }
    }
}
