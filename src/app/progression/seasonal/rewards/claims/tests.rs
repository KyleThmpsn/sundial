use super::*;
use crate::catalog::{
    InventoryMetadata, InventoryScope, ItemDef, ItemPackageMetadata, ItemStackability,
    UnlockDefinition,
};
use serde_json::json;

fn catalog() -> Catalog {
    Catalog::for_test_with_inventory(
        vec![ItemDef {
            hash: 987,
            name: "Pass Material".into(),
            type_name: "Material".into(),
            bucket_hash: 0,
            class_type: 3,
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        }],
        [(
            987,
            ItemPackageMetadata {
                definition_index: 7,
                ..Default::default()
            },
        )]
        .into(),
        [(
            987,
            InventoryMetadata {
                scope: InventoryScope::Profile,
                native_bucket_id: 0,
                stackability: ItemStackability::Stackable,
                max_stack_size: Some(99),
                bucket_capacity: Some(100),
            },
        )]
        .into(),
    )
    .with_test_progression(
        vec![UnlockDefinition {
            code: 1,
            compact_slot: Some(200),
            ..Default::default()
        }],
        vec![],
        vec![],
    )
    .with_test_seasonal(crate::investment::seasonal::Definition {
        power_steps: vec![100000],
        point_steps: vec![100000],
        mods: vec![],
        reward_grants: [(987, RewardGrant::Item)].into(),
    })
}

fn reward() -> ProgressionRewardDefinition {
    ProgressionRewardDefinition {
        rewarded_at_progression_level: 2,
        item_hash: 987,
        quantity: 201,
        claim_flag: Some(0),
    }
}

fn document() -> Value {
    json!({"_native_progression":{"character_slot":0},"_reward_context":{"character":0,"character_count":1,"class":1},"future":17,"state":{"unlocks":{"account_progressions":[[38,100000,7,8]]}}})
}

#[test]
fn dawn_seasonal_authoring_and_item_claims_are_refused_without_mutation() {
    use crate::app::account_workspace::WorkspaceDocument;
    let dir = crate::test_support::TestDirectory::new("dawn-seasonal-xp");
    crate::persistence::dawn_account::tests::create_fixture(&dir.0.join("player-state.db"));
    let path = dir.0.join("settings.json");
    let seed = json!({"version":6});
    let workspace = WorkspaceDocument::load(seed.clone(), &path, true);
    let catalog = catalog();
    let mut view = workspace.progression_view(0);
    let before = view.clone();
    for edit in [
        crate::app::progression::seasonal::Edit::Experience(100000),
        crate::app::progression::seasonal::Edit::Reset,
        crate::app::progression::seasonal::Edit::Mod {
            sale_index: 0,
            owned: true,
        },
    ] {
        assert!(
            crate::app::progression::seasonal::apply(&mut view, &catalog, edit)
                .unwrap_err()
                .contains("unavailable for Dawn")
        );
        assert_eq!(view, before);
    }
    assert!(
        claim(&mut view, &catalog, &reward())
            .unwrap_err()
            .contains("Dawn Season Pass reward claims")
    );
    assert!(Job::new(&view, &catalog, vec![0], Some(100)).is_err());
    assert_eq!(view, before);
}

#[test]
fn unsupported_rewards_and_locked_ranks_leave_claims_unchanged() {
    let catalog = catalog();
    let mut document = document();
    let before = document.clone();
    let mut reward = reward();
    reward.rewarded_at_progression_level = 3;
    assert!(
        claim(&mut document, &catalog, &reward)
            .unwrap_err()
            .contains("Reach Rank")
    );
    assert_eq!(document, before);
    reward.rewarded_at_progression_level = 2;
    reward.item_hash = 999;
    assert!(claim(&mut document, &catalog, &reward).is_err());
    assert_eq!(document, before);
    document
        .as_object_mut()
        .unwrap()
        .remove("_native_progression");
    let before = document.clone();
    assert!(claim(&mut document, &catalog, &reward).is_err());
    assert_eq!(document, before);
}

#[test]
fn season_pass_claims_persist_with_pending_rewards_in_the_native_account() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = catalog();
    let directory = crate::test_support::TestDirectory::new("pass-pending-rewards");
    let path = directory.0.join("settings.json");
    let settings = json!({"version":18,"future":17});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let dbpath = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&dbpath, 3);
    let mut workspace = WorkspaceDocument::load(settings.clone(), &path, false);
    let mut view = workspace.progression_view(0);
    crate::app::progression::seasonal::apply(
        &mut view,
        &catalog,
        crate::app::progression::seasonal::Edit::Experience(100000),
    )
    .unwrap();
    claim(&mut view, &catalog, &reward()).unwrap();
    workspace.apply_progression_view(0, view).unwrap();
    assert_eq!(workspace.json(), &settings);
    let pending = workspace
        .native_account()
        .unwrap()
        .pending_rewards()
        .to_vec();
    assert_eq!(
        pending.iter().map(|reward| reward.quantity).sum::<i32>(),
        201
    );
    crate::persistence::sqlite_account::tests::save_fixture_document(
        workspace.native_account_mut().unwrap(),
        &directory.0.join("backup.sqlite3"),
    );
    let reloaded = WorkspaceDocument::load(settings.clone(), &path, false);
    assert_eq!(reloaded.json(), &settings);
    assert_eq!(
        reloaded.native_account().unwrap().pending_rewards(),
        pending
    );
    assert!(
        collection_state_snapshot(&reloaded.progression_view(0))
            .unwrap()
            .flags
            .contains(&(1, 200))
    );
}
