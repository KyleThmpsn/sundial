use super::*;
use crate::catalog::{RecordProgress, RecordRuntime};

#[test]
fn consumable_triumph_claims_wait_for_confirmation_and_are_undoable() {
    use crate::app::account_workspace::WorkspaceDocument;
    let base = runtime_catalog(false);
    let mut record = base.records().unwrap()[0].clone();
    record.runtime.as_mut().unwrap().rewards = vec![(1000, 2), (9000, 1)];
    let catalog = crate::app::progression::rewards::tests::catalog()
        .with_test_progression(
            base.unlock_flag_definitions().to_vec(),
            base.unlock_value_definitions().to_vec(),
            vec![],
        )
        .with_test_objectives(base.objectives().to_vec())
        .with_test_records(vec![record.clone()]);
    let directory = crate::test_support::TestDirectory::new("triumph-consumable-confirm");
    let path = directory.0.join("settings.json");
    let settings = json!({"version":18});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    crate::persistence::sqlite_account::tests::create_fixture(
        &directory.0.join("data/investment.sqlite3"),
        3,
    );
    let mut workspace = WorkspaceDocument::load(settings.clone(), &path);
    let before = workspace.clone();
    let mut document = workspace.progression_view(0);
    let original = document.clone();
    let ctx = egui::Context::default();
    let mut state = State::default();
    let frame = |document: &mut Value, state: &mut State, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 680.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, document, &catalog, state, false);
                });
            },
        )
    };
    for button in ["Cancel", "Apply Anyway"] {
        state.job = Some(edit::Job::new(&document, vec![record.clone()], true));
        let mut output = egui::FullOutput::default();
        for _ in 0..8 {
            output = frame(&mut document, &mut state, vec![]);
        }
        assert!(state.ready.is_some());
        assert_eq!(document, original, "No direct grant before confirmation");
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(texts.iter().any(|text| {
            text.galley
                .job
                .text
                .contains("Sunrise Pending Rewards doesn’t support granting consumables.")
        }));
        let text = texts
            .iter()
            .find(|text| text.galley.job.text == button)
            .expect("Review button");
        let pos = text.pos + text.galley.size() * 0.5;
        crate::app::tests::capture::write(&ctx, &output, "consumable-reward-confirmation");
        for pressed in [true, false] {
            frame(
                &mut document,
                &mut state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        if button == "Cancel" {
            assert_eq!(document, original);
        }
    }
    assert_ne!(document, original);
    workspace.apply_progression_view(0, document).unwrap();
    assert_eq!(
        workspace.native_account().unwrap().character_stacks(0)[0].quantity,
        2
    );
    assert_eq!(
        workspace.native_account().unwrap().pending_rewards().len(),
        1
    );
    let mut repeat = workspace.progression_view(0);
    edit::apply_record(&mut repeat, &catalog, &record, true).unwrap();
    assert!(repeat.get("_progression_consumables").is_none());
    crate::persistence::sqlite_account::tests::save_fixture_document(
        workspace.native_account_mut().unwrap(),
        &directory.0.join("claimed.sqlite3"),
    );
    let mut undo = before;
    undo.rebase_account_revision_from(&workspace);
    crate::persistence::sqlite_account::tests::save_fixture_document(
        undo.native_account_mut().unwrap(),
        &directory.0.join("undo.sqlite3"),
    );
    let reloaded = WorkspaceDocument::load(settings, &path);
    assert_eq!(reloaded.progression_view(0), original);
}

fn tiered_flag_catalog() -> Catalog {
    let base = runtime_catalog(true);
    let mut record = base.records().unwrap()[0].clone();
    record.completion_flag = Some(0);
    record.runtime.as_mut().unwrap().progress = vec![
        RecordProgress {
            objective: 0,
            slot: 2746,
            threshold: 1000,
        },
        RecordProgress {
            objective: 0,
            slot: 2747,
            threshold: 1000,
        },
    ];
    let mut values = base.unlock_value_definitions().to_vec();
    values[4].compact_slot = Some(2747);
    let flags = base.unlock_flag_definitions().to_vec();
    base.with_test_progression(flags, values, vec![])
        .with_test_records(vec![record])
}

#[test]
fn flagged_tiers_keep_progress_separate_from_the_claimed_count() {
    let catalog = tiered_flag_catalog();
    let record = &catalog.records().unwrap()[0];
    for native in [false, true] {
        let mut document = json!({"state":{"unlocks":{}},"future":17});
        if native {
            document["_native_progression"] = json!({});
        }
        edit::apply_record(&mut document, &catalog, record, true).unwrap();
        let snapshot = collection_state_snapshot(&document).unwrap();
        assert_eq!(snapshot.values[&(1, 2746)], 1000);
        assert_eq!(snapshot.values[&(1, 2747)], 2);
        assert_eq!(snapshot.values[&(1, 2115)], 30);
        let after = document.clone();
        edit::apply_record(&mut document, &catalog, record, true).unwrap();
        assert_eq!(document, after);
        edit::apply_record(&mut document, &catalog, record, false).unwrap();
        assert_eq!(document["future"], json!(17));
    }
}

fn runtime_catalog(interval: bool) -> Catalog {
    catalog()
        .with_test_progression(
            vec![UnlockDefinition {
                code: 1,
                compact_slot: Some(4),
                ..Default::default()
            }],
            [10, 2746, 2747, 2115, 15]
                .into_iter()
                .map(|slot| UnlockDefinition {
                    code: 1,
                    compact_slot: Some(slot),
                    ..Default::default()
                })
                .collect(),
            vec![],
        )
        .with_test_records(vec![RecordDefinition {
            hash: 100,
            name: "First Victory".into(),
            objectives: vec![0],
            completion_flag: (!interval).then_some(0),
            redeemed_intervals: interval.then_some(4),
            interval_count: if interval { 2 } else { 0 },
            runtime: Some(RecordRuntime {
                progress: vec![RecordProgress {
                    objective: 0,
                    slot: 2746,
                    threshold: 5,
                }],
                score: if interval { 0 } else { 25 },
                interval_scores: if interval { vec![10, 20] } else { vec![] },
                rewards: vec![],
                interval_items: vec![],
            }),
            ..Default::default()
        }])
}

#[test]
fn skipping_conflicts_rebuilds_score_rewards_and_freed_inventory_capacity() {
    let base = runtime_catalog(true);
    let mut conflicting = base.records().unwrap()[0].clone();
    conflicting.name = "Conflicting Tiers".into();
    conflicting.completion_flag = Some(0);
    conflicting.interval_count = 5;
    let runtime = conflicting.runtime.as_mut().unwrap();
    runtime.interval_scores = vec![10; 5];
    runtime.interval_items = (1000..1005).map(Some).collect();

    let mut retained = runtime_catalog(false).records().unwrap()[0].clone();
    retained.hash = 200;
    retained.name = "Retained Counter".into();
    retained.completion_flag = Some(1);
    retained.runtime.as_mut().unwrap().progress = vec![RecordProgress {
        objective: 0,
        slot: 15,
        threshold: 10,
    }];
    let mut reward = retained.clone();
    reward.hash = 300;
    reward.name = "Reward After Capacity Is Freed".into();
    reward.completion_flag = Some(2);
    reward.runtime.as_mut().unwrap().score = 7;
    reward.runtime.as_mut().unwrap().rewards = vec![(1005, 1), (9000, 2)];
    let records = vec![conflicting, retained, reward];
    let catalog = crate::app::progression::rewards::tests::catalog()
        .with_test_progression(
            (4..7)
                .map(|slot| UnlockDefinition {
                    code: 1,
                    compact_slot: Some(slot),
                    ..Default::default()
                })
                .collect(),
            base.unlock_value_definitions().to_vec(),
            vec![],
        )
        .with_test_objectives(base.objectives().to_vec())
        .with_test_records(records.clone());
    let mut document = json!({
        "future": 42, "_native_progression": {},
        "state":{"unlocks":{"objective_values":[[2115,100]]}},
        "_reward_context":{"character":0,"character_count":1,"class":0,"consumables":[],"inventory":[],"equipment":[],"next_serial":1},
        "_progression_rewards":[{"kind":1,"hash":9000,"quantity":1}]
    });
    let original = document.clone();
    let mut job = edit::Job::new(&document, records, true);
    while !job.step(&catalog) {}
    assert_eq!(document, original, "Review must not mutate the account");
    assert_eq!(job.conflicts.len(), 1);
    assert!(
        job.issues.is_empty(),
        "The capacity failure must be retried"
    );
    assert_eq!(job.supported_count(), 2);
    assert_eq!(job.direct_count(), 1);
    assert_eq!(job.queued_count(), 1);
    assert_eq!(job.finish(&mut document, &catalog).unwrap(), 2);
    let snapshot = collection_state_snapshot(&document).unwrap();
    assert!(
        !snapshot.flags.contains(&(1, 4)),
        "Skipped completion flag must be discarded"
    );
    assert!(snapshot.flags.contains(&(1, 5)) && snapshot.flags.contains(&(1, 6)));
    assert_eq!(
        snapshot.values[&(1, 2115)],
        132,
        "Skipped score must be discarded"
    );
    assert_eq!(
        document["_progression_consumables"],
        json!([{"hash":1005,"quantity":1,"maximum":10}])
    );
    assert_eq!(
        document["_progression_rewards"],
        json!([
            {"kind":1,"hash":9000,"quantity":1}, {"kind":1,"hash":9000,"quantity":2}
        ])
    );
    assert_eq!(document["future"], 42);
}

#[test]
fn native_record_progress_score_and_stage_claims_round_trip_both_accounts() {
    for native in [false, true] {
        for interval in [false, true] {
            assert_runtime_round_trip(native, interval);
        }
    }
}

fn assert_runtime_round_trip(native: bool, interval: bool) {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = runtime_catalog(interval);
    let record = &catalog.records().unwrap()[0];
    let directory = crate::test_support::TestDirectory::new("record-runtime-routing");
    let path = directory.0.join("settings.json");
    let json = json!({"version":if native {18}else{8},"future":true,"state":{"characters":[]}});
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    if native {
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("data/investment.sqlite3"),
            3,
        );
    }
    let mut workspace = WorkspaceDocument::load(json.clone(), &path);
    let mut view = workspace.progression_view(0);
    crate::app::progression::mutations::set_unlock_value(&mut view, "objective_values", 10, 91);
    crate::app::progression::mutations::set_unlock_value(&mut view, "objective_values", 2115, 100);
    workspace.apply_progression_view(0, view).unwrap();
    for (step, complete) in [true, true, false, false].into_iter().enumerate() {
        let mut view = workspace.progression_view(0);
        edit::apply_record(&mut view, &catalog, record, complete).unwrap();
        workspace.apply_progression_view(0, view).unwrap();
        if native {
            assert_eq!(workspace.json(), &json);
            crate::persistence::sqlite_account::tests::save_fixture_document(
                workspace.native_account_mut().unwrap(),
                &directory.0.join(format!("backup-{step}.sqlite3")),
            );
        } else {
            std::fs::write(&path, serde_json::to_vec(workspace.json()).unwrap()).unwrap();
        }
        workspace = WorkspaceDocument::load(
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
            &path,
        );
        let snapshot = collection_state_snapshot(&workspace.progression_view(0)).unwrap();
        assert_eq!(
            snapshot.values.get(&(1, 10)),
            Some(&91),
            "The objective source counter must stay unchanged"
        );
        assert_eq!(
            snapshot.values.get(&(1, 2746)).copied().unwrap_or(0),
            if complete { 5 } else { 0 }
        );
        assert_eq!(
            snapshot.values.get(&(1, 2115)).copied().unwrap_or(0),
            100 + if complete {
                if interval { 30 } else { 25 }
            } else {
                0
            }
        );
        assert_eq!(
            rows(&catalog, &snapshot)[0].status,
            if complete {
                Status::Completed
            } else {
                Status::NotCompleted
            }
        );
    }
}

