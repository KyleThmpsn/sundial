use super::*;

#[test]
fn stock_and_different_custom_variants_can_share_a_source_but_identical_choices_cannot() {
    let mut recipe =
        WeaponRecipe::from_json_str(include_str!("../../../../recipes/redacted.parhelion.json"))
            .unwrap();
    let private = recipe.overrides.socket_plug_variants[0].clone();
    let source = private.source_plug_hash.parse_u32().unwrap();
    assert!(!choice_conflicts(&recipe, 0, 1, source, None, &[source]));
    assert!(choice_conflicts(
        &recipe,
        0,
        1,
        source,
        Some(&private),
        &[source]
    ));
    let mut different = private.clone();
    different.name = Some("Different Frame".into());
    assert!(!choice_conflicts(
        &recipe,
        0,
        1,
        source,
        Some(&different),
        &[source]
    ));
    recipe.overrides.socket_plug_variants.clear();
    assert!(choice_conflicts(&recipe, 0, 1, source, None, &[source]));
    assert!(!choice_conflicts(
        &recipe,
        0,
        1,
        source,
        Some(&private),
        &[source]
    ));
}

#[test]
fn installed_frame_selection_carries_complete_recipe_data() {
    let source = WeaponRecipe::from_json_str(include_str!(
        "../../../../recipes/vaultbreaker.parhelion.json"
    ))
    .unwrap();
    let before = source.clone();
    let selected = find_picked_perk(
        std::slice::from_ref(&source),
        0xD8EF_B0FD,
        |_, socket, choice| {
            assert_eq!((socket, choice), (0, 0));
            Some(0xD8EF_B0FD)
        },
    )
    .unwrap();
    let mut target = WeaponRecipe::new_weapon("parhelion.picked-frame").unwrap();
    attach_picked_perk(&mut target, 2, 1, selected);
    let expected = &source.overrides.socket_plug_variants[0];
    let actual = &target.overrides.socket_plug_variants[0];
    assert_eq!(actual.socket_index, 2);
    assert_eq!(actual.choice_index, 1);
    assert_eq!(actual.source_plug_hash, expected.source_plug_hash);
    assert_eq!(
        actual.classification_donor_hash,
        expected.classification_donor_hash
    );
    assert_eq!(actual.sandbox_perks, expected.sandbox_perks);
    assert_eq!(
        actual.additional_sandbox_perks,
        expected.additional_sandbox_perks
    );
    let reloaded = WeaponRecipe::from_json_str(&serde_json::to_string(&target).unwrap()).unwrap();
    assert_eq!(target, reloaded);
    assert_eq!(source, before);
}

#[test]
fn installed_custom_selection_uses_native_identity_and_rejects_missing_or_conflicting_sources() {
    let source =
        WeaponRecipe::from_json_str(include_str!("../../../../recipes/redacted.parhelion.json"))
            .unwrap();
    let hash = 0x1234_5678;
    assert_eq!(
        find_picked_perk(std::slice::from_ref(&source), hash, |_, _, _| Some(hash)).unwrap(),
        source.overrides.socket_plug_variants[0]
    );
    assert!(
        find_picked_perk(&[], hash, |_, _, _| None)
            .unwrap_err()
            .contains("Import its source recipe")
    );
    let mut conflicting = source.clone();
    conflicting.overrides.socket_plug_variants[0].name = Some("Different".into());
    assert!(
        find_picked_perk(&[source, conflicting], hash, |_, _, _| Some(hash))
            .unwrap_err()
            .contains("conflicting")
    );
}
