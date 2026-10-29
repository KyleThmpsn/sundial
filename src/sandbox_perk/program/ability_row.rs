//! Bank-scoped properties preserve a modifier's ability restriction and private identity.
use super::*;
use crate::ability::bank::{self, Modifier};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbilityRowModifier {
    /// One logical imported property retains all its ordered rows under one applied key.
    Ordered {
        origin: AbilityRowOrigin,
        rows: Vec<bank::FixedRow>,
    },
    Attack {
        #[serde(with = "super::hex_key")]
        template_key: u32,
        selection: bank::FixedSelection,
        /// Fixed outgoing amounts for the template's two routes, stored as Float32 bits.
        amounts: [u32; 2],
    },
    Group {
        #[serde(with = "super::hex_key")]
        template_key: u32,
        count: u8,
        entries: [bank::GroupEntry; 8],
    },
    Fixed {
        #[serde(with = "super::hex_key")]
        template_key: u32,
        #[serde(with = "super::hex_key")]
        class: u32,
        words: [u32; 8],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selection: Option<bank::FixedSelection>,
    },
    Charges {
        count: u8,
    },
    Scalar {
        input: u8,
        #[serde(with = "super::f32_bits")]
        value: u32,
        multiply: bool,
    },
    Parameter {
        #[serde(with = "super::hex_key")]
        name: u32,
        #[serde(with = "super::f32_bits")]
        reset: u32,
        #[serde(with = "super::f32_bits")]
        value: u32,
        add: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityRowOrigin {
    pub domain: String,
    #[serde(with = "super::hex_key")]
    pub bank: u32,
    #[serde(with = "super::hex_key")]
    pub key: u32,
}

impl AbilityRowOrigin {
    fn valid(&self) -> bool {
        !self.domain.is_empty()
            && self.domain.len() <= 64
            && self
                .domain
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            && ![0, u32::MAX, EMPTY_KEY].contains(&self.bank)
            && ![0, u32::MAX, EMPTY_KEY].contains(&self.key)
    }

    /// Stable logical membership can be named before its producer is selected. The full
    /// ordered definition is compared separately, so two unequal definitions cannot share it.
    pub fn identity(&self, bank: u32, slot: AbilityTarget) -> u32 {
        crate::hash::fnv1_name_hash(&format!(
            "parhelion.ability.ordered.v1.{}.{:08x}.{:08x}.{bank:08x}.{slot}",
            self.domain, self.bank, self.key
        ))
    }

    pub fn property(&self, key: u32) -> Self {
        Self {
            key,
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbilityRow {
    #[serde(with = "super::hex_key")]
    pub key: u32,
    #[serde(with = "super::hex_key")]
    pub bank: u32,
    pub slot: AbilityTarget,
    /// The stored mode copied into the native active-property list.
    pub mode: u8,
    pub modifier: AbilityRowModifier,
}

impl AbilityRow {
    pub fn new(bank: u32, slot: AbilityTarget, mode: u8, modifier: AbilityRowModifier) -> Self {
        let mut row = Self {
            key: 0,
            bank,
            slot,
            mode,
            modifier,
        };
        row.key = row.identity();
        row
    }

    fn identity(&self) -> u32 {
        let body = match self.modifier {
            AbilityRowModifier::Ordered { ref origin, .. } => {
                return origin.identity(self.bank, self.slot);
            }
            AbilityRowModifier::Attack {
                template_key,
                selection,
                amounts,
            } => format!(
                "attack.{template_key:08x}.{:08x}.{}.{:08x}.{:08x}.{:08x}",
                selection.name, selection.priority, selection.required_key, amounts[0], amounts[1]
            ),
            AbilityRowModifier::Group {
                template_key,
                count,
                entries,
            } => format!(
                "group.{template_key:08x}.{count}.{}",
                entries
                    .iter()
                    .map(|v| format!("{:08x}.{}.{:08x}", v.class, v.input, v.value))
                    .collect::<Vec<_>>()
                    .join(".")
            ),
            AbilityRowModifier::Fixed {
                template_key,
                class,
                words,
                selection,
            } => {
                let body = format!(
                    "fixed.{template_key:08x}.{class:08x}.{}",
                    words
                        .iter()
                        .map(|v| format!("{v:08x}"))
                        .collect::<Vec<_>>()
                        .join(".")
                );
                // Existing serialized rows omit selection and retain their original key.
                match selection {
                    Some(s) => format!(
                        "{body}.selection.{:08x}.{}.{:08x}",
                        s.name, s.priority, s.required_key
                    ),
                    None => body,
                }
            }
            AbilityRowModifier::Charges { count } => format!("charges.{count}"),
            AbilityRowModifier::Scalar {
                input,
                value,
                multiply,
            } => {
                format!("scalar.{input}.{value:08x}.{}", u8::from(multiply))
            }
            AbilityRowModifier::Parameter {
                name,
                reset,
                value,
                add,
            } => format!(
                "parameter.{name:08x}.{reset:08x}.{value:08x}.{}",
                u8::from(add)
            ),
        };
        crate::hash::fnv1_name_hash(&format!(
            "parhelion.ability.bank.{:08x}.{}.{}.{}",
            self.bank, self.slot, self.mode, body
        ))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.key != self.identity()
            || !bank::slot_banks(self.slot).contains(&self.bank)
            || !matches!(self.mode, 1 | 255)
        {
            return Err("Private ability row has an invalid identity or bank scope".into());
        }
        let valid = match self.modifier {
            AbilityRowModifier::Ordered {
                ref origin,
                ref rows,
            } => {
                self.slot == AbilityTarget::Jump
                    && self.mode == 255
                    && origin.valid()
                    && ![0, u32::MAX, EMPTY_KEY].contains(&self.key)
                    && !rows.is_empty()
                    && rows.iter().all(|row| {
                        row.validate().is_ok()
                            && (row.selection.required_key == row.witness.selection.required_key
                                || (row.witness.selection.required_key != 0
                                    && row.selection.required_key
                                        == origin
                                            .property(row.witness.selection.required_key)
                                            .identity(self.bank, self.slot)))
                    })
            }
            AbilityRowModifier::Attack {
                template_key,
                selection,
                amounts,
            } => {
                self.slot == AbilityTarget::Movement
                    && self.mode == 255
                    && bank::AttackModifier {
                        template_key,
                        selection,
                        damage: None,
                    }
                    .validate()
                    .is_ok()
                    && amounts
                        .iter()
                        .all(|&v| f32::from_bits(v).is_finite() && f32::from_bits(v) > 0.0)
            }
            AbilityRowModifier::Group {
                template_key,
                count,
                entries,
            } => bank::GroupModifier {
                template_key,
                count,
                entries,
            }
            .validate()
            .is_ok(),
            AbilityRowModifier::Fixed {
                template_key,
                class,
                words,
                selection,
            } => bank::FixedModifier {
                template_key,
                class,
                words,
                selection,
            }
            .validate()
            .is_ok(),
            AbilityRowModifier::Charges { count } => (1..=16).contains(&count),
            AbilityRowModifier::Scalar {
                input,
                value,
                multiply,
            } => {
                input < 7
                    && f32::from_bits(value).is_finite()
                    && (!multiply || f32::from_bits(value) >= 0.0)
            }
            AbilityRowModifier::Parameter {
                name, reset, value, ..
            } => {
                ![0, u32::MAX, EMPTY_KEY].contains(&name)
                    && f32::from_bits(reset).is_finite()
                    && f32::from_bits(value).is_finite()
            }
        };
        if !valid {
            return Err("Private ability row has an invalid value".into());
        }
        Ok(())
    }

    /// Private gate dependencies use the same logical source scope as their future producer.
    /// An absent producer is allowed, but an unrelated definition at that key is a conflict.
    pub fn memberships(&self) -> Vec<(u32, AbilityRowOrigin)> {
        let AbilityRowModifier::Ordered { origin, rows } = &self.modifier else {
            return Vec::new();
        };
        rows.iter()
            .filter(|row| row.selection.required_key != row.witness.selection.required_key)
            .map(|row| {
                (
                    row.selection.required_key,
                    origin.property(row.witness.selection.required_key),
                )
            })
            .collect()
    }

    /// Checks the bank's current handler and reset contract before requesting a mutation.
    pub fn check_bank(&self, payload: &[u8]) -> Result<(), String> {
        self.validate()?;
        if bank::bank_owner(payload)? != self.bank {
            return Err("Private ability row has no compatible bank handler".into());
        }
        if let AbilityRowModifier::Ordered { ref rows, .. } = self.modifier {
            bank::with_ordered_rows(payload, self.key, rows)?;
            return Ok(());
        }
        let value = self.value()?;
        if bank::handler_slot(payload, value)?.is_none() {
            return Err("Private ability row has no compatible bank handler".into());
        }
        if let AbilityRowModifier::Parameter { name, reset, .. } = self.modifier {
            let matching: Vec<_> = bank::parameters(payload)?
                .into_iter()
                .filter(|p| p.name == name)
                .collect();
            if matching.is_empty() || matching.iter().any(|p| p.reset.to_bits() != reset) {
                return Err("Private ability parameter reset differs from its native bank".into());
            }
        }
        if matches!(
            self.modifier,
            AbilityRowModifier::Scalar { .. }
                | AbilityRowModifier::Fixed { .. }
                | AbilityRowModifier::Group { .. }
                | AbilityRowModifier::Attack { .. }
        ) {
            // Exercise the shared writer's structural checks. Numeric rows require
            // flattened inputs and a local provider. Fixed bodies require reciprocal
            // pairs and a matching bank-local handler witness.
            bank::with_property_row_mode(payload, self.key, value, self.mode)?;
        }
        Ok(())
    }

    pub fn value(&self) -> Result<Modifier, String> {
        Ok(match self.modifier {
            AbilityRowModifier::Ordered { .. } => {
                return Err("An ordered ability group has no single modifier value".into());
            }
            AbilityRowModifier::Attack {
                template_key,
                selection,
                ..
            } => Modifier::Attack(bank::AttackModifier {
                template_key,
                selection,
                damage: None,
            }),
            AbilityRowModifier::Group {
                template_key,
                count,
                entries,
            } => Modifier::Group(bank::GroupModifier {
                template_key,
                count,
                entries,
            }),
            AbilityRowModifier::Fixed {
                template_key,
                class,
                words,
                selection,
            } => Modifier::Fixed(bank::FixedModifier {
                template_key,
                class,
                words,
                selection,
            }),
            AbilityRowModifier::Charges { count } => Modifier::Charges(i64::from(count)),
            AbilityRowModifier::Scalar {
                input,
                value,
                multiply,
            } => Modifier::Scalar {
                input,
                value: f32::from_bits(value),
                multiply,
            },
            AbilityRowModifier::Parameter {
                name, value, add, ..
            } => Modifier::Parameter {
                name,
                applied: f32::from_bits(value),
                add,
            },
        })
    }
}
