use super::*;
use crate::app::item_editor;
use eframe::egui;

fn tooltip_text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .filter(|text| {
            ["TEST BARREL", "Improves handling.", "Handling", "+7"].contains(&text.as_str())
        })
        .collect()
}

#[test]
fn plug_picker_tooltips_follow_popup_ownership_and_keep_their_stats() {
    let catalog = Catalog::for_test_plug_tooltip();
    let snapshot = PlugPickerSnapshot {
        preview: None,
        preview_guarded: false,
        socket_index: 0,
        socket_label: "Barrel".into(),
        current_hash: None,
        custom_current: false,
        current_label: "None".into(),
        native_default: None,
        native_default_label: None,
        choices: vec![PlugChoice {
            hash: 42,
            label: "Test Barrel".into(),
            type_name: "Barrel".into(),
        }],
        show_types: true,
        mode: Default::default(),
        scope_labels: vec![],
    };
    let ctx = egui::Context::default();
    let mut query = String::new();
    let mut frame = |events: Vec<egui::Event>, unrelated: bool| {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 760.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let button = ui.button("Choose Plug");
                    item_editor::draw_plug_icon_picker(
                        ui,
                        &catalog,
                        "tooltip-picker",
                        &mut query,
                        &snapshot,
                        PickerHeight {
                            min: 160.,
                            max: 300.,
                        },
                        &button,
                    );
                    let row = ui.button("Outside Plug");
                    item_editor::catalog_item_tooltip(row, &catalog, 42);
                    if unrelated {
                        let id = egui::Id::new("unrelated-menu");
                        egui::Popup::open_id(ui.ctx(), id);
                        crate::ui::dropdown(&button, id).show(|ui| {
                            ui.label("Unrelated Menu");
                        });
                    }
                });
            },
        )
    };
    frame(vec![], false);
    // The button's rectangle is stable, so retrieve it from the painted label for real clicks.
    let output = frame(vec![], false);
    let at = crate::test_support::text_position(&output, "Choose Plug").unwrap();
    frame(crate::test_support::primary_press(at, true), false);
    frame(crate::test_support::primary_press(at, false), false);
    let output = frame(vec![], false);
    let row = crate::test_support::text_position(&output, "Test Barrel").unwrap();
    frame(vec![egui::Event::PointerMoved(row)], false);
    let output = frame(vec![], false);
    let text = tooltip_text(&output);
    for expected in ["TEST BARREL", "Improves handling.", "Handling", "+7"] {
        assert!(
            text.iter().any(|text| text == expected),
            "missing {expected}: {text:?}"
        );
    }
    crate::test_support::capture::write(&ctx, &output, "plug-picker-tooltip");
    for pressed in [true, false] {
        frame(
            vec![egui::Event::PointerButton {
                pos: row,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
        );
    }
    let output = frame(vec![], false);
    assert!(
        tooltip_text(&output).is_empty(),
        "context menu must suppress the row tooltip"
    );
    egui::Popup::close_all(&ctx);
    let output = frame(vec![], false);
    let at = crate::test_support::text_position(&output, "Outside Plug").unwrap();
    frame(vec![egui::Event::PointerMoved(at)], false);
    let output = frame(vec![], false);
    assert!(
        tooltip_text(&output)
            .iter()
            .any(|text| text == "Improves handling.")
    );
    frame(vec![], true);
    let output = frame(vec![], true);
    assert!(
        tooltip_text(&output).is_empty(),
        "an unrelated menu must suppress outside hover"
    );
}
