//! The real mixed-source save path, including failure between database and JSON commits.
use super::*;
use crate::app::{settings::SaveJsonError, workspace_save::save_changed_sources_with_json};
use crate::persistence::native_account::snapshot;
use sundial_account::{AccountSettingGroup, KeyBindingSlot};

fn fixture() -> (TestDirectory, std::path::PathBuf, WorkspaceDocument) {
    let directory = TestDirectory::new("dawn-coordinated-save");
    let settings = directory.0.join("settings.json");
    let seed = json!({"version":6,"server":{"port":1234},"state":{"account":{"settings":{"sentinel":"keep"}}}});
    fs::write(&settings, serde_json::to_vec(&seed).unwrap()).unwrap();
    crate::persistence::dawn_account::tests::create_fixture(&directory.0.join("player-state.db"));
    let workspace = WorkspaceDocument::load(seed, &settings, true);
    assert_eq!(workspace.source_kind(), AccountSourceKind::Dawn);
    (directory, settings, workspace)
}

fn edit_settings(workspace: &mut WorkspaceDocument) {
    account::apply_account_settings(
        workspace,
        vec![
            AccountSettingsCommand::Set {
                key: AccountSettingKey::preference(AccountSettingGroup::Display, "field_of_view"),
                value: AccountSettingValue::Unsigned(100),
            },
            AccountSettingsCommand::Set {
                key: AccountSettingKey::key_binding("fire", KeyBindingSlot::Primary),
                value: AccountSettingValue::InputCode(60),
            },
        ],
    )
    .unwrap();
}

#[test]
fn mixed_save_failure_then_retry_persists_dawn_preferences_and_bindings() {
    let (_directory, settings, persisted) = fixture();
    let mut edited = persisted.clone();
    edit_settings(&mut edited);
    account::add_profile_item(&mut edited, 3159615086, 5).unwrap();
    let db = Connection::open(settings.with_file_name("player-state.db")).unwrap();
    let before = snapshot::capture(&db).unwrap();
    let error = save_changed_sources_with_json(
        &mut edited,
        &persisted,
        &settings,
        true,
        true,
        |_, _, _, _| {
            Err(SaveJsonError {
                message: "injected JSON failure".into(),
                may_have_committed: false,
            })
        },
    )
    .err()
    .unwrap();
    assert!(matches!(error.sqlite_rollback, Some(Ok(()))));
    assert_eq!(snapshot::capture(&db).unwrap(), before);
    save_changed_sources_with_json(
        &mut edited,
        &persisted,
        &settings,
        true,
        true,
        |path, value, expected, normalize| {
            crate::app::settings::save_test_json_checked(
                path,
                value,
                expected,
                normalize,
                &path.parent().unwrap().join("json-backups"),
            )
        },
    )
    .unwrap();
    let reopened = WorkspaceDocument::load(persisted.json().clone(), &settings, true);
    assert_eq!(
        account::account_settings_map(&reopened).unwrap(),
        account::account_settings_map(&edited).unwrap()
    );
    let items = account::profile_items(&reopened).unwrap().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(
        (items[1].definition_hash, items[1].quantity),
        (3159615086, 5)
    );
    assert_eq!(reopened.json(), persisted.json());
    assert_eq!(
        db.query_row(
            "SELECT integer_value FROM settings_values WHERE key='pc.fieldOfViewAdjustment'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        100
    );
    assert_eq!(
        db.query_row(
            "SELECT primary_input FROM key_bindings WHERE action=0",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        60
    );
}

#[test]
fn mixed_save_failure_does_not_rollback_over_a_new_runtime_write() {
    let (_directory, settings, persisted) = fixture();
    let mut edited = persisted.clone();
    edit_settings(&mut edited);
    let db = Connection::open(settings.with_file_name("player-state.db")).unwrap();
    let mut newer = Vec::new();
    let error = save_changed_sources_with_json(
        &mut edited,
        &persisted,
        &settings,
        true,
        true,
        |_, _, _, _| {
            db.execute("INSERT INTO family5_values VALUES(3,7)", [])
                .unwrap();
            newer = snapshot::capture(&db).unwrap();
            Err(SaveJsonError {
                message: "injected JSON failure".into(),
                may_have_committed: false,
            })
        },
    )
    .err()
    .unwrap();
    assert!(matches!(error.sqlite_rollback, Some(Err(_))));
    assert_eq!(snapshot::capture(&db).unwrap(), newer);
}
