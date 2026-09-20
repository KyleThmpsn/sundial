use super::*;

pub(crate) fn seed_debts(path: &std::path::Path) {
    Connection::open(path).unwrap().execute_batch(
        "INSERT INTO reward_debts(debt_id,account_soid,character_soid,mission_hash,runtime_epoch,session_id,run_id,definition_hash,quantity,credited,delivered) VALUES
        (1,'9EAA300100100100','9EAA300100100101',100,'0000000000000001','0000000000000002','0000000000000003',3159615086,50,0,0),
        (2,'9EAA300100100100','9EAA300100100101',100,'0000000000000001','0000000000000002','0000000000000004',3159615086,50,30,1);"
    ).unwrap();
}

#[test]
fn dawn_policies_round_trip_with_the_correct_capacity_and_no_sunrise_filters() {
    let dir = TestDirectory::new("dawn-payout-policies");
    let path = dir.0.join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&path);
    let settings = dir.0.join("settings.json");
    let mut workspace = WorkspaceDocument::load(json!({"version":6}), &settings, true);
    assert!(account::dismantle_rewards_available(&workspace));
    assert!(account::dismantle_rewards_editable(&workspace));
    assert_eq!(account::dismantle_reward_capacity(&workspace), Some(8));
    assert!(!account::filtered_dismantle_rewards(&workspace));
    assert!(!account::supports_combined_dismantle_gear_class(&workspace));
    for hash in 100..108 {
        account::add_dismantle_reward(&mut workspace, hash).unwrap();
    }
    assert!(account::add_dismantle_reward(&mut workspace, 108).is_err());
    assert!(account::add_dismantle_reward(&mut workspace, 100).is_err());
    let location = super::super::DismantleRewardLocation { index: 0 };
    assert!(
        account::apply_dismantle_reward_action(
            &mut workspace,
            location,
            super::super::DismantleRewardAction::SetPolicy {
                definition_hash: 100,
                quantity: 3,
                rarities: vec![],
                gear_class: Some(sundial_account::DismantleGearClass::Weapon),
                masterworked: None,
            }
        )
        .is_err()
    );
    account::apply_dismantle_reward_action(
        &mut workspace,
        location,
        super::super::DismantleRewardAction::SetPolicy {
            definition_hash: 100,
            quantity: 3,
            rarities: vec![],
            gear_class: None,
            masterworked: None,
        },
    )
    .unwrap();
    workspace.save_dawn().unwrap();
    let mut workspace = WorkspaceDocument::load(json!({"version":6}), &settings, true);
    assert_eq!(
        account::dismantle_rewards(&workspace).unwrap().unwrap()[0].quantity,
        3
    );
    account::apply_dismantle_reward_action(
        &mut workspace,
        location,
        super::super::DismantleRewardAction::Remove,
    )
    .unwrap();
    workspace.save_dawn().unwrap();
    let workspace = WorkspaceDocument::load(json!({"version":6}), &settings, true);
    let rewards = account::dismantle_rewards(&workspace).unwrap().unwrap();
    assert_eq!(rewards.len(), 7);
    assert_eq!(rewards[0].definition_hash, 101);
}

#[test]
fn dawn_delivery_history_and_identities_survive_profile_and_progression_edits() {
    let dir = TestDirectory::new("dawn-delivery-ledger");
    let path = dir.0.join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&path);
    seed_debts(&path);
    let settings = dir.0.join("settings.json");
    let mut workspace = WorkspaceDocument::load(json!({"version":6}), &settings, true);
    let debts = workspace.dawn_account().unwrap().reward_debts().to_vec();
    assert_eq!(debts.len(), 2);
    assert!(!debts[0].delivered);
    assert_eq!(debts[0].character_soid, 0x9EAA300100100101);
    assert_eq!(debts[0].run_id, 3);
    assert!(debts[1].delivered);
    assert_eq!(debts[1].credited, 30);
    account::add_profile_item(&mut workspace, 999, 2).unwrap();
    account::add_dismantle_reward(&mut workspace, 100).unwrap();
    let mut view = workspace.progression_view(0);
    view["state"]["unlocks"]["account_flag_runs"] = json!([[123, 1]]);
    workspace.apply_progression_view(0, view).unwrap();
    workspace.save_dawn().unwrap();
    let workspace = WorkspaceDocument::load(json!({"version":6}), &settings, true);
    assert_eq!(workspace.dawn_account().unwrap().reward_debts(), debts);
    assert_eq!(
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT value FROM metadata WHERE key='reward_epoch'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}
