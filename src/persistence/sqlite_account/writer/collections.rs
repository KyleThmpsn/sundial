//! Project validated account collections into native rows, preserving opaque columns.
use rusqlite::{Transaction, params};
use sundial_account::{
    CharacterAbilities, DismantleGearClass, DismantleRarity, ItemInstance, ItemPlugs,
};

use super::{NativeRow, insert, matching, put};
use crate::persistence::sqlite_account::{
    SqliteAccountDocument, SqliteAccountError,
    contract::{EQUIPMENT_LOCATION, EQUIPMENT_SLOTS, INVENTORY_LOCATION},
    package, settings,
};

pub(super) fn write_document(
    transaction: &Transaction<'_>,
    document: &SqliteAccountDocument,
) -> Result<(), SqliteAccountError> {
    let old_rewards = document.preserved_rows("dismantle_rewards");
    let old_profile = document.preserved_rows("profile_items");
    let old_items = document.preserved_rows("items");
    let old_sockets = document.preserved_rows("sockets");
    transaction.execute_batch("DELETE FROM sockets; DELETE FROM items; DELETE FROM profile_items; DELETE FROM dismantle_rewards;")
        .map_err(|error| SqliteAccountError::sqlite("replace account collections in", error))?;
    for (position, reward) in document.profile().dismantle_rewards().iter().enumerate() {
        let mut row = document
            .original_reward_position(reward.id)
            .map(|p| matching(old_rewards, &[("position", p as i64)]))
            .unwrap_or_default();
        put(&mut row, "position", sql_count(position)?);
        put(
            &mut row,
            "definition_hash",
            i64::from(reward.definition_hash.get()),
        );
        put(&mut row, "quantity", reward.quantity);
        put(
            &mut row,
            "tier_mask",
            i64::from(
                reward
                    .rarities
                    .iter()
                    .fold(0_u8, |mask, rarity| mask | rarity_bit(*rarity)),
            ),
        );
        put(
            &mut row,
            "class_mask",
            match reward.gear_class {
                None => 0,
                Some(DismantleGearClass::Weapon) => 1,
                Some(DismantleGearClass::Armor) => 2,
                Some(DismantleGearClass::Both) => 3,
            },
        );
        put(
            &mut row,
            "masterwork",
            match reward.masterworked {
                None => 0,
                Some(true) => 1,
                Some(false) => 2,
            },
        );
        insert(transaction, "dismantle_rewards", row)?;
    }
    for (position, item) in document.profile().profile_items().iter().enumerate() {
        let (soid, serial) = document.profile_persistence(item.id)?;
        let mut row = document
            .original_profile_position(item.id)
            .map(|p| matching(old_profile, &[("position", p as i64)]))
            .unwrap_or_default();
        row.entry("seen".into())
            .or_insert(package::Cell::Integer(1));
        if let Some(seen) = document.profile_seen_override(item.id) {
            put(&mut row, "seen", i64::from(seen));
        }
        put(&mut row, "position", sql_count(position)?);
        put(&mut row, "instance_soid", sql_u64(soid));
        put(
            &mut row,
            "definition_hash",
            i64::from(item.definition_hash.get()),
        );
        put(&mut row, "quantity", item.quantity);
        put(&mut row, "mutation_serial", serial);
        insert(transaction, "profile_items", row)?;
    }
    for character in document.characters().characters() {
        let metadata = character
            .metadata
            .ok_or_else(|| SqliteAccountError::invalid_data("characters", "missing metadata"))?;
        let soid = character
            .soid
            .ok_or_else(|| SqliteAccountError::invalid_data("characters", "missing SOID"))?;
        let slot: usize = transaction
            .query_row(
                "SELECT slot FROM characters WHERE soid=?",
                [sql_u64(soid.get())],
                |row| row.get(0),
            )
            .map_err(|error| SqliteAccountError::sqlite("identify character in", error))?;
        let serial = document.next_inventory_serial(character.id)?;
        transaction.execute("UPDATE characters SET race=?, gender=?, class=?, next_inventory_serial=? WHERE slot=?", params![metadata.race, metadata.gender, metadata.class_type, serial, slot])
            .map_err(|error| SqliteAccountError::sqlite("write characters to", error))?;
        let context = ItemWriter {
            transaction,
            document,
            old_items,
            old_sockets,
            character_slot: slot,
        };
        for (position, name) in EQUIPMENT_SLOTS.into_iter().enumerate() {
            if let Some(item) = character
                .equipment
                .get(&sundial_account::EquipmentSlot::new(name))
                .and_then(Option::as_ref)
            {
                let abilities = if name == "subclass" {
                    metadata.abilities
                } else {
                    document.item_persistence(item.id)?.1
                };
                context.write(EQUIPMENT_LOCATION, position, item, abilities)?;
            }
        }
        for (position, item) in character.inventory.iter().enumerate() {
            context.write(
                INVENTORY_LOCATION,
                position,
                item,
                document.item_persistence(item.id)?.1,
            )?;
        }
    }
    document.save_inventory_state(transaction)?;
    document.save_progression(transaction)?;
    document.save_entitlements(transaction)?;
    document.save_runtime(transaction)?;
    settings::save(transaction, document.settings())
}

