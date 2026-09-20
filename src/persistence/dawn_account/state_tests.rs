use super::*;
use crate::catalog::{InventoryMetadata, InventoryScope, ItemStackability};
use rusqlite::{Connection, params};

const OWNER: &str = "9EAA300100100101";
const ACCOUNT: &str = "9EAA300100100100";
const STORED: u64 = 0x400000000000001A;
const EQUIPPED: u64 = 0x4000000000000004;

#[test]
fn vendor_swaps_and_unlock_reordering_preserve_extension_columns() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE vendor_progress ADD COLUMN extra TEXT DEFAULT 'new';
        ALTER TABLE vendor_unlocks ADD COLUMN extra TEXT DEFAULT 'new';
        INSERT INTO vendor_progress VALUES('9EAA300100100100',0,20,10,0,'first'),('9EAA300100100100',1,21,20,0,'second');
        INSERT INTO vendor_unlocks VALUES('9EAA300100100100',0,0,14,1,'slot14'),('9EAA300100100100',0,1,15,2,'slot15');").unwrap();
    let mut doc = open(&path);
    let mut state = doc.activity_state().clone();
    state.vendors[0].vendor = 21;
    state.vendors[1].vendor = 20;
    state.unlocks[0].position = 1;
    state.unlocks[1].position = 0;
    doc.set_activity_state(state).unwrap();
    save(&mut doc).unwrap();
    assert_eq!(open(&path).activity_state(), doc.activity_state());
    assert_eq!(
        db.query_row(
            "SELECT extra FROM vendor_progress WHERE position=0",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "first"
    );
    assert_eq!(
        db.query_row("SELECT extra FROM vendor_unlocks WHERE slot=14", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "slot14"
    );
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("player-state.db");
    tests::create_fixture(&path);
    (temp, path)
}

fn open(path: &std::path::Path) -> Box<DawnAccountDocument> {
    let DawnAccountDocumentLoad::Loaded(doc) = load(path).unwrap() else {
        panic!("fixture failed to load")
    };
    doc
}

fn metadata(scope: InventoryScope, bucket: u8, capacity: u16, max: u32) -> InventoryMetadata {
    InventoryMetadata {
        scope,
        native_bucket_id: bucket,
        bucket_capacity: Some(capacity),
        max_stack_size: Some(max),
        stackability: ItemStackability::Stackable,
    }
}

#[test]
fn vendor_mission_and_campaign_edits_roundtrip_without_rewards_or_unknown_column_loss() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "ALTER TABLE vendor_unlocks ADD COLUMN extra TEXT DEFAULT 'keep';
        INSERT INTO vendor_progress VALUES('9EAA300100100100',2,20,8000,2);
        INSERT INTO vendor_unlocks(owner_soid,kind,position,slot,value) VALUES
          ('9EAA300100100100',0,0,14,2),('9EAA300100100100',0,1,15,1),('9EAA300100100100',0,2,16,0);
        INSERT INTO missions VALUES('9EAA300100100101',123,456,2,7,9,0,100);",
    )
    .unwrap();
    let mut doc = open(&path);
    let mut state = doc.activity_state().clone();
    state.vendors[0].points = 12000;
    state.vendors[0].rewards = 3;
    state.unlocks.remove(0);
    for (i, r) in state.unlocks.iter_mut().enumerate() {
        r.position = i as i32;
    }
    state.unlocks[0].value = 2;
    state.missions[0].checkpoint = 0;
    state.missions[0].slice = 0;
    state.missions[0].progress = 0;
    state.missions[0].completed = true;
    doc.set_activity_state(state).unwrap();
    doc.set_vendor_campaigns(0, 5).unwrap();
    let receipt = save(&mut doc).unwrap();
    assert!(receipt.backup.exists());
    let after = open(&path);
    assert_eq!(after.activity_state(), doc.activity_state());
    assert_eq!(after.vendor_campaigns(0), Some(5));
    assert_eq!(after.activity_state().unlocks[0].slot, 15);
    assert_eq!(
        db.query_row("SELECT extra FROM vendor_unlocks WHERE slot=15", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        "keep"
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM reward_debts", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
    assert!(after.activity_state().missions[0].updated > 100);
}

#[test]
fn vendor_and_mission_edits_detect_unversioned_external_changes() {
    for mission in [false, true] {
        let (_temp, path) = fixture();
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "INSERT INTO vendor_progress VALUES('9EAA300100100100',0,20,2000,0);
            INSERT INTO missions VALUES('9EAA300100100101',123,456,2,7,9,0,100);",
        )
        .unwrap();
        let mut doc = open(&path);
        let mut state = doc.activity_state().clone();
        state.vendors[0].points = 6000;
        doc.set_activity_state(state).unwrap();
        db.execute_batch(if mission {
            "UPDATE missions SET progress=12"
        } else {
            "UPDATE vendor_progress SET points=4000"
        })
        .unwrap();
        assert!(save(&mut doc).unwrap_err().to_string().contains("Reload"));
        assert_eq!(
            db.query_row(
                "SELECT value FROM metadata WHERE key='account_revision'",
                [],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            2
        );
    }
}

#[test]
fn vendor_validation_is_atomic_and_scoped() {
    let (_temp, path) = fixture();
    let mut doc = open(&path);
    let original = doc.clone();
    for bad in [
        VendorProgress {
            owner: ACCOUNT.into(),
            position: 16,
            vendor: 20,
            points: 0,
            rewards: 0,
        },
        VendorProgress {
            owner: OWNER.into(),
            position: 0,
            vendor: u16::MAX,
            points: 0,
            rewards: 0,
        },
        VendorProgress {
            owner: "0000000000000001".into(),
            position: 0,
            vendor: 20,
            points: 0,
            rewards: 0,
        },
    ] {
        let mut state = doc.activity_state().clone();
        state.vendors.push(bad);
        assert!(doc.set_activity_state(state).is_err());
        assert_eq!(doc, original);
    }
    assert!(doc.set_vendor_campaigns(0, 8).is_err());
    assert_eq!(doc, original);
}

#[test]
fn recovery_checks_bucket_capacity_and_retains_instance_and_roll_state() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE character_items SET postmaster=1 WHERE location=1",
        [],
    )
    .unwrap();
    let mut doc = open(&path);
    let before = doc.clone();
    assert!(
        doc.recover_postmaster(0, STORED, 1, |_| Some(metadata(
            InventoryScope::Character,
            0,
            1,
            1
        )))
        .is_err()
    );
    assert_eq!(doc, before);
    doc.recover_postmaster(0, STORED, 1, |_| {
        Some(metadata(InventoryScope::Character, 0, 2, 1))
    })
    .unwrap();
    assert!(!doc.is_postmaster(STORED));
    assert_eq!(
        doc.characters().characters()[0].inventory[0]
            .instance_soid
            .get(),
        STORED
    );
    save(&mut doc).unwrap();
    assert!(!open(&path).is_postmaster(STORED));
    assert_eq!(
        db.query_row(
            "SELECT mutation_serial FROM character_items WHERE location=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        93
    );
}

