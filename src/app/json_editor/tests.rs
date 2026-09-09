use std::collections::HashSet;

use super::{
    fold_projection::{
        FoldProjection, ProjectionEditError, fold_regions, projected_matches, reveal_source_range,
        visible_line_numbers,
    },
    syntax::{JsonTokenKind, find_matches, json_tokens, line_column},
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
