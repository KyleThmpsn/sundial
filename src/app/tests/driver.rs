//! A headless app over disposable fixtures, and the frame helpers the workflow tests share.
//! Never loads or writes the user's installation or preferences.
use crate::app::*;
pub(in crate::app) use crate::test_support::TestDirectory;
use serde_json::json;

pub(in crate::app) const FIXTURES: [&str; 3] = [
    include_str!("../../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"),
    include_str!("../../../tests/fixtures/sunrise-v13-a57dc9a9-defaults.json"),
    include_str!("../../../tests/fixtures/sunrise-v16-1120748-defaults.json"),
];

pub(in crate::app) fn app(install_path: PathBuf) -> SundialApp {
    let json = serde_json::json!({"version": 8, "state": {"characters": []}});
    let document = WorkspaceDocument::json_only(json.clone());
    SundialApp {
        runtime_choice: runtime_choice::RuntimeChoice::inspect(&install_path),
        account_details: Default::default(),
        settings_path: install_path.join("settings.json"),
        settings_layout: SettingsLayout::GameRoot,
        runtime_state: RuntimeState::inspect(&install_path),
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
        edit_baseline: None,
        document_repaint_pending: false,
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

pub(in crate::app) fn with_document(path: PathBuf, json: Value) -> SundialApp {
    let mut app = app(path);
    app.document = WorkspaceDocument::json_only(json.clone());
    app.persisted_document = app.document.clone();
    app.raw_json = serde_json::to_string_pretty(&json).unwrap();
    app.raw_json_document = json;
    app
}

pub(in crate::app) fn for_source(directory: &TestDirectory, sqlite: bool) -> SundialApp {
    let mut app = app(directory.0.clone());
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v8-d0fe8886-defaults.json"
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

pub(in crate::app) fn button(output: &egui::FullOutput, label: &str) -> (egui::Pos2, bool) {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, node)| {
            if node.label() != Some(label) {
                return None;
            }
            let rect = node.bounds()?;
            Some((
                egui::pos2(
                    ((rect.x0 + rect.x1) * 0.5) as f32,
                    ((rect.y0 + rect.y1) * 0.5) as f32,
                ),
                !node.is_disabled(),
            ))
        })
        .filter(|(position, _)| position.y > 0.0 && position.y < 740.0)
        .min_by(|(left, _), (right, _)| left.y.total_cmp(&right.y))
        .unwrap_or_else(|| panic!("missing visible button {label}"))
}

pub(in crate::app) fn contains_text(output: &egui::FullOutput, needle: &str) -> bool {
    fn contains(shape: &egui::Shape, needle: &str) -> bool {
        match shape {
            egui::Shape::Text(text) => text.galley.job.text.contains(needle),
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains(shape, needle)),
            _ => false,
        }
    }
    output
        .shapes
        .iter()
        .any(|shape| contains(&shape.shape, needle))
}
