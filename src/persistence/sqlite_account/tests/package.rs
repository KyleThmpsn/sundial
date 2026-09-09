use super::super::package::{preview, read, replace};
use super::*;
use crate::investment::{AuthoredCollectionUnlock, AuthoredSocketChange};
use std::collections::BTreeSet;

#[test]
fn override_only_cleanup_is_reported_as_removed_account_data() {
    let dir = TestDirectory::new("sqlite-override-cleanup");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    Connection::open(&path)
        .unwrap()
        .execute("INSERT INTO family5 VALUES(0,0,7,2)", [])
        .unwrap();
    let cleanup = preview(
        &path,
        &BTreeSet::new(),
        &[AuthoredCollectionUnlock {
            definition_index: 7,
            bank: 1,
            slot: 42,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(cleanup.cleared_unlocks, 1);
    replace(&path, &cleanup.original_bytes, &cleanup.cleaned_bytes).unwrap();
    assert_eq!(
        Connection::open(&path)
            .unwrap()
            .query_row("SELECT count(*) FROM family5", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn native_cleanup_is_read_only_until_commit_and_recovers_exactly_with_wal() {
    let dir = TestDirectory::new("sqlite-package");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; INSERT INTO unlocks VALUES(-1,0,42,0,1); INSERT INTO family5 VALUES(0,0,7,2),(0,1,8,2); INSERT INTO pending_rewards(character_slot,kind,definition_hash,quantity) VALUES(0,0,300,1); CREATE TABLE extension (value TEXT); INSERT INTO extension VALUES('keep');").unwrap();
    let original = read(&path).unwrap();
    let hashes = BTreeSet::from([300]);
    let unlocks = [AuthoredCollectionUnlock {
        definition_index: 7,
        bank: 1,
        slot: 42,
    }];
    let proposal = preview(&path, &hashes, &unlocks, &[]).unwrap();
    assert_eq!(read(&path).unwrap(), original);
    assert_eq!(proposal.removed_items[&300], 1);
    assert_eq!(proposal.removed_reward_rules, 1);
    assert_eq!(proposal.cleared_unlocks, 1);
    replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).unwrap();
    assert_eq!(
        loaded(&path).characters().characters()[0].inventory.len(),
        0
    );
    assert_eq!(
        db.query_row("SELECT position FROM family5 WHERE slot=8", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM family5 WHERE slot=7", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let repeat = preview(&path, &hashes, &unlocks, &[]).unwrap();
    assert_eq!(repeat.original_bytes, repeat.cleaned_bytes);
    replace(&path, &proposal.cleaned_bytes, &proposal.original_bytes).unwrap();
    assert_eq!(read(&path).unwrap(), original);
    db.execute("UPDATE extension SET value='outside'", [])
        .unwrap();
    let outside = read(&path).unwrap();
    assert!(replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).is_err());
    assert_eq!(read(&path).unwrap(), outside);
}

#[test]
fn socket_replacement_preserves_selected_lanes_and_handles_growth_and_shrink() {
    let dir = TestDirectory::new("sqlite-socket-replacement");
    let path = dir.0.join("investment.sqlite3");
    create_fixture(&path, 3);
    for (previous, defaults, expected) in [
        (
            2,
            vec![Some(99), Some(98), Some(88)],
            vec![Some(77), None, Some(88)],
        ),
        (3, vec![Some(99)], vec![Some(77)]),
    ] {
        let proposal = preview(
            &path,
            &BTreeSet::new(),
            &[],
            &[AuthoredSocketChange {
                definition_hash: 200,
                previous_socket_count: previous,
                default_plugs: defaults,
            }],
        )
        .unwrap();
        assert_eq!(proposal.resized_items[&200], 1);
        replace(&path, &proposal.original_bytes, &proposal.cleaned_bytes).unwrap();
        let doc = loaded(&path);
        let item = doc.characters().characters()[0].equipment[&EquipmentSlot::new("subclass")]
            .as_ref()
            .unwrap();
        assert_eq!(
            item.plugs,
            ItemPlugs::Authored(
                expected
                    .into_iter()
                    .map(|v| v.map(sundial_account::DefinitionHash::new))
                    .collect()
            )
        );
    }
}