#[test]
fn rewards_are_queued_once_with_claims_and_undo_restores_both() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = reward_catalog(false, 3, false, 3);
    let directory = crate::test_support::TestDirectory::new("triumph-pending-rewards");
    let path = directory.0.join("settings.json");
    let settings = json!({"version":18,"future":true});
    std::fs::write(&path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let dbpath = directory.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&dbpath, 3);
    let mut workspace = WorkspaceDocument::load(settings.clone(), &path);
    let before = workspace.clone();
    let record = &catalog.records().unwrap()[0];
    for _ in 0..2 {
        let mut view = workspace.progression_view(0);
        edit::apply_record(&mut view, &catalog, record, true).unwrap();
        workspace.apply_progression_view(0, view).unwrap();
        let rewards = workspace.native_account().unwrap().pending_rewards();
        assert_eq!(rewards.len(), 1);
        assert_eq!(rewards[0].definition_hash, 987);
        assert_eq!(rewards[0].quantity, 3);
        assert_eq!(rewards[0].character_slot, 0);
    }
    crate::persistence::sqlite_account::tests::save_fixture_document(
        workspace.native_account_mut().unwrap(),
        &directory.0.join("claimed.sqlite3"),
    );
    let loaded = WorkspaceDocument::load(settings.clone(), &path);
    assert_eq!(loaded.native_account().unwrap().pending_rewards().len(), 1);
    assert_eq!(
        rows(
            &catalog,
            &collection_state_snapshot(&loaded.progression_view(0)).unwrap()
        )[0]
        .status,
        Status::Completed
    );
    // History restores the full account document, including the delivery queue.
    let mut undo = before;
    undo.rebase_account_revision_from(&workspace);
    crate::persistence::sqlite_account::tests::save_fixture_document(
        undo.native_account_mut().unwrap(),
        &directory.0.join("undo.sqlite3"),
    );
    let loaded = WorkspaceDocument::load(settings, &path);
    assert!(
        loaded
            .native_account()
            .unwrap()
            .pending_rewards()
            .is_empty()
    );
    assert_ne!(
        rows(
            &catalog,
            &collection_state_snapshot(&loaded.progression_view(0)).unwrap()
        )[0]
        .status,
        Status::Completed
    );
    let mut legacy = json!({"version":8});
    let original = legacy.clone();
    assert!(
        edit::apply_record(&mut legacy, &catalog, record, true)
            .unwrap_err()
            .contains("no Pending Rewards queue")
    );
    assert_eq!(legacy, original);
}

