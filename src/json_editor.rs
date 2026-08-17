use std::{collections::HashSet, ops::Range};

use eframe::egui;

type FoldId = Vec<usize>;

pub(super) struct JsonEditorState {
    query: String,
    current_match: Option<usize>,
    folded: HashSet<FoldId>,
    modified: bool,
    reset_pending: bool,
    cursor_line: usize,
    cursor_column: usize,
    cursor_source_byte: usize,
    scroll_offset: egui::Vec2,
    restore_location: bool,
}

impl Default for JsonEditorState {
    fn default() -> Self {
        Self {
            query: String::new(),
            current_match: None,
            folded: HashSet::new(),
            modified: false,
            reset_pending: false,
            cursor_line: 1,
            cursor_column: 1,
            cursor_source_byte: 0,
            scroll_offset: egui::Vec2::ZERO,
            restore_location: false,
        }
    }
}

impl JsonEditorState {
    pub(super) const fn has_unapplied_changes(&self) -> bool {
        self.modified
    }

    pub(super) fn mark_synced(&mut self) {
        self.modified = false;
        self.reset_pending = false;
    }

    pub(super) fn mark_modified(&mut self) {
        self.modified = true;
        self.reset_pending = false;
    }

    pub(super) fn restore_location_next_draw(&mut self) {
        self.restore_location = true;
    }
}

