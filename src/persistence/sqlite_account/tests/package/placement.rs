use super::*;
use crate::investment::{AuthoredMoveOutcome, AuthoredSlotChange, AuthoredSlotReplacement};
use crate::persistence::sqlite_account::package::preview_replacement;
use std::collections::BTreeMap;

fn slots(capacity: usize) -> AuthoredSlotReplacement {
    AuthoredSlotReplacement {
        changes: vec![AuthoredSlotChange {
            definition_hash: 100,
            previous_bucket: 0,
            incoming_bucket: 1,
        }],
        incoming_buckets: BTreeMap::from([(100, 1), (200, 16), (300, 1)]),
        weapon_capacities: [10, capacity, 10],
    }
}

#[test]
fn slot_replacement_preserves_sqlite_rows_wal_extensions_and_exact_rollback() {
    for (capacity, removed, expected) in [
        (2, BTreeSet::new(), AuthoredMoveOutcome::MovedToInventory),
        (
            1,
            BTreeSet::new(),
            AuthoredMoveOutcome::DeletedInventoryFull,
        ),
        (
            1,
            BTreeSet::from([300]),
            AuthoredMoveOutcome::MovedToInventory,
        ),
    ] {
        let dir = TestDirectory::new("sqlite-slot-replacement");
        let path = dir.0.join("investment.sqlite3");
        create_fixture(&path, 3);
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            ALTER TABLE items ADD COLUMN future TEXT DEFAULT 'keep';
            UPDATE items SET flags=3,socket_policy=1,plug_count=2,seen=1 WHERE definition_hash=100;
            INSERT INTO sockets SELECT instance_soid,0,88 FROM items WHERE definition_hash=100;
            CREATE TABLE extension(value TEXT);
            INSERT INTO extension VALUES('keep');
            INSERT INTO characters SELECT 1,soid+1,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters WHERE slot=0;
            INSERT INTO characters SELECT 2,soid+2,race,gender,class,level,preview_available,appearance_value,last_orbited_destination,content_bypass,equipped_title,acquired_subclass_mask,next_inventory_serial FROM characters WHERE slot=0;")
            .unwrap();
        // Characters with items use their actual slot, not their item-query enumeration order.
        db.execute_batch("INSERT INTO items SELECT 2,location,position,instance_soid+100,definition_hash,level,quantity,mutation_serial,flags,socket_policy,plug_count,movement_ability,grenade_ability,super_ability,melee_ability,class_ability,seen,future FROM items WHERE definition_hash=100;").unwrap();
        let original = read(&path).unwrap();
        let replacement = slots(capacity);
        let proposal = preview_replacement(&path, &removed, &[], &[], Some(&replacement)).unwrap();
        assert_eq!(read(&path).unwrap(), original);
        assert_eq!(proposal.slot_moves.len(), 2);
        assert_eq!(proposal.slot_moves[0].outcome, expected);
        assert_eq!(proposal.slot_moves[1].character_index, 2);
        assert_eq!(
            proposal.slot_moves[1].outcome,
            AuthoredMoveOutcome::MovedToInventory
        );
        replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).unwrap();
        if expected == AuthoredMoveOutcome::MovedToInventory {
            let state = db.query_row("SELECT location,position,flags,plug_count,seen,future FROM items WHERE instance_soid=4611686018427387905", [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, String>(5)?))).unwrap();
            assert_eq!(
                state,
                (1, i64::from(removed.is_empty()), 3, 2, 1, "keep".into())
            );
            assert_eq!(
                db.query_row(
                    "SELECT plug_hash FROM sockets WHERE instance_soid=4611686018427387905",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
                88
            );
        } else {
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM items WHERE instance_soid=4611686018427387905",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM sockets WHERE instance_soid=4611686018427387905",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
        }
        let repeat = preview_replacement(&path, &removed, &[], &[], Some(&replacement)).unwrap();
        assert_eq!(repeat.original_bytes, repeat.cleaned_bytes);
        replace(&path, &proposal.cleaned_bytes, &proposal.original_bytes).unwrap();
        assert_eq!(read(&path).unwrap(), original);
        db.execute("UPDATE extension SET value='changed'", [])
            .unwrap();
        assert!(replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).is_err());
    }
}
