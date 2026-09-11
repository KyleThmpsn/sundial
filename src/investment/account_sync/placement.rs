//! Shared capacity planning for native weapon-slot replacement in both account backends.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredSlotChange {
    pub definition_hash: u32,
    /// Native weapon buckets: Kinetic 0, Energy 1, Power 2.
    pub previous_bucket: u8,
    pub incoming_bucket: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredSlotReplacement {
    pub changes: Vec<AuthoredSlotChange>,
    /// All definitions in the incoming generation, including nonweapon inventory buckets.
    pub incoming_buckets: BTreeMap<u32, u8>,
    /// Native capacities include both equipped and stored copies.
    pub weapon_capacities: [usize; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoredMoveOutcome {
    MovedToInventory,
    DeletedInventoryFull,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredItemMove {
    pub character_index: usize,
    pub definition_hash: u32,
    pub equipment_slot: String,
    pub incoming_bucket: u8,
    pub outcome: AuthoredMoveOutcome,
}

impl AuthoredItemMove {
    pub fn destination_label(&self) -> &'static str {
        match self.incoming_bucket {
            0 => "Kinetic",
            1 => "Energy",
            2 => "Power",
            _ => "Unknown",
        }
    }
    pub fn source_label(&self) -> &str {
        match self.equipment_slot.as_str() {
            "kinetic" => "Kinetic",
            "energy" => "Energy",
            "heavy" => "Power",
            other => other,
        }
    }
}

pub(crate) struct CharacterItems {
    pub character_index: usize,
    pub inventory: Vec<u32>,
    pub equipment: Vec<(String, u32)>,
}

pub(crate) fn plan(
    characters: &[CharacterItems],
    removed: &BTreeSet<u32>,
    replacement: &AuthoredSlotReplacement,
) -> Result<Vec<AuthoredItemMove>, String> {
    let changes = validate(replacement, removed)?;
    if changes.is_empty() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for character in characters {
        if character
            .inventory
            .iter()
            .any(|hash| changes.contains_key(hash))
            || character
                .equipment
                .iter()
                .any(|(_, hash)| changes.contains_key(hash))
        {
            result.extend(plan_character(character, replacement, &changes)?);
        }
    }
    Ok(result)
}

fn validate<'a>(
    replacement: &'a AuthoredSlotReplacement,
    removed: &BTreeSet<u32>,
) -> Result<BTreeMap<u32, &'a AuthoredSlotChange>, String> {
    let mut changes = BTreeMap::new();
    for change in &replacement.changes {
        if change.previous_bucket > 2
            || change.incoming_bucket > 2
            || change.previous_bucket == change.incoming_bucket
            || removed.contains(&change.definition_hash)
            || replacement.incoming_buckets.get(&change.definition_hash)
                != Some(&change.incoming_bucket)
            || changes.insert(change.definition_hash, change).is_some()
        {
            return Err("Conflicting or unsupported replacement weapon slots".into());
        }
    }
    if replacement
        .weapon_capacities
        .iter()
        .any(|capacity| *capacity == 0 || *capacity > 350)
    {
        return Err("The incoming weapon inventory capacities are invalid".into());
    }
    Ok(changes)
}

fn plan_character(
    character: &CharacterItems,
    replacement: &AuthoredSlotReplacement,
    changes: &BTreeMap<u32, &AuthoredSlotChange>,
) -> Result<Vec<AuthoredItemMove>, String> {
    let bucket = |hash: u32| {
        replacement.incoming_buckets.get(&hash).copied()
        .ok_or_else(|| format!("Cannot verify inventory capacity for item 0x{hash:08X}. Its incoming definition is missing"))
    };
    let mut result = vec![];
    let mut counts = [0usize; 3];
    let mut inventory_count = character.inventory.len();
    let mut candidates = vec![];
    for hash in &character.inventory {
        let native = bucket(*hash)?;
        if native < 3 {
            counts[usize::from(native)] += 1;
        }
    }
    for (slot, hash) in &character.equipment {
        let native = bucket(*hash)?;
        let destination = ["kinetic", "energy", "heavy"].get(usize::from(native));
        if changes.contains_key(hash) && destination.is_some_and(|target| *target != slot) {
            candidates.push((slot, *hash, native));
        } else if native < 3 {
            counts[usize::from(native)] += 1;
        }
    }
    // Unequipped copies also change bucket. Do not install a loadout that cannot be placed,
    // or delete stored/unrelated copies under consent limited to affected equipment.
    for hash in &character.inventory {
        if let Some(change) = changes.get(hash) {
            let native = usize::from(change.incoming_bucket);
            if counts[native] > replacement.weapon_capacities[native] {
                return Err(format!(
                    "Character {} already has too many stored weapons for the incoming slot of item 0x{hash:08X}. Free space in that inventory bucket before installing",
                    character.character_index + 1
                ));
            }
        }
    }
    for (slot, hash, native) in candidates {
        let count = &mut counts[usize::from(native)];
        let fits = inventory_count < crate::account_contract::CHARACTER_INVENTORY_CAPACITY
            && *count < replacement.weapon_capacities[usize::from(native)];
        let outcome = if fits {
            inventory_count += 1;
            *count += 1;
            AuthoredMoveOutcome::MovedToInventory
        } else {
            AuthoredMoveOutcome::DeletedInventoryFull
        };
        result.push(AuthoredItemMove {
            character_index: character.character_index,
            definition_hash: hash,
            equipment_slot: slot.clone(),
            incoming_bucket: native,
            outcome,
        });
    }
    Ok(result)
}
