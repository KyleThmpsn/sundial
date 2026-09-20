use super::*;
use crate::{catalog::ItemStackability, persistence::dawn_account as dawn};
use rusqlite::Connection;
use sundial_account::EquipmentSlot;

const OLD_HELMET: u64 = 4070132608;
const OLD_WEAPON: u64 = 2715114534;
const FIRST_NEW: u64 = 0x4000_0000_0000_02AB;

fn fixture() -> (tempfile::TempDir, account::WorkspaceDocument) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("player-state.db");
    dawn::tests::create_fixture(&path);
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "INSERT INTO item_rolls VALUES('4000000000000004',X'0102030405060708',0,zeroblob(96));
        INSERT INTO item_rolls VALUES('400000000000001A',X'0807060504030201',0,zeroblob(96));",
    )
    .unwrap();
    let doc = open(dir.path());
    (dir, doc)
}

fn open(root: &std::path::Path) -> account::WorkspaceDocument {
    let doc = account::WorkspaceDocument::load(
        serde_json::json!({"version":6}),
        &root.join("settings.json"),
        true,
    );
    assert!(doc.dawn_account().is_some());
    doc
}

fn catalog() -> Catalog {
    let definitions = [
        (OLD_HELMET, "Old Helmet", "helmet", 3),
        (OLD_WEAPON, "Old Weapon", "kinetic", 0),
        (100, "New Helmet", "helmet", 3),
        (200, "New Weapon", "kinetic", 0),
        (400, "Ghost", "ghost", 8),
        (401, "Artifact", "artifact", 49),
    ];
    let mut items = Vec::new();
    let mut metadata = HashMap::new();
    for (hash, name, slot, bucket) in definitions {
        items.push(ItemDef {
            hash,
            name: name.into(),
            type_name: name.into(),
            bucket_hash: slot_definition(slot).unwrap().2,
            // Retained fixtures are valid inventory but not generation candidates for this Hunter.
            class_type: if hash == OLD_HELMET || hash == OLD_WEAPON {
                0
            } else {
                3
            },
            default_plugs: vec![],
            sockets: vec![],
            abilities: Default::default(),
        });
        metadata.insert(
            hash,
            InventoryMetadata {
                scope: InventoryScope::Character,
                native_bucket_id: bucket,
                stackability: ItemStackability::Instanced,
                max_stack_size: Some(1),
                bucket_capacity: Some(10),
            },
        );
    }
    // Filler rows represent other character buckets, not candidate weapon/armor definitions.
    metadata.insert(
        300,
        InventoryMetadata {
            scope: InventoryScope::Character,
            native_bucket_id: 40,
            stackability: ItemStackability::Instanced,
            max_stack_size: Some(1),
            bucket_capacity: Some(334),
        },
    );
    let emotes = emote_catalog();
    let collection = crate::account_contract::EMOTE_COLLECTION_DEFINITION_HASH;
    items.push(emotes.item(collection).unwrap().clone());
    metadata.insert(collection, *emotes.inventory_metadata(collection).unwrap());
    Catalog::for_test_with_inventory(items, HashMap::new(), metadata)
}

fn helmet(doc: &account::WorkspaceDocument) -> &sundial_account::ItemInstance {
    doc.dawn_account().unwrap().characters().characters()[0].equipment
        [&EquipmentSlot::new("helmet")]
        .as_ref()
        .unwrap()
}

fn randomize(
    doc: &mut account::WorkspaceDocument,
    options: LoadoutOptions,
) -> Result<String, String> {
    randomize_full_loadout(
        doc,
        &catalog(),
        0,
        PlugSelectionMode::Supported,
        false,
        options,
    )
}

