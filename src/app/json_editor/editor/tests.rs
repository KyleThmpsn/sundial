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
        click_label(&output, "Replace All"),
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

fn frame(
    ctx: &egui::Context,
    text: &mut String,
    state: &mut JsonEditorState,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 720.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
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
