//! Schema 7 keeps ability picks and item values per item, and a title and customization block
//! per character.
//!
//! Ways a save can break a schema 7 account, each visible in the database afterwards:
//! - a rewritten character row falls back to the column defaults for its title or customization
//! - a rewritten item row falls back to the default ability entries
//! - `item_values` cascades away with its item and is not put back
//! - a row in `world_reward_outbox` or `recent_collectibles` changes
//! - an ability edit reaches the character row but not the equipped subclass, which Dawn reads
//! - the loaded abilities come from a stale character row instead of the equipped subclass
//! - a schema 5 layout that reports version 7 is read as if it held the new columns
use super::*;
use crate::persistence::dawn_account::tests::create_fixture_v7;
use sundial_account::{CharacterAbilities, CharacterCommand, CharacterMetadataUpdate};

fn fixture_v7() -> (tempfile::TempDir, DawnAccountDocument) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture_v7(&path);
    let document = loaded(&path);
    (directory, document)
}

fn item_abilities(db: &Connection, soid: &str) -> [i64; 5] {
    db.query_row(
        "SELECT movement_ability,grenade_ability,super_ability,melee_ability,class_ability \
         FROM character_items WHERE instance_soid=?1",
        [soid],
        |row| {
            Ok([
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ])
        },
    )
    .unwrap()
}

#[test]
fn a_schema_seven_save_changes_only_what_was_edited() {
    let (_directory, mut document) = fixture_v7();
    let db = Connection::open(&document.path).unwrap();
    let edited = ["settings_values", "key_bindings", "metadata"];
    let mut before = snapshot::snapshot(&db).unwrap();
    edit_settings(&mut document);
    save(&mut document).unwrap();
    let mut after = snapshot::snapshot(&db).unwrap();
    for table in edited {
        assert_ne!(before.tables.remove(table), None, "{table}");
        after.tables.remove(table);
    }
    assert_eq!(after, before);
    // The document a save leaves behind must be able to save again.
    add_profile_item(&mut document);
    save(&mut document).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM item_values", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn an_ability_edit_lands_on_the_equipped_subclass_alone() {
    let (_directory, document) = fixture_v7();
    let db = Connection::open(&document.path).unwrap();
    // Dawn updates the subclass item when the player picks in game and leaves the character row
    // behind, so the row is stale here on purpose.
    db.execute(
        "UPDATE characters SET movement_ability=6,grenade_ability=7,super_ability=10,\
         melee_ability=15,class_ability=2",
        [],
    )
    .unwrap();
    let mut document = loaded(&document.path);
    let character = &document.characters().characters()[0];
    assert_eq!(
        character.metadata.unwrap().abilities,
        CharacterAbilities {
            movement: 5,
            grenade: 8,
            super_ability: 12,
            melee: 14,
            class_ability: 3,
        }
    );

    let character_id = character.id;
    document
        .characters_mut()
        .apply(
            DawnAccountDocument::character_capabilities(),
            CharacterCommand::UpdateMetadata {
                character_id,
                update: CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 4,
                    grenade: 9,
                    super_ability: 13,
                    melee: 17,
                    class_ability: 1,
                }),
            },
        )
        .unwrap();
    save(&mut document).unwrap();
    assert_eq!(item_abilities(&db, "4000000000000030"), [4, 9, 13, 17, 1]);
    assert_eq!(item_abilities(&db, "400000000000001A"), [6, 9, 13, 16, 1]);
    assert_eq!(item_abilities(&db, "4000000000000004"), [4, 7, 10, 11, 2]);
}

#[test]
fn an_ability_entry_dawn_cannot_store_is_refused_before_writing() {
    let (_directory, mut document) = fixture_v7();
    let db = Connection::open(&document.path).unwrap();
    let character_id = document.characters().characters()[0].id;
    let loaded_abilities = document.characters().characters()[0]
        .metadata
        .unwrap()
        .abilities;
    let before = snapshot::capture(&db).unwrap();
    // The account model refuses the entry, so the document never holds it and a later save
    // writes the abilities that were loaded.
    document
        .characters_mut()
        .apply(
            DawnAccountDocument::character_capabilities(),
            CharacterCommand::UpdateMetadata {
                character_id,
                update: CharacterMetadataUpdate::SetAbilities(CharacterAbilities {
                    movement: 64,
                    grenade: 8,
                    super_ability: 12,
                    melee: 14,
                    class_ability: 3,
                }),
            },
        )
        .unwrap_err();
    assert_eq!(
        document.characters().characters()[0]
            .metadata
            .unwrap()
            .abilities,
        loaded_abilities
    );
    assert_eq!(snapshot::capture(&db).unwrap(), before);
}

#[test]
fn a_schema_five_layout_reporting_seven_is_not_read() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("player-state.db");
    create_fixture(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version=7")
        .unwrap();
    assert!(matches!(
        load(&path).unwrap(),
        DawnAccountDocumentLoad::Incompatible(_)
    ));
}
