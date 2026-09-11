use crate::app::change_review::collect_change_summaries;
use crate::app::*;
use crate::test_support::TestDirectory;
use std::fs;

#[test]
fn settings_change_review_reports_nested_values_and_respects_its_limit() {
    let before = serde_json::json!({"state": {"characters": [{"class": 0, "race": 1}]}});
    let after = serde_json::json!({"state": {"characters": [{"class": 2, "race": 3}]}});

    let changes = collect_change_summaries(&before, &after, 10);
    assert_eq!(changes.len(), 2);
    assert!(changes[0].contains("/state/characters/0/class"));
    assert!(changes[1].contains("/state/characters/0/race"));

    let limited = collect_change_summaries(&before, &after, 1);
    assert_eq!(limited.len(), 1);
}

#[test]
fn reopening_a_detached_window_uses_a_fresh_viewport_generation() {
    let mut open = true;
    let mut generation = 0;

    update_detached_window_state(&mut open, &mut generation, false);
    assert!(!open);
    assert_eq!(generation, 1);

    update_detached_window_state(&mut open, &mut generation, true);
    assert!(open);
    assert_eq!(generation, 1);

    update_detached_window_state(&mut open, &mut generation, false);
    assert!(!open);
    assert_eq!(generation, 2);

    update_detached_window_state(&mut open, &mut generation, false);
    assert_eq!(generation, 2);
}

#[test]
fn focus_refresh_only_runs_on_a_clean_focus_transition() {
    assert!(should_refresh_workspace_on_focus(false, true, false));
    assert!(!should_refresh_workspace_on_focus(true, true, false));
    assert!(!should_refresh_workspace_on_focus(false, false, false));
    assert!(!should_refresh_workspace_on_focus(false, true, true));

    assert!(should_poll_pending_workspace_refresh(true, false, true));
    assert!(!should_poll_pending_workspace_refresh(false, false, true));
    assert!(!should_poll_pending_workspace_refresh(true, true, true));
    assert!(!should_poll_pending_workspace_refresh(true, false, false));
}

#[test]
fn detached_json_editor_preference_only_applies_when_json_view_is_selected() {
    assert!(should_open_json_editor_window_on_selection(
        true,
        ViewMode::AdvancedJson
    ));
    assert!(!should_open_json_editor_window_on_selection(
        true,
        ViewMode::Characters
    ));
    assert!(!should_open_json_editor_window_on_selection(
        false,
        ViewMode::AdvancedJson
    ));
}

#[test]
fn sqlite_default_restore_preserves_only_inactive_json_account_domains() {
    let mut defaults = serde_json::json!({
        "version": 8,
        "state": {
            "account": {"settings": {"from": "defaults"}},
            "characters": [{"from": "defaults"}],
            "server": {"port": 1}
        }
    });
    let source = serde_json::json!({
        "version": 8,
        "state": {
            "account": {"settings": {"sentinel": true}},
            "characters": "stale but untouched",
            "server": {"port": 2}
        }
    });

    preserve_inactive_json_account_domains(&mut defaults, &source);

    assert_eq!(
        defaults.pointer("/state/account/settings/sentinel"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        defaults.pointer("/state/characters"),
        Some(&serde_json::json!("stale but untouched"))
    );
    assert_eq!(
        defaults.pointer("/state/server/port"),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn sqlite_default_restore_does_not_create_missing_json_account_domains() {
    let mut defaults = serde_json::json!({
        "state": {"account": {}, "characters": [], "server": {}}
    });

    preserve_inactive_json_account_domains(&mut defaults, &serde_json::json!({"state": {}}));

    assert!(defaults.pointer("/state/account").is_none());
    assert!(defaults.pointer("/state/characters").is_none());
    assert!(defaults.pointer("/state/server").is_some());
}

#[test]
fn check_mode_accepts_compatible_sqlite_and_rejects_blocked_account_sources() {
    let compatible = TestDirectory::new("check-compatible-sqlite");
    let compatible_settings = compatible.0.join("settings.json");
    crate::persistence::sqlite_account::tests::create_fixture(
        &compatible.0.join("data").join("investment.sqlite3"),
        3,
    );
    let compatible_document = WorkspaceDocument::load(
        serde_json::json!({
            "version": 8,
            "state": {"account": "stale", "characters": "stale"}
        }),
        &compatible_settings,
    );
    assert_eq!(validate_for_check(&compatible_document), Ok(()));

    let blocked = TestDirectory::new("check-blocked-sqlite");
    let blocked_settings = blocked.0.join("settings.json");
    fs::create_dir_all(blocked.0.join("data")).unwrap();
    fs::write(
        blocked.0.join("data").join("investment.sqlite3"),
        b"not SQLite",
    )
    .unwrap();
    let blocked_document =
        WorkspaceDocument::load(serde_json::json!({"version": 8}), &blocked_settings);
    assert!(
        validate_for_check(&blocked_document)
            .unwrap_err()
            .contains("Account source is incompatible")
    );
}
