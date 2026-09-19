use super::*;
use crate::app::account_workspace::{AccountSourceKind, add_inventory_item, character_inventory};
use crate::app::inventory::NewInventoryItem;
use crate::package_runtime::installation::RuntimeLocation;
use std::fs;

fn sunrise_v18() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/sunrise-v18-169fd29-defaults.json"
    ))
    .unwrap()
}

fn set_runtime(inspection: &mut RuntimeInspection, dawn: bool, schema: u64) {
    let copy = &mut inspection.copies[0];
    copy.dawn = dawn;
    copy.dawn_runtime = dawn.then(|| Runtime::inspect(&copy.dll_path));
    copy.bundled_schema = Some(schema);
}

#[test]
fn sunrise_and_dawn_v6_keep_item_edits_in_the_same_json_account() {
    let sunrise: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"
    ))
    .unwrap();
    let dawn: Value = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/dawn-v6-42fc41e-defaults.json"
    ))
    .unwrap();
    for source in [sunrise, dawn] {
        let (_directory, mut app, mut inspection) = setup();
        reload_source(&mut app, source);
        let database = crate::persistence::investment_path(&app.settings_path);
        crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
        let database_before = fs::read(&database).unwrap();
        app.document.json_mut()["experiments"] = json!({"omega": {"future_flag": [1, null]}});
        let account_before = app.document.json()["state"]["account"].clone();
        let initial_items = character_inventory(&app.document, 0)
            .unwrap()
            .unwrap()
            .len();
        for (step, dawn) in [false, true, false].into_iter().enumerate() {
            set_runtime(&mut inspection, dawn, 6);
            add_inventory_item(
                &mut app.document,
                0,
                NewInventoryItem::single(42 + step as u32, 10),
            )
            .unwrap();
            validate_save_and_reload(&mut app, &inspection, AccountSourceKind::Json);
            assert_eq!(app.document.json()["version"], 6);
            assert_eq!(app.document.json()["state"]["account"], account_before);
            assert_eq!(
                app.document.json()["experiments"]["omega"]["future_flag"],
                json!([1, null])
            );
            assert_eq!(
                character_inventory(&app.document, 0)
                    .unwrap()
                    .unwrap()
                    .len(),
                initial_items + step + 1
            );
            assert_eq!(fs::read(&database).unwrap(), database_before);
        }
    }
}

#[test]
fn runtime_format_checks_cover_both_directions_and_every_settings_location() {
    for layout in SettingsLayout::ALL {
        let (directory, mut app, mut inspection) = setup();
        app.settings_path = directory.0.join(layout.relative_path());
        set_runtime(&mut inspection, false, 18);
        let before = app.document.clone();
        let error = app
            .validation_warning_for_runtime(&app.document, &inspection)
            .unwrap_err();
        assert!(error.contains("Sunrise requires settings v18"), "{error}");
        assert!(error.contains("SQLite account storage"), "{error}");
        assert_eq!(app.document, before);

        let database = crate::persistence::investment_path(&app.settings_path);
        crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
        app.document = WorkspaceDocument::load(sunrise_v18(), &app.settings_path, false);
        app.persisted_document = app.document.clone();
        add_inventory_item(&mut app.document, 0, NewInventoryItem::single(42, 10)).unwrap();
        assert!(app.document.account_changed_from(&app.persisted_document));
        assert!(!app.document.json_changed_from(&app.persisted_document));
        let before = app.document.clone();
        let database_before = fs::read(&database).unwrap();
        set_runtime(&mut inspection, true, 6);
        // Dawn keeps its account in player-state.db whatever its settings schema says, so the
        // v18 storage rule is Sunrise's own migration and says nothing about Dawn. What Dawn
        // does expect of its settings is reported by its own runtime validation instead.
        let checked = app.validation_warning_for_runtime(&app.document, &inspection);
        assert!(
            checked
                .as_ref()
                .err()
                .is_none_or(|error| !error.contains("account storage")),
            "{checked:?}"
        );
        assert_eq!(app.document, before);
        assert_eq!(fs::read(&database).unwrap(), database_before);
    }
}

#[test]
fn runtime_swap_blocks_raw_apply_and_save_review_for_each_settings_layout() {
    for layout in SettingsLayout::ALL {
        let (directory, mut app, _) = setup();
        app.settings_path = directory.0.join(layout.relative_path());
        app.settings_layout = layout;
        fs::create_dir_all(app.settings_path.parent().unwrap()).unwrap();
        fs::write(&app.settings_path, &app.raw_json).unwrap();
        let mut dll = directory.0.join("steam_api64.dll");
        // Both bin/x64 layouts put the runtime there, so the DLL has to move for either or the
        // swap is not a swap in place.
        if matches!(layout, SettingsLayout::BinX64 | SettingsLayout::DawnBinX64) {
            let bin = directory.0.join("bin/x64/steam_api64.dll");
            fs::rename(&dll, &bin).unwrap();
            dll = bin;
        }
        app.runtime_choice.inspection = RuntimeInspection::inspect(&directory.0);
        fs::write(dll, b"swapped DLL").unwrap();
        let before = app.document.clone();
        let saved = fs::read(&app.settings_path).unwrap();
        let mut draft = app.document.json().clone();
        draft["future_setting"] = json!("keep this draft");
        app.raw_json = draft.to_string();
        assert!(!app.apply_raw_json());
        assert!(app.status.contains("runtime DLL changed"), "{}", app.status);
        assert_eq!(app.document, before);
        assert_eq!(app.raw_json, draft.to_string());

        app.sync_raw_json();
        app.document.json_mut()["future_setting"] = json!(true);
        app.preferences.review_changes_before_saving = true;
        app.request_save(&egui::Context::default(), SaveAction::Save);
        assert!(app.confirmation.is_none());
        assert!(app.status.contains("runtime DLL changed"), "{}", app.status);
        assert_eq!(fs::read(&app.settings_path).unwrap(), saved);
    }
}