#[test]
fn clearing_a_claim_preserves_unrelated_score_and_overflow_rolls_back() {
    let catalog = runtime_catalog(false);
    let record = &catalog.records().unwrap()[0];
    let mut document = json!({"state":{"unlocks":{"objective_values":[[2115,i32::MAX]]}}});
    let before = document.clone();
    assert!(
        edit::apply_record(&mut document, &catalog, record, true)
            .unwrap_err()
            .contains("score")
    );
    assert_eq!(document, before);
}

#[test]
fn completion_reconciles_overrides_with_the_native_claim_and_progress() {
    let catalog = runtime_catalog(false);
    let record = &catalog.records().unwrap()[0];
    let mut document = json!({"state":{"unlocks":{"account_flag_runs":[[4,1]],"objective_values":[[2746,5],[2115,25]]},"investment":{"family5_flag_overrides":[[0,1]],"family5_value_overrides":[[1,0]]}}});
    edit::apply_record(&mut document, &catalog, record, true).unwrap();
    let snapshot = collection_state_snapshot(&document).unwrap();
    assert!(snapshot.flag_overrides.is_empty());
    assert!(snapshot.value_overrides.is_empty());
    assert_eq!(snapshot.values.get(&(1, 2115)), Some(&25));
}

fn reward_catalog(interval: bool, quantity: i32, instanced: bool, class_type: u64) -> Catalog {
    use crate::catalog::{
        InventoryMetadata, InventoryScope, ItemDef, ItemPackageMetadata, ItemStackability,
    };
    let base = runtime_catalog(interval);
    let mut records = base.records().unwrap().to_vec();
    let runtime = records[0].runtime.as_mut().unwrap();
    runtime.rewards = vec![(7, quantity)];
    runtime.interval_items = vec![Some(7), Some(7)];
    Catalog::for_test_with_inventory(
        vec![ItemDef {
            hash: 987,
            name: "Triumph Material".into(),
            type_name: "Material".into(),
            bucket_hash: 0,
            class_type,
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
                scope: if instanced {
                    InventoryScope::Character
                } else {
                    InventoryScope::Profile
                },
                native_bucket_id: 0,
                stackability: if instanced {
                    ItemStackability::Instanced
                } else {
                    ItemStackability::Stackable
                },
                max_stack_size: Some(99),
                bucket_capacity: Some(100),
            },
        )]
        .into(),
    )
    .with_test_progression(
        base.unlock_flag_definitions().to_vec(),
        base.unlock_value_definitions().to_vec(),
        vec![],
    )
    .with_test_objectives(base.objectives().to_vec())
    .with_test_records(records)
}

