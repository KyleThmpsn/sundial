use super::*;
use serde_json::json;

#[test]
fn stored_values_keep_unknown_native_banks_bytes_and_lanes_visible() {
    let catalog = Catalog::for_test(Vec::new(), Default::default());
    let document = json!({"_native_progression": {"family": [[0, 4, 255]], "unlocks": [[9, 3, 0, 42], [6, 15, 7, -123], [0, 19, 0, 1]]}});
    let rows = rows(&document, &catalog).unwrap();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.blocked.is_some()));
    assert!(
        rows.iter()
            .any(|row| row.kind == Kind::Unknown && row.value == 42)
    );
    assert!(
        rows.iter()
            .any(|row| row.key.lane == 7 && row.value == -123)
    );
}

#[test]
fn saved_rank_value_edits_preserve_the_other_lanes() {
    let catalog = Catalog::for_test(Vec::new(), Default::default());
    for native in [false, true] {
        let mut document = json!({"future": true, "state": {"unlocks": {"account_progressions": [[15, 9, 8, 7]]}}});
        if native {
            document["_native_progression"] =
                json!({"unlocks": [[6, 15, 0, 9], [6, 15, 1, 8], [6, 15, 2, 7]]});
        }
        let rows = rows(&document, &catalog).unwrap();
        let row = rows.iter().find(|row| row.key.lane == 1).unwrap();
        assert!(edits::apply(
            &mut document,
            row,
            Some(44),
            &mut UiState::default()
        ));
        assert_eq!(
            saved_progression_lanes(&document, ProgressionScope::Account, 15),
            Some([9, 44, 7])
        );
        assert_eq!(document["future"], true);
    }
}

#[test]
fn saved_values_round_trip_through_json_and_sqlite_without_losing_rank_lanes() {
    use crate::app::account_workspace::WorkspaceDocument;
    let catalog = Catalog::for_test(Vec::new(), Default::default());
    for native in [false, true] {
        let directory = crate::test_support::TestDirectory::new("saved-value-routing");
        let path = directory.0.join("settings.json");
        let json =
            json!({"version":if native {18} else {8},"state":{"characters":[]},"future":true});
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        if native {
            crate::persistence::sqlite_account::tests::create_fixture(
                &directory.0.join("data/investment.sqlite3"),
                3,
            );
        }
        let mut workspace = WorkspaceDocument::load(json.clone(), &path);
        let mut view = workspace.progression_view(0);
        assert!(super::super::mutations::set_progression_value(
            &mut view,
            "account_progressions",
            15,
            [9, 8, 7]
        ));
        let values = rows(&view, &catalog).unwrap();
        let row = values
            .iter()
            .find(|row| row.key.bank == 6 && row.key.slot == 15 && row.key.lane == 1)
            .unwrap();
        assert!(edits::apply(
            &mut view,
            row,
            Some(44),
            &mut UiState::default()
        ));
        workspace.apply_progression_view(0, view).unwrap();
        if native {
            assert_eq!(workspace.json(), &json);
            crate::persistence::sqlite_account::tests::save_fixture_document(
                workspace.native_account_mut().unwrap(),
                &directory.0.join("backup.sqlite3"),
            );
        } else {
            std::fs::write(&path, serde_json::to_vec(workspace.json()).unwrap()).unwrap();
        }
        let loaded = WorkspaceDocument::load(
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
            &path,
        );
        assert_eq!(
            saved_progression_lanes(&loaded.progression_view(0), ProgressionScope::Account, 15),
            Some([9, 44, 7])
        );
        assert_eq!(loaded.json()["future"], true);
    }
}
