use super::*;
mod runtime;
use crate::catalog::{ObjectiveOwnerKind, ProgressionContextKind};
use serde_json::json;

fn catalog() -> Catalog {
    let owner = crate::catalog::ObjectiveOwnerDef {
        hash: 100,
        kind: ObjectiveOwnerKind::Record,
        name: "First Victory".into(),
        type_name: "Record".into(),
        description: String::new(),
        traits: Vec::new(),
        paths: vec![vec!["Crucible".into(), "Triumphs".into()]],
    };
    let context = crate::catalog::ProgressionContextDef {
        hash: owner.hash,
        kind: ProgressionContextKind::Record,
        name: owner.name.clone(),
        type_name: String::new(),
        description: String::new(),
        paths: owner.paths.clone(),
        condition_programs: Vec::new(),
        direct_references: vec!["Record completion flag".into()],
    };
    Catalog::for_test(Vec::new(), Default::default())
        .with_test_progression(
            vec![UnlockDefinition {
                code: 1,
                compact_slot: Some(0),
                tested_by: vec![context],
                ..Default::default()
            }],
            vec![UnlockDefinition {
                hash: 33,
                code: 1,
                compact_slot: Some(0),
                ..Default::default()
            }],
            Vec::new(),
        )
        .with_test_objectives(vec![ObjectiveDef {
            hash: 33,
            completion_value: 5,
            related_unlock_value_definition_index: Some(0),
            owners: vec![owner],
            ..Default::default()
        }])
        .with_test_records(vec![RecordDefinition {
            hash: 100,
            name: "First Victory".into(),
            paths: vec![vec!["Crucible".into(), "Triumphs".into()]],
            objectives: vec![0],
            completion_flag: Some(0),
            ..Default::default()
        }])
}

#[test]
fn triumphs_merge_record_references_and_distinguish_objectives_from_completion() {
    let catalog = catalog();
    assert_eq!(catalog.records().unwrap().len(), 1);
    for native in [false, true] {
        let mut document = json!({"state":{"unlocks":{"objective_values":[[0,5]]}}});
        if native {
            document["_native_progression"] = json!({});
        }
        let result = rows(&catalog, &collection_state_snapshot(&document).unwrap());
        assert_eq!(result[0].status, Status::ObjectivesComplete);
        assert_eq!(result[0].completed_objectives, 1);
        assert!(
            set_collection_flag(
                &mut document,
                0,
                catalog.unlock_flag_definition(0).unwrap(),
                true
            )
            .changed()
        );
        let result = rows(&catalog, &collection_state_snapshot(&document).unwrap());
        assert_eq!(result[0].status, Status::Completed);
    }
}

#[test]
fn countdown_objectives_and_unresolved_records_are_not_assumed_complete() {
    let objective = ObjectiveDef {
        completion_value: 0,
        is_counting_downward: true,
        ..Default::default()
    };
    assert!(objective_complete(&objective, 0));
    assert!(!objective_complete(&objective, 1));
    let catalog = catalog();
    let result = rows(&catalog, &collection_state_snapshot(&json!({})).unwrap());
    assert_eq!(result[0].status, Status::NotCompleted);
}

#[test]
fn authoritative_records_include_unlinked_and_unnamed_rows() {
    let catalog = catalog().with_test_records(vec![
        RecordDefinition {
            hash: 44,
            name: "Unlinked Triumph".into(),
            ..Default::default()
        },
        RecordDefinition {
            index: 1,
            hash: 45,
            ..Default::default()
        },
    ]);
    let rows = rows(&catalog, &collection_state_snapshot(&json!({})).unwrap());
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].name, "Unlinked Triumph");
    assert_eq!(rows[1].name, "Unnamed Triumph #1");
    assert!(rows.iter().all(|row| row.status == Status::Unresolved));
}

