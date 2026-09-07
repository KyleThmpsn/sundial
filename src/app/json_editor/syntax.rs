use eframe::egui;

#[derive(Default)]
pub(super) struct JsonLayoutCache {
    entry: Option<LayoutEntry>,
}

struct LayoutEntry {
    text: String,
    matches: Vec<(usize, usize)>,
    current: Option<usize>,
    style: std::sync::Arc<egui::Style>,
    job: egui::text::LayoutJob,
}

impl JsonLayoutCache {
    pub(super) fn layout(
        &mut self,
        ui: &egui::Ui,
        text: &str,
        matches: &[(usize, usize)],
        current: Option<usize>,
    ) -> std::sync::Arc<egui::Galley> {
        if !self.entry.as_ref().is_some_and(|entry| {
            entry.text == text
                && entry.matches == matches
                && entry.current == current
                && entry.style == *ui.style()
        }) {
            self.entry = Some(LayoutEntry {
                text: text.to_owned(),
                matches: matches.to_vec(),
                current,
                style: ui.style().clone(),
                job: highlight_job(ui, text, matches, current),
            });
        }
        ui.fonts(|fonts| fonts.layout_job(self.entry.as_ref().unwrap().job.clone()))
    }
}

fn highlight_job(
    ui: &egui::Ui,
    text: &str,
    matches: &[(usize, usize)],
    current_match: Option<usize>,
) -> egui::text::LayoutJob {
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
    job
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JsonTokenKind {
    Default,
    Key,
    String,
    Number,
    Literal,
}

pub(super) fn json_tokens(text: &str) -> Vec<(usize, usize, JsonTokenKind)> {
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

pub(super) fn line_column(text: &str, character_index: usize) -> (usize, usize) {
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

pub(super) fn character_to_byte(text: &str, character_index: usize) -> Option<usize> {
    text.char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .nth(character_index)
}

pub(super) fn line_column_at_byte(text: &str, byte: usize) -> (usize, usize) {
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

pub(super) fn find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
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
