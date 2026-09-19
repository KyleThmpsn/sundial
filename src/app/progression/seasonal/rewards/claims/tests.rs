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
fn season_pass_consumables_are_claimed_once_and_full_stacks_remain_unclaimed() {
    let catalog = super::super::super::super::rewards::tests::catalog()
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
            reward_grants: [(1000, RewardGrant::Item)].into(),
        });
    let mut document = document();
    document["_reward_context"]["consumables"] = json!([{"definition_hash":1000,"quantity":8}]);
    document["_reward_context"]["inventory"] = json!([]);
    document["_reward_context"]["next_serial"] = json!(1);
    let mut reward = reward();
    reward.item_hash = 1000;
    reward.quantity = 3;
    let before = document.clone();
    assert!(
        claim(&mut document, &catalog, &reward)
            .unwrap_err()
            .contains("stack cap")
    );
    assert_eq!(document, before);
    reward.quantity = 2;
    assert!(claim(&mut document, &catalog, &reward).unwrap());
    assert_eq!(
        document["_progression_consumables"][0]["quantity"],
        json!(2)
    );
    let after = document.clone();
    assert!(!claim(&mut document, &catalog, &reward).unwrap());
    assert_eq!(document, after);
}

#[test]
fn claims_queue_split_stacks_once_and_keep_the_claim_atomic() {
    let catalog = catalog();
    let mut document = document();
    assert!(claim(&mut document, &catalog, &reward()).unwrap());
    assert_eq!(
        document["_progression_rewards"],
        json!([{"kind":1,"hash":987,"quantity":99},{"kind":1,"hash":987,"quantity":99},{"kind":1,"hash":987,"quantity":3}])
    );
    assert!(
        collection_state_snapshot(&document)
            .unwrap()
            .flags
            .contains(&(1, 200))
    );
    let after = document.clone();
    assert!(!claim(&mut document, &catalog, &reward()).unwrap());
    assert_eq!(document, after);
    assert_eq!(document["future"], json!(17));
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
fn pass_completion_is_reviewed_and_rejects_concurrent_edits() {
    let catalog = catalog();
    let pass: ProgressionDefinition = serde_json::from_value(json!({"definition_index":40,"hash":40,"scope":"Account","scope_slot":40,"repeat_last_step":false,"reward_items":[reward()]})).unwrap();
    let mut document = document();
    let before = document.clone();
    let mut job = Job::new(&document, &catalog, vec![0], Some(100)).unwrap();
    while !job.step(&catalog, &pass) {}
    assert!(job.issues.is_empty());
    assert_eq!(job.claimed, 1);
    assert_eq!(job.queued(), 3);
    assert_eq!(document, before);
    let stale = job.clone();
    assert!(job.finish(&mut document).unwrap());
    assert_eq!(
        collection_state_snapshot(&document).unwrap().seasonal_xp(),
        9900000
    );
    let after = document.clone();
    assert!(stale.finish(&mut document).is_err());
    assert_eq!(document, after);
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

#[test]
#[ignore = "Requires SUNDIAL_PROGRESSION_INSTALL and installed packages"]
fn installed_pass_rewards_queue_for_each_class() {
    let install = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_PROGRESSION_INSTALL").expect("install path"),
    );
    let catalog = Catalog::load_or_scan_with_progress(
        &install,
        "examples/progression-ui-check/installed-catalog.json".into(),
        false,
        |progress| eprintln!("{}", progress.message),
    )
    .unwrap();
    let pass = catalog
        .progression_definitions()
        .iter()
        .find(|definition| usize::from(definition.definition_index) == PASS_PROGRESSION)
        .unwrap();
    for class in 0..3 {
        let mut document = json!({"_native_progression":{},"_reward_context":{"character":0,"character_count":1,"class":class,"consumables":[],"inventory":[],"next_serial":1},"future":17});
        let mut job = Job::new(
            &document,
            &catalog,
            (0..pass.reward_items.len()).collect(),
            Some(100),
        )
        .unwrap();
        while !job.step(&catalog, pass) {}
        eprintln!(
            "Class {class}: {} claims, {} pending entries, {} consumable grants, {} skipped",
            job.claimed,
            job.queued(),
            job.direct_count(),
            job.issues.len()
        );
        for (name, reason) in &job.issues {
            assert!(
                reason.contains("No inventory space")
                    || reason.contains("stack cap")
                    || reason.contains("different character class"),
                "{name}: {reason}"
            );
        }
        assert!(job.claimed > 100);
        assert!(job.queued() > 100);
        assert!(job.direct_count() > 0);
        job.finish(&mut document).unwrap();
        let mut repeat = Job::new(
            &document,
            &catalog,
            (0..pass.reward_items.len()).collect(),
            Some(100),
        )
        .unwrap();
        while !repeat.step(&catalog, pass) {}
        assert_eq!(repeat.claimed, 0);
        assert_eq!(repeat.queued(), 0);
        assert!(!repeat.changed());
    }
}
