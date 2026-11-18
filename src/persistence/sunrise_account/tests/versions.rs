//! Real-file workflows against the reviewed upstream schemas and migration SQL.
use super::super::{defaults::ResetPlan, package, snapshot};
use super::*;
use std::collections::BTreeSet;

pub(crate) const CASES: [&str; 6] = [
    "v2",
    "v3-fresh",
    "v3-migrated",
    "v5-fresh",
    "v7-fresh",
    "v8-fresh",
];

/// The account schema a fixture case is saved with.
pub(crate) fn version(case: &str) -> i64 {
    match case {
        "v2" => 2,
        "v5-fresh" => 5,
        "v7-fresh" => 7,
        "v8-fresh" => 8,
        _ => 3,
    }
}

/// The version the bundled schema of a case's Sunrise build stamps. Schema 8 saves come from a
/// bundled schema that still stamps 7.
fn bundled(case: &str) -> i64 {
    version(case).min(7)
}

/// The flag rows of the schema 8 fixture, which no Sundial workflow edits: one a script no mod
/// ships keeps, one boolean and one counter.
const MISSION_FLAGS: &str = r#"[["","intro_seen",1,1],["example.mod","kills",0,-37]]"#;

/// The item `create_fixture` equips first, which no workflow here removes.
const KEPT_ITEM: i64 = 0x4000_0000_0000_0001;

/// The bundled investment schema a fixture starts from. Schema 8 starts from schema 7's.
fn investment_schema(v5: bool, v7: bool) -> &'static str {
    if v7 {
        include_str!("../fixtures/v7/investment_schema.sql")
    } else if v5 {
        include_str!("../fixtures/v5/investment_schema.sql")
    } else {
        include_str!("../fixtures/v3/investment_schema.sql")
    }
}

