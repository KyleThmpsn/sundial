use super::*;

#[test]
fn runtime_selection_routes_progression_without_touching_the_other_database_or_seed() {
    let dir = TestDirectory::new("progression-runtime-selection");
    let settings = dir.0.join("settings.json");
    let seed = json!({"version":18,"state":{"unlocks":{"account_flag_runs":[[999,1]]}}});
    fs::write(&settings, serde_json::to_vec(&seed).unwrap()).unwrap();
    let dawn = dir.0.join("player-state.db");
    let sunrise = dir.0.join("data/investment.sqlite3");
    crate::persistence::dawn_account::tests::create_fixture(&dawn);
    crate::persistence::sqlite_account::tests::create_fixture(&sunrise, 3);
    let other_bytes = fs::read(&sunrise).unwrap();
    let mut workspace = WorkspaceDocument::load(seed.clone(), &settings, true);
    assert_eq!(workspace.source_kind(), AccountSourceKind::Dawn);
    let before = workspace.clone();
    let mut view = workspace.progression_view(0);
    assert_eq!(view["state"]["unlocks"]["account_flag_runs"], json!([]));
    view["state"]["unlocks"]["account_flag_runs"] = json!([[123, 1]]);
    workspace.apply_progression_view(0, view).unwrap();
    assert!(workspace.account_changed_from(&before));
    assert!(
        workspace
            .account_change_summaries(&before, 5)
            .iter()
            .any(|line| line.starts_with("player-state.db/progression/"))
    );
    workspace.save_dawn().unwrap();
    assert_eq!(workspace.json(), &seed);
    assert_eq!(fs::read(&sunrise).unwrap(), other_bytes);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&settings).unwrap()).unwrap(),
        seed
    );
    let loaded = WorkspaceDocument::load(seed.clone(), &settings, true);
    assert_eq!(
        loaded.progression_view(0)["state"]["unlocks"]["account_flag_runs"],
        json!([[123, 1]])
    );
    let other = WorkspaceDocument::load(seed, &settings, false);
    assert_eq!(other.source_kind(), AccountSourceKind::Sqlite);
    assert_ne!(
        other.progression_view(0)["state"]["unlocks"]["account_flag_runs"],
        json!([[123, 1]])
    );
}

#[test]
fn missing_dawn_database_never_exposes_seed_progression() {
    let dir = TestDirectory::new("progression-missing-dawn");
    let seed = json!({"version":6,"state":{"unlocks":{"account_flag_runs":[[999,1]]}}});
    let workspace = WorkspaceDocument::load(seed, &dir.0.join("settings.json"), true);
    assert_eq!(workspace.source_kind(), AccountSourceKind::Blocked);
    // The view names the block for the inspector and carries none of the seed's state.
    let view = workspace.progression_view(0);
    assert!(view["_blocked"].is_string(), "{view}");
    assert!(view.get("state").is_none(), "{view}");
}
