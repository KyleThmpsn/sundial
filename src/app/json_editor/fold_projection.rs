use std::{collections::HashSet, ops::Range};

use eframe::egui;

const FOLD_PLACEHOLDER: &str = " … ";

pub(super) type FoldId = Vec<usize>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FoldRegion {
    pub(super) id: FoldId,
    open_byte: usize,
    close_byte: usize,
    pub(super) open_line: usize,
}

#[derive(Clone, Debug)]
struct ProjectionCopy {
    display: Range<usize>,
    source: Range<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct ProjectionPlaceholder {
    pub(super) display: Range<usize>,
    source: Range<usize>,
    id: FoldId,
}

#[derive(Debug)]
pub(super) enum ProjectionEditError {
    Hidden(FoldId),
    InvalidMapping,
}

pub(super) struct FoldProjection {
    pub(super) text: String,
    copies: Vec<ProjectionCopy>,
    pub(super) placeholders: Vec<ProjectionPlaceholder>,
}

impl FoldProjection {
    pub(super) fn new(source: &str, regions: &[FoldRegion], folded: &HashSet<FoldId>) -> Self {
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
        let display_start = self.text.len();
        self.text.push_str(&source[range.clone()]);
        self.copies.push(ProjectionCopy {
            display: display_start..self.text.len(),
            source: range,
        });
    }

    pub(super) fn display_to_source(&self, position: usize) -> Option<usize> {
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

    pub(super) fn source_to_display(&self, position: usize) -> Option<usize> {
        self.copies.iter().find_map(|copy| {
            (copy.source.start..=copy.source.end)
                .contains(&position)
                .then(|| copy.display.start + position - copy.source.start)
        })
    }

    fn source_range_to_display(&self, range: Range<usize>) -> Option<(usize, usize)> {
        self.copies.iter().find_map(|copy| {
            (copy.source.start <= range.start && range.end <= copy.source.end).then(|| {
                (
                    copy.display.start + range.start - copy.source.start,
                    copy.display.start + range.end - copy.source.start,
                )
            })
        })
    }

    pub(super) fn apply_edit(
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

pub(super) fn reveal_source_range(
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

pub(super) fn projected_matches(
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

pub(super) fn fold_regions(text: &str) -> Vec<FoldRegion> {
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

pub(super) fn visible_line_numbers(source: &str, projection: &FoldProjection) -> Vec<usize> {
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

pub(super) fn line_number_gutter_width(ui: &egui::Ui, line_count: usize) -> i8 {
    let digits = line_count.to_string().len();
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let digit_width = ui.fonts(|fonts| fonts.glyph_width(&font_id, '0'));
    ((digits as f32 * digit_width + 34.0).ceil() as i8).clamp(44, 120)
}

pub(super) fn paint_line_numbers(
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