struct ItemWriter<'a, 'connection> {
    transaction: &'a Transaction<'connection>,
    document: &'a SqliteAccountDocument,
    old_items: &'a [NativeRow],
    old_sockets: &'a [NativeRow],
    character_slot: usize,
}
impl ItemWriter<'_, '_> {
    fn write(
        &self,
        location: i64,
        position: usize,
        item: &ItemInstance,
        abilities: CharacterAbilities,
    ) -> Result<(), SqliteAccountError> {
        let soid = sql_u64(item.instance_soid.get());
        let mut row = matching(self.old_items, &[("instance_soid", soid)]);
        row.entry("seen".into())
            .or_insert(package::Cell::Integer(0));
        let (policy, plugs): (i64, &[Option<sundial_account::DefinitionHash>]) = match &item.plugs {
            ItemPlugs::NativeDefaults => (0, &[]),
            ItemPlugs::Authored(plugs) => (1, plugs),
        };
        for (name, value) in [
            ("character_slot", sql_count(self.character_slot)?),
            ("location", location),
            ("position", sql_count(position)?),
            ("instance_soid", soid),
            ("definition_hash", i64::from(item.definition_hash.get())),
            ("level", i64::from(item.level)),
            ("quantity", i64::from(item.quantity)),
            (
                "mutation_serial",
                i64::from(self.document.item_persistence(item.id)?.0),
            ),
            ("flags", i64::from(item.flags.unwrap_or(0))),
            ("socket_policy", policy),
            ("plug_count", sql_count(plugs.len())?),
            ("movement_ability", i64::from(abilities.movement)),
            ("grenade_ability", i64::from(abilities.grenade)),
            ("super_ability", i64::from(abilities.super_ability)),
            ("melee_ability", i64::from(abilities.melee)),
            ("class_ability", i64::from(abilities.class_ability)),
        ] {
            put(&mut row, name, value);
        }
        insert(self.transaction, "items", row)?;
        for (lane, plug) in plugs.iter().enumerate() {
            if let Some(hash) = plug {
                let mut row = matching(
                    self.old_sockets,
                    &[("instance_soid", soid), ("lane", sql_count(lane)?)],
                );
                put(&mut row, "instance_soid", soid);
                put(&mut row, "lane", sql_count(lane)?);
                put(&mut row, "plug_hash", i64::from(hash.get()));
                insert(self.transaction, "sockets", row)?;
            }
        }
        Ok(())
    }
}

const fn rarity_bit(rarity: DismantleRarity) -> u8 {
    match rarity {
        DismantleRarity::Common => 1 << 1,
        DismantleRarity::Uncommon => 1 << 2,
        DismantleRarity::Rare => 1 << 3,
        DismantleRarity::Legendary => 1 << 4,
        DismantleRarity::Exotic => 1 << 5,
    }
}

fn sql_count(value: usize) -> Result<i64, SqliteAccountError> {
    i64::try_from(value).map_err(|_| {
        SqliteAccountError::invalid_data("SQLite position", "value does not fit in signed 64 bits")
    })
}

const fn sql_u64(value: u64) -> i64 {
    value as i64
}
