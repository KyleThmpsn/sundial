use super::*;

fn pending() -> EffectDraft {
    EffectDraft {
        action: None,
        index: 405,
        values: Vec::new(),
        action_values: Vec::new(),
        projectiles: Vec::new(),
        field_text: Vec::new(),
        pending_movement: None,
    }
}

#[test]
fn saved_payload_stays_usable_while_a_draft_is_unfinished() {
    let temp = tempfile::tempdir().unwrap();
    let library = Library::open(temp.path().to_owned()).unwrap();
    let mut perk = PerkRecipe::new();
    perk.name = "Saved Perk".into();
    let entry = library.save(&perk, None).unwrap();
    let mut document = Document::new(perk.clone(), Some(entry.baseline.clone()));
    document.pending_effect = Some(pending());
    let workbench = Workbench::default();
    let collect = |document| {
        super::collect(std::slice::from_ref(&entry), &[document], [], |recipe| {
            workbench.perk_issue(recipe)
        })
    };
    let choices = collect(document.clone());
    assert_eq!(choices.len(), 1);
    assert!(choices[0].issue.is_none());
    assert_eq!(choices[0].source, "Custom Perks");

    document.recipe.name = "Changed Draft".into();
    let choices = collect(document);
    assert_eq!(choices.len(), 2);
    assert!(
        choices
            .iter()
            .find(|choice| choice.recipe == perk)
            .unwrap()
            .issue
            .is_none()
    );
    let draft = choices
        .iter()
        .find(|choice| choice.source == "Workbench Draft")
        .unwrap();
    assert!(draft.issue.as_deref().unwrap().contains("Finish editing"));
}
