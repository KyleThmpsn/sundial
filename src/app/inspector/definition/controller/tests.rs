use super::super::matches::CatalogHashMatchIndex;
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

fn fnv1a(name: &str) -> u64 {
    u64::from(name.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    }))
}

/// Artifact mods and Season Pass rewards resolved only through the unlock flag behind them.
/// Inspecting the mod's own item or collectible hash, or a reward item, now names them.
#[test]
fn seasonal_definitions_are_found_by_item_collectible_and_reward_hash() {
    let catalog = catalog_with_artifact_mod();
    for hash in [1, 2] {
        let index = CatalogHashMatchIndex::collect(&catalog, hash);
        assert_eq!(index.artifact_mods, vec![0], "hash {hash}");
        let matches = CatalogHashMatches::from_index(&catalog, hash, &index);
        assert_eq!(matches.artifact_mods[0].sale_index, 0);
        assert!(hash_inspector_sections(&matches).contains(&HashInspectorSection::Progression));
    }
    let catalog = catalog.with_test_seasonal(crate::investment::seasonal::Definition {
        power_steps: vec![100],
        point_steps: vec![100],
        mods: vec![],
        reward_grants: [(987, crate::investment::seasonal::RewardGrant::ExoticEngram)].into(),
    });
    let index = CatalogHashMatchIndex::collect(&catalog, 987);
    assert!(index.season_pass_reward);
    let matches = CatalogHashMatches::from_index(&catalog, 987, &index);
    assert_eq!(
        matches.season_pass_reward.map(|grant| grant.label()),
        Some("Exotic Engram")
    );
    assert!(
        matches
            .match_groups()
            .iter()
            .any(|group| group.label == "Season Pass Reward")
    );
}

/// Dawn tracks a mission by the FNV-1a hash of its scenario package name, which is not a
/// definition hash at all. The index recognises the names this build knows.
#[test]
fn dawn_missions_are_found_by_scenario_hash() {
    let catalog = Catalog::for_test(vec![], Default::default());
    let hash = fnv1a("mission_scot");
    let index = CatalogHashMatchIndex::collect(&catalog, hash);
    assert_eq!(index.mission_scenario, Some("mission_scot"));
    let matches = CatalogHashMatches::from_index(&catalog, hash, &index);
    assert!(hash_inspector_sections(&matches).contains(&HashInspectorSection::Progression));
    assert!(
        matches
            .match_groups()
            .iter()
            .any(|group| group.label == "Dawn Mission")
    );
    assert!(
        CatalogHashMatchIndex::collect(&catalog, fnv1a("not_a_mission"))
            .mission_scenario
            .is_none()
    );
    assert_eq!(
        crate::app::dawn_state::vendors::vendor_for_progression(58),
        Some((11, true, 2000))
    );
    assert_eq!(
        crate::app::dawn_state::vendors::vendor_for_progression(1),
        None
    );
}

/// Stat groups and power-cap rows were printed on items as bare indices with nowhere to go.
/// Both now resolve by their own hash and list the items that use them.
#[test]
fn stat_groups_and_power_caps_resolve_by_hash_with_their_items() {
    let catalog = Catalog::for_test(vec![], Default::default())
        .with_test_stat_groups(vec![
            ItemStatGroup::default(),
            ItemStatGroup {
                hash: 0x5A5A,
                maximum_value: 100,
                ..Default::default()
            },
        ])
        .with_test_power_caps(vec![PowerCapDefinition {
            hash: 0x7070,
            power_cap: 1_600,
        }])
        .with_test_item_package_metadata(
            0xA1,
            ItemPackageMetadata {
                stat_group_index: Some(1),
                power_cap_groups: vec![0],
                socket_entry_list_index: Some(4),
                ..Default::default()
            },
        )
        .with_test_item_package_metadata(
            0xA2,
            ItemPackageMetadata {
                stat_group_index: Some(0),
                socket_entry_list_index: Some(4),
                ..Default::default()
            },
        );
    let index = CatalogHashMatchIndex::collect(&catalog, 0x5A5A);
    assert_eq!(index.stat_group, Some(1));
    let matches = CatalogHashMatches::from_index(&catalog, 0x5A5A, &index);
    assert_eq!(
        matches
            .item_stat_group
            .map(|(_, group)| group.maximum_value),
        Some(100)
    );
    assert_eq!(matches.stat_group_items, vec![0xA1]);
    assert!(hash_inspector_sections(&matches).contains(&HashInspectorSection::Item));

    let index = CatalogHashMatchIndex::collect(&catalog, 0x7070);
    assert_eq!(index.power_cap, Some(0));
    let matches = CatalogHashMatches::from_index(&catalog, 0x7070, &index);
    assert_eq!(
        matches.power_cap_definition.map(|(_, cap)| cap.power_cap),
        Some(1_600)
    );
    assert_eq!(matches.power_cap_items, vec![0xA1]);

    assert_eq!(catalog.items_with_socket_entry_list(4), vec![0xA1, 0xA2]);
    assert!(catalog.items_with_socket_entry_list(5).is_empty());
}

/// Records were the one catalogued progression kind the hash index never collected, so a
/// triumph hash reported no indexed entity while the Triumphs page named it.
#[test]
fn records_are_found_by_hash_objective_and_completion_flag() {
    let catalog = Catalog::for_test(vec![], Default::default())
        .with_test_progression(
            vec![UnlockDefinition {
                hash: 700,
                code: 1,
                compact_slot: Some(1),
                ..Default::default()
            }],
            vec![],
            vec![],
        )
        .with_test_objectives(vec![ObjectiveDef {
            hash: 500,
            ..Default::default()
        }])
        .with_test_records(vec![RecordDefinition {
            index: 0,
            hash: 300,
            name: "First Victory".into(),
            objectives: vec![0],
            completion_flag: Some(0),
            ..Default::default()
        }]);
    let by_hash = CatalogHashMatchIndex::collect(&catalog, 300);
    assert_eq!(by_hash.record_matches, vec![0]);
    assert!(by_hash.record_references.is_empty());
    let matches = CatalogHashMatches::from_index(&catalog, 300, &by_hash);
    assert_eq!(matches.record_matches[0].1.name, "First Victory");
    assert!(hash_inspector_sections(&matches).contains(&HashInspectorSection::Progression));
    assert!(
        matches
            .match_groups()
            .iter()
            .any(|group| group.label == "Record" && group.count == 1)
    );

    let by_objective = CatalogHashMatchIndex::collect(&catalog, 500);
    assert_eq!(by_objective.record_references, vec![(0, "Objective")]);
    let by_flag = CatalogHashMatchIndex::collect(&catalog, 700);
    assert_eq!(by_flag.record_references, vec![(0, "Completion Flag")]);
    assert!(by_flag.record_matches.is_empty());
}
