use serde_json::Value;

// Format validated tokens without reserializing numbers or string escapes.
pub(super) fn format_json(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut depth: usize = 0;
    let mut previous = None;
    for (start, end, kind) in super::syntax::json_tokens(text) {
        if kind != super::syntax::JsonTokenKind::Default {
            output.push_str(&text[start..end]);
            previous = text[..end].chars().next_back();
            continue;
        }
        for (offset, character) in text[start..end].char_indices() {
            match character {
                '{' | '[' => {
                    output.push(character);
                    depth += 1;
                    let next = text[start + offset + 1..].trim_start().chars().next();
                    if !matches!(next, Some('}' | ']')) {
                        indent(&mut output, depth);
                    }
                }
                '}' | ']' => {
                    depth = depth.saturating_sub(1);
                    if !matches!(previous, Some('{' | '[')) {
                        indent(&mut output, depth);
                    }
                    output.push(character);
                }
                ',' => {
                    output.push(',');
                    indent(&mut output, depth);
                }
                ':' => output.push_str(": "),
                _ => continue,
            }
            previous = Some(character);
        }
    }
    output
}

fn indent(output: &mut String, depth: usize) {
    output.push('\n');
    output.extend(std::iter::repeat_n(' ', depth * 2));
}

#[derive(Clone, Debug)]
pub(super) struct Location {
    pub pointer: String,
    pub range: (usize, usize),
}

// The value comes from strict_json, so this walk only needs to locate validated tokens.
pub(super) fn locations(text: &str, value: &Value) -> Vec<Location> {
    let mut result = Vec::new();
    walk(text, value, &mut 0, String::new(), &mut result);
    result
}

fn whitespace(text: &str, cursor: &mut usize) {
    while text
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
}

fn string_end(text: &str, cursor: &mut usize) {
    *cursor += 1;
    while let Some(&byte) = text.as_bytes().get(*cursor) {
        *cursor += 1;
        match byte {
            b'\\' => *cursor += 1,
            b'"' => break,
            _ => {}
        }
    }
}

fn walk(text: &str, value: &Value, cursor: &mut usize, pointer: String, out: &mut Vec<Location>) {
    whitespace(text, cursor);
    let start = *cursor;
    match value {
        Value::Object(members) => {
            *cursor += 1;
            for (index, (key, child)) in members.iter().enumerate() {
                whitespace(text, cursor);
                if index > 0 {
                    *cursor += 1;
                    whitespace(text, cursor);
                }
                string_end(text, cursor);
                whitespace(text, cursor);
                *cursor += 1;
                let escaped = key.replace('~', "~0").replace('/', "~1");
                walk(text, child, cursor, format!("{pointer}/{escaped}"), out);
            }
            whitespace(text, cursor);
            *cursor += 1;
        }
        Value::Array(values) => {
            *cursor += 1;
            for (index, child) in values.iter().enumerate() {
                whitespace(text, cursor);
                if index > 0 {
                    *cursor += 1;
                }
                walk(text, child, cursor, format!("{pointer}/{index}"), out);
            }
            whitespace(text, cursor);
            *cursor += 1;
        }
        Value::String(_) => string_end(text, cursor),
        _ => {
            while text.as_bytes().get(*cursor).is_some_and(|byte| {
                !byte.is_ascii_whitespace() && !matches!(byte, b',' | b'}' | b']')
            }) {
                *cursor += 1;
            }
        }
    }
    out.push(Location {
        pointer,
        range: (start, *cursor),
    });
}

pub(super) fn error_byte(text: &str, line: usize, column: usize) -> usize {
    let start = text
        .match_indices('\n')
        .map(|(offset, _)| offset + 1)
        .nth(line.saturating_sub(2));
    let start = if line <= 1 {
        0
    } else {
        start.unwrap_or(text.len())
    };
    let end = text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset);
    let mut position = start.saturating_add(column.saturating_sub(1)).min(end);
    while !text.is_char_boundary(position) {
        position -= 1;
    }
    position
}

pub(super) fn replace_ranges(text: &str, ranges: &[(usize, usize)], replacement: &str) -> String {
    let mut result = text.to_owned();
    for &(start, end) in ranges.iter().rev() {
        result.replace_range(start..end, replacement);
    }
    result
}

pub(super) fn add_member(
    text: &str,
    range: (usize, usize),
    key: &str,
    value: &Value,
    empty: bool,
) -> String {
    let object = &text[range.0..range.1];
    let insertion = range.0 + object[..object.len() - 1].trim_end().len();
    let line_start = text[..range.0].rfind('\n').map_or(0, |offset| offset + 1);
    let indent = text[line_start..range.0]
        .chars()
        .take_while(|c| *c == ' ')
        .count()
        + 2;
    let member = format!(
        "{}\n{}{}: {}",
        if empty { "" } else { "," },
        " ".repeat(indent),
        serde_json::to_string(key).expect("JSON keys serialize"),
        value
    );
    let mut result = text.to_owned();
    result.insert_str(insertion, &member);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_preserves_number_spelling_escapes_and_empty_containers() {
        let text = r#"{"big":18446744073709551616,"n":1e-20,"s":"\u0061","a":[{},[],{"b":true}]}"#;
        let formatted = format_json(text);
        assert!(formatted.contains("18446744073709551616"));
        assert!(formatted.contains("1e-20"));
        assert!(formatted.contains(r#""\u0061""#));
        assert!(formatted.contains("{}"));
        assert!(formatted.contains("[]"));
        assert_eq!(
            crate::strict_json::from_str::<Value>(&formatted).unwrap(),
            crate::strict_json::from_str::<Value>(text).unwrap()
        );
        assert_eq!(format_json(&formatted), formatted);
    }

    #[test]
    fn pointers_resolve_escaped_keys_arrays_and_unicode_to_exact_source() {
        let text = r#"{ "a/b": [{"~é": "🔥\\\""}, 42], "empty": {} }"#;
        let value: Value = crate::strict_json::from_str(text).unwrap();
        for location in locations(text, &value) {
            let actual: Value =
                crate::strict_json::from_str(&text[location.range.0..location.range.1]).unwrap();
            assert_eq!(Some(&actual), value.pointer(&location.pointer));
        }
    }

    #[test]
    fn completion_preserves_existing_values_and_uses_escaped_keys() {
        for text in [r#"{}"#, "{\n  \"unknown\": 42\n}"] {
            let value: Value = crate::strict_json::from_str(text).unwrap();
            let updated = add_member(
                text,
                (0, text.len()),
                "a/b",
                &Value::Bool(true),
                value.as_object().unwrap().is_empty(),
            );
            let actual: Value = crate::strict_json::from_str(&updated).unwrap();
            assert_eq!(actual["a/b"], true);
            if value.get("unknown").is_some() {
                assert_eq!(actual["unknown"], 42);
            }
        }
    }

    #[test]
    fn replacement_is_non_recursive_and_unicode_safe() {
        assert_eq!(replace_ranges("é é", &[(0, 2), (3, 5)], "éé"), "éé éé");
        assert_eq!(replace_ranges("abc", &[(0, 3)], ""), "");
    }

    #[test]
    fn error_positions_stay_on_character_boundaries() {
        assert_eq!(error_byte("é\nx", 1, 2), 0);
        assert_eq!(error_byte("é\nx", 2, 1), 3);
        assert_eq!(error_byte("é\nx", 99, 99), 4);
    }
}
