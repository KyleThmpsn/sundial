use super::super::tests::create_fixture;
use super::*;
use crate::account::{AuthoredSlotChange, AuthoredSocketChange};
use rusqlite::Connection;

fn held(path: &Path, table: &str) -> Vec<(i64, i64)> {
    let db = Connection::open(path).unwrap();
    let mut statement = db
        .prepare(&format!(
            "SELECT position,definition_hash FROM {table} ORDER BY position"
        ))
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn apply(path: &Path, hashes: &[u32]) -> AuthoredAccountCleanup {
    let report =
        preview_replacement(path, &hashes.iter().copied().collect(), &[], &[], None).unwrap();
    replace(path, &report.original_bytes, &report.cleaned_bytes).unwrap();
    report
}

/// An item whose package is going away has to leave the account with it. Dawn abandons the whole
/// character loadout over one unresolvable item, so a cleanup that misses it costs the player
/// every equipped item until the package comes back.
#[test]
fn removing_a_package_takes_its_items_out_of_the_account() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    // Three inventory rows, the middle one from the package being removed.
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "DELETE FROM character_items;
             INSERT INTO character_items VALUES
                ('9EAA300100100101',1,0,'4000000000000101',111,106,1,0,0,0,0),
                ('9EAA300100100101',1,1,'4000000000000102',222,106,1,0,0,0,0),
                ('9EAA300100100101',1,2,'4000000000000103',333,106,1,0,0,0,0);
             INSERT INTO item_sockets VALUES('4000000000000102',0,222);",
        )
        .unwrap();
    }

    let revision = |path: &Path| -> i64 {
        Connection::open(path)
            .unwrap()
            .query_row(
                "SELECT value FROM metadata WHERE key='account_revision'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    let before = revision(&path);

    let report = apply(&path, &[222]);

    assert_eq!(report.removed_items.get(&222), Some(&1));
    assert!(report.changed_anything());
    // A save from a document loaded before this cleanup must now fail its revision check rather
    // than write the removed item back.
    assert_eq!(revision(&path), before + 1);
    // The rows that stay are renumbered, because Dawn refuses an account whose inventory
    // positions do not run contiguously from zero.
    assert_eq!(held(&path, "character_items"), vec![(0, 111), (1, 333)]);
    let orphans: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM item_sockets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(orphans, 0, "the removed item's sockets cascade with it");
}

/// The cleanup must leave everything it was not asked to remove exactly as it found it.
#[test]
fn a_cleanup_with_nothing_to_remove_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let before = held(&path, "character_items");

    let report =
        preview_replacement(&path, &[4_242_424].into_iter().collect(), &[], &[], None).unwrap();

    assert!(report.removed_items.is_empty());
    assert_eq!(report.cleared_plugs, 0);
    assert!(!report.changed_anything());
    assert_eq!(report.original_bytes, report.cleaned_bytes);
    assert_eq!(held(&path, "character_items"), before);
}

/// Equipment is sparse by design: a character with nothing in the first slots really does start at
/// position 3, so compaction must not renumber it.
#[test]
fn equipped_slots_keep_their_positions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "DELETE FROM character_items;
             INSERT INTO character_items VALUES
                ('9EAA300100100101',0,3,'4000000000000201',111,106,1,0,0,0,0),
                ('9EAA300100100101',0,7,'4000000000000202',222,106,1,0,0,0,0);",
        )
        .unwrap();
    }

    apply(&path, &[999]);

    assert_eq!(held(&path, "character_items"), vec![(3, 111), (7, 222)]);
}