#[derive(Default)]
pub(super) struct JsonEditorResponse {
    pub(super) save: bool,
    pub(super) reset: bool,
    pub(super) toggle_window: bool,
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    text: &mut String,
    state: &mut JsonEditorState,
    detached: bool,
) -> JsonEditorResponse {
    let mut response = JsonEditorResponse::default();
    let regions = fold_regions(text);
    state
        .folded
        .retain(|id| regions.iter().any(|region| region.id == *id));
    let shortcuts_fit_header = ui.available_width() >= 860.0;
    ui.horizontal(|ui| {
        ui.heading("All settings");
        response.toggle_window = ui
            .button(if detached {
                "Dock in main window"
            } else {
                "Open in window"
            })
            .clicked();
        if detached {
            response.save |= ui.button("Save").clicked();
        }
        if shortcuts_fit_header {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                draw_shortcut_hint(ui);
            });
        }
    });
    ui.horizontal(|ui| {
        let can_collapse = regions
            .iter()
            .any(|region| !state.folded.contains(&region.id));
        if ui
            .add_enabled(can_collapse, egui::Button::new("Collapse all"))
            .clicked()
        {
            state
                .folded
                .extend(regions.iter().map(|region| region.id.clone()));
        }
        if ui
            .add_enabled(!state.folded.is_empty(), egui::Button::new("Expand all"))
            .clicked()
        {
            state.folded.clear();
        }
        if state.reset_pending {
            response.reset = ui
                .button(egui::RichText::new("Discard edits").color(ui.visuals().error_fg_color))
                .clicked();
            if ui.button("Cancel").clicked() {
                state.reset_pending = false;
            }
        } else if ui
            .add_enabled(state.modified, egui::Button::new("Reset editor"))
            .clicked()
        {
            state.reset_pending = true;
        }
        if state.modified {
            ui.label(egui::RichText::new("Unsaved JSON edits").color(ui.visuals().warn_fg_color));
        }
    });
    if !shortcuts_fit_header {
        let shortcut_row_height = ui.text_style_height(&egui::TextStyle::Body);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), shortcut_row_height),
            egui::Layout::right_to_left(egui::Align::Center),
            draw_shortcut_hint,
        );
    }
    response.save |=
        ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
    ui.label(if detached {
        "Edit JSON directly, then use Save above to validate and write the file."
    } else {
        "Edit JSON directly, then use Save in the top-right to validate and write the file."
    });
    ui.add_space(4.0);

    let focus_find =
        ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
    let mut jump_to_match = false;
    let mut source_matches = Vec::new();
    ui.horizontal(|ui| {
        ui.label("Find");
        let find_response = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .id_salt("json_editor_find")
                .hint_text("Search JSON…")
                .desired_width(220.0),
        );
        if focus_find {
            find_response.request_focus();
        }

        let query_changed = find_response.changed();
        source_matches = find_matches(text, &state.query);
        if source_matches.is_empty() {
            state.current_match = None;
        } else if query_changed
            || state.current_match.is_none()
            || state
                .current_match
                .is_some_and(|index| index >= source_matches.len())
        {
            state.current_match = Some(0);
            jump_to_match = true;
        }

        let enter =
            find_response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let f3 = ui.input(|input| input.key_pressed(egui::Key::F3));
        let reverse = ui.input(|input| input.modifiers.shift);
        let previous = ui
            .add_enabled(!source_matches.is_empty(), egui::Button::new("Previous"))
            .clicked()
            || (!source_matches.is_empty() && (enter || f3) && reverse);
        let next = ui
            .add_enabled(!source_matches.is_empty(), egui::Button::new("Next"))
            .clicked()
            || (!source_matches.is_empty() && (enter || f3) && !reverse);
        if previous {
            let current = state.current_match.unwrap_or(0);
            state.current_match = Some((current + source_matches.len() - 1) % source_matches.len());
            jump_to_match = true;
        } else if next {
            let current = state.current_match.unwrap_or(0);
            state.current_match = Some((current + 1) % source_matches.len());
            jump_to_match = true;
        }

        match state.current_match {
            Some(current) => {
                ui.label(format!("{} of {}", current + 1, source_matches.len()));
            }
            None if !state.query.is_empty() => {
                ui.label("No matches");
            }
            None => {}
        }
        if ui
            .add_enabled(!state.query.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            state.query.clear();
            state.current_match = None;
            source_matches.clear();
        }
    });

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
    let current_range = current_match.and_then(|index| matches.get(index).copied());
    let projected_before_edit = projection.text.clone();
    let source_line_count = text.lines().count().max(1);
    let visible_line_numbers = visible_line_numbers(text, &projection);
    let mut layouter = |ui: &egui::Ui, source: &str, _wrap_width: f32| {
        highlighted_json(ui, source, &matches, current_match)
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
        if restore_location {
            if let Some(display) =
                projection.source_to_display(state.cursor_source_byte.min(text.len()))
            {
                let character = projection.text[..display].chars().count();
                output
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(character),
                    )));
                output.state.clone().store(ui.ctx(), output.response.id);
            }
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
            match projection.apply_edit(text, &projected_before_edit) {
                Ok(()) => state.mark_modified(),
                Err(ProjectionEditError::Hidden(id)) => {
                    state.folded.remove(&id);
                }
                Err(ProjectionEditError::InvalidMapping) => {
                    state.folded.clear();
                }
            }
        }
        if let Some(cursor_range) = output.cursor_range {
            let character_index = cursor_range.primary.ccursor.index;
            let source_position =
                character_to_byte(&projection.text, character_index).and_then(|display| {
                    if output.response.changed() {
                        FoldProjection::new(text, &regions, &state.folded)
                            .display_to_source(display)
                    } else {
                        projection.display_to_source(display)
                    }
                });
            if let Some(source) = source_position {
                state.cursor_source_byte = source;
            }
            (state.cursor_line, state.cursor_column) = source_position.map_or_else(
                || line_column(&projection.text, character_index),
                |source| line_column_at_byte(text, source),
            );
        }
        if jump_to_match {
            if let Some((start, end)) = current_range {
                let start = projection.text[..start].chars().count();
                let end = projection.text[..end].chars().count();
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
        }
    });
    state.scroll_offset = scroll_output.state.offset;

    let line_count = text.lines().count().max(1);
    let previous_item_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(_) => {
            ui.label(
                egui::RichText::new(format!(
                    "Valid JSON · {line_count} lines · {:.1} KiB · Ln {}, Col {}",
                    text.len() as f64 / 1024.0,
                    state.cursor_line,
                    state.cursor_column
                ))
                .weak(),
            );
        }
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!(
                    "Syntax error at line {}, column {} · {:.1} KiB",
                    error.line(),
                    error.column(),
                    text.len() as f64 / 1024.0
                ),
            );
        }
    }
    ui.spacing_mut().item_spacing.y = previous_item_spacing;

    response
}

fn draw_shortcut_hint(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new(
            "Ctrl+F Find · F3 / Shift+F3 Navigate · Ctrl+S Save · Ctrl+Z / Ctrl+Y Undo/Redo",
        )
        .weak(),
    );
}