#[test]
fn replacing_rolled_equipment_saves_reloads_and_undo_restores_its_roll() {
    let (dir, mut doc) = fixture();
    let mut before = doc.clone();
    let old = helmet(&doc).clone();
    let roll = doc
        .dawn_account()
        .unwrap()
        .saved_roll(old.instance_soid.get())
        .unwrap();
    randomize(
        &mut doc,
        LoadoutOptions {
            armor: true,
            ..Default::default()
        },
    )
    .unwrap();
    let generated = helmet(&doc).clone();
    assert_eq!(generated.definition_hash.get(), 100);
    assert!(generated.instance_soid.get() >= FIRST_NEW);
    assert_ne!(generated.instance_soid, old.instance_soid);
    assert_eq!(
        doc.dawn_account()
            .unwrap()
            .saved_roll(generated.instance_soid.get())
            .unwrap(),
        dawn::SavedRoll::default()
    );
    doc.save_dawn().unwrap();
    let reloaded = open(dir.path());
    assert_eq!(helmet(&reloaded).instance_soid, generated.instance_soid);
    let db = Connection::open(dir.path().join("player-state.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM item_rolls WHERE instance_soid='4000000000000004'",
            [],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    before
        .dawn_account_mut()
        .unwrap()
        .adopt_revision(doc.dawn_account().unwrap());
    before.save_dawn().unwrap();
    let undone = open(dir.path());
    assert_eq!(helmet(&undone).instance_soid, old.instance_soid);
    assert_eq!(
        undone
            .dawn_account()
            .unwrap()
            .saved_roll(old.instance_soid.get())
            .unwrap(),
        roll
    );
    assert_eq!(
        undone
            .dawn_account()
            .unwrap()
            .saved_roll(0x4000_0000_0000_001A)
            .unwrap(),
        doc.dawn_account()
            .unwrap()
            .saved_roll(0x4000_0000_0000_001A)
            .unwrap()
    );
}

#[test]
fn locked_and_postmaster_items_keep_their_identities_and_rolls() {
    let (dir, _) = fixture();
    let db = Connection::open(dir.path().join("player-state.db")).unwrap();
    db.execute_batch(
        "UPDATE character_items SET flags=1 WHERE location=0;
        UPDATE character_items SET postmaster=1 WHERE location=1;",
    )
    .unwrap();
    for position in 1..21 {
        db.execute(
            "INSERT INTO character_items VALUES('9EAA300100100101',1,?1,?2,?3,100,1,?1,0,0,1)",
            rusqlite::params![
                position,
                format!("{:016X}", FIRST_NEW + position as u64),
                OLD_WEAPON
            ],
        )
        .unwrap();
    }
    let mut doc = open(dir.path());
    let old_helmet = helmet(&doc).clone();
    let old_mail = doc.dawn_account().unwrap().characters().characters()[0].inventory[0].clone();
    let old_roll = doc
        .dawn_account()
        .unwrap()
        .saved_roll(old_mail.instance_soid.get())
        .unwrap();
    randomize(
        &mut doc,
        LoadoutOptions {
            weapons: true,
            armor: true,
            replace_held_inventory: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(helmet(&doc), &old_helmet);
    assert!(
        doc.dawn_account().unwrap().characters().characters()[0]
            .inventory
            .contains(&old_mail)
    );
    // A full 21-item Postmaster must not reduce the nine ordinary held kinetic slots.
    assert_eq!(
        account::character_inventory(&doc, 0)
            .unwrap()
            .unwrap()
            .iter()
            .filter(|i| i.definition_hash == 200)
            .count(),
        9
    );
    doc.save_dawn().unwrap();
    let loaded = open(dir.path());
    assert!(
        loaded
            .dawn_account()
            .unwrap()
            .is_postmaster(old_mail.instance_soid.get())
    );
    assert_eq!(
        loaded
            .dawn_account()
            .unwrap()
            .saved_roll(old_mail.instance_soid.get())
            .unwrap(),
        old_roll
    );
}

#[test]
fn generated_items_use_the_allocator_and_do_not_reuse_removed_unsaved_ids() {
    let (_dir, mut doc) = fixture();
    let first =
        account::add_inventory_item(&mut doc, 0, inventory::NewInventoryItem::single(200, 100))
            .unwrap();
    let first_id = account::character_inventory(&doc, 0)
        .unwrap()
        .unwrap()
        .iter()
        .find(|i| i.location == first)
        .unwrap()
        .instance_soid;
    assert_eq!(first_id, FIRST_NEW);
    account::remove_character_inventory_items(&mut doc, 0, [first.item_index]).unwrap();
    let second =
        account::add_inventory_item(&mut doc, 0, inventory::NewInventoryItem::single(200, 100))
            .unwrap();
    let second_id = account::character_inventory(&doc, 0)
        .unwrap()
        .unwrap()
        .iter()
        .find(|i| i.location == second)
        .unwrap()
        .instance_soid;
    assert_eq!(second_id, first_id + 1);
    doc.save_dawn().unwrap();
}

#[test]
fn shared_storage_limit_includes_mail_without_charging_its_weapon_bucket() {
    let (dir, _) = fixture();
    let db = Connection::open(dir.path().join("player-state.db")).unwrap();
    db.execute(
        "UPDATE character_items SET postmaster=1 WHERE location=1",
        [],
    )
    .unwrap();
    for position in 1..333 {
        let hash = if position < 21 { OLD_WEAPON } else { 300 };
        db.execute(
            "INSERT INTO character_items VALUES('9EAA300100100101',1,?1,?2,?3,100,1,?1,0,0,?4)",
            rusqlite::params![
                position,
                format!("{:016X}", FIRST_NEW + position as u64),
                hash,
                i32::from(position < 21)
            ],
        )
        .unwrap();
    }
    let mut doc = open(dir.path());
    assert_eq!(account::character_inventory_capacity(&doc), 334);
    randomize(
        &mut doc,
        LoadoutOptions {
            weapons: true,
            replace_held_inventory: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        account::character_inventory_storage_len(&doc, 0).unwrap(),
        334
    );
    assert_eq!(
        account::character_inventory(&doc, 0)
            .unwrap()
            .unwrap()
            .iter()
            .filter(|i| i.definition_hash == 200)
            .count(),
        1
    );
    assert_eq!(
        doc.dawn_account().unwrap().characters().characters()[0]
            .inventory
            .iter()
            .filter(|i| doc
                .dawn_account()
                .unwrap()
                .is_postmaster(i.instance_soid.get()))
            .count(),
        21
    );
    assert!(
        inventory_add_blocker(
            &doc,
            &catalog(),
            0,
            Some(&Candidate {
                item_hash: 200,
                plugs: vec![]
            })
        )
        .unwrap()
        .contains("full")
    );
    let before = doc.clone();
    assert!(
        add_candidate_to_inventory(
            &mut doc,
            &catalog(),
            0,
            &Candidate {
                item_hash: 200,
                plugs: vec![]
            }
        )
        .is_err()
    );
    assert_eq!(doc, before);
    doc.save_dawn().unwrap();
    assert_eq!(
        account::character_inventory_storage_len(&open(dir.path()), 0).unwrap(),
        334
    );
}

#[test]
fn single_item_discard_does_not_inherit_the_replaced_saved_roll() {
    let (_dir, mut doc) = fixture();
    let old = helmet(&doc).instance_soid;
    // Unknown placement of the old item requires the explicit discard route.
    let source = catalog();
    let catalog = Catalog::for_test_with_inventory(
        vec![source.item(100).unwrap().clone()],
        HashMap::new(),
        [(100, *source.inventory_metadata(100).unwrap())].into(),
    );
    apply_candidate(
        &mut doc,
        &catalog,
        0,
        &Candidate {
            item_hash: 100,
            plugs: vec![],
        },
        true,
    )
    .unwrap();
    assert_ne!(helmet(&doc).instance_soid, old);
    doc.save_dawn().unwrap();
}

#[test]
fn single_item_preservation_moves_the_original_roll_with_the_old_item() {
    let (dir, mut doc) = fixture();
    let old = helmet(&doc).clone();
    let old_roll = doc
        .dawn_account()
        .unwrap()
        .saved_roll(old.instance_soid.get())
        .unwrap();
    apply_candidate(
        &mut doc,
        &catalog(),
        0,
        &Candidate {
            item_hash: 100,
            plugs: vec![],
        },
        false,
    )
    .unwrap();
    assert_ne!(helmet(&doc).instance_soid, old.instance_soid);
    assert!(
        doc.dawn_account().unwrap().characters().characters()[0]
            .inventory
            .contains(&old)
    );
    doc.save_dawn().unwrap();
    assert_eq!(
        open(dir.path())
            .dawn_account()
            .unwrap()
            .saved_roll(old.instance_soid.get())
            .unwrap(),
        old_roll
    );
}

#[test]
fn storage_limits_follow_runtime_not_settings_version() {
    let (dir, dawn) = fixture();
    crate::persistence::sqlite_account::tests::create_fixture(
        &dir.path().join("data/investment.sqlite3"),
        1,
    );
    let sunrise = account::WorkspaceDocument::load(
        serde_json::json!({"version":18}),
        &dir.path().join("settings.json"),
        false,
    );
    assert!(sunrise.native_account().is_some());
    assert_eq!(account::character_inventory_capacity(&sunrise), 135);
    assert_eq!(sunrise.equipment_slots().len(), 17);
    assert_eq!(account::character_inventory_capacity(&dawn), 334);
    assert_eq!(dawn.equipment_slots().len(), 16);
    for version in [8, 12, 16] {
        assert_eq!(
            account::character_inventory_capacity(&emote_document(version)),
            135
        );
    }
    let selected_dawn = account::WorkspaceDocument::load(
        serde_json::json!({"version":18}),
        &dir.path().join("settings.json"),
        true,
    );
    assert_eq!(account::character_inventory_capacity(&selected_dawn), 334);
}

#[test]
fn flair_randomization_excludes_sunrise_only_equipment_on_dawn() {
    let (_dir, mut doc) = fixture();
    randomize(
        &mut doc,
        LoadoutOptions {
            equipment_flair: true,
            ..Default::default()
        },
    )
    .unwrap();
    let slots = &doc.dawn_account().unwrap().characters().characters()[0].equipment;
    assert!(slots.contains_key(&EquipmentSlot::new("ghost")));
    assert!(!slots.contains_key(&EquipmentSlot::new("artifact")));
    doc.save_dawn().unwrap();
    let mut json = emote_document(16);
    randomize(
        &mut json,
        LoadoutOptions {
            equipment_flair: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        account::equipped_item_snapshots(&json, 0)
            .unwrap()
            .iter()
            .any(|item| item.slot == "artifact")
    );
}

#[test]
fn replacing_held_rolls_twice_creates_fresh_identities_and_can_be_undone_after_save() {
    let (dir, mut doc) = fixture();
    let mut original = doc.clone();
    let old_roll = doc
        .dawn_account()
        .unwrap()
        .saved_roll(0x4000_0000_0000_001A)
        .unwrap();
    let options = LoadoutOptions {
        weapons: true,
        replace_held_inventory: true,
        ..Default::default()
    };
    randomize(&mut doc, options).unwrap();
    let first = account::character_inventory(&doc, 0)
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|i| i.instance_soid)
        .collect::<HashSet<_>>();
    assert_eq!(first.len(), 9);
    randomize(&mut doc, options).unwrap();
    for item in account::character_inventory(&doc, 0).unwrap().unwrap() {
        assert!(!first.contains(&item.instance_soid));
        assert_eq!(
            doc.dawn_account()
                .unwrap()
                .saved_roll(item.instance_soid)
                .unwrap(),
            dawn::SavedRoll::default()
        );
    }
    doc.save_dawn().unwrap();
    original
        .dawn_account_mut()
        .unwrap()
        .adopt_revision(doc.dawn_account().unwrap());
    original.save_dawn().unwrap();
    assert_eq!(
        open(dir.path())
            .dawn_account()
            .unwrap()
            .saved_roll(0x4000_0000_0000_001A)
            .unwrap(),
        old_roll
    );
}

#[test]
fn exhausted_dawn_allocator_leaves_the_entire_loadout_unchanged() {
    let (dir, _) = fixture();
    let db = Connection::open(dir.path().join("player-state.db")).unwrap();
    db.execute(
        "UPDATE allocators SET next_value='FFFFFFFFFFFFFFFF' WHERE name='item_instance'",
        [],
    )
    .unwrap();
    let mut doc = open(dir.path());
    let before = doc.clone();
    assert!(
        randomize(
            &mut doc,
            LoadoutOptions {
                armor: true,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("identity")
    );
    assert_eq!(doc, before);
}
