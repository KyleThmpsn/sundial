//! Headless workflow regressions. Never load or write the user's installation/preferences.
mod account_smoke;
mod character_fields;
mod confirmation_layout;
mod dawn;
mod loadout_safety;
mod parhelion_confirmation;
mod progression_access;
mod recovery;
mod runtime_selection;
mod save_validation;
mod schema_smoke;
mod shortcuts;
mod update;

use crate::app::*;
use crate::test_support::TestDirectory;

fn app(install_path: PathBuf) -> SundialApp {
    let json = serde_json::json!({"version": 8, "state": {"characters": []}});
    let document = WorkspaceDocument::json_only(json.clone());
    SundialApp {
        runtime_choice: runtime_installation::RuntimeChoice::inspect(&install_path),
        account_details: Default::default(),
        settings_path: install_path.join("settings.json"),
        settings_layout: SettingsLayout::GameRoot,
        persistence_compatibility: PersistenceCompatibility::inspect(&install_path),
        install_path,
        sunrise_version: String::new(),
        manifest: Manifest::for_test(Vec::new(), HashMap::new()),
        persisted_document: document.clone(),
        document,
        source_warning: None,
        class_armor_defaults: HashMap::new(),
        selected_character: 0,
        searches: HashMap::new(),
        plug_searches: HashMap::new(),
        character_inventory_query: String::new(),
        character_inventory_source_filter: Default::default(),
        character_inventory_lock_filter: Default::default(),
        character_inventory_sort: Default::default(),
        armor_stats_adjuster: Default::default(),
        plug_selection_mode: PlugSelectionMode::Supported,
        preferences: Preferences::default(),
        preferences_load_warning: None,
        troubleshooting_log_error: None,
        remember_plug_selection_mode_after_confirmation: false,
        show_dummy_items: false,
        view_mode: ViewMode::Characters,
        preferences_tab: Default::default(),
        progression_section: Default::default(),
        game_settings_tab: game_settings::Tab::Player,
        key_binding_ui: Default::default(),
        progression_ui: Default::default(),
        collections_ui: Default::default(),
        hash_inspection: Default::default(),
        raw_json: json.to_string(),
        raw_json_document: json,
        json_editor: Default::default(),
        json_editor_window_open: false,
        json_editor_window_generation: 0,
        logo: None,
        #[cfg(target_os = "linux")]
        title_bar_icon: None,
        about_open: false,
        update_check: Default::default(),
        confirmation: None,
        pending_save_action: None,
        pending_equipment_delete: None,
        pending_sqlite_restore: None,
        pending_sqlite_reset: None,
        exit_confirmed: false,
        dirty: false,
        undo_history: Vec::new(),
        redo_history: Vec::new(),
        suppress_history_record: false,
        status: String::new(),
        status_is_error: false,
        activity_log: Default::default(),
        activity_log_open: false,
        window_was_focused: true,
        workspace_refresh_pending: false,
        next_workspace_refresh_poll: Instant::now(),
        pending_install_choice: None,
        pending_future_schema: None,
        catalog_task: None,
        package_authoring: None,
        package_authoring_open: false,
        package_authoring_busy: false,
        package_authoring_dirty: false,
        package_authoring_packages_changed: false,
        destiny_symbol_font_install: None,
        destiny_symbol_font_error: None,
    }
}

#[test]
fn accepting_a_loaded_source_cannot_record_or_restore_the_previous_source() {
    let directory = TestDirectory::new("history-source-boundary");
    let mut app = app(directory.0.clone());
    let previous = app.document.clone();
    let entry = DocumentHistoryEntry {
        document: previous.clone(),
        label: "Old account".into(),
    };
    app.undo_history.push(entry.clone());
    app.redo_history.push(entry);
    let next = WorkspaceDocument::json_only(
        serde_json::json!({"version": 8, "state": {"characters": [], "sentinel": "new account"}}),
    );
    app.replace_loaded_document(next.clone());
    app.record_document_change(previous);
    assert!(app.undo_history.is_empty());
    assert!(app.redo_history.is_empty());
    app.undo();
    assert_eq!(app.document, next);
    assert!(!app.dirty);
    assert!(!app.settings_path.exists());
}

#[test]
fn disconnected_catalog_worker_releases_the_authoring_pause() {
    use crate::app::background_tasks::{CatalogTask, CatalogTaskKind};
    let directory = TestDirectory::new("catalog-worker-disconnect");
    let mut app = app(directory.0.clone());
    app.manifest.suspend_package_access();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.catalog_task = Some(CatalogTask {
        kind: CatalogTaskKind::Rebuild,
        receiver,
        progress: CatalogProgress {
            message: "Testing",
            completed: 0,
            total: 1,
        },
    });
    drop(sender);
    app.poll_catalog_task();
    assert!(app.catalog_task.is_none());
    assert!(!app.manifest.inspection_access().is_suspended());
    assert!(app.status_is_error);
}

#[test]
fn applying_stale_json_preserves_the_current_document_and_the_draft() {
    let directory = TestDirectory::new("json-draft-conflict");
    let mut app = app(directory.0.clone());
    let draft = app.raw_json.clone();
    app.document.json_mut()["state"]["sentinel"] = serde_json::json!(true);
    let current = app.document.clone();
    assert!(!app.apply_raw_json());
    assert_eq!(app.document, current);
    assert_eq!(app.raw_json, draft);
}