/// Copies every table of the schema 3 `seed` into `db`, adding the columns the later schemas
/// hold with their defaults and leaving out schema 7's dropped reward kind.
fn copy_seed_tables(db: &Connection, seed: &Path, v5: bool, v7: bool) {
    db.execute("ATTACH DATABASE ? AS seed", [seed.to_str().unwrap()])
        .unwrap();
    let names: Vec<String> = db
        .prepare(
            "SELECT name FROM seed.sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    db.execute_batch("BEGIN").unwrap();
    for table in names {
        let columns: Vec<String> = db
            .prepare(&format!("PRAGMA seed.table_info(\"{table}\")"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let columns = columns
            .iter()
            // Schema 7 stores no reward kind.
            .filter(|c| !(v7 && table == "pending_rewards" && *c == "kind"))
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        let (extra, values) = if table == "characters" && v5 {
            (",appearance_header", ",zeroblob(36)")
        } else if table == "characters" {
            (
                ",appearance_header,appearance_header_valid",
                ",zeroblob(36),0",
            )
        } else if v7 && matches!(table.as_str(), "items" | "character_stacks") {
            (",placement", ",0")
        } else {
            ("", "")
        };
        db.execute_batch(&format!("INSERT INTO main.\"{table}\" ({columns}{extra}) SELECT {columns}{values} FROM seed.\"{table}\";")).unwrap();
    }
    db.execute_batch("COMMIT; DETACH DATABASE seed").unwrap();
}

pub(crate) fn fixture(path: &Path, case: &str) {
    assert!(CASES.contains(&case), "Unknown account fixture: {case}");
    let v8 = case == "v8-fresh";
    // Schema 8 is schema 7 with the mission flags table.
    let v7 = case == "v7-fresh" || v8;
    // Schema 5 and later give every character a header and keep no validity flag.
    let v5 = case == "v5-fresh" || v7;
    if matches!(case, "v2" | "v3-migrated") {
        create_fixture(path, 3);
        if case == "v3-migrated" {
            Connection::open(path)
                .unwrap()
                .execute_batch(include_str!("../fixtures/v3/migration.sql"))
                .unwrap();
        }
    } else {
        let seed = path.with_extension("seed.sqlite3");
        create_fixture(&seed, 3);
        let db = Connection::open(path).unwrap();
        db.execute_batch(investment_schema(v5, v7)).unwrap();
        if v8 {
            // Sunrise stamps the save again once the table its bundled schema lacks exists.
            db.execute_batch(include_str!("../fixtures/v8/code_tables.sql"))
                .unwrap();
            db.execute_batch("PRAGMA user_version=8").unwrap();
        } else if v7 {
            db.execute_batch(include_str!("../fixtures/v7/code_tables.sql"))
                .unwrap();
        } else if v5 {
            db.execute_batch(include_str!("../fixtures/v5/parked_items.sql"))
                .unwrap();
        }
        db.execute_batch(super::super::contract::SETTINGS_SCHEMA)
            .unwrap();
        db.execute_batch(if v5 {
            include_str!("../fixtures/v5/character_starter.sql")
        } else {
            include_str!("../fixtures/v3/character_starter.sql")
        })
        .unwrap();
        copy_seed_tables(&db, &seed, v5, v7);
    }
    if case != "v2" {
        let db = Connection::open(path).unwrap();
        db.execute(
            if v5 {
                "UPDATE characters SET appearance_header=?"
            } else {
                "UPDATE characters SET appearance_header=?,appearance_header_valid=1"
            },
            [vec![0x5a_u8; 36]],
        )
        .unwrap();
        db.execute_batch("UPDATE starter_character SET level=42; INSERT INTO starter_items VALUES(2,7,123456789);").unwrap();
        if v5 {
            db.execute_batch(
                "INSERT INTO parked_items (kind,character_slot,location,position,instance_soid,definition_hash,level,quantity,mutation_serial,flags,socket_policy,plug_count,plugs,movement_ability,grenade_ability,super_ability,melee_ability,class_ability,seen)
                 SELECT kind,0,1,kind,4611686018427388000+kind,300,105,2,7,3,1,1,CAST(X'4D000000' || zeroblob(44) AS BLOB),4,7,10,11,2,1
                 FROM (SELECT 0 AS kind UNION ALL SELECT 1 UNION ALL SELECT 2);",
            ).unwrap();
        }
        if v7 {
            // Rows only schema 7 holds: objectives on a kept and on a removable item, a stack in
            // its recovery bucket and a reward without a kind.
            db.execute_batch(&format!(
                "INSERT INTO item_objectives VALUES({KEPT_ITEM},CAST(X'{}' AS BLOB)),({},zeroblob(32));
                 INSERT INTO character_stacks VALUES(0,0,20,3,1,1);
                 INSERT INTO pending_rewards(character_slot,definition_hash,quantity) VALUES(0,10,4);",
                "11".repeat(32),
                KEPT_ITEM + 2,
            ))
            .unwrap();
        }
        if v8 {
            db.execute_batch(
                "INSERT INTO mission_flags VALUES('','intro_seen',1,1),('example.mod','kills',0,-37);",
            )
            .unwrap();
        }
    }
}

/// The schema 8 fixture's mission flags, which no Sundial workflow edits.
fn preserved_mission_flags(db: &Connection) {
    let flags: String = db
        .query_row(
            "SELECT json_group_array(json_array(mod_id,name,boolean,value)) FROM (SELECT * FROM mission_flags ORDER BY mod_id,name)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        flags, MISSION_FLAGS,
        "Mission flags must survive active-account writes"
    );
}

/// The schema 7 fixture's objectives, placements, recovery stack and kindless reward.
fn preserved_placed_rows(db: &Connection) {
    let rows: (String, i64, String, String) = db
        .query_row(
            &format!(
                "SELECT (SELECT hex(lanes) FROM item_objectives WHERE instance_soid={KEPT_ITEM}),
                    (SELECT placement FROM items WHERE instance_soid={KEPT_ITEM}),
                    (SELECT json_array(character_slot,position,definition_hash,quantity,mutation_serial,placement) FROM character_stacks WHERE definition_hash=20),
                    (SELECT json_array(id,character_slot,definition_hash,quantity) FROM pending_rewards WHERE definition_hash=10)"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        rows,
        (
            "11".repeat(32),
            0,
            "[0,0,20,3,1,1]".into(),
            "[1,0,10,4]".into()
        ),
        "Schema 7 objectives, placements and rewards must survive active-account writes"
    );
}

pub(crate) fn preserved(db: &Connection, case: &str) {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let v8 = case == "v8-fresh";
    let v7 = case == "v7-fresh" || v8;
    let v5 = case == "v5-fresh" || v7;
    assert_eq!(version, self::version(case));
    if v8 {
        preserved_mission_flags(db);
    }
    if v7 {
        preserved_placed_rows(db);
    }
    if case != "v2" {
        let validity = if v5 { "1" } else { "appearance_header_valid" };
        let row: (Vec<u8>,i64,i64,i64) = db.query_row(&format!("SELECT appearance_header,{validity},(SELECT level FROM starter_character),(SELECT definition_hash FROM starter_items WHERE class=2 AND position=7) FROM characters WHERE slot=0"),[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(row, (vec![0x5a; 36], 1, 42, 123456789));
    }
    if v5 {
        let rows: Vec<serde_json::Value> = db.prepare(
            "SELECT json_array(id,kind,character_slot,location,position,instance_soid,definition_hash,level,quantity,mutation_serial,flags,socket_policy,plug_count,hex(plugs),movement_ability,grenade_ability,super_ability,melee_ability,class_ability,seen) FROM parked_items ORDER BY id",
        ).unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap()
            .map(|r| serde_json::from_str(&r.unwrap()).unwrap()).collect();
        let plugs = format!("4D000000{}", "00".repeat(44));
        let expected: Vec<_> = (0..3_i64)
            .map(|kind| {
                serde_json::json!([
                    kind + 1,
                    kind,
                    0,
                    1,
                    kind,
                    4611686018427388000_i64 + kind,
                    300,
                    105,
                    2,
                    7,
                    3,
                    1,
                    1,
                    plugs,
                    4,
                    7,
                    10,
                    11,
                    2,
                    1
                ])
            })
            .collect();
        assert_eq!(
            rows, expected,
            "Parked items must survive active-account writes"
        );
        assert_eq!(
            db.query_row(
                "SELECT value FROM starter_unlocks WHERE bank=2 AND slot=16 AND lane=0",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
    }
}

/// Synthetic rows use the upstream starter table, so reset must initialize it before the rows.
fn v5_resources() -> super::super::AccountDefaults {
    super::super::AccountDefaults {
        schema: include_str!("../fixtures/v5/investment_schema.sql").into(),
        starter: Some(include_str!("../fixtures/v5/character_starter.sql").into()),
        rows: "INSERT INTO account VALUES(1,-7013740688821976832,1);
            INSERT INTO characters
            SELECT slot,-7013740688821976831+slot,0,0,slot,level,preview_available,appearance_value,last_orbited_destination,content_bypass,65535,-1,0,zeroblob(36)
            FROM starter_character CROSS JOIN (SELECT 0 AS slot UNION ALL SELECT 1 UNION ALL SELECT 2);".into(),
        ..default_resources()
    }
}

/// Schema 7 keeps schema 5's characters and starter tables, so the same synthetic rows fit.
fn v7_resources() -> super::super::AccountDefaults {
    super::super::AccountDefaults {
        schema: include_str!("../fixtures/v7/investment_schema.sql").into(),
        ..v5_resources()
    }
}

#[test]
fn schema_workflows_preserve_native_state_and_refuse_unsafe_edits() {
    for case in CASES {
        let dir = TestDirectory::new(case);
        let path = dir.0.join("investment.sqlite3");
        fixture(&path, case);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0")
            .unwrap();
        let mut doc = loaded(&path);
        let item_id = doc.characters().characters()[0].inventory[0].id;
        let capabilities = doc.character_capabilities();
        doc.characters_mut()
            .apply(
                capabilities,
                sundial_account::CharacterCommand::UpdateInventoryItem {
                    item_id,
                    update: sundial_account::ItemUpdate::SetQuantity(9),
                },
            )
            .unwrap();
        // A material and a queued reward added here must save on every schema. Schema 7 needs a
        // placement and its own serial for the stack and stores no kind for the reward.
        doc.add_character_stack(0, 10, 2).unwrap();
        doc.add_pending_reward(0, 1, 20, 3).unwrap();
        save_fixture_document(&mut doc, &dir.0.join("saved.sqlite3"));
        let saved = loaded(&path);
        assert_eq!(saved.characters().characters()[0].inventory[0].quantity, 9);
        let stack = saved.character_stacks(0).last().unwrap();
        assert_eq!((stack.definition_hash, stack.quantity), (10, 2));
        let reward = saved.pending_rewards().last().unwrap();
        assert_eq!(
            (
                reward.character_slot,
                reward.definition_hash,
                reward.quantity
            ),
            (0, 20, 3)
        );
        if version(case) >= 7 {
            added_stack_is_placed(&db);
        }
        preserved(&db, case);
        let before = snapshot::read(&path).unwrap();
        let proposal = package::preview(&path, &BTreeSet::from([300]), &[], &[]).unwrap();
        package::replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).unwrap();
        assert!(
            loaded(&path).characters().characters()[0]
                .inventory
                .is_empty()
        );
        preserved(&db, case);
        package::replace(&path, &proposal.cleaned_bytes, &proposal.original_bytes).unwrap();
        assert_eq!(snapshot::read(&path).unwrap(), before);
        if case != "v2" {
            assert!(
                package::preview(&path, &BTreeSet::from([123456789]), &[], &[])
                    .unwrap_err()
                    .contains("starter")
            );
            let missing = package::preview(&path, &BTreeSet::new(), &[], &[])
                .unwrap()
                .missing_incoming_definitions(&BTreeSet::from([
                    10, 20, 100, 200, 300, 77, 3105935002, 3635991036, 3481861797,
                ]))
                .unwrap();
            assert_eq!(missing, BTreeSet::from([123456789]));
            let id = doc.characters().characters()[0].id;
            doc.characters_mut()
                .apply(
                    capabilities,
                    sundial_account::CharacterCommand::UpdateMetadata {
                        character_id: id,
                        update: sundial_account::CharacterMetadataUpdate::SetAppearanceAndClass {
                            race: 0,
                            gender: 0,
                            class_type: 2,
                        },
                    },
                )
                .unwrap();
            assert!(writer::save_for_test(&mut doc, dir.0.join("unsafe.sqlite3")).is_err());
            assert_eq!(snapshot::read(&path).unwrap(), before);
            let mut stale = loaded(&path);
            db.execute_batch("UPDATE starter_character SET level=43")
                .unwrap();
            assert!(matches!(
                writer::save_for_test(&mut stale, dir.0.join("stale.sqlite3")),
                Err(SunriseAccountError::SourceChanged)
            ));
            db.execute_batch("UPDATE starter_character SET level=42")
                .unwrap();
        }
        keep_artifact(&db, case);
    }
}

/// On schema 7 and later the added material stack sits in its authored bucket after the fixture's
/// recovery stack and takes a serial of its own.
fn added_stack_is_placed(db: &Connection) {
    let added: (i64, i64, i64) = db
        .query_row(
            "SELECT placement,position,(SELECT count(DISTINCT mutation_serial) FROM character_stacks) FROM character_stacks WHERE definition_hash=10",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(added, (0, 1, 2));
}

/// Copies the workflow's final account to the artifact folder, when one is configured.
fn keep_artifact(db: &Connection, case: &str) {
    if let Some(output) = crate::test_support::artifacts(&format!("schema-{case}.sqlite3")) {
        db.backup(
            rusqlite::MAIN_DB,
            Path::new(&output),
            rusqlite::backup::NO_PROGRESS,
        )
        .unwrap();
    }
}

#[test]
fn reset_and_recovery_keep_the_selected_schema() {
    for case in CASES {
        let dir = TestDirectory::new("schema-reset");
        let path = dir.0.join("investment.sqlite3");
        fixture(&path, case);
        let before = snapshot::read(&path).unwrap();
        let incompatible = ["v2", "v3-fresh", "v5-fresh", "v7-fresh", "v8-fresh"]
            .into_iter()
            .filter(|other| version(other) != version(case));
        for other in incompatible {
            let backup = dir.0.join(format!("{other}.sqlite3"));
            fixture(&backup, other);
            let backup_before = snapshot::read(&backup).unwrap();
            assert!(
                snapshot::restore(&path, &before, &backup).is_err(),
                "{case} accepted a {other} backup"
            );
            assert_eq!(snapshot::read(&path).unwrap(), before);
            assert_eq!(snapshot::read(&backup).unwrap(), backup_before);
        }
        preserved(&Connection::open(&path).unwrap(), case);
        if let Some(output) =
            crate::test_support::artifacts(&format!("schema-recovery-{case}.sqlite3"))
        {
            Connection::open(&path)
                .unwrap()
                .backup(rusqlite::MAIN_DB, &output, rusqlite::backup::NO_PROGRESS)
                .unwrap();
        }
        // Each schema refuses the other's defaults and snapshots. Schemas 7 and 8 share a bundled
        // stamp, and each resets from it to its own schema.
        let resources = [
            (2, default_resources()),
            (3, v3_resources()),
            (5, v5_resources()),
            (7, v7_resources()),
        ];
        let mut defaults = None;
        for (schema, resources) in resources {
            if schema == bundled(case) {
                defaults = Some(resources);
            } else {
                assert!(
                    ResetPlan::prepare(&path, &resources).is_err(),
                    "{case} accepted schema {schema} defaults"
                );
            }
        }
        let defaults = defaults.expect("Every case has matching defaults");
        let receipt = ResetPlan::prepare(&path, &defaults)
            .unwrap()
            .apply()
            .unwrap();
        let reset = loaded(&path);
        assert_eq!(reset.characters().characters().len(), 3);
        let stamped: i64 = Connection::open(&path)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(stamped, version(case), "{case} reset changed the schema");
        snapshot::restore(
            &path,
            &snapshot::read(&path).unwrap(),
            &receipt.safety_backup,
        )
        .unwrap();
        assert_eq!(snapshot::read(&path).unwrap(), before);
        preserved(&Connection::open(&path).unwrap(), case);
    }
}

#[test]
fn malformed_accounts_are_refused_without_mutation() {
    for case in ["v3-fresh", "v5-fresh", "v7-fresh", "v8-fresh"] {
        let mut failures = vec![
            "PRAGMA user_version=4",
            "DROP TABLE starter_items",
            "PRAGMA ignore_check_constraints=ON; UPDATE characters SET appearance_header=zeroblob(35)",
            "DELETE FROM starter_character",
            "UPDATE starter_character SET appearance_value=1e300",
            "UPDATE starter_character SET last_orbited_destination=-1",
            "WITH RECURSIVE n(x) AS (VALUES(10) UNION ALL SELECT x+1 FROM n WHERE x<26) INSERT INTO starter_items SELECT 0,x,100 FROM n",
        ];
        if version(case) >= 5 {
            failures.extend(["DROP TABLE parked_items", "DROP TABLE starter_unlocks"]);
        } else {
            failures.extend([
                "ALTER TABLE characters DROP COLUMN appearance_header_valid",
                "PRAGMA ignore_check_constraints=ON; UPDATE characters SET appearance_header_valid=2",
            ]);
        }
        if version(case) >= 7 {
            failures.extend([
                "PRAGMA user_version=6",
                "DROP TABLE item_objectives",
                // An equipped item in a recovery bucket, and two stacks sharing a serial.
                "PRAGMA ignore_check_constraints=ON; UPDATE items SET placement=1 WHERE location=0",
                "INSERT INTO character_stacks VALUES(0,1,30,1,1,0)",
            ]);
        }
        if case == "v8-fresh" {
            failures.extend([
                "DROP TABLE mission_flags",
                // A boolean flag holding a count.
                "PRAGMA ignore_check_constraints=ON; INSERT INTO mission_flags VALUES('example.mod','done',1,2)",
            ]);
        }
        for sql in failures {
            let dir = TestDirectory::new("malformed-account");
            let path = dir.0.join("investment.sqlite3");
            fixture(&path, case);
            Connection::open(&path).unwrap().execute_batch(sql).unwrap();
            let before = snapshot::capture_path(&path).unwrap();
            assert!(
                !matches!(
                    document::load(&path),
                    Ok(SunriseAccountDocumentLoad::Loaded(_))
                ),
                "{case}: {sql}"
            );
            assert!(package::preview(&path, &BTreeSet::from([300]), &[], &[]).is_err());
            assert_eq!(snapshot::capture_path(&path).unwrap(), before);
        }
    }
}

#[test]
fn native_roster_changes_require_reload_and_preserve_surviving_identity() {
    let dir = TestDirectory::new("native-roster");
    let path = dir.0.join("investment.sqlite3");
    fixture(&path, "v3-migrated");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("INSERT INTO characters SELECT 1,12345,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial,appearance_header,appearance_header_valid FROM characters WHERE slot=0; INSERT INTO characters SELECT 2,12346,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial,appearance_header,appearance_header_valid FROM characters WHERE slot=0").unwrap();
    for count in (0..=3).rev() {
        let mut doc = loaded(&path);
        assert_eq!(doc.characters().characters().len(), count);
        save_fixture_document(&mut doc, &dir.0.join(format!("roster-{count}.sqlite3")));
        if count > 0 {
            db.execute_batch("PRAGMA foreign_keys=ON; DELETE FROM items WHERE character_slot=0; DELETE FROM characters WHERE slot=0; UPDATE characters SET slot=slot-1").unwrap();
            assert!(matches!(
                writer::save_for_test(&mut doc, dir.0.join("stale.sqlite3")),
                Err(SunriseAccountError::SourceChanged)
            ));
            if count == 3 {
                assert_eq!(
                    db.query_row("SELECT soid FROM characters WHERE slot=0", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    12345
                );
            }
        }
    }
    if let Some(output) = crate::test_support::artifacts("schema-v3-empty.sqlite3") {
        db.backup(
            rusqlite::MAIN_DB,
            Path::new(&output),
            rusqlite::backup::NO_PROGRESS,
        )
        .unwrap();
    }
}
