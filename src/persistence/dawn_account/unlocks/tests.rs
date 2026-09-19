use super::super::tests::create_fixture;
use super::*;
use sundial_account::UnlockScope;

const ACCOUNT_SOID: &str = "9EAA300100100100";
const CHARACTER_SOID: &str = "9EAA300100100101";

fn unlock(bank: u8, slot: u16) -> AuthoredUnlock {
    AuthoredUnlock {
        definition_index: 1,
        bank,
        slot,
    }
}

fn open(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn flags(path: &Path) -> Vec<(i64, String, i64, i64)> {
    let connection = open(path);
    let mut statement = connection
        .prepare(
            "SELECT scope,owner_soid,slot,value FROM durable_flags ORDER BY scope,owner_soid,slot",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn revision(path: &Path) -> i64 {
    open(path)
        .query_row(
            "SELECT value FROM metadata WHERE key='account_revision'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

/// An account-scope unlock becomes one durable flag owned by the account, stored as the biased
/// true value Dawn reads, and the commit advances the revision the way Dawn's own writes do.
#[test]
fn an_account_unlock_sets_one_durable_flag_and_advances_the_revision() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let before = revision(&path);

    let receipt =
        apply_authored_unlocks(&path, &[unlock(UnlockScope::Account.bank(), 11_930)]).unwrap();

    assert_eq!(receipt.changed, 1);
    assert!(receipt.backup.is_some_and(|backup| backup.exists()));
    assert_eq!(
        flags(&path),
        vec![(0, ACCOUNT_SOID.to_owned(), 11_930, i64::from(FLAG_SET))]
    );
    assert_eq!(revision(&path), before + 1);
}

/// Installing the same package twice must not look like a second change. A re-applied unlock
/// writes nothing, takes no backup, and leaves the revision alone, so Dawn never sees a commit
/// that did nothing.
#[test]
fn re_applying_an_unlock_writes_nothing_and_takes_no_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let unlocks = [unlock(UnlockScope::Account.bank(), 11_930)];
    apply_authored_unlocks(&path, &unlocks).unwrap();
    let after_first = revision(&path);
    let rows = flags(&path);

    let receipt = apply_authored_unlocks(&path, &unlocks).unwrap();

    assert_eq!(receipt.changed, 0);
    assert_eq!(receipt.backup, None);
    assert_eq!(flags(&path), rows);
    assert_eq!(revision(&path), after_first);
}

/// Each character owns its own copy of the character banks, so an unlock in one is set for every
/// character rather than only for whichever happens to be first.
#[test]
fn a_character_bank_unlock_is_set_for_every_character() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let second = "9EAA300100100102";
    open(&path)
        .execute(
            "INSERT INTO characters VALUES(1,?1,0,0,0,1,50,1,1,1.0,308080871,1,6,7,10,15,2,93,0)",
            params![second],
        )
        .unwrap();

    let receipt =
        apply_authored_unlocks(&path, &[unlock(UnlockScope::Character.bank(), 200)]).unwrap();

    // One definition changed, even though it took a row per character.
    assert_eq!(receipt.changed, 1);
    assert_eq!(
        flags(&path),
        vec![
            (2, CHARACTER_SOID.to_owned(), 200, i64::from(FLAG_SET)),
            (2, second.to_owned(), 200, i64::from(FLAG_SET)),
        ]
    );
}

/// A slot outside the bank's region would land in whatever the engine keeps after it, so the
/// whole write is refused before the database is opened for writing.
#[test]
fn a_slot_beyond_its_bank_is_refused_before_anything_is_written() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let before = revision(&path);

    let error = apply_authored_unlocks(
        &path,
        &[
            unlock(UnlockScope::Account.bank(), 0),
            unlock(UnlockScope::Character.bank(), 256),
        ],
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("character slot 256"), "{error}");
    assert!(flags(&path).is_empty(), "nothing may be written");
    assert_eq!(revision(&path), before);
}

/// Dawn refuses to boot when the shape of its metadata, allocators or account graph changes, so
/// an unlock write must leave every one of them exactly as it found them.
#[test]
fn setting_an_unlock_leaves_the_boot_critical_tables_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    let snapshot = |table: &str| {
        let connection = open(&path);
        let mut statement = connection
            .prepare(&format!("SELECT count(*) FROM {table}"))
            .unwrap();
        statement.query_row([], |row| row.get::<_, i64>(0)).unwrap()
    };
    let before = [
        snapshot("metadata"),
        snapshot("allocators"),
        snapshot("characters"),
        snapshot("character_items"),
        snapshot("item_sockets"),
        snapshot("profile_items"),
    ];

    apply_authored_unlocks(&path, &[unlock(UnlockScope::Account.bank(), 12_299)]).unwrap();

    let after = [
        snapshot("metadata"),
        snapshot("allocators"),
        snapshot("characters"),
        snapshot("character_items"),
        snapshot("item_sockets"),
        snapshot("profile_items"),
    ];
    assert_eq!(before, after);
    let ok: String = open(&path)
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
    let orphans: i64 = open(&path)
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(1))
        .optional()
        .unwrap()
        .unwrap_or(0);
    assert_eq!(orphans, 0, "an unlock write must not orphan any row");
}
