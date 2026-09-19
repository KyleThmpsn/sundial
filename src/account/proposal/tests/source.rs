use super::*;
use crate::account::{
    read_authored_account_source as read, replace_authored_account_source as replace,
};

fn proposal(settings: &Path) -> AuthoredAccountCleanup {
    preview_replacement(
        settings,
        &BTreeSet::from([300]),
        &[],
        &[AuthoredSocketChange {
            definition_hash: 200,
            previous_socket_count: 2,
            default_plugs: vec![Some(99), Some(98), Some(88)],
        }],
        None,
    )
    .unwrap()
}

#[test]
fn selected_source_proposals_preserve_unknown_data_and_recover_without_touching_inactive_accounts()
{
    for version in [8, 18] {
        verify_selected_source(version);
    }
}

fn verify_selected_source(version: u64) {
    let dir = crate::test_support::TestDirectory::new(&format!("proposal-source-{version}"));
    let settings = dir.0.join("settings.json");
    let database = dir.0.join("data/investment.sqlite3");
    crate::persistence::sqlite_account::tests::create_fixture(&database, 3);
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE extension(value TEXT); INSERT INTO extension VALUES('keep');").unwrap();
    let mut equipped = item(20, 200);
    equipped["plugs"] = json!([77, null]);
    let document = json!({"version": version, "unknown_setting": {"keep": true}, "state": {
        "account": {"primary_soid": "0x0000000000000001", "extension": {"keep": 18446744073709551615u64}},
        "characters": [{"soid": "0x0000000000000002", "class": 0, "equipment": {"kinetic": equipped}, "inventory": [item(21, 300)]}]
    }});
    std::fs::write(&settings, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let json_before = std::fs::read(&settings).unwrap();
    let sqlite_before = crate::persistence::sqlite_account::snapshot::read(&database).unwrap();
    let plan = proposal(&settings);
    let active = if version == 18 { &database } else { &settings };
    assert_eq!(&plan.settings_path, active);
    assert_eq!(read(active).unwrap(), plan.original_bytes);
    assert_eq!(plan.removed_items, BTreeMap::from([(300, 1)]));
    assert_eq!(plan.resized_items, BTreeMap::from([(200, 1)]));
    replace(active, &plan.original_bytes, &plan.cleaned_bytes).unwrap();
    assert_eq!(read(active).unwrap(), plan.cleaned_bytes);
    if version == 8 {
        let saved: Value = serde_json::from_slice(&plan.cleaned_bytes).unwrap();
        assert_eq!(saved["unknown_setting"], document["unknown_setting"]);
        assert_eq!(
            saved.pointer("/state/account/extension"),
            document.pointer("/state/account/extension")
        );
        assert_eq!(
            crate::persistence::sqlite_account::snapshot::read(&database).unwrap(),
            sqlite_before
        );
    } else {
        assert_eq!(std::fs::read(&settings).unwrap(), json_before);
        assert_eq!(
            db.query_row("SELECT value FROM extension", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "keep"
        );
    }
    verify_recovery(&settings, active, &db, &plan, &document, version);
}

fn verify_recovery(
    settings: &Path,
    active: &Path,
    db: &rusqlite::Connection,
    plan: &AuthoredAccountCleanup,
    document: &Value,
    version: u64,
) {
    let again = proposal(settings);
    assert_eq!(again.original_bytes, again.cleaned_bytes);
    replace(active, &plan.cleaned_bytes, &plan.original_bytes).unwrap();
    assert_eq!(read(active).unwrap(), plan.original_bytes);
    if version == 18 {
        db.execute("UPDATE extension SET value='outside'", [])
            .unwrap();
    } else {
        let mut outside = document.clone();
        outside["outside"] = json!(true);
        std::fs::write(active, serde_json::to_vec(&outside).unwrap()).unwrap();
    }
    let outside = read(active).unwrap();
    assert!(replace(active, &plan.original_bytes, &plan.cleaned_bytes).is_err());
    assert_eq!(read(active).unwrap(), outside);
}

#[test]
fn sqlite_settings_never_fall_back_to_valid_inactive_json() {
    let dir = crate::test_support::TestDirectory::new("proposal-no-fallback");
    let settings = dir.0.join("settings.json");
    let bytes = br#"{"version":18,"state":{"characters":[]}}"#;
    std::fs::write(&settings, bytes).unwrap();
    assert!(preview_replacement(&settings, &BTreeSet::new(), &[], &[], None).is_err());
    assert_eq!(std::fs::read(settings).unwrap(), bytes);
}
