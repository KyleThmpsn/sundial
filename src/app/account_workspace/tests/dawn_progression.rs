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

/// A Dawn workspace with two characters, vendor rows for the account and each character, and
/// one mission each character has started.
fn workspace_with_activity(dir: &TestDirectory) -> WorkspaceDocument {
    let settings = dir.0.join("settings.json");
    let seed = json!({"version":18,"state":{"unlocks":{}}});
    fs::write(&settings, serde_json::to_vec(&seed).unwrap()).unwrap();
    let dawn = dir.0.join("player-state.db");
    crate::persistence::dawn_account::tests::create_fixture(&dawn);
    let db = rusqlite::Connection::open(&dawn).unwrap();
    db.execute_batch(
        "INSERT INTO characters VALUES(1,'9EAA300100100102',0,0,0,1,50,1,1,1.0,308080871,1,6,7,10,15,2,93,0);
         INSERT INTO vendor_progress VALUES('9EAA300100100100',0,20,8000,2);
         INSERT INTO vendor_progress VALUES('9EAA300100100101',0,11,4000,1);
         INSERT INTO vendor_progress VALUES('9EAA300100100102',0,11,6000,0);
         INSERT INTO missions VALUES('9EAA300100100101',123,456,2,7,9,0,100);
         INSERT INTO missions VALUES('9EAA300100100102',123,0,0,7,1,1,100);",
    )
    .unwrap();
    drop(db);
    let workspace = WorkspaceDocument::load(seed, &settings, true);
    assert_eq!(workspace.source_kind(), AccountSourceKind::Dawn);
    workspace
}

/// The inspector reads vendor reputation and mission state from the view, scoped to the account
/// and the selected character. Another character's rows stay out, and nothing here is applied
/// back: the view round-trips through apply untouched.
#[test]
fn dawn_view_carries_vendor_and_mission_rows_for_the_selected_character() {
    let dir = TestDirectory::new("progression-dawn-activity");
    let mut workspace = workspace_with_activity(&dir);

    let view = workspace.progression_view(0);
    let vendors = view["_dawn_activity"]["vendors"].as_array().unwrap();
    assert_eq!(vendors.len(), 2, "{vendors:?}");
    assert_eq!(vendors[0]["scope"], "Account");
    assert_eq!(vendors[0]["vendor"], 20);
    assert_eq!(vendors[1]["scope"], "Character");
    assert_eq!(vendors[1]["points"], 4000);
    let missions = view["_dawn_activity"]["missions"].as_array().unwrap();
    assert_eq!(missions.len(), 1, "{missions:?}");
    assert_eq!(missions[0]["hash"], 123);
    assert_eq!(missions[0]["completed"], false);

    let other = workspace.progression_view(1);
    assert_eq!(other["_dawn_activity"]["vendors"][1]["points"], 6000);
    assert_eq!(other["_dawn_activity"]["missions"][0]["completed"], true);

    let before = workspace.clone();
    workspace.apply_progression_view(0, view).unwrap();
    assert!(!workspace.account_changed_from(&before));
}

/// Vendor state rides along in the view for the inspector, but it is not progression: a vendor
/// edit is summarised once as vendor state, not as a progression change on every character.
#[test]
fn dawn_vendor_edits_are_not_summarised_as_progression_changes() {
    let dir = TestDirectory::new("progression-dawn-vendor-summary");
    let mut workspace = workspace_with_activity(&dir);
    let before = workspace.clone();

    let mut activity = workspace.dawn_account().unwrap().activity_state().clone();
    activity.vendors[0].points += 1000;
    workspace
        .dawn_account_mut()
        .unwrap()
        .set_activity_state(activity)
        .unwrap();

    let summaries = workspace.account_change_summaries(&before, 10);
    assert_eq!(
        summaries,
        vec!["player-state.db/vendor and mission state: updated".to_owned()]
    );
}
