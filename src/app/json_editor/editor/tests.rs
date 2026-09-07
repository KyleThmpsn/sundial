use super::super::fold_projection::FoldProjection;
use super::*;

fn click_label(output: &egui::FullOutput, label: &str) -> Vec<egui::Event> {
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("button label was rendered");
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
    ]
}

#[test]
fn collapse_all_keeps_top_level_keys_and_source_visible() {
    let ctx = egui::Context::default();
    let original = "{\n  \"version\": 13,\n  \"state\": {\n    \"hidden\": 42\n  },\n  \"preferences\": {\n    \"fov\": 105\n  }\n}";
    let mut text = original.to_owned();
    let mut state = JsonEditorState::default();
    state.folded.insert(fold_regions(&text)[0].id.clone());
    let output = frame(&ctx, &mut text, &mut state, vec![]);
    frame(
        &ctx,
        &mut text,
        &mut state,
        click_label(&output, "Collapse All"),
    );
    let projection = FoldProjection::new(&text, &state.regions, &state.folded);
    for key in ["version", "state", "preferences"] {
        assert!(projection.text.contains(key));
    }
    assert!(!projection.text.contains("hidden"));
    assert!(!state.folded.iter().any(|id| id.len() == 1));
    assert_eq!(text, original);
    assert!(!state.has_unapplied_changes());
}

#[test]
fn replace_all_includes_folded_unicode_content_and_undoes_once() {
    let ctx = egui::Context::default();
    let original = "{\n  \"hidden\": {\n    \"é\": \"é\"\n  }\n}";
    let mut text = original.to_owned();
    let mut state = JsonEditorState {
        query: "é".into(),
        replacement: "🔥".into(),
        replace_open: true,
        ..Default::default()
    };
    frame(&ctx, &mut text, &mut state, vec![]);
    state.folded.insert(state.regions[1].id.clone());
    let output = frame(&ctx, &mut text, &mut state, vec![]);
    frame(
        &ctx,
        &mut text,
        &mut state,
        click_label(&output, "Replace all"),
    );
    assert_eq!(text, original.replace('é', "🔥"));
    select(&ctx, &state, 0, 0);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
    );
    assert_eq!(text, original);
}

#[test]
fn format_shortcut_preserves_unknown_fields_and_rejects_duplicate_keys() {
    for original in [
        r#"{"opaque":{"é":[1,true]},"version":8}"#,
        r#"{"x":1,"x":2}"#,
    ] {
        let ctx = egui::Context::default();
        let mut text = original.to_owned();
        let mut state = JsonEditorState::default();
        frame(&ctx, &mut text, &mut state, vec![]);
        frame(
            &ctx,
            &mut text,
            &mut state,
            vec![key(
                egui::Key::F,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            )],
        );
        if let Ok(value) = crate::strict_json::from_str::<serde_json::Value>(original) {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&text).unwrap(),
                value
            );
            assert!(text.contains('\n'));
            select(&ctx, &state, 0, 0);
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
            );
        }
        assert_eq!(text, original);
    }
}

#[test]
fn navigation_reveals_and_selects_an_escaped_pointer() {
    let ctx = egui::Context::default();
    let mut text = "{\n  \"a/b\": {\n    \"~key\": \"🔥\"\n  }\n}".to_owned();
    let mut state = JsonEditorState {
        navigation_open: true,
        pointer: "/a~1b/~0key".into(),
        ..Default::default()
    };
    state.folded.insert(fold_regions(&text)[0].id.clone());
    let output = frame(&ctx, &mut text, &mut state, vec![]);
    frame(&ctx, &mut text, &mut state, click_label(&output, "Go"));
    assert!(state.folded.is_empty());
    let edit = egui::text_edit::TextEditState::load(&ctx, state.text_edit_id.unwrap()).unwrap();
    let cursor = edit.cursor.char_range().unwrap();
    let range = cursor.primary.index.min(cursor.secondary.index)
        ..cursor.primary.index.max(cursor.secondary.index);
    assert_eq!(
        text.chars()
            .skip(range.start)
            .take(range.len())
            .collect::<String>(),
        "\"🔥\""
    );
}

