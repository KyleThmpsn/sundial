//! Document transitions and the runtime settings projection on both account sources.
use crate::app::tests::driver::*;
use crate::app::*;
use serde_json::json;

#[test]
fn raw_json_cannot_change_the_active_account_source() {
    for sqlite in [true, false] {
        let directory = TestDirectory::new("raw-json-account-source");
        let mut app = for_source(&directory, sqlite);
        if !sqlite {
            let source = serde_json::from_str(include_str!(
                "../../../tests/fixtures/sunrise-v16-1120748-defaults.json"
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
            app.document.save_sqlite(|| Ok(())).unwrap();
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
