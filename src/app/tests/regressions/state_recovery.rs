//! Regressions for document transitions and the runtime settings projection.
use super::*;
use serde_json::json;
use sundial_account::{AccountSettingKey, AccountSettingValue, AccountSettingsCommand};

pub(super) fn for_source(directory: &TestDirectory, sqlite: bool) -> SundialApp {
    let mut app = app(directory.0.clone());
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
    ))
    .unwrap();
    if sqlite {
        crate::persistence::sqlite_account::tests::create_fixture(
            &directory.0.join("data/investment.sqlite3"),
            3,
        );
        source["version"] = json!(18);
    }
    app.document = WorkspaceDocument::load(source, &app.settings_path, false);
    app.persisted_document = app.document.clone();
    app.sync_raw_json();
    app
}

#[test]
fn reverting_a_setting_clears_unsaved_state_for_both_sources() {
    for sqlite in [false, true] {
        let directory = TestDirectory::new("reverted-dirty-state");
        let mut app = for_source(&directory, sqlite);
        let original =
            account_workspace::account_settings_map(&app.document).unwrap()["display"]["show_fps"]
                .as_bool()
                .unwrap();
        for value in [!original, original] {
            assert!(
                account_workspace::apply_account_settings(
                    &mut app.document,
                    vec![AccountSettingsCommand::Set {
                        key: AccountSettingKey::known_preference("show_fps").unwrap(),
                        value: AccountSettingValue::Boolean(value),
                    },]
                )
                .unwrap()
            );
            app.record_edit("Show FPS Updated");
            assert_eq!(app.has_unsaved_changes(), value != original);
        }
        assert_eq!(app.document, app.persisted_document);
        app.raw_json.push(' ');
        app.json_editor.mark_modified();
        assert!(
            app.has_unsaved_changes(),
            "An unapplied JSON draft still needs attention"
        );
    }
}

#[test]
fn undo_clamps_character_selection_after_undoing_added_characters() {
    let directory = TestDirectory::new("undo-character-selection");
    let mut app = for_source(&directory, false);
    app.document.json_mut()["state"]["characters"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    app.persisted_document = app.document.clone();
    app.sync_raw_json();
    let before = app.document.clone();
    let mut added = before.json().clone();
    let mut character = added["state"]["characters"][0].clone();
    character["soid"] = json!(99);
    character["equipment"] = json!({});
    character["inventory"] = json!([]);
    added["state"]["characters"]
        .as_array_mut()
        .unwrap()
        .push(character);
    app.raw_json = added.to_string();
    assert!(app.apply_raw_json(), "{}", app.status);
    app.selected_character = 1;
    app.undo();
    assert_eq!(app.character_count(), 1);
    assert_eq!(app.selected_character, 0);
    assert!(!app.dirty);
    app.redo();
    assert_eq!(app.character_count(), 2);
    assert!(app.selected_character < app.character_count());
}

#[test]
fn raw_json_cannot_change_the_active_account_source() {
    for sqlite in [true, false] {
        let directory = TestDirectory::new("raw-json-account-source");
        let mut app = for_source(&directory, sqlite);
        if !sqlite {
            let source = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
            ))
            .unwrap();
            app.document = WorkspaceDocument::load(source, &app.settings_path, false);
            app.persisted_document = app.document.clone();
            app.sync_raw_json();
        }
        let original = app.document.clone();
        let mut changed = app.document.json().clone();
        changed["version"] = json!(if sqlite { 8 } else { 18 });
        app.raw_json = changed.to_string();
        app.json_editor.mark_modified();
        assert!(
            !app.apply_raw_json(),
            "A source-changing draft must not enter the workspace"
        );
        assert_eq!(app.document, original);
        assert!(app.json_editor.has_unapplied_changes());
        assert!(app.status.contains("account source"), "{}", app.status);
        assert!(!app.settings_path.exists());
    }
}

#[test]
fn native_runtime_projection_preserves_inactive_container_values() {
    for parent in ["state", "server"] {
        for container in [
            None,
            Some(Value::Null),
            Some(json!("legacy")),
            Some(json!([])),
            Some(json!({"opaque": true})),
        ] {
            let directory = TestDirectory::new("runtime-inactive-containers");
            let mut app = for_source(&directory, true);
            rusqlite::Connection::open(directory.0.join("data/investment.sqlite3"))
                .unwrap()
                .execute("INSERT INTO entitlements VALUES(0, 'retained', 1)", [])
                .unwrap();
            let mut source = json!({"version": 18, "steam": {"user": {"persona_name": "Before"}}});
            if let Some(value) = container {
                source[parent] = value;
            }
            let mut document = WorkspaceDocument::load(source.clone(), &app.settings_path, false);
            let original = document.clone();
            let mut view = document.runtime_view();
            view["steam"]["user"]["persona_name"] = json!("After");
            document.apply_runtime_view(view).unwrap();
            source["steam"]["user"]["persona_name"] = json!("After");
            assert_eq!(document.json(), &source);
            assert!(!document.account_changed_from(&original));
            let mut view = document.runtime_view();
            view["state"]["characters"][0]["level"] = json!(42);
            document.apply_runtime_view(view).unwrap();
            assert_eq!(document.json(), &source);
            app.document = document;
            app.document.save_sqlite().unwrap();
            let reopened = WorkspaceDocument::load(source, &app.settings_path, false);
            assert_eq!(
                reopened.runtime_view()["state"]["characters"][0]["level"],
                42
            );
            assert_eq!(
                reopened.runtime_view()["server"]["entitlements"],
                json!([{"name": "retained", "owned": "handle"}])
            );
        }
    }
}

#[test]
fn incomplete_runtime_draft_cannot_erase_native_account_data() {
    let directory = TestDirectory::new("runtime-incomplete-draft");
    let mut app = for_source(&directory, true);
    let before = app.document.clone();
    let mut view = app.document.runtime_view();
    view["state"]["characters"][0]["level"] = json!(99);
    view["server"]
        .as_object_mut()
        .unwrap()
        .remove("entitlements");
    assert!(app.document.apply_runtime_view(view).is_err());
    assert_eq!(app.document, before);
}
