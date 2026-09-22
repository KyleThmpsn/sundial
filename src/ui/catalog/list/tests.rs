use super::*;
#[test]
fn filling_a_reserved_toolbar_status_does_not_rewind_the_body_cursor() {
    for width in [320.0, 1050.0] {
        let ctx = egui::Context::default();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut status = egui::Rect::NOTHING;
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Search Behaviors");
                        status = ui.allocate_space(egui::vec2(100.0, 20.0)).1;
                        ui.label("A filter that can wrap onto the next line");
                    });
                    let divider = ui.separator().rect;
                    let cursor = ui.cursor();
                    let response = super::super::toolbar_status(ui, status, "67 Results");
                    assert_eq!(ui.cursor(), cursor, "status must not move the body");
                    assert!(status.contains_rect(response.rect));
                    let row = ui.label("First Result").rect;
                    assert!(row.top() >= divider.bottom());
                });
            },
        );
    }
}

#[test]
fn empty_search_preserves_the_list_height_and_never_opens_a_detail() {
    for width in [620.0, 1050.0] {
        let ctx = egui::Context::default();
        for keys in [&[1_u64, 2][..], &[][..], &[2][..]] {
            let mut extent = 0.0;
            for _ in 0..3 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 900.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let top = ui.cursor().top();
                            BrowserList {
                                keys,
                                height: 650.0,
                                reset: true,
                                row_height: 30.0,
                                select: None,
                            }
                            .draw_body(
                                ui,
                                |ui, index, selected| {
                                    ui.selectable_label(selected, format!("Choice {}", keys[index]))
                                },
                                |ui, index| {
                                    ui.label(format!("Detail {}", keys[index]));
                                    None::<u32>
                                },
                            );
                            extent = ui.cursor().top() - top;
                        });
                    },
                );
            }
            assert!(
                (650.0..685.0).contains(&extent),
                "width {width}, keys {keys:?}: {extent}"
            );
        }
    }
}

#[test]
fn alternating_rows_follow_result_indices_after_keyboard_scrolling() {
    let ctx = egui::Context::default();
    let keys = (0..200).collect::<Vec<u64>>();
    let mut visible = Vec::new();
    let mut stripe_color = egui::Color32::TRANSPARENT;
    let mut inspected = 0;
    let mut output = egui::FullOutput::default();
    for step in 0..31 {
        visible.clear();
        output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1050.0, 500.0),
                )),
                events: if step == 0 {
                    vec![]
                } else {
                    vec![egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        physical_key: None,
                        pressed: true,
                        repeat: true,
                        modifiers: egui::Modifiers::NONE,
                    }]
                },
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    stripe_color = ui.visuals().faint_bg_color;
                    BrowserList {
                        keys: &keys,
                        height: 300.0,
                        reset: false,
                        row_height: 48.0,
                        select: None,
                    }
                    .draw_body(
                        ui,
                        |ui, index, _| {
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 48.0),
                                egui::Sense::click(),
                            );
                            visible.push((index, rect));
                            response
                        },
                        |_, index| {
                            inspected = index;
                            None::<()>
                        },
                    );
                });
            },
        );
    }
    assert_eq!(inspected, 30);
    assert!(visible[0].0 > 0, "the list must have scrolled");
    assert!(visible.len() < 20, "rows must remain virtualized");
    for (index, rect) in visible {
        let striped = output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(background) if background.rect == rect && background.fill == stripe_color
        ));
        assert_eq!(striped, index % 2 == 1, "result {index}");
    }
}

#[test]
fn inspecting_filtering_and_confirming_are_separate_and_keep_stable_identity() {
    for width in [620.0, 1050.0] {
        let ctx = egui::Context::default();
        let mut keys = vec![10, 20, 30];
        let mut inspected = 0;
        let mut results = Vec::new();
        let mut frame = |keys: &[u64], events, reset| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let value = BrowserList {
                            keys,
                            height: 650.0,
                            reset,
                            row_height: 30.0,
                            select: None,
                        }
                        .draw(
                            ui,
                            |ui, index, selected| {
                                ui.selectable_label(selected, format!("Choice {}", keys[index]))
                            },
                            |ui, index| {
                                inspected = keys[index];
                                ui.label(format!("Preview {}", keys[index]));
                                ui.button("Use Choice").clicked().then_some(keys[index])
                            },
                        );
                        if let Some(value) = value {
                            results.push(value);
                        }
                    });
                },
            )
        };
        let mut output = frame(&keys, vec![], false);
        for _ in 0..2 {
            output = frame(&keys, vec![], false);
        }
        let click = |position: egui::Pos2, pressed| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let position = label(&output, "Choice 20").center();
        frame(&keys, click(position, true), false);
        output = frame(&keys, click(position, false), false);
        label(&output, "Preview 20");
        keys = vec![30, 20];
        output = frame(&keys, vec![], true);
        label(&output, "Preview 20");
        let position = label(&output, "Use Choice").center();
        frame(&keys, click(position, true), false);
        frame(&keys, click(position, false), false);
        assert_eq!(inspected, 20);
        assert_eq!(results, vec![20]);
    }
}

#[test]
fn arrow_keys_browse_without_applying_the_preview() {
    let ctx = egui::Context::default();
    let keys = [10, 20, 30];
    let mut inspected = 0;
    let mut frame = |events| {
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(
                        BrowserList {
                            keys: &keys,
                            height: 300.0,
                            reset: false,
                            row_height: 30.0,
                            select: None,
                        }
                        .draw(
                            ui,
                            |ui, index, selected| ui
                                .selectable_label(selected, keys[index].to_string()),
                            |_, index| {
                                inspected = keys[index];
                                None::<u64>
                            }
                        )
                        .is_none()
                    );
                });
            },
        );
    };
    frame(vec![]);
    frame(vec![egui::Event::Key {
        key: egui::Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert_eq!(inspected, 20);
}
fn label(output: &egui::FullOutput, name: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {name}"))
}
