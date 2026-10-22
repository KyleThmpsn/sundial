use super::*;
use serde_json::json;

fn catalog_with_artifact_mod() -> Catalog {
    Catalog::for_test(vec![], Default::default())
        .with_test_progression(
            vec![UnlockDefinition {
                code: 1,
                compact_slot: Some(5),
                ..Default::default()
            }],
            vec![],
            vec![],
        )
        .with_test_seasonal(crate::investment::seasonal::Definition {
            power_steps: vec![100],
            point_steps: vec![100],
            mods: vec![crate::investment::seasonal::ArtifactMod {
                sale_index: 0,
                category_index: 0,
                item_hash: 1,
                collectible_hash: 2,
                flag_definition: 0,
                character_slot: 9,
            }],
            reward_grants: Default::default(),
        })
}

/// An artifact mod's flag goes through Sunrise's seasonal editor, which refuses Dawn. On Dawn
/// the inspector has to write the flag directly, which is the only path that can succeed.
#[test]
fn dawn_artifact_mod_flags_are_written_directly() {
    let catalog = catalog_with_artifact_mod();
    let mut dawn = json!({
        "_native_progression": {"runtime": "dawn", "character_slot": 0},
        "state": {"unlocks": {}, "investment": {}}
    });
    let edit = InspectorProgressionEdit::Flag {
        definition_index: 0,
        set: true,
    };
    let message = apply_inspector_progression_edit(&mut dawn, &catalog, edit).unwrap();
    assert!(message.ends_with("set"), "{message}");
    assert_eq!(
        dawn["state"]["unlocks"]["account_flag_runs"],
        json!([[5, 1]]),
        "the flag is stored as a plain account flag"
    );

    let mut sunrise = json!({
        "_native_progression": {"character_slot": 0},
        "state": {"unlocks": {}, "investment": {}}
    });
    let result = apply_inspector_progression_edit(&mut sunrise, &catalog, edit);
    assert_ne!(
        result
            .as_deref()
            .ok()
            .map(|message| message.ends_with("set")),
        Some(true),
        "Sunrise routes the same flag through the seasonal editor: {result:?}"
    );
}