const FOLD_PLACEHOLDER: &str = " … ";

#[derive(Clone, Debug, PartialEq, Eq)]
struct FoldRegion {
    id: FoldId,
    open_byte: usize,
    close_byte: usize,
    open_line: usize,
}

#[derive(Clone, Debug)]
struct ProjectionCopy {
    display: Range<usize>,
    source: Range<usize>,
}

#[derive(Clone, Debug)]
struct ProjectionPlaceholder {
    display: Range<usize>,
    source: Range<usize>,
    id: FoldId,
}

#[derive(Debug)]
enum ProjectionEditError {
    Hidden(FoldId),
    InvalidMapping,
}

struct FoldProjection {
    text: String,
    copies: Vec<ProjectionCopy>,
    placeholders: Vec<ProjectionPlaceholder>,
}

impl FoldProjection {
    fn new(source: &str, regions: &[FoldRegion], folded: &HashSet<FoldId>) -> Self {
        let mut selected = regions
            .iter()
            .filter(|region| folded.contains(&region.id))
            .collect::<Vec<_>>();
        selected.sort_by_key(|region| region.open_byte);

        let mut outermost = Vec::new();
        let mut hidden_until = 0;
        for region in selected {
            if region.open_byte < hidden_until {
                continue;
            }
            hidden_until = region.close_byte + 1;
            outermost.push(region);
        }

        let mut projection = Self {
            text: String::with_capacity(source.len()),
            copies: Vec::new(),
            placeholders: Vec::new(),
        };
        let mut source_cursor = 0;
        for region in outermost {
            let hidden = region.open_byte + 1..region.close_byte;
            projection.push_copy(source, source_cursor..hidden.start);
            let display_start = projection.text.len();
            projection.text.push_str(FOLD_PLACEHOLDER);
            let display_end = projection.text.len();
            projection.placeholders.push(ProjectionPlaceholder {
                display: display_start..display_end,
                source: hidden.clone(),
                id: region.id.clone(),
            });
            source_cursor = hidden.end;
        }
        projection.push_copy(source, source_cursor..source.len());
        projection
    }

    fn push_copy(&mut self, source: &str, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let display_start = self.text.len();
        self.text.push_str(&source[range.clone()]);
        self.copies.push(ProjectionCopy {
            display: display_start..self.text.len(),
            source: range,
        });
    }

    fn display_to_source(&self, position: usize) -> Option<usize> {
        for copy in &self.copies {
            if (copy.display.start..=copy.display.end).contains(&position) {
                return Some(copy.source.start + position - copy.display.start);
            }
        }
        for placeholder in &self.placeholders {
            if position == placeholder.display.start {
                return Some(placeholder.source.start);
            }
            if position == placeholder.display.end {
                return Some(placeholder.source.end);
            }
        }
        None
    }

    fn source_to_display(&self, position: usize) -> Option<usize> {
        self.copies.iter().find_map(|copy| {
            (copy.source.start..=copy.source.end)
                .contains(&position)
                .then_some(copy.display.start + position - copy.source.start)
        })
    }

    fn source_range_to_display(&self, range: Range<usize>) -> Option<(usize, usize)> {
        self.copies.iter().find_map(|copy| {
            (copy.source.start <= range.start && range.end <= copy.source.end).then_some((
                copy.display.start + range.start - copy.source.start,
                copy.display.start + range.end - copy.source.start,
            ))
        })
    }

    fn apply_edit(
        &self,
        source: &mut String,
        projected_before_edit: &str,
    ) -> Result<(), ProjectionEditError> {
        if self.text == projected_before_edit {
            return Ok(());
        }
        let prefix = common_prefix_bytes(projected_before_edit, &self.text);
        let suffix = common_suffix_bytes(projected_before_edit, &self.text, prefix);
        let old_end = projected_before_edit.len() - suffix;
        let new_end = self.text.len() - suffix;

        if let Some(placeholder) = self.placeholders.iter().find(|placeholder| {
            if prefix == old_end {
                placeholder.display.start < prefix && prefix < placeholder.display.end
            } else {
                prefix < placeholder.display.end && placeholder.display.start < old_end
            }
        }) {
            return Err(ProjectionEditError::Hidden(placeholder.id.clone()));
        }

        let source_start = self
            .display_to_source(prefix)
            .ok_or(ProjectionEditError::InvalidMapping)?;
        let source_end = self
            .display_to_source(old_end)
            .ok_or(ProjectionEditError::InvalidMapping)?;
        source.replace_range(source_start..source_end, &self.text[prefix..new_end]);
        Ok(())
    }
}

