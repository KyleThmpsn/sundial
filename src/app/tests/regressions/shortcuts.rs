use super::*;

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
    let previous = app.document.clone();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            if draw {
                app.draw_app_chrome(ctx, None);
                app.draw_active_view(ctx);
            }
            app.handle_workspace_shortcuts(ctx);
        },
    );
    app.record_document_change(previous);
    output
}

fn edit(app: &mut SundialApp, value: u32) {
    let previous = app.document.clone();
    app.document.json_mut()["review_value"] = serde_json::json!(value);
    app.dirty = true;
    app.set_status(format!("Changed review value to {value}"), false);
    app.record_document_change(previous);
}

#[test]
fn shift_z_redoes_and_never_falls_through_to_undo() {
    let directory = TestDirectory::new("shortcut-redo");
    let mut app = app(directory.0.clone());
    let ctx = egui::Context::default();
    edit(&mut app, 1);
    edit(&mut app, 2);
    let current = app.document.clone();
    let redo = || {
        key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )
    };

    frame(&mut app, &ctx, vec![redo()], false);
    assert_eq!(
        app.document, current,
        "Redo with no future state must be a no-op"
    );
    assert_eq!(app.undo_history.len(), 2);
    frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        false,
    );
    assert_eq!(app.document.json()["review_value"], 1);
    frame(&mut app, &ctx, vec![redo()], false);
    assert_eq!(app.document, current);
    assert_eq!(app.undo_history.len(), 2);
    assert!(app.redo_history.is_empty());
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

#[test]
fn pending_json_draft_blocks_account_undo_and_redo() {
    let directory = TestDirectory::new("history-preserves-json-draft");
    let mut app = app(directory.0.clone());
    edit(&mut app, 1);
    edit(&mut app, 2);
    app.undo();
    let document = app.document.clone();
    app.raw_json = "{ unfinished draft".into();
    app.json_editor.mark_modified();
    app.undo();
    app.redo();
    assert_eq!(app.raw_json, "{ unfinished draft");
    assert_eq!(app.document, document);
    assert_eq!(app.undo_history.len(), 1);
    assert_eq!(app.redo_history.len(), 1);
}

#[test]
fn focused_text_field_keeps_undo_from_changing_the_account() {
    let directory = TestDirectory::new("shortcut-search-focus");
    let mut app = app(directory.0.clone());
    edit(&mut app, 1);
    let document = app.document.clone();
    let ctx = egui::Context::default();
    let id = egui::Id::new("search-field");
    egui::text_edit::TextEditState::default().store(&ctx, id);
    ctx.memory_mut(|memory| memory.request_focus(id));
    frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        false,
    );
    assert_eq!(app.document, document);
    assert_eq!(app.undo_history.len(), 1);
}

#[test]
fn open_confirmation_keeps_undo_from_changing_the_reviewed_account() {
    let directory = TestDirectory::new("shortcut-confirmation");
    let mut app = app(directory.0.clone());
    edit(&mut app, 1);
    let document = app.document.clone();
    app.confirmation = Some(ConfirmationDialog::ReviewSave);
    frame(
        &mut app,
        &egui::Context::default(),
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
        false,
    );
    assert_eq!(app.document, document);
    assert_eq!(app.undo_history.len(), 1);
}

#[test]
fn save_shortcut_opens_review_outside_the_json_editor() {
    let directory = TestDirectory::new("shortcut-save");
    let mut app = app(directory.0.clone());
    let defaults = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
    ))
    .unwrap();
    app.document = WorkspaceDocument::json_only(defaults);
    app.persisted_document = app.document.clone();
    app.sync_raw_json();
    edit(&mut app, 1);
    app.preferences.review_changes_before_saving = true;
    frame(
        &mut app,
        &egui::Context::default(),
        vec![key(egui::Key::S, egui::Modifiers::COMMAND)],
        false,
    );
    assert!(
        matches!(app.confirmation, Some(ConfirmationDialog::ReviewSave)),
        "{}",
        app.status
    );
    assert!(!app.settings_path.exists());
}