#[test]
fn profile_recovery_supports_partial_stacks_and_is_atomic_when_capped() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE character_items SET postmaster=1,quantity=10,definition_hash=3159615086 WHERE location=1",[]).unwrap();
    let mut doc = open(&path);
    let info = metadata(InventoryScope::Profile, 18, 1, 73600);
    doc.recover_postmaster(0, STORED, 5, |_| Some(info))
        .unwrap();
    assert_eq!(doc.profile().profile_items()[0].quantity, 73600);
    assert_eq!(doc.characters().characters()[0].inventory[0].quantity, 5);
    assert!(doc.is_postmaster(STORED));
    let before = doc.clone();
    assert!(
        doc.recover_postmaster(0, STORED, 1, |_| Some(info))
            .is_err()
    );
    assert_eq!(doc, before);
    save(&mut doc).unwrap();
    let after = open(&path);
    assert_eq!(after.profile().profile_items()[0].quantity, 73600);
    assert_eq!(after.characters().characters()[0].inventory[0].quantity, 5);
}

#[test]
fn discard_respects_lock_and_never_creates_reward_debts() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE character_items SET postmaster=1,flags=1 WHERE location=1",
        [],
    )
    .unwrap();
    let mut doc = open(&path);
    let before = doc.clone();
    assert!(doc.discard_postmaster(0, STORED).is_err());
    assert_eq!(doc, before);
    db.execute("UPDATE character_items SET flags=0 WHERE location=1", [])
        .unwrap();
    let mut doc = open(&path);
    doc.discard_postmaster(0, STORED).unwrap();
    save(&mut doc).unwrap();
    assert!(
        open(&path).characters().characters()[0]
            .inventory
            .is_empty()
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM reward_debts", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
}

