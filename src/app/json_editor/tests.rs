use std::collections::HashSet;

use super::fold_projection::{FoldProjection, ProjectionEditError, fold_regions};

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