#[test]
fn completion_uses_matching_defaults_without_overwriting_unknown_fields() {
    for version in [8, 13] {
        let ctx = egui::Context::default();
        let mut text = format!("{{\"version\":{version},\"opaque\":42}}");
        let original = text.clone();
        let mut state = JsonEditorState {
            completion_open: true,
            ..Default::default()
        };
        state.set_completion_defaults(Ok(serde_json::json!({"version":13,"fov":105})));
        let output = frame(&ctx, &mut text, &mut state, vec![]);
        if version == 13 {
            frame(&ctx, &mut text, &mut state, click_label(&output, "Add fov"));
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["fov"], 105);
            assert_eq!(value["opaque"], 42);
            select(&ctx, &state, 0, 0);
            frame(
                &ctx,
                &mut text,
                &mut state,
                vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
            );
        } else {
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text.contains("same version"))));
        }
        assert_eq!(text, original);
    }
}

#[test]
fn go_to_error_focuses_the_editor_without_changing_source() {
    let ctx = egui::Context::default();
    let original = "{\n  \"a\": [1,]\n}";
    let mut text = original.to_owned();
    let mut state = JsonEditorState::default();
    let output = frame(&ctx, &mut text, &mut state, vec![]);
    frame(
        &ctx,
        &mut text,
        &mut state,
        click_label(&output, "Go to Error"),
    );
    assert!(ctx.memory(|memory| memory.has_focus(state.text_edit_id.unwrap())));
    assert_eq!(text, original);
}

fn frame(
    ctx: &egui::Context,
    text: &mut String,
    state: &mut JsonEditorState,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 720.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw(ui, text, state, false, true);
            });
        },
    )
}

fn select(ctx: &egui::Context, state: &JsonEditorState, start: usize, end: usize) {
    let id = state.text_edit_id.unwrap();
    ctx.memory_mut(|memory| memory.request_focus(id));
    let mut edit = egui::text_edit::TextEditState::load(ctx, id).unwrap();
    edit.cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(start),
            egui::text::CCursor::new(end),
        )));
    edit.store(ctx, id);
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn unicode_paste_with_active_search_does_not_use_old_highlight_offsets() {
    let ctx = egui::Context::default();
    let mut text = "abc".to_owned();
    let mut state = JsonEditorState {
        query: "b".into(),
        ..Default::default()
    };
    frame(&ctx, &mut text, &mut state, vec![]);
    select(&ctx, &state, 0, 3);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![egui::Event::Text("🔥".into())],
    );
    assert_eq!(text, "🔥");
}

#[test]
fn folded_source_survives_edit_undo_and_redo() {
    let ctx = egui::Context::default();
    let original = "{\n  \"hidden\": {\n    \"value\": 1\n  },\n  \"tail\": 2\n}";
    let mut text = original.to_owned();
    let mut state = JsonEditorState::default();
    let regions = fold_regions(&text);
    state.folded.insert(regions[1].id.clone());
    let projection = FoldProjection::new(&text, &regions, &state.folded);
    let byte = projection.text.rfind('2').unwrap();
    let character = projection.text[..byte].chars().count();
    frame(&ctx, &mut text, &mut state, vec![]);
    select(&ctx, &state, character, character + 1);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![egui::Event::Text("3".into())],
    );
    let edited = original.replace("\"tail\": 2", "\"tail\": 3");
    assert_eq!(text, edited);
    // Changing the projection must not enter the undo history.
    state.folded.clear();
    frame(&ctx, &mut text, &mut state, vec![]);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
    );
    assert_eq!(text, original);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::Y, egui::Modifiers::COMMAND)],
    );
    assert_eq!(text, edited);
    assert!(!text.contains('…'));
}