fn definition() -> crate::catalog::ItemDef {
    serde_json::from_value(serde_json::json!({"hash":4070132608_u64,"name":"Test","type_name":"Helmet","bucket_hash":0,"class_type":0,"default_plugs":[],"sockets":[{"socket_type":1,"sources":[{"kind":{"source":"randomized_set","index":0},"pool":0,"valid":true,"ordered_members":[3961599962_u64,10,11]}]}]})).unwrap()
}

#[test]
fn saved_roll_roundtrip_preserves_native_row_order_and_little_endian_masks() {
    let (_temp, path) = fixture();
    let mut doc = open(&path);
    let roll = SavedRoll {
        entropy: [1, 2, 3, 4, 5, 6, 7, 8],
        lanes: 1,
        owned: [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    doc.set_saved_roll(0, EQUIPPED, roll.clone(), &definition())
        .unwrap();
    save(&mut doc).unwrap();
    assert_eq!(open(&path).saved_roll(EQUIPPED).unwrap(), roll);
    let db = Connection::open(&path).unwrap();
    let bytes = db
        .query_row(
            "SELECT owned_rows FROM item_rolls WHERE instance_soid=?1",
            params![format!("{EQUIPPED:016X}")],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .unwrap();
    assert_eq!(&bytes[..8], &5_u64.to_le_bytes());
    let before = doc.clone();
    let mut invalid = roll;
    invalid.owned[0] = 8;
    assert!(
        doc.set_saved_roll(0, EQUIPPED, invalid, &definition())
            .is_err()
    );
    assert_eq!(doc, before);
    doc.set_saved_roll(0, EQUIPPED, SavedRoll::default(), &definition())
        .unwrap();
    save(&mut doc).unwrap();
    assert_eq!(
        open(&path).saved_roll(EQUIPPED).unwrap(),
        SavedRoll::default()
    );
}

#[test]
fn saved_roll_rejects_unowned_lanes_and_native_default_sockets() {
    let (_temp, path) = fixture();
    let mut doc = open(&path);
    let original = doc.clone();
    for invalid in [
        SavedRoll {
            lanes: 4096,
            ..Default::default()
        },
        SavedRoll {
            lanes: 4,
            ..Default::default()
        },
        SavedRoll {
            owned: [1; 12],
            ..Default::default()
        },
    ] {
        assert!(
            doc.set_saved_roll(0, EQUIPPED, invalid, &definition())
                .is_err()
        );
        assert_eq!(doc, original);
    }
    let mut def = definition();
    def.hash = 2715114534;
    assert!(
        doc.set_saved_roll(
            0,
            STORED,
            SavedRoll {
                lanes: 1,
                ..Default::default()
            },
            &def
        )
        .is_err()
    );
}

#[test]
fn recovery_and_roll_save_refuse_unversioned_bookkeeping_changes() {
    let (_temp, path) = fixture();
    let mut doc = open(&path);
    doc.set_saved_roll(
        0,
        EQUIPPED,
        SavedRoll {
            entropy: [3; 8],
            ..Default::default()
        },
        &definition(),
    )
    .unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute("UPDATE characters SET vendor_campaigns=2", [])
        .unwrap();
    assert!(save(&mut doc).unwrap_err().to_string().contains("Reload"));
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM item_rolls", [], |r| r
            .get::<_, i32>(0))
            .unwrap(),
        0
    );
}

#[test]
fn recovering_a_new_profile_stack_preserves_zero_identity_and_allocates_global_serial() {
    let (_temp, path) = fixture();
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE character_items SET postmaster=1,quantity=10,definition_hash=999 WHERE location=1",
        [],
    )
    .unwrap();
    let mut doc = open(&path);
    let before_recovery = doc.clone();
    doc.recover_postmaster(0, STORED, 10, |hash| {
        Some(metadata(
            InventoryScope::Profile,
            if hash == 999 { 19 } else { 18 },
            1,
            100000,
        ))
    })
    .unwrap();
    assert!(doc.characters().characters()[0].inventory.is_empty());
    let allocator = doc.next_profile_item_soid();
    assert_eq!(doc.profile().profile_items()[1].instance_soid, None);
    save(&mut doc).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT mutation_serial FROM profile_items WHERE definition_hash=999",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        5
    );
    assert_eq!(open(&path).profile().profile_items()[1].instance_soid, None);
    assert_eq!(doc.next_profile_item_soid(), allocator);
    let mut undone = before_recovery;
    undone.adopt_revision(&doc);
    save(&mut undone).unwrap();
    assert_eq!(undone.next_profile_item_soid(), allocator);
}