fn reveal_source_range(
    folded: &mut HashSet<FoldId>,
    regions: &[FoldRegion],
    range: (usize, usize),
) {
    folded.retain(|id| {
        let Some(region) = regions.iter().find(|region| region.id == *id) else {
            return false;
        };
        let hidden = region.open_byte + 1..region.close_byte;
        !(range.0 < hidden.end && hidden.start < range.1)
    });
}

fn projected_matches(
    projection: &FoldProjection,
    source_matches: &[(usize, usize)],
    current_source_match: Option<usize>,
) -> (Vec<(usize, usize)>, Option<usize>) {
    let mut current = None;
    let matches = source_matches
        .iter()
        .enumerate()
        .filter_map(|(source_index, &(start, end))| {
            projection
                .source_range_to_display(start..end)
                .map(|range| (source_index, range))
        })
        .enumerate()
        .map(|(projected_index, (source_index, range))| {
            if Some(source_index) == current_source_match {
                current = Some(projected_index);
            }
            range
        })
        .collect();
    (matches, current)
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character.len_utf8())
        .sum()
}

fn common_suffix_bytes(left: &str, right: &str, prefix: usize) -> usize {
    let maximum = left.len().min(right.len()).saturating_sub(prefix);
    let mut suffix = 0;
    for (left, right) in left.chars().rev().zip(right.chars().rev()) {
        if left != right || suffix + left.len_utf8() > maximum {
            break;
        }
        suffix += left.len_utf8();
    }
    suffix
}

