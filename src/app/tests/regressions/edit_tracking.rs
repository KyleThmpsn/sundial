//! History follows successful actions, independently of frames and status text.
use super::*;

fn level(app: &SundialApp) -> i64 {
    account_workspace::equipped_item_snapshots(&app.document, 0)
        .unwrap()
        .iter()
        .find(|item| item.slot == "kinetic")
        .unwrap()
        .level
        .unwrap()
}

#[test]
fn equipment_actions_record_history_immediately_and_no_ops_keep_redo() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("explicit-edit-history");
        let mut app = state_recovery::for_source(&directory, sqlite);
        let original = app.document.clone();
        let original_level = level(&app);
        app.set_status("An unrelated message. Click Save to try again", true);
        app.select_equipment_level(0, "kinetic", original_level + 1);
        assert_eq!(app.undo_history.len(), 1);
        assert_eq!(app.undo_history[0].label, "Equipment Power Updated");
        assert!(app.dirty);
        assert!(app.document_repaint_pending);
        app.set_status("Catalog refreshed", false);
        app.undo();
        assert_eq!(app.document, original);
        assert_eq!(app.status, "Undid: Equipment Power Updated");
        app.document_repaint_pending = false;
        app.select_equipment_level(0, "kinetic", original_level);
        assert!(!app.dirty);
        assert!(!app.document_repaint_pending);
        assert_eq!(app.redo_history.len(), 1);
        assert!(app.undo_history.is_empty());
        app.redo();
        assert_eq!(level(&app), original_level + 1);
    }
}

#[test]
fn saving_updates_the_edit_baseline_without_adding_history_for_either_source() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("saved-edit-history");
        let mut app = state_recovery::for_source(&directory, sqlite);
        let original_level = level(&app);
        app.select_equipment_level(0, "kinetic", original_level + 1);
        if let Some(document) = app.document.native_account_mut() {
            crate::persistence::sqlite_account::tests::save_fixture_document(
                document,
                &directory.0.join("backup.sqlite3"),
            );
        } else {
            std::fs::write(
                &app.settings_path,
                app.persisted_document.json().to_string(),
            )
            .unwrap();
            settings::save_test_json_checked(
                &app.settings_path,
                app.document.json(),
                app.persisted_document.json(),
                true,
                &directory.0.join("backups"),
            )
            .unwrap();
        }
        app.mark_document_saved();
        assert_eq!(app.undo_history.len(), 1);
        assert!(!app.dirty);
        app.select_equipment_level(0, "kinetic", original_level + 2);
        assert_eq!(app.undo_history.len(), 2);
        app.undo();
        assert_eq!(app.document, app.persisted_document);
        assert!(!app.dirty);
        app.undo();
        assert_eq!(level(&app), original_level);
        assert!(app.dirty);
    }
}
