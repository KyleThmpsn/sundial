//! Real-file workflows against the reviewed upstream schemas and migration SQL.
use super::super::{defaults::ResetPlan, package, snapshot};
use super::*;
use std::collections::BTreeSet;

pub(crate) const CASES: [&str; 4] = ["v2", "v3-fresh", "v3-migrated", "v5-fresh"];

pub(crate) fn fixture(path: &Path, case: &str) {
    assert!(CASES.contains(&case), "Unknown account fixture: {case}");
    let v5 = case == "v5-fresh";
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
        db.execute_batch(if v5 {
            include_str!("../fixtures/v5/investment_schema.sql")
        } else {
            include_str!("../fixtures/v3/investment_schema.sql")
        })
        .unwrap();
        if v5 {
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
        db.execute("ATTACH DATABASE ? AS seed", [seed.to_str().unwrap()])
            .unwrap();
        let names: Vec<String> = db.prepare("SELECT name FROM seed.sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
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
            } else {
                ("", "")
            };
            db.execute_batch(&format!("INSERT INTO main.\"{table}\" ({columns}{extra}) SELECT {columns}{values} FROM seed.\"{table}\";")).unwrap();
        }
        db.execute_batch("COMMIT; DETACH DATABASE seed").unwrap();
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
    }
}

pub(crate) fn preserved(db: &Connection, case: &str) {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let v5 = case == "v5-fresh";
    assert_eq!(
        version,
        if case == "v2" {
            2
        } else if v5 {
            5
        } else {
            3
        }
    );
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
        doc.characters_mut()
            .apply(
                SqliteAccountDocument::character_capabilities(),
                sundial_account::CharacterCommand::UpdateInventoryItem {
                    item_id,
                    update: sundial_account::ItemUpdate::SetQuantity(9),
                },
            )
            .unwrap();
        save_fixture_document(&mut doc, &dir.0.join("saved.sqlite3"));
        assert_eq!(
            loaded(&path).characters().characters()[0].inventory[0].quantity,
            9
        );
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
                    SqliteAccountDocument::character_capabilities(),
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
                Err(SqliteAccountError::SourceChanged)
            ));
            db.execute_batch("UPDATE starter_character SET level=42")
                .unwrap();
        }
        if let Some(output) = crate::test_support::artifacts(&format!("schema-{case}.sqlite3")) {
            db.backup(
                rusqlite::MAIN_DB,
                Path::new(&output),
                rusqlite::backup::NO_PROGRESS,
            )
            .unwrap();
        }
    }
}

#[test]
fn reset_and_recovery_keep_the_selected_schema() {
    for case in CASES {
        let dir = TestDirectory::new("schema-reset");
        let path = dir.0.join("investment.sqlite3");
        fixture(&path, case);
        // Each schema refuses the other's defaults and snapshots.
        let defaults = if case == "v2" {
            let other = dir.0.join("v3.sqlite3");
            fixture(&other, "v3-fresh");
            assert!(snapshot::restore(&path, &snapshot::read(&path).unwrap(), &other).is_err());
            default_resources()
        } else {
            assert!(ResetPlan::prepare(&path, &default_resources()).is_err());
            if case == "v5-fresh" {
                assert!(ResetPlan::prepare(&path, &v3_resources()).is_err());
                v5_resources()
            } else {
                assert!(ResetPlan::prepare(&path, &v5_resources()).is_err());
                v3_resources()
            }
        };
        let before = snapshot::read(&path).unwrap();
        let receipt = ResetPlan::prepare(&path, &defaults)
            .unwrap()
            .apply()
            .unwrap();
        let reset = loaded(&path);
        assert_eq!(reset.characters().characters().len(), 3);
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
    for case in ["v3-fresh", "v5-fresh"] {
        let mut failures = vec![
            "PRAGMA user_version=4",
            "DROP TABLE starter_items",
            "PRAGMA ignore_check_constraints=ON; UPDATE characters SET appearance_header=zeroblob(35)",
            "DELETE FROM starter_character",
            "UPDATE starter_character SET appearance_value=1e300",
            "UPDATE starter_character SET last_orbited_destination=-1",
            "WITH RECURSIVE n(x) AS (VALUES(10) UNION ALL SELECT x+1 FROM n WHERE x<26) INSERT INTO starter_items SELECT 0,x,100 FROM n",
        ];
        failures.extend(if case == "v5-fresh" {
        ["DROP TABLE parked_items", "DROP TABLE starter_unlocks"]
    } else {
        ["ALTER TABLE characters DROP COLUMN appearance_header_valid", "PRAGMA ignore_check_constraints=ON; UPDATE characters SET appearance_header_valid=2"]
    });
        for sql in failures {
            let dir = TestDirectory::new("malformed-account");
            let path = dir.0.join("investment.sqlite3");
            fixture(&path, case);
            Connection::open(&path).unwrap().execute_batch(sql).unwrap();
            let before = snapshot::capture_path(&path).unwrap();
            assert!(
                !matches!(
                    document::load(&path),
                    Ok(SqliteAccountDocumentLoad::Loaded(_))
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
                Err(SqliteAccountError::SourceChanged)
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
