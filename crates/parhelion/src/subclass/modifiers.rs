//! What an ability or node changes about the abilities of its subclass while it is selected.
//!
//! The game's own nodes change abilities this way: a record of the entry's pool names a key and
//! the ability row it applies to, Dawn files the key into the bucket of the selected ability of
//! that row, and the ability's bank applies the property row under the key. A modifier is such
//! a record. A stock key needs nothing else. Extra charges and a script parameter's value need a
//! property row of their own in the ability's bank, under a key the build derives from what the
//! row does, so equal rows are shared.
use serde::{Deserialize, Serialize};

use super::{AbilitySlot, Place, layout};

/// Most extra charges one modifier gives.
pub const MOST_CHARGES: u8 = 4;

/// A change to the ability in entry `target` of this subclass, while both are selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityModifier {
    /// The entry whose ability it changes, a slot's entry or an attunement node that holds one.
    pub target: u8,
    pub effect: ModifierEffect,
}

/// What a modifier does to its ability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModifierEffect {
    /// A key of the ability's stock bank, as a stock node applies it.
    Key {
        #[serde(with = "super::hex_hash")]
        key: u32,
    },
    /// More charges.
    Charges { count: u8 },
    /// A script parameter the ability's bank lists, set to a value or raised by it.
    Parameter {
        #[serde(with = "super::hex_hash")]
        parameter: u32,
        #[serde(rename = "value", with = "super::f32_bits")]
        value_bits: u32,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        add: bool,
    },
}

/// A record of the entry's stock pool: the key it applies and the ability row it applies it to.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockModifier {
    #[serde(with = "super::hex_hash")]
    pub key: u32,
    pub row: u8,
}

/// A script parameter of the ability's own bank, set to a value while the ability is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterValue {
    #[serde(with = "super::hex_hash")]
    pub parameter: u32,
    #[serde(rename = "value", with = "super::f32_bits")]
    pub value_bits: u32,
}

impl ParameterValue {
    #[must_use]
    pub fn value(self) -> f32 {
        f32::from_bits(self.value_bits)
    }
}

impl ModifierEffect {
    /// The value of a parameter effect.
    #[must_use]
    pub fn value(self) -> Option<f32> {
        match self {
            Self::Parameter { value_bits, .. } => Some(f32::from_bits(value_bits)),
            Self::Key { .. } | Self::Charges { .. } => None,
        }
    }

    pub(super) fn validate(self) -> Result<(), String> {
        match self {
            Self::Key { key: 0 | u32::MAX } => Err("A modifier names no key".to_owned()),
            Self::Charges { count } if count == 0 || count > MOST_CHARGES => Err(format!(
                "A modifier gives 1 to {MOST_CHARGES} charges, not {count}"
            )),
            Self::Parameter { value_bits, .. } if !f32::from_bits(value_bits).is_finite() => {
                Err("A modifier's parameter value is not a number".to_owned())
            }
            _ => Ok(()),
        }
    }
}

/// The list entry a place fills, which a modifier names its target by.
#[must_use]
pub fn place_entry(place: Place) -> u8 {
    match place {
        Place::Ability(entry) => entry,
        Place::Node(path, position) => path.entries()[usize::from(position)],
    }
}

/// Whether list entry `entry` holds an ability, which a modifier can change.
#[must_use]
pub fn holds_ability(entry: u8) -> bool {
    AbilitySlot::of_entry(entry).is_some() || layout::PATH_ABILITIES.contains(&entry)
}

/// The place that fills list entry `entry`, when an ability or node does.
#[must_use]
pub fn entry_place(entry: u8) -> Option<Place> {
    Place::all().find(|place| place_entry(*place) == entry)
}

/// Checks an entry's modifiers, stock removals and parameter values against each other.
pub(super) fn validate(
    context: &str,
    (modifiers, removed, parameters): (&[AbilityModifier], &[StockModifier], &[ParameterValue]),
) -> Result<(), String> {
    for modifier in modifiers {
        if !holds_ability(modifier.target) {
            return Err(format!(
                "{context} changes entry {}, which holds no ability",
                modifier.target
            ));
        }
        modifier
            .effect
            .validate()
            .map_err(|error| format!("{context}: {error}"))?;
    }
    if modifiers
        .iter()
        .enumerate()
        .any(|(index, modifier)| modifiers[..index].contains(modifier))
    {
        return Err(format!("{context} applies one modifier twice"));
    }
    if removed
        .iter()
        .enumerate()
        .any(|(index, stock)| removed[..index].contains(stock))
    {
        return Err(format!("{context} removes one stock modifier twice"));
    }
    for (index, parameter) in parameters.iter().enumerate() {
        if !parameter.value().is_finite() {
            return Err(format!("{context}: a parameter value is not a number"));
        }
        if parameters[..index]
            .iter()
            .any(|other| other.parameter == parameter.parameter)
        {
            return Err(format!("{context} sets one parameter twice"));
        }
    }
    Ok(())
}