#[test]
fn pending_rewards_split_stacks_and_grant_only_new_stages() {
    for (interval, flag) in [(false, false), (true, false), (true, true)] {
        let catalog = reward_catalog(interval, 201, false, 3);
        let mut record = catalog.records().unwrap()[0].clone();
        if flag {
            record.completion_flag = Some(0);
        }
        let mut document = json!({"_native_progression":{},"_reward_context":{"character":0,"character_count":1,"class":3,"pending":[]},"state":{"unlocks":{"objective_values":[[15,1],[2115,10]]}}});
        edit::apply_record(&mut document, &catalog, &record, true).unwrap();
        let rewards = document["_progression_rewards"].as_array().unwrap();
        assert_eq!(
            rewards
                .iter()
                .map(|row| row["quantity"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            if interval { vec![1] } else { vec![99, 99, 3] }
        );
        let original = document.clone();
        edit::apply_record(&mut document, &catalog, &record, true).unwrap();
        assert_eq!(document, original);
    }
}

#[test]
fn instanced_rewards_check_class_before_changing_the_triumph() {
    let catalog = reward_catalog(false, 2, true, 1);
    let mut document = json!({"_native_progression":{},"_reward_context":{"character":0,"character_count":1,"class":0,"pending":[]}});
    let before = document.clone();
    assert!(
        edit::apply_record(
            &mut document,
            &catalog,
            &catalog.records().unwrap()[0],
            true
        )
        .unwrap_err()
        .contains("different character class")
    );
    assert_eq!(document, before);
    document["_reward_context"]["class"] = json!(1);
    edit::apply_record(
        &mut document,
        &catalog,
        &catalog.records().unwrap()[0],
        true,
    )
    .unwrap();
    assert_eq!(
        document["_progression_rewards"],
        json!([{"kind":0,"hash":987,"quantity":1},{"kind":0,"hash":987,"quantity":1}])
    );
}

#[test]
fn ordinary_claims_do_not_zero_an_overlapping_interval_field() {
    let catalog = runtime_catalog(false);
    let mut record = catalog.records().unwrap()[0].clone();
    record.redeemed_intervals = Some(1);
    let mut document = json!({});
    edit::apply_record(&mut document, &catalog, &record, true).unwrap();
    let snapshot = collection_state_snapshot(&document).unwrap();
    assert_eq!(snapshot.values.get(&(1, 2746)), Some(&5));
    assert_eq!(snapshot.values.get(&(1, 2115)), Some(&25));
}

#[test]
fn a_reserved_progress_lane_can_hold_the_claimed_stage_count() {
    let catalog = runtime_catalog(true);
    let mut record = catalog.records().unwrap()[0].clone();
    record
        .runtime
        .as_mut()
        .unwrap()
        .progress
        .push(RecordProgress {
            slot: 15,
            objective: 0,
            threshold: 5,
        });
    let mut document = json!({});
    edit::apply_record(&mut document, &catalog, &record, true).unwrap();
    let snapshot = collection_state_snapshot(&document).unwrap();
    assert_eq!(snapshot.values.get(&(1, 2746)), Some(&5));
    assert_eq!(snapshot.values.get(&(1, 15)), Some(&2));
    assert_eq!(snapshot.values.get(&(1, 2115)), Some(&30));
}