#[test]
fn deleting_text_before_a_fold_rebuilds_its_offsets() {
    let ctx = egui::Context::default();
    let mut text =
        "{\n  \"long_prefix\": 123456789,\n  \"nested\": {\n    \"value\": 1\n  }\n}".to_owned();
    let mut state = JsonEditorState::default();
    state.folded.insert(fold_regions(&text)[1].id.clone());
    frame(&ctx, &mut text, &mut state, vec![]);
    let end = text.find("  \"nested\"").unwrap();
    select(&ctx, &state, 2, end);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![egui::Event::Text(String::from(" "))],
    );
    assert!(text.contains("\"value\": 1"));
    assert!(crate::strict_json::from_str::<serde_json::Value>(&text).is_ok());
    frame(&ctx, &mut text, &mut state, vec![]);
}

#[test]
fn validation_reports_duplicate_keys_and_clears_after_correction() {
    let mut state = JsonEditorState::default();
    assert!(
        state
            .validate(r#"{"x":1,"x":2}"#)
            .unwrap()
            .contains("duplicate")
    );
    assert!(state.validate(r#"{"x":1}"#).is_none());
    state.set_application_error("unsupported setting".into());
    state.mark_modified();
    assert!(state.application_error.is_none());
}

#[test]
fn resetting_the_source_drops_history_from_the_previous_document() {
    let mut state = JsonEditorState::default();
    let mut text = "old".to_owned();
    state.history.feed_state(0.0, &text);
    state.reset_history();
    text = "new".to_owned();
    state.history.feed_state(1.0, &text);
    state.restore_history(&mut text, false);
    assert_eq!(text, "new");
}

#[test]
fn enter_in_find_advances_without_editing_the_document() {
    let ctx = egui::Context::default();
    let mut text = r#"{"x": "x"}"#.to_owned();
    let original = text.clone();
    let mut state = JsonEditorState {
        query: "x".into(),
        ..Default::default()
    };
    frame(&ctx, &mut text, &mut state, vec![]);
    assert_eq!(state.current_match, Some(0));
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::F, egui::Modifiers::COMMAND)],
    );
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(state.current_match, Some(1));
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(state.current_match, Some(0));
    assert_eq!(text, original);
}

#[test]
fn empty_editor_accepts_text() {
    let ctx = egui::Context::default();
    let mut text = String::new();
    let mut state = JsonEditorState::default();
    frame(&ctx, &mut text, &mut state, vec![]);
    select(&ctx, &state, 0, 0);
    frame(
        &ctx,
        &mut text,
        &mut state,
        vec![egui::Event::Text("{}".into())],
    );
    assert_eq!(text, "{}");
    assert!(state.has_unapplied_changes());
}

#[test]
fn applying_or_saving_does_not_erase_source_history() {
    let mut state = JsonEditorState::default();
    state.history.feed_state(0.0, &"{\"x\":1}".to_owned());
    let mut text = "{\"x\":2}".to_owned();
    state.history.feed_state(0.1, &text);
    state.mark_synced();
    state.restore_history(&mut text, false);
    assert_eq!(text, "{\"x\":1}");
    assert!(state.has_unapplied_changes());
}

#[test]
fn rejected_edits_are_retried_only_when_the_user_changes_them() {
    let mut state = JsonEditorState::default();
    state.mark_modified();
    assert!(state.take_auto_apply_request());
    assert!(!state.take_auto_apply_request());
    assert!(state.has_unapplied_changes());
    state.mark_modified();
    assert!(state.take_auto_apply_request());
    state.mark_modified();
    state.mark_synced();
    assert!(!state.take_auto_apply_request());
}

#[test]
fn rendered_editor_reports_unsaved_changes_and_the_error_reason() {
    let ctx = egui::Context::default();
    let mut text = r#"{"x":1,"x":2}"#.to_owned();
    let mut state = JsonEditorState::default();
    let output = frame(&ctx, &mut text, &mut state, vec![]);
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(labels.contains(&"Unsaved changes"));
    assert!(labels.iter().any(|text| text.contains("duplicate")));
}
