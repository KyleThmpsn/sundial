//! What a Subclass ability modifier needs beyond its pool record: the bank the ability's script
//! reads, and the keys its own property rows are filed under.
//!
//! An ability entity names its bank as one of its component owners, and exactly one of them is
//! one of the 54 root banks (`bank::SLOT_BANKS` and `bank::UNPLACED_BANKS`), 51 of the 52 stock
//! rows. A row of extra charges is keyed by the count alone and a parameter row by the tuning
//! key of its bank's slot, so equal rows are shared across subclasses and with perks' tunings.
use crate::ability::bank::{
    Modifier, Parameter, SLOT_BANKS, UNPLACED_BANKS, handler_slot, parameters, tuning_key,
};
use crate::package_payload::{native_array_at, u32_at};

/// The entity's component array, and the owner tag each 12-byte row starts with.
const COMPONENTS: usize = 0x10;
const COMPONENT_ROW_SIZE: usize = 0x0C;

/// Whether `tag` is one of the root ability banks.
#[must_use]
pub fn is_bank(tag: u32) -> bool {
    SLOT_BANKS.iter().any(|(_, banks)| banks.contains(&tag)) || UNPLACED_BANKS.contains(&tag)
}

/// The Ability Property slot stock keys place a bank in, when one does.
#[must_use]
pub fn bank_slot(bank: u32) -> Option<super::AbilityTarget> {
    SLOT_BANKS
        .iter()
        .find(|(_, banks)| banks.contains(&bank))
        .map(|(slot, _)| *slot)
}

/// The bank an ability entity's script reads: the one component owner that is a root bank.
pub fn entity_bank(entity: &[u8]) -> Result<Option<u32>, String> {
    let (count, _, first, _) = native_array_at(entity, COMPONENTS)?;
    let mut found = None;
    for component in 0..count {
        let owner = u32_at(entity, first + component * COMPONENT_ROW_SIZE)?;
        if !is_bank(owner) || found == Some(owner) {
            continue;
        }
        if let Some(other) = found {
            return Err(format!(
                "The ability entity names two banks, {other:08X} and {owner:08X}"
            ));
        }
        found = Some(owner);
    }
    Ok(found)
}

/// The script parameters a row can set in a bank, each once as its table first lists it: those
/// it lists whose handler slot its own rows show. A row handed to a guessed slot faults the
/// client, so the rest stay stock. A bank's table can list one parameter several times.
pub fn settable_parameters(bank: &[u8]) -> Result<Vec<Parameter>, String> {
    let mut settable = Vec::<Parameter>::new();
    for parameter in parameters(bank)? {
        if settable.iter().any(|each| each.name == parameter.name) {
            continue;
        }
        let modifier = Modifier::Parameter {
            name: parameter.name,
            applied: parameter.applied,
            add: parameter.add,
        };
        if matches!(handler_slot(bank, modifier), Ok(Some(_))) {
            settable.push(parameter);
        }
    }
    Ok(settable)
}

/// The key a row of `count` extra charges is filed under.
#[must_use]
pub fn charge_key(count: u8) -> u32 {
    crate::hash::fnv1_name_hash(&format!("parhelion.ability.charges.{count}"))
}

/// The key a row setting or raising `parameter` is filed under in `bank`: the tuning key of the
/// bank's slot, so a modifier and a perk's tuning doing the same share one row.
#[must_use]
pub fn parameter_key(bank: u32, parameter: u32, value_bits: u32, add: bool) -> u32 {
    tuning_key(
        bank_slot(bank).unwrap_or(super::AbilityTarget::Unknown(u8::MAX)),
        parameter,
        value_bits,
        add,
    )
}
