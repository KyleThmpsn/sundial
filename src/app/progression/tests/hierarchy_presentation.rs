use super::*;

#[test]
fn definition_contexts_have_no_cap_and_merge_identical_visible_contexts() {
    let mut contexts = (0..300)
        .map(|index| ProgressionContextDef {
            direct_references: Vec::new(),
            hash: index,
            kind: ProgressionContextKind::Activity,
            name: format!("Activity {index}"),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        })
        .collect::<Vec<_>>();
    contexts.push(ProgressionContextDef {
        direct_references: Vec::new(),
        hash: 999,
        kind: ProgressionContextKind::ActivityAvailability,
        name: "Activity 0".into(),
        type_name: String::new(),
        description: String::new(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
    });
    let definition = UnlockDefinition {
        runtime_writers: Vec::new(),
        tested_by: contexts.into_iter().map(std::sync::Arc::new).collect(),
        ..UnlockDefinition::default()
    };

    let lines = definition_context_lines(&definition);

    assert_eq!(lines.len(), 300);
    assert_eq!(
        lines
            .iter()
            .find(|line| line.text() == "Activity 0")
            .unwrap()
            .contexts
            .len(),
        2
    );
}
