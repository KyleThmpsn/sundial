use crate::app::tests::driver::*;
use crate::app::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn frame(
    app: &mut SundialApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    draw: bool,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            if draw {
                app.draw_app_chrome(ui, None);
                app.draw_active_view(ui);
            }
            app.handle_workspace_shortcuts(ui);
        },
    )
}

fn edit(app: &mut SundialApp, value: u32) {
    app.document.json_mut()["review_value"] = serde_json::json!(value);
    app.record_edit(format!("Changed review value to {value}"));
    app.set_status(format!("Changed review value to {value}"), false);
}

#[test]
fn docked_json_undo_and_redo_preserve_account_history() {
    let directory = TestDirectory::new("shortcut-json-draft");
    let mut app = app(directory.0.clone());
    edit(&mut app, 1);
    let document = app.document.clone();
    app.select_view(ViewMode::AdvancedJson);
    let original = app.raw_json.clone();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![], true);

    let pos = egui::pos2(500.0, 250.0);
    frame(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        true,
    );
    frame(
        &mut app,
        &ctx,
        vec![
            key(egui::Key::Home, egui::Modifiers::COMMAND),
            key(egui::Key::Delete, egui::Modifiers::NONE),
        ],
        true,
    );
    let draft = app.raw_json.clone();
    assert_ne!(draft, original);
    assert!(serde_json::from_str::<Value>(&draft).is_err());
    assert!(app.json_editor.has_unapplied_changes());

    frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        true,
    );
    assert_eq!(app.raw_json, original);
    assert_eq!(app.document, document);
    assert_eq!(app.undo_history.len(), 1);
    assert!(app.redo_history.is_empty());
    frame(
        &mut app,
        &ctx,
        vec![key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
        true,
    );
    assert_eq!(app.raw_json, draft);
    assert_eq!(app.document, document);
    assert_eq!(app.undo_history.len(), 1);
    assert!(app.redo_history.is_empty());
}
