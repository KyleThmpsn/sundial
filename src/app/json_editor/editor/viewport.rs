//! Folded text rendering, source edits, and cursor/scroll restoration.
use super::super::{
    fold_projection::{
        FoldProjection, ProjectionEditError, fold_regions, line_number_gutter_width,
        paint_line_numbers, projected_matches, reveal_source_range, visible_line_numbers,
    },
    syntax::{character_to_byte, find_matches, line_column, line_column_at_byte},
};
use super::{JsonEditorState, controls::SearchNavigation};
use eframe::egui;

pub(super) fn draw(
    ui: &mut egui::Ui,
    text: &mut String,
    state: &mut JsonEditorState,
    navigation: SearchNavigation,
    time: f64,
) {
    let SearchNavigation {
        matches: mut source_matches,
        mut jump_to_match,
    } = navigation;
    state.draw_replace(ui, text, &source_matches);
    state.refresh_analysis(text);
    let regions = state.regions.clone();
    let pending_jump = state.pending_jump.take();
    if let Some(range) = pending_jump {
        reveal_source_range(&mut state.folded, &regions, range);
        state.cursor_source_byte = range.0;
        state.restore_location = true;
    }
    // Tool actions may have changed the source after the Find row was drawn.
    source_matches = find_matches(text, &state.query);
    ui.add_space(4.0);

    if jump_to_match {
        if let Some(range) = state
            .current_match
            .and_then(|index| source_matches.get(index).copied())
        {
            reveal_source_range(&mut state.folded, &regions, range);
        }
    }
    let mut projection = FoldProjection::new(text, &regions, &state.folded);
    let (matches, current_match) =
        projected_matches(&projection, &source_matches, state.current_match);
    let current_range = pending_jump
        .and_then(|(start, end)| {
            Some((
                projection.source_to_display(start)?,
                projection.source_to_display(end)?,
            ))
        })
        .or_else(|| current_match.and_then(|index| matches.get(index).copied()));
    jump_to_match |= pending_jump.is_some();
    let projected_before_edit = projection.text.clone();
    let source_line_count = text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let visible_line_numbers = visible_line_numbers(text, &projection);
    let mut layout_cache = std::mem::take(&mut state.layout_cache);
    let mut layouter = |ui: &egui::Ui, source: &str, _wrap_width: f32| {
        // TextEdit may call the layouter after changing the text in this same frame.
        if source == projected_before_edit {
            layout_cache.layout(ui, source, &matches, current_match)
        } else {
            layout_cache.layout(ui, source, &[], None)
        }
    };
    let footer_height = ui.text_style_height(&egui::TextStyle::Body);
    let footer_gap = ui.spacing().item_spacing.y;
    let editor_height = (ui.available_height() - footer_height - footer_gap).max(160.0);
    let restore_location = std::mem::take(&mut state.restore_location);
    let mut scroll_area = egui::ScrollArea::both()
        .id_salt("json_editor_scroll")
        .max_height(editor_height)
        .auto_shrink([false, false]);
    if restore_location {
        scroll_area = scroll_area.scroll_offset(state.scroll_offset);
    }
    let scroll_output = scroll_area.show(ui, |ui| {
        let gutter_width = line_number_gutter_width(ui, source_line_count);
        let mut output = egui::TextEdit::multiline(&mut projection.text)
            .id_salt("json_editor_text")
            .code_editor()
            .desired_width(f32::INFINITY)
            .desired_rows(40)
            .margin(egui::Margin {
                left: gutter_width,
                right: 4,
                top: 2,
                bottom: 2,
            })
            .layouter(&mut layouter)
            .show(ui);
        if restore_location && !output.response.changed() {
            restore_cursor(
                ui,
                &mut output,
                &projection,
                state.cursor_source_byte,
                text.len(),
            );
        }
        if let Some(id) = paint_line_numbers(
            ui,
            &output,
            &visible_line_numbers,
            &regions,
            &state.folded,
            &projection,
        ) {
            if !state.folded.remove(&id) {
                state.folded.insert(id);
            }
        }
        if output.response.changed() {
            apply_projection_edit(text, &projection, &projected_before_edit, state);
        }
        update_cursor_location(&output, &projection, text, state);
        if jump_to_match && !output.response.changed() {
            if let Some(range) = current_range {
                select_range(ui, &mut output, &projection, range, pending_jump.is_some());
            }
        }
        // Undo stores complete source documents, never the display's fold placeholders.
        state.text_edit_id = Some(output.response.id);
        output.state.clear_undoer();
        output.state.store(ui.ctx(), output.response.id);
    });
    state.layout_cache = layout_cache;
    state.history.feed_state(time, text);
    state.scroll_offset = scroll_output.state.offset;
}

fn restore_cursor(
    ui: &egui::Ui,
    output: &mut egui::text_edit::TextEditOutput,
    projection: &FoldProjection,
    source_byte: usize,
    source_len: usize,
) {
    if let Some(display) = projection.source_to_display(source_byte.min(source_len)) {
        let character = projection
            .text
            .get(..display)
            .unwrap_or_default()
            .chars()
            .count();
        output
            .state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(character),
            )));
        output.state.clone().store(ui.ctx(), output.response.id);
    }
}

fn apply_projection_edit(
    text: &mut String,
    projection: &FoldProjection,
    projected_before_edit: &str,
    state: &mut JsonEditorState,
) {
    match projection.apply_edit(text, projected_before_edit) {
        Ok(()) => state.mark_modified(),
        Err(ProjectionEditError::Hidden(id)) => {
            state.folded.remove(&id);
        }
        Err(ProjectionEditError::InvalidMapping) => {
            state.folded.clear();
        }
    }
}

fn update_cursor_location(
    output: &egui::text_edit::TextEditOutput,
    projection: &FoldProjection,
    text: &str,
    state: &mut JsonEditorState,
) {
    if let Some(cursor_range) = output.cursor_range {
        let character_index = cursor_range.primary.ccursor.index;
        let source_position =
            character_to_byte(&projection.text, character_index).and_then(|display| {
                if output.response.changed() {
                    FoldProjection::new(text, &fold_regions(text), &state.folded)
                        .display_to_source(display)
                } else {
                    projection.display_to_source(display)
                }
            });
        let source_position = source_position.filter(|&position| text.is_char_boundary(position));
        if let Some(source) = source_position {
            state.cursor_source_byte = source;
        }
        (state.cursor_line, state.cursor_column) = source_position.map_or_else(
            || line_column(&projection.text, character_index),
            |source| line_column_at_byte(text, source),
        );
    }
}

fn select_range(
    ui: &mut egui::Ui,
    output: &mut egui::text_edit::TextEditOutput,
    projection: &FoldProjection,
    (start, end): (usize, usize),
    focus_editor: bool,
) {
    let start = projection.text[..start].chars().count();
    let end = projection.text[..end].chars().count();
    output
        .state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(start),
            egui::text::CCursor::new(end),
        )));
    if focus_editor {
        output.response.request_focus();
    }
    let start_cursor = output.galley.from_ccursor(egui::text::CCursor::new(start));
    let end_cursor = output.galley.from_ccursor(egui::text::CCursor::new(end));
    let start_rect = output
        .galley
        .pos_from_cursor(&start_cursor)
        .translate(output.galley_pos.to_vec2());
    let end_rect = output
        .galley
        .pos_from_cursor(&end_cursor)
        .translate(output.galley_pos.to_vec2());
    ui.scroll_to_rect(
        start_rect.union(end_rect).expand2(egui::vec2(8.0, 20.0)),
        Some(egui::Align::Center),
    );
}
