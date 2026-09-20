//! Named, checked access to Dawn's little-endian per-instance roll state.
use super::{DawnAccountDocument, carried::ItemRoll};
use crate::catalog::ItemDef;
use sundial_account::ItemPlugs;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SavedRoll {
    pub entropy: [u8; 8],
    pub lanes: u16,
    pub owned: [u64; 12],
}

impl SavedRoll {
    fn decode(row: &ItemRoll) -> Result<Self, String> {
        let entropy = row
            .entropy
            .as_slice()
            .try_into()
            .map_err(|_| "Roll entropy must contain eight bytes")?;
        let lanes = u16::try_from(row.lane_mask)
            .ok()
            .filter(|mask| *mask < 4096)
            .ok_or("Roll state exceeds twelve socket lanes")?;
        if row.owned_rows.len() != 96 {
            return Err("Roll ownership must contain twelve 64-bit masks".into());
        }
        let mut owned = [0; 12];
        for (index, bytes) in row.owned_rows.chunks_exact(8).enumerate() {
            owned[index] = u64::from_le_bytes(bytes.try_into().unwrap());
        }
        Ok(Self {
            entropy,
            lanes,
            owned,
        })
    }

    fn validate(&self, plugs: &ItemPlugs) -> Result<(), String> {
        if self.lanes >= 4096 {
            return Err("Roll state exceeds twelve socket lanes".into());
        }
        for (lane, owned) in self.owned.iter().enumerate() {
            let rolled = self.lanes & (1 << lane) != 0;
            if rolled && !matches!(plugs, ItemPlugs::Authored(plugs) if lane < plugs.len()) {
                return Err("Rolled lanes require an authored socket at that position".into());
            }
            if !rolled && *owned != 0 {
                return Err("A non-rolled socket cannot own random perk rows".into());
            }
        }
        Ok(())
    }
}

impl DawnAccountDocument {
    pub(crate) fn inventory_bookkeeping_differs(&self, before: &Self) -> bool {
        self.carried.postmaster != before.carried.postmaster
            || self.carried.item_rolls != before.carried.item_rolls
    }

    pub(crate) fn saved_roll(&self, soid: u64) -> Result<SavedRoll, String> {
        self.carried
            .item_rolls
            .iter()
            .find(|r| u64::from_str_radix(&r.instance_soid, 16).ok() == Some(soid))
            .map_or(Ok(SavedRoll::default()), SavedRoll::decode)
    }

    pub(crate) fn set_saved_roll(
        &mut self,
        index: usize,
        soid: u64,
        roll: SavedRoll,
        definition: &ItemDef,
    ) -> Result<(), String> {
        let character = self
            .characters()
            .characters()
            .get(index)
            .ok_or("Choose a character")?;
        let item = character
            .inventory
            .iter()
            .chain(character.equipment.values().flatten())
            .find(|r| r.instance_soid.get() == soid)
            .ok_or("The item is no longer present")?;
        if definition.hash != u64::from(item.definition_hash.get()) {
            return Err("The installed item definition does not match".into());
        }
        roll.validate(&item.plugs)?;
        let old = self.saved_roll(soid)?;
        for lane in 0..12 {
            if roll.owned[lane] == old.owned[lane]
                && (roll.lanes & (1 << lane)) == (old.lanes & (1 << lane))
            {
                continue;
            }
            if roll.lanes & (1 << lane) == 0 {
                continue;
            }
            let choices = definition
                .sockets
                .get(lane)
                .map(|s| s.ordered_randomized_choices())
                .unwrap_or_default();
            if choices.is_empty()
                || choices.len() > 64
                || (choices.len() < 64 && roll.owned[lane] >> choices.len() != 0)
            {
                return Err(
                    "Owned perks must belong to the installed randomized socket pool".into(),
                );
            }
        }
        self.bump_inventory_serial(index, soid)?;
        let key = self
            .carried
            .item_rolls
            .iter()
            .find(|r| u64::from_str_radix(&r.instance_soid, 16).ok() == Some(soid))
            .map(|r| r.instance_soid.clone())
            .unwrap_or_else(|| super::contract::format_soid(soid));
        self.carried.item_rolls.retain(|r| r.instance_soid != key);
        self.carried.item_rolls.push(ItemRoll {
            instance_soid: key,
            entropy: roll.entropy.to_vec(),
            lane_mask: i64::from(roll.lanes),
            owned_rows: roll.owned.iter().flat_map(|m| m.to_le_bytes()).collect(),
        });
        Ok(())
    }

    pub(crate) fn validate_item_state(&self) -> Result<(), String> {
        for character in self.characters().characters() {
            for item in character.equipment.values().flatten() {
                let already_equipped = self
                    .loaded_characters
                    .characters()
                    .iter()
                    .flat_map(|c| c.equipment.values().flatten())
                    .any(|old| old.instance_soid == item.instance_soid);
                if self.is_postmaster(item.instance_soid.get()) && !already_equipped {
                    return Err("Recover Postmaster items before equipping them".into());
                }
            }
            for item in character
                .inventory
                .iter()
                .chain(character.equipment.values().flatten())
            {
                let soid = item.instance_soid.get();
                let old = self
                    .loaded_characters
                    .characters()
                    .iter()
                    .flat_map(|c| c.inventory.iter().chain(c.equipment.values().flatten()))
                    .find(|old| old.instance_soid == item.instance_soid);
                let baseline = self
                    .loaded_carried
                    .item_rolls
                    .iter()
                    .find(|r| u64::from_str_radix(&r.instance_soid, 16).ok() == Some(soid));
                let current = self
                    .carried
                    .item_rolls
                    .iter()
                    .find(|r| u64::from_str_radix(&r.instance_soid, 16).ok() == Some(soid));
                if baseline != current
                    || old.is_none_or(|old| {
                        old.plugs != item.plugs || old.definition_hash != item.definition_hash
                    })
                {
                    let roll = self.saved_roll(soid)?;
                    if old.is_some_and(|old| old.definition_hash != item.definition_hash)
                        && roll != SavedRoll::default()
                    {
                        return Err(
                            "Clear the saved roll before replacing this item's definition".into(),
                        );
                    }
                    roll.validate(&item.plugs)?;
                }
            }
        }
        Ok(())
    }
}