fn lanes(path: &Path, soid: &str) -> Vec<Option<i64>> {
    let db = Connection::open(path).unwrap();
    let mut statement = db
        .prepare("SELECT plug_hash FROM item_sockets WHERE instance_soid=?1 ORDER BY lane")
        .unwrap();
    statement
        .query_map([soid], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// A retained item whose definition gained or lost sockets keeps the plugs it had and takes the
/// new defaults only for the lanes it did not have. Empty lanes are written too, because Dawn
/// refuses an item whose lanes do not run contiguously from zero.
#[test]
fn a_replaced_definition_resizes_the_sockets_of_items_that_stay() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    // The fixture's equipped item stores two lanes; the inventory item is on native defaults.
    for (previous, defaults, expected) in [
        (
            2,
            vec![Some(99), Some(98), None, Some(88)],
            vec![Some(3961599962), None, None, Some(88)],
        ),
        (4, vec![Some(99)], vec![Some(3961599962)]),
    ] {
        let report = preview_replacement(
            &path,
            &BTreeSet::new(),
            &[],
            &[AuthoredSocketChange {
                definition_hash: 4070132608,
                previous_socket_count: previous,
                default_plugs: defaults,
            }],
            None,
        )
        .unwrap();
        assert_eq!(report.resized_items[&4070132608], 1);
        replace(&path, &report.original_bytes, &report.cleaned_bytes).unwrap();
        assert_eq!(lanes(&path, "4000000000000004"), expected);
        assert!(lanes(&path, "400000000000001A").is_empty());
    }
    // The account still loads under Dawn's own contiguity and policy checks.
    assert!(matches!(
        super::super::load(&path).unwrap(),
        super::super::DawnAccountDocumentLoad::Loaded(_)
    ));
}

/// A socket count that does not match the reviewed package means the account moved on, and the
/// proposal refuses rather than guessing which lanes to keep.
#[test]
fn a_socket_count_the_review_did_not_see_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let error = preview_replacement(
        &path,
        &BTreeSet::new(),
        &[],
        &[AuthoredSocketChange {
            definition_hash: 4070132608,
            previous_socket_count: 3,
            default_plugs: vec![Some(1)],
        }],
        None,
    )
    .unwrap_err();
    assert!(error.contains("socket count differs"), "{error}");
}

/// An equipped weapon whose native slot changed moves to the inventory, or leaves the account
/// when that inventory bucket is full, so the loadout Dawn prepares never holds a weapon in a
/// slot its definition no longer fits.
#[test]
fn a_weapon_whose_slot_changed_leaves_its_equipment_position() {
    for (capacity, expected) in [
        (2, AuthoredMoveOutcome::MovedToInventory),
        (1, AuthoredMoveOutcome::DeletedInventoryFull),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("player-state.db");
        create_fixture(&path);
        {
            let db = Connection::open(&path).unwrap();
            // Item 100 sits in the kinetic slot (position 0) and moves to energy. A stored energy
            // weapon (300) already counts against that bucket's capacity.
            db.execute_batch(
                "DELETE FROM character_items;
                 INSERT INTO character_items VALUES
                    ('9EAA300100100101',0,0,'4000000000000301',100,106,1,0,3,1,0),
                    ('9EAA300100100101',1,0,'4000000000000302',300,106,1,0,0,0,0);
                 INSERT INTO item_sockets VALUES('4000000000000301',0,88);",
            )
            .unwrap();
        }
        let replacement = AuthoredSlotReplacement {
            changes: vec![AuthoredSlotChange {
                definition_hash: 100,
                previous_bucket: 0,
                incoming_bucket: 1,
            }],
            incoming_buckets: BTreeMap::from([(100, 1), (300, 1)]),
            weapon_capacities: [10, capacity, 10],
        };
        let report =
            preview_replacement(&path, &BTreeSet::new(), &[], &[], Some(&replacement)).unwrap();
        assert_eq!(report.slot_moves.len(), 1);
        assert_eq!(report.slot_moves[0].outcome, expected);
        assert_eq!(report.slot_moves[0].equipment_slot, "kinetic");
        replace(&path, &report.original_bytes, &report.cleaned_bytes).unwrap();

        let db = Connection::open(&path).unwrap();
        let rows: Vec<(i64, i64, i64)> = db
            .prepare("SELECT location,position,definition_hash FROM character_items ORDER BY location,position")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        match expected {
            AuthoredMoveOutcome::MovedToInventory => {
                assert_eq!(rows, vec![(1, 0, 300), (1, 1, 100)]);
                assert_eq!(lanes(&path, "4000000000000301"), vec![Some(88)]);
            }
            AuthoredMoveOutcome::DeletedInventoryFull => {
                assert_eq!(rows, vec![(1, 0, 300)]);
                assert!(lanes(&path, "4000000000000301").is_empty());
            }
        }
        assert!(matches!(
            super::super::load(&path).unwrap(),
            super::super::DawnAccountDocumentLoad::Loaded(_)
        ));
        // Applying the same review again finds nothing left to do.
        let repeat =
            preview_replacement(&path, &BTreeSet::new(), &[], &[], Some(&replacement)).unwrap();
        assert!(repeat.slot_moves.is_empty());
    }
}
