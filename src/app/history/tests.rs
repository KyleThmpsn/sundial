//! History follows successful actions, independently of frames and status text.
use crate::app::tests::driver::*;
use crate::app::*;

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
fn saving_updates_the_edit_baseline_without_adding_history_for_either_source() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("saved-edit-history");
        let mut app = for_source(&directory, sqlite);
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
