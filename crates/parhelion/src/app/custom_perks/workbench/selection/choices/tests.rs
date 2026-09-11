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
    assert_eq!(choices[0].source, "My Perks");

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

#[test]
fn candidates_use_payload_identity_and_share_workbench_validation() {
    let mut perk = PerkRecipe::new();
    perk.name = "Portable Perk".into();
    let mut same = perk.clone();
    same.id = PerkRecipe::new().id;
    let mut different = same.clone();
    different.stats.push(WeaponStatOverride {
        definition_index: 15,
        value: 5,
    });
    let mut invalid = perk.clone();
    invalid.name = "Broken Perk".into();
    invalid.classification = Some(0.into());
    let workbench = Workbench::default();
    let choices = collect(
        &[],
        &[Document::new(PerkRecipe::new(), None)],
        [
            (perk, "Weapon Recipe · Alpha".into()),
            (same, "Weapon Recipe · Duplicate".into()),
            (different, "Weapon Recipe · Beta".into()),
            (invalid, "Weapon Recipe · Invalid".into()),
        ],
        |recipe| workbench.perk_issue(recipe),
    );
    assert_eq!(
        choices.len(),
        3,
        "Omit untouched scratch documents and duplicate payloads"
    );
    let broken = choices
        .iter()
        .find(|choice| choice.recipe.name == "Broken Perk")
        .unwrap();
    assert_eq!(broken.issue, workbench.perk_issue(&broken.recipe));
    assert!(broken.issue.is_some());
    assert_eq!(
        choices
            .iter()
            .filter(|choice| choice.issue.is_none())
            .count(),
        2
    );
    assert!(
        choices
            .iter()
            .any(|choice| choice.matches("portable alpha"))
    );
    assert!(!choices.iter().any(|choice| choice.matches("duplicate")));
}