#[test]
fn complete_and_reset_preserve_unknown_fields_and_existing_excess_progress() {
    let catalog = catalog();
    for native in [false, true] {
        let mut document = json!({"future": {"preserve": 42}, "state": {"unlocks": {"objective_values": [[0, 12]], "future": true}}});
        if native {
            document["_native_progression"] = json!({});
        }
        let record = &catalog.records().unwrap()[0];
        edit::apply_record(&mut document, &catalog, record, true).unwrap();
        let snapshot = collection_state_snapshot(&document).unwrap();
        assert_eq!(snapshot.evaluated_flag(0, &catalog), Some(true));
        assert_eq!(snapshot.evaluated_value(0, &catalog), Some(12));
        edit::apply_record(&mut document, &catalog, record, false).unwrap();
        let snapshot = collection_state_snapshot(&document).unwrap();
        assert_eq!(snapshot.evaluated_flag(0, &catalog), Some(false));
        assert_eq!(snapshot.evaluated_value(0, &catalog), Some(0));
        assert_eq!(document["future"]["preserve"], 42);
        assert_eq!(document["state"]["unlocks"]["future"], true);
    }
}

#[test]
fn batches_skip_shared_counter_conflicts_and_reject_concurrent_changes() {
    let mut catalog = catalog();
    let first = catalog.records().unwrap()[0].clone();
    let mut objectives = catalog.objectives().to_vec();
    objectives.push(ObjectiveDef {
        completion_value: 0,
        is_counting_downward: true,
        related_unlock_value_definition_index: Some(0),
        ..Default::default()
    });
    catalog = catalog.with_test_objectives(objectives);
    let second = RecordDefinition {
        hash: 200,
        name: "Countdown".into(),
        objectives: vec![1],
        ..Default::default()
    };
    let mut document = json!({"future": 1, "state":{"unlocks":{"objective_values":[[0,5]]}}});
    let before = document.clone();
    let mut job = edit::Job::new(&document, vec![first.clone(), second], true);
    while !job.step(&catalog) {}
    assert!(job.issues.is_empty());
    assert_eq!(job.conflicts.len(), 1);
    assert_eq!(job.conflicts[0].name, "First Victory");
    assert_eq!(job.supported_count(), 1);
    assert_eq!(document, before);
    assert_eq!(job.finish(&mut document, &catalog).unwrap(), 1);
    let result = collection_state_snapshot(&document).unwrap();
    assert_eq!(result.evaluated_flag(0, &catalog), Some(false));
    assert_eq!(result.evaluated_value(0, &catalog), Some(0));
    let mut job = edit::Job::new(&document, vec![first], true);
    while !job.step(&catalog) {}
    document["future"] = json!(2);
    let concurrent = document.clone();
    assert!(
        job.finish(&mut document, &catalog)
            .unwrap_err()
            .contains("account changed")
    );
    assert_eq!(document, concurrent);
}

#[test]
fn triumph_edits_persist_in_json_and_sqlite_accounts() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = catalog();
    let record = &catalog.records().unwrap()[0];
    for native in [false, true] {
        let directory = crate::test_support::TestDirectory::new("triumph-account-routing");
        let path = directory.0.join("settings.json");
        let json = json!({"version": if native {18} else {8}, "state": {"characters": [], "unlocks": {"future": true}}});
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        if native {
            crate::persistence::sqlite_account::tests::create_fixture(
                &directory.0.join("data/investment.sqlite3"),
                3,
            );
        }
        let mut workspace = WorkspaceDocument::load(json.clone(), &path, false);
        for complete in [true, false] {
            let mut view = workspace.progression_view(0);
            edit::apply_record(&mut view, &catalog, record, complete).unwrap();
            workspace.apply_progression_view(0, view).unwrap();
            if native {
                assert_eq!(workspace.json(), &json);
                crate::persistence::sqlite_account::tests::save_fixture_document(
                    workspace.native_account_mut().unwrap(),
                    &directory.0.join(format!("backup-{complete}.sqlite3")),
                );
            } else {
                std::fs::write(&path, serde_json::to_vec(workspace.json()).unwrap()).unwrap();
            }
            workspace = WorkspaceDocument::load(
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
                &path,
                false,
            );
            let snapshot = collection_state_snapshot(&workspace.progression_view(0)).unwrap();
            assert_eq!(snapshot.evaluated_flag(0, &catalog), Some(complete));
            assert_eq!(
                snapshot.evaluated_value(0, &catalog),
                Some(if complete { 5 } else { 0 })
            );
        }
    }
}
