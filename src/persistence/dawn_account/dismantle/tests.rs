use crate::persistence::dawn_account::{
    self as dawn, DawnAccountDocument, DawnAccountDocumentLoad,
};
use rusqlite::Connection;
use sundial_account::DismantleRewardCommand;

fn fixture(count: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("player-state.db");
    dawn::tests::create_fixture(&path);
    let db = Connection::open(&path).unwrap();
    for i in 0..count {
        db.execute("INSERT INTO dismantle_rewards VALUES(?1,?2,1)", [i, i + 10])
            .unwrap();
    }
    (dir, path)
}

fn loaded(path: &std::path::Path) -> Box<DawnAccountDocument> {
    let DawnAccountDocumentLoad::Loaded(doc) = dawn::load(path).unwrap() else {
        panic!("Expected Dawn fixture")
    };
    doc
}

#[test]
fn dismantle_shrink_compacts_rows_and_survives_saved_undo() {
    let (_dir, path) = fixture(8);
    let mut doc = loaded(&path);
    let mut before = doc.clone();
    let id = doc.profile().dismantle_rewards()[2].id;
    doc.profile_mut()
        .apply_dismantle_reward(
            DawnAccountDocument::profile_capabilities(),
            DismantleRewardCommand::Remove { id },
        )
        .unwrap();
    dawn::save(&mut doc).unwrap();
    let read = loaded(&path);
    assert_eq!(read.account_revision(), 3);
    assert_eq!(super::rows(read.profile()), super::rows(doc.profile()));
    assert_eq!(read.profile().dismantle_rewards().len(), 7);
    before.adopt_revision(&doc);
    dawn::save(&mut before).unwrap();
    assert_eq!(loaded(&path).profile().dismantle_rewards().len(), 8);
}

#[test]
fn invalid_dismantle_capacity_or_positions_report_incompatibility_without_writes() {
    for (count, gap) in [(9, false), (2, true)] {
        let (_dir, path) = fixture(count);
        let db = Connection::open(&path).unwrap();
        if gap {
            db.execute(
                "UPDATE dismantle_rewards SET position=3 WHERE position=1",
                [],
            )
            .unwrap();
        }
        let before = crate::persistence::native_account::snapshot::capture(&db).unwrap();
        assert!(matches!(
            dawn::load(&path).unwrap(),
            DawnAccountDocumentLoad::Incompatible(_)
        ));
        assert_eq!(
            crate::persistence::native_account::snapshot::capture(&db).unwrap(),
            before
        );
    }
}

#[test]
fn dismantle_edits_refuse_unversioned_changes() {
    let (_dir, path) = fixture(2);
    let mut doc = loaded(&path);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE dismantle_rewards SET quantity=2 WHERE position=0",
        [],
    )
    .unwrap();
    let before = crate::persistence::native_account::snapshot::capture(&db).unwrap();
    assert!(dawn::save(&mut doc).is_err());
    assert_eq!(
        crate::persistence::native_account::snapshot::capture(&db).unwrap(),
        before
    );
}
