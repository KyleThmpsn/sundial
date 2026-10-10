//! Ability properties a program defines for itself, and their inputs.
use super::*;

/// A property a program defines for itself. The build gives every bank of `slot` that lists
/// `parameter` a row under `key` that sets the parameter to `value`, added to the running
/// value or written over it, so an Ability Property action applying `key` on that slot works
/// on every Subclass. The key is a hash of the tuning (`ability::bank::tuning_key`), so equal
/// tunings on any perk share one row. It is 32 bits, so different tunings can share a key, and
/// the program and the build refuse that rather than let one take the other's row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityTuning {
    #[serde(with = "super::hex_key")]
    pub key: u32,
    pub slot: AbilityTarget,
    #[serde(with = "super::hex_key")]
    pub parameter: u32,
    #[serde(rename = "value", with = "super::f32_bits")]
    pub value_bits: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub add: bool,
}

/// A private adjustment to a native base ability input. The property lifetime controls
/// its weight, so removing or holstering the item restores the unmodified ability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityInput {
    #[serde(with = "super::hex_key")]
    pub key: u32,
    pub slot: AbilityTarget,
    pub input: u8,
    #[serde(rename = "value", with = "super::f32_bits")]
    pub value_bits: u32,
    pub multiply: bool,
}

impl AbilityInput {
    #[must_use]
    pub fn new(slot: AbilityTarget, input: u8, value: f32, multiply: bool) -> Self {
        let value_bits = value.to_bits();
        let key = crate::hash::fnv1_name_hash(&format!(
            "parhelion.ability.input.{slot}.{input}.{value_bits:08x}.{}",
            u8::from(multiply)
        ));
        Self {
            key,
            slot,
            input,
            value_bits,
            multiply,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let value = f32::from_bits(self.value_bits);
        if self.input >= 7
            || !value.is_finite()
            || (self.multiply && value < 0.0)
            || crate::ability::bank::slot_banks(self.slot).is_empty()
            || self.key != Self::new(self.slot, self.input, value, self.multiply).key
        {
            return Err("Invalid private base ability input adjustment".into());
        }
        Ok(())
    }
}

impl AbilityTuning {
    /// A tuning with the key its definition hashes to.
    #[must_use]
    pub fn new(slot: AbilityTarget, parameter: u32, value: f32, add: bool) -> Self {
        Self {
            key: crate::ability::bank::tuning_key(slot, parameter, value.to_bits(), add),
            slot,
            parameter,
            value_bits: value.to_bits(),
            add,
        }
    }

    /// The value the row writes or adds.
    #[must_use]
    pub fn value(&self) -> f32 {
        f32::from_bits(self.value_bits)
    }

    /// Whether the key is the one this tuning's definition hashes to.
    #[must_use]
    pub fn keyed_by_definition(&self) -> bool {
        self.key
            == crate::ability::bank::tuning_key(
                self.slot,
                self.parameter,
                self.value_bits,
                self.add,
            )
    }
}

/// Effect kind 7, Ability Property, whose record holds the slot at +2 and the key at +4.
pub const ABILITY_PROPERTY_KIND: u8 = 7;
pub(super) const ABILITY_PROPERTY_CLASS: u32 = 0x8080_3E1D;

/// The slot and key of every Ability Property action in a compiled action payload. An empty
/// payload, a declaration with no action of its own, applies nothing.
pub fn ability_properties_in(payload: &[u8]) -> Result<BTreeSet<(AbilityTarget, u32)>, String> {
    if payload.is_empty() {
        return Ok(BTreeSet::new());
    }
    let decoded = crate::sandbox_perk::action::decode(payload)?;
    Ok(decoded
        .effects()
        .filter(|effect| effect.kind == ABILITY_PROPERTY_KIND)
        .filter_map(|effect| ability_property_of(&effect.native))
        .collect())
}

pub(super) fn ability_property_of(record: &[u8]) -> Option<(AbilityTarget, u32)> {
    let slot = *record.get(2)?;
    let key = u32::from_le_bytes(record.get(4..8)?.try_into().ok()?);
    Some((AbilityTarget::from_byte(slot), key))
}