fn fold_regions(text: &str) -> Vec<FoldRegion> {
    struct OpenSection {
        delimiter: u8,
        byte: usize,
        line: usize,
        id: FoldId,
        next_child: usize,
    }

    let bytes = text.as_bytes();
    let mut regions = Vec::new();
    let mut stack = Vec::<OpenSection>::new();
    let mut root_index = 0;
    let mut line = 1;
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    let id = if let Some(parent) = stack.last_mut() {
                        let child = parent.next_child;
                        parent.next_child += 1;
                        let mut id = parent.id.clone();
                        id.push(child);
                        id
                    } else {
                        let id = vec![root_index];
                        root_index += 1;
                        id
                    };
                    stack.push(OpenSection {
                        delimiter: byte,
                        byte: index,
                        line,
                        id,
                        next_child: 0,
                    });
                }
                b'}' | b']' => {
                    let expected = if byte == b'}' { b'{' } else { b'[' };
                    if stack.last().is_some_and(|open| open.delimiter == expected) {
                        let open = stack.pop().expect("the matching section was checked");
                        if line > open.line {
                            regions.push(FoldRegion {
                                id: open.id,
                                open_byte: open.byte,
                                close_byte: index,
                                open_line: open.line,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        if byte == b'\n' {
            line += 1;
        }
        index += 1;
    }
    regions.sort_by_key(|region| region.open_byte);
    regions
}

fn visible_line_numbers(source: &str, projection: &FoldProjection) -> Vec<usize> {
    let starts = std::iter::once(0).chain(
        projection
            .text
            .match_indices('\n')
            .map(|(position, _)| position + 1),
    );
    let mut source_cursor = 0;
    let mut source_line = 1;
    starts
        .map(|display| {
            let source_position = projection
                .display_to_source(display)
                .unwrap_or(source_cursor);
            if source_position >= source_cursor {
                source_line += source[source_cursor..source_position]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count();
                source_cursor = source_position;
            }
            source_line
        })
        .collect()
}

fn line_number_gutter_width(ui: &egui::Ui, line_count: usize) -> i8 {
    let digits = line_count.to_string().len();
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let digit_width = ui.fonts(|fonts| fonts.glyph_width(&font_id, '0'));
    ((digits as f32 * digit_width + 34.0).ceil() as i8).clamp(44, 120)
}

fn paint_line_numbers(
    ui: &mut egui::Ui,
    output: &egui::text_edit::TextEditOutput,
    line_numbers: &[usize],
    regions: &[FoldRegion],
    folded: &HashSet<FoldId>,
    projection: &FoldProjection,
) -> Option<FoldId> {
    let painter = ui.painter().clone();
    let clip_rect = ui.clip_rect();
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let text_color = ui.visuals().weak_text_color();
    let right = output.galley_pos.x - 8.0;
    let separator_x = output.galley_pos.x - 4.0;
    let icon_x = output.response.rect.left() + 9.0;
    let mut toggled = None;

    for (index, row) in output.galley.rows.iter().enumerate() {
        let center_y = output.galley_pos.y + row.rect.center().y;
        if center_y < clip_rect.top() - row.rect.height()
            || center_y > clip_rect.bottom() + row.rect.height()
        {
            continue;
        }
        painter.text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            line_numbers.get(index).copied().unwrap_or(index + 1),
            font_id.clone(),
            text_color,
        );
        let line_number = line_numbers.get(index).copied().unwrap_or(index + 1);
        let region = regions.iter().find(|region| {
            region.open_line == line_number
                && projection.source_to_display(region.open_byte).is_some()
        });
        if let Some(region) = region {
            let is_folded = folded.contains(&region.id);
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(icon_x, center_y),
                egui::vec2(16.0, row.rect.height()),
            );
            let interaction = ui
                .interact(
                    icon_rect,
                    egui::Id::new(("json_fold_toggle", &region.id)),
                    egui::Sense::click(),
                )
                .on_hover_text(if is_folded {
                    "Expand section"
                } else {
                    "Collapse section"
                });
            painter.text(
                egui::pos2(icon_x, center_y),
                egui::Align2::CENTER_CENTER,
                if is_folded { "▶" } else { "▼" },
                font_id.clone(),
                if interaction.hovered() {
                    ui.visuals().text_color()
                } else {
                    text_color
                },
            );
            if interaction.clicked() {
                toggled = Some(region.id.clone());
            }
        }
    }

    let top = output.galley_pos.y.min(clip_rect.bottom());
    let bottom = (output.galley_pos.y + output.galley.size().y).min(clip_rect.bottom());
    if bottom > clip_rect.top() {
        painter.vline(
            separator_x,
            top.max(clip_rect.top())..=bottom,
            egui::Stroke::new(1.0, text_color.gamma_multiply(0.35)),
        );
    }
    toggled
}

fn highlighted_json(
    ui: &egui::Ui,
    text: &str,
    matches: &[(usize, usize)],
    current_match: Option<usize>,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let normal = egui::TextFormat {
        font_id: egui::TextStyle::Monospace.resolve(ui.style()),
        color: ui.visuals().text_color(),
        ..Default::default()
    };
    let mut match_index = 0;
    let search_highlights = SearchHighlights {
        matches,
        current: current_match,
        match_background: ui.visuals().warn_fg_color.gamma_multiply(0.25),
        current_background: ui.visuals().selection.bg_fill,
        current_text: ui.visuals().selection.stroke.color,
    };
    for (start, end, kind) in json_tokens(text) {
        let mut format = normal.clone();
        format.color = match kind {
            JsonTokenKind::Default => normal.color,
            JsonTokenKind::Key => ui.visuals().hyperlink_color,
            JsonTokenKind::String => egui::Color32::from_rgb(152, 195, 121),
            JsonTokenKind::Number => ui.visuals().warn_fg_color,
            JsonTokenKind::Literal => egui::Color32::from_rgb(198, 120, 221),
        };
        append_with_search_highlights(
            &mut job,
            text,
            start,
            end,
            &format,
            &mut match_index,
            &search_highlights,
        );
    }
    ui.fonts(|fonts| fonts.layout_job(job))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JsonTokenKind {
    Default,
    Key,
    String,
    Number,
    Literal,
}

fn json_tokens(text: &str) -> Vec<(usize, usize, JsonTokenKind)> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let kind = match bytes[index] {
            b'"' => {
                index += 1;
                let mut escaped = false;
                while index < bytes.len() {
                    let byte = bytes[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
                let mut next = index;
                while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':') {
                    JsonTokenKind::Key
                } else {
                    JsonTokenKind::String
                }
            }
            b'-' | b'0'..=b'9' => {
                index += 1;
                while bytes.get(index).is_some_and(|byte| {
                    matches!(byte, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                }) {
                    index += 1;
                }
                JsonTokenKind::Number
            }
            _ if text[index..].starts_with("true") => {
                index += 4;
                JsonTokenKind::Literal
            }
            _ if text[index..].starts_with("false") => {
                index += 5;
                JsonTokenKind::Literal
            }
            _ if text[index..].starts_with("null") => {
                index += 4;
                JsonTokenKind::Literal
            }
            _ => {
                index += text[index..].chars().next().map_or(1, char::len_utf8);
                while index < bytes.len()
                    && !matches!(bytes[index], b'"' | b'-' | b'0'..=b'9')
                    && !text[index..].starts_with("true")
                    && !text[index..].starts_with("false")
                    && !text[index..].starts_with("null")
                {
                    index += text[index..].chars().next().map_or(1, char::len_utf8);
                }
                JsonTokenKind::Default
            }
        };
        tokens.push((start, index, kind));
    }
    tokens
}

struct SearchHighlights<'a> {
    matches: &'a [(usize, usize)],
    current: Option<usize>,
    match_background: egui::Color32,
    current_background: egui::Color32,
    current_text: egui::Color32,
}

fn append_with_search_highlights(
    job: &mut egui::text::LayoutJob,
    text: &str,
    start: usize,
    end: usize,
    format: &egui::TextFormat,
    match_index: &mut usize,
    highlights: &SearchHighlights<'_>,
) {
    while highlights
        .matches
        .get(*match_index)
        .is_some_and(|(_, match_end)| *match_end <= start)
    {
        *match_index += 1;
    }
    let mut cursor = start;
    let mut local_match = *match_index;
    while let Some(&(match_start, match_end)) = highlights.matches.get(local_match) {
        if match_start >= end {
            break;
        }
        let highlighted_start = cursor.max(match_start);
        let highlighted_end = end.min(match_end);
        if cursor < highlighted_start {
            job.append(&text[cursor..highlighted_start], 0.0, format.clone());
        }
        if highlighted_start < highlighted_end {
            let mut highlighted = format.clone();
            if Some(local_match) == highlights.current {
                highlighted.background = highlights.current_background;
                highlighted.color = highlights.current_text;
            } else {
                highlighted.background = highlights.match_background;
            }
            job.append(&text[highlighted_start..highlighted_end], 0.0, highlighted);
            cursor = highlighted_end;
        }
        if match_end <= end {
            local_match += 1;
        } else {
            break;
        }
    }
    if cursor < end {
        job.append(&text[cursor..end], 0.0, format.clone());
    }
    while highlights
        .matches
        .get(*match_index)
        .is_some_and(|(_, match_end)| *match_end <= end)
    {
        *match_index += 1;
    }
}

fn line_column(text: &str, character_index: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in text.chars().take(character_index) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn character_to_byte(text: &str, character_index: usize) -> Option<usize> {
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(character_index)
}

fn line_column_at_byte(text: &str, byte: usize) -> (usize, usize) {
    let prefix = &text[..byte];
    let line = prefix
        .bytes()
        .filter(|character| *character == b'\n')
        .count()
        + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let text_folded = text.to_ascii_lowercase();
    let query_folded = query.to_ascii_lowercase();
    text_folded
        .match_indices(&query_folded)
        .map(|(start, found)| (start, start + found.len()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        FoldProjection, JsonTokenKind, ProjectionEditError, find_matches, fold_regions,
        json_tokens, line_column, projected_matches, reveal_source_range, visible_line_numbers,
    };

    #[test]
    fn json_search_is_ascii_case_insensitive_and_non_overlapping() {
        assert_eq!(
            find_matches("Plug plug PLUG", "plug"),
            vec![(0, 4), (5, 9), (10, 14)]
        );
        assert_eq!(find_matches("Plug plug", "Plug"), vec![(0, 4), (5, 9)]);
        assert!(find_matches("anything", "").is_empty());
    }

    #[test]
    fn json_syntax_tokens_distinguish_keys_and_values() {
        let text = r#"{"name":"Sundial","count":2,"enabled":true,"missing":null}"#;
        let tokens = json_tokens(text);
        assert!(tokens.iter().any(|&(start, end, kind)| {
            kind == JsonTokenKind::Key && &text[start..end] == "\"name\""
        }));
        assert!(tokens.iter().any(|&(start, end, kind)| {
            kind == JsonTokenKind::String && &text[start..end] == "\"Sundial\""
        }));
        assert!(
            tokens
                .iter()
                .any(|&(_, _, kind)| kind == JsonTokenKind::Number)
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|&&(_, _, kind)| kind == JsonTokenKind::Literal)
                .count(),
            2
        );
    }

    #[test]
    fn cursor_position_counts_unicode_characters_not_bytes() {
        assert_eq!(line_column("é\nvalue", 2), (2, 1));
        assert_eq!(line_column("é\nvalue", 5), (2, 4));
    }

    fn folding_example() -> String {
        r#"{
  "nested": {
    "brace": "not } or ]",
    "value": 1
  },
  "tail": 2
}"#
        .to_owned()
    }

    #[test]
    fn multiline_json_sections_can_be_folded_without_parsing_braces_in_strings() {
        let source = folding_example();
        let regions = fold_regions(&source);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].open_line, 1);
        assert_eq!(regions[1].open_line, 2);

        let folded = HashSet::from([regions[1].id.clone()]);
        let projection = FoldProjection::new(&source, &regions, &folded);
        assert!(projection.text.contains("\"nested\": { … },"));
        assert!(!projection.text.contains("\"value\": 1"));
        assert_eq!(visible_line_numbers(&source, &projection), vec![1, 2, 6, 7]);
    }

    #[test]
    fn edits_outside_a_fold_update_the_full_source_without_losing_hidden_json() {
        let mut source = folding_example();
        let regions = fold_regions(&source);
        let folded = HashSet::from([regions[1].id.clone()]);
        let mut projection = FoldProjection::new(&source, &regions, &folded);
        let before = projection.text.clone();
        let tail = projection.text.rfind('2').unwrap();
        projection.text.replace_range(tail..tail + 1, "3");

        projection.apply_edit(&mut source, &before).unwrap();
        assert!(source.contains("\"brace\": \"not } or ]\""));
        assert!(source.contains("\"value\": 1"));
        assert!(source.contains("\"tail\": 3"));
    }

    #[test]
    fn editing_a_fold_marker_unfolds_instead_of_overwriting_hidden_json() {
        let mut source = folding_example();
        let regions = fold_regions(&source);
        let folded = HashSet::from([regions[1].id.clone()]);
        let mut projection = FoldProjection::new(&source, &regions, &folded);
        let before = projection.text.clone();
        let marker = projection.placeholders[0].display.clone();
        projection.text.replace_range(marker, "oops");

        assert!(matches!(
            projection.apply_edit(&mut source, &before),
            Err(ProjectionEditError::Hidden(_))
        ));
        assert_eq!(source, folding_example());
    }

    #[test]
    fn search_counts_matches_hidden_by_folds() {
        let source = folding_example();
        let regions = fold_regions(&source);
        let folded = HashSet::from([regions[1].id.clone()]);
        let projection = FoldProjection::new(&source, &regions, &folded);
        let source_matches = find_matches(&source, "value");

        assert_eq!(source_matches.len(), 1);
        assert_eq!(
            projected_matches(&projection, &source_matches, Some(0)),
            (Vec::new(), None)
        );
    }

    #[test]
    fn selecting_a_hidden_search_match_reveals_its_section() {
        let source = folding_example();
        let regions = fold_regions(&source);
        let mut folded = HashSet::from([regions[1].id.clone()]);
        let source_matches = find_matches(&source, "value");

        reveal_source_range(&mut folded, &regions, source_matches[0]);
        let projection = FoldProjection::new(&source, &regions, &folded);
        let (matches, current) = projected_matches(&projection, &source_matches, Some(0));

        assert!(folded.is_empty());
        assert_eq!(matches, source_matches);
        assert_eq!(current, Some(0));
    }
}