#[test]
fn a_new_game_folder_dll_cannot_hide_behind_the_selected_bin_copy() {
    let directory = TestDirectory::new("dawn-runtime-precedence");
    let bin = directory.0.join("bin/x64");
    fs::create_dir_all(bin.join("Sunrise")).unwrap();
    fs::write(bin.join("steam_api64.dll"), b"bin runtime").unwrap();
    let mut app = app(directory.0.clone());
    app.settings_path = bin.join("Sunrise/settings.json");
    fs::write(directory.0.join("steam_api64.dll"), b"new root runtime").unwrap();
    let error = app.validation_warning_for_write(&app.document).unwrap_err();
    assert!(error.contains("takes precedence"), "{error}");
    assert!(error.contains("matching settings"), "{error}");

    let mut inspection = RuntimeInspection::inspect(&directory.0);
    // The old bin copy still matches v18, but Dawn at the root takes precedence.
    inspection.copies[1].bundled_schema = Some(18);
    set_runtime(&mut inspection, true, 6);
    let error = inspection
        .workspace_problem(&app.settings_path, &sunrise_v18())
        .unwrap();
    assert!(error.contains("takes precedence"), "{error}");
    inspection.copies.reverse();
    assert_eq!(
        inspection.launch_copy().unwrap().location,
        RuntimeLocation::Root
    );
}

#[test]
fn reloading_each_runtime_keeps_json_and_sqlite_item_edits_in_their_own_accounts() {
    let (_directory, mut app, mut inspection) = setup();
    let database = crate::persistence::investment_path(&app.settings_path);
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let original_database = fs::read(&database).unwrap();
    app.document.json_mut()["experiments"] = json!({"omega": {"future_flag": [1, null]}});
    fs::write(&app.settings_path, app.document.json().to_string()).unwrap();
    app.persisted_document = app.document.clone();
    add_inventory_item(&mut app.document, 0, NewInventoryItem::single(42, 10)).unwrap();
    validate_save_and_reload(&mut app, &inspection, AccountSourceKind::Json);
    let dawn_account = app.document.json().clone();
    assert_eq!(fs::read(&database).unwrap(), original_database);
    assert_eq!(
        dawn_account["experiments"]["omega"]["future_flag"],
        json!([1, null])
    );

    // A v18 source may retain an inactive JSON account. It must never be edited as a fallback.
    let mut current = sunrise_v18();
    current["state"]["account"] = dawn_account["state"]["account"].clone();
    current["state"]["characters"] = dawn_account["state"]["characters"].clone();
    set_runtime(&mut inspection, false, 18);
    reload_source(&mut app, current);
    let inactive = app.document.json().clone();
    add_inventory_item(&mut app.document, 0, NewInventoryItem::single(43, 10)).unwrap();
    validate_save_and_reload(&mut app, &inspection, AccountSourceKind::Sqlite);
    assert_eq!(app.document.json(), &inactive);
    let saved_database = fs::read(&database).unwrap();
    assert_ne!(saved_database, original_database);

    set_runtime(&mut inspection, true, 6);
    reload_source(&mut app, dawn_account.clone());
    assert_eq!(app.document.source_kind(), AccountSourceKind::Json);
    assert_eq!(
        app.validation_warning_for_runtime(&app.document, &inspection),
        Ok(None)
    );
    assert_eq!(app.document.json(), &dawn_account);
    assert_eq!(fs::read(&database).unwrap(), saved_database);
}

fn reload_source(app: &mut SundialApp, source: Value) {
    fs::write(&app.settings_path, source.to_string()).unwrap();
    assert!(app.reload(), "{}", app.status);
}

fn validate_save_and_reload(
    app: &mut SundialApp,
    inspection: &RuntimeInspection,
    source: AccountSourceKind,
) {
    assert_eq!(app.document.source_kind(), source);
    assert_eq!(
        app.validation_warning_for_runtime(&app.document, inspection),
        Ok(None)
    );
    let mut items = character_inventory(&app.document, 0).unwrap();
    if source == AccountSourceKind::Sqlite {
        // SQLite persists an omitted item flag value as its native default of zero.
        for item in items.iter_mut().flatten() {
            item.flags = Some(item.flags.unwrap_or_default());
        }
    }
    let json_changed = app.document.json_changed_from(&app.persisted_document);
    let account_changed = app.document.account_changed_from(&app.persisted_document);
    let receipt = crate::app::workspace_save::save_changed_sources_with_json(
        &mut app.document,
        &app.persisted_document,
        &app.settings_path,
        json_changed,
        account_changed,
        |path, value, expected, normalize| {
            settings::save_test_json_checked(
                path,
                value,
                expected,
                normalize,
                &path.parent().unwrap().join("backups"),
            )
        },
    )
    .unwrap();
    assert_eq!(receipt.json.is_some(), source == AccountSourceKind::Json);
    assert_eq!(
        receipt.sqlite.is_some(),
        source == AccountSourceKind::Sqlite
    );
    assert!(app.reload(), "{}", app.status);
    assert_eq!(app.document.source_kind(), source);
    assert_eq!(character_inventory(&app.document, 0).unwrap(), items);
}
