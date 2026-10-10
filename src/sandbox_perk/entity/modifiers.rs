//! Property modifier records: what an attached entity changes on the object it is attached
//! to, one record to a row. A hop-on's Property Modifiers component (class `80803B00`) holds
//! settings records of class `80803B06`: the amount at `+0x28`, the operation at `+0x2C`, 0
//! Add or 1 Multiply, the ability slot at `+0x48`, the input at `+0x4A` and the component
//! interface at `+0x4C`. The client applies Add as `amount * weight` and Multiply as
//! `1 + (factor - 1) * weight`, so Add 0 and Multiply 1 leave the input alone. Black Hole's
//! attachment takes one round from each Barrel burst lane this way, and Crimson's Banned
//! Weapon attachment holds 21 such records over the Weapon Controller, the Magazine and the
//! Barrel.
//!
//! The records reach the runtime graph as native declarations under the component's roots, so
//! an edit is an ordinary field override that the build writes into a private copy of the
//! entity. A record is found by its settings schema and read by field offset, the same way the
//! Perk Workbench shows it, so a record whose fields do not all decode is left out rather than
//! guessed.
use std::collections::BTreeMap;

use crate::runtime::{WeaponRuntimeField, WeaponRuntimeGraph, WeaponRuntimeValue, modifiers};

/// One modifier record and the fields it is read from.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub owner_tag: u32,
    /// Where the record starts in its owner.
    pub owner_offset: u32,
    pub amount: WeaponRuntimeField,
    pub operation: WeaponRuntimeField,
    /// The ability slot, which only the Abilities interface reads.
    pub ability: Option<WeaponRuntimeField>,
    pub input: WeaponRuntimeField,
    pub component: WeaponRuntimeField,
}

impl Modifier {
    /// The stock amount.
    #[must_use]
    pub fn stock(&self) -> f32 {
        match self.amount.value {
            WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(bits),
            _ => f32::NAN,
        }
    }

    #[must_use]
    pub fn operation_byte(&self) -> i64 {
        number(&self.operation.value).unwrap_or(-1)
    }

    #[must_use]
    pub fn input_number(&self) -> i64 {
        number(&self.input.value).unwrap_or(-1)
    }

    #[must_use]
    pub fn component_number(&self) -> i64 {
        number(&self.component.value).unwrap_or(-1)
    }

    #[must_use]
    pub fn adds(&self) -> bool {
        self.operation_byte() == modifiers::OPERATION_ADD
    }

    #[must_use]
    pub fn multiplies(&self) -> bool {
        self.operation_byte() == modifiers::OPERATION_MULTIPLY
    }

    /// The amount that leaves the input as it was: nothing added, or multiplied by one. Any
    /// other operation byte leaves the input alone whatever the amount, which zero records.
    #[must_use]
    pub fn neutral(&self) -> f32 {
        if self.multiplies() { 1.0 } else { 0.0 }
    }

    /// Whether the stock record already leaves its input alone.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.stock() == self.neutral()
    }
}

fn number(value: &WeaponRuntimeValue) -> Option<i64> {
    match value {
        WeaponRuntimeValue::Signed(value) => Some(*value),
        WeaponRuntimeValue::Unsigned(value) => i64::try_from(*value).ok(),
        _ => None,
    }
}

/// Every modifier record of the graph whose amount, operation, input and component all
/// decode, by owner and then by where each starts. A record reached through two roots is
/// read once, from the first.
#[must_use]
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<Modifier> {
    let mut records = BTreeMap::<(u32, u32), [Option<&WeaponRuntimeField>; 5]>::new();
    let roots = graph
        .resources
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(move |root| (resource.owner_tag, root))
        })
        .chain(
            graph
                .owners
                .iter()
                .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
        );
    for (owner, root) in roots {
        for field in &root.fields {
            if field.locator.type_handle.get() != modifiers::SETTINGS_SCHEMA {
                continue;
            }
            let slot = match field.locator.value_offset {
                modifiers::AMOUNT_OFFSET => 0,
                modifiers::OPERATION_OFFSET => 1,
                0x48 => 2,
                modifiers::INPUT_OFFSET => 3,
                modifiers::COMPONENT_OFFSET => 4,
                _ => continue,
            };
            let Some(start) = field.owner_offset.checked_sub(field.locator.value_offset) else {
                continue;
            };
            let record = records.entry((owner, start)).or_default();
            if record[slot].is_none() {
                record[slot] = Some(field);
            }
        }
    }
    records
        .into_iter()
        .filter_map(|((owner_tag, owner_offset), fields)| {
            let [
                Some(amount),
                Some(operation),
                ability,
                Some(input),
                Some(component),
            ] = fields
            else {
                return None;
            };
            if !matches!(amount.value, WeaponRuntimeValue::Float32Bits(_)) {
                return None;
            }
            for field in [operation, input, component] {
                number(&field.value)?;
            }
            Some(Modifier {
                owner_tag,
                owner_offset,
                amount: amount.clone(),
                operation: operation.clone(),
                ability: ability.cloned(),
                input: input.clone(),
                component: component.clone(),
            })
        })
        .collect()
}

/// The size of one settings record.
pub const RECORD_SIZE: usize = 0x58;

/// Where an owner keeps its modifier records: the array's descriptor, which counts the records
/// and points at their header, and the header, which the records follow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordArray {
    pub descriptor: usize,
    pub header: usize,
    pub count: usize,
}

/// The array `records` are the elements of, in their owner's bytes. The records must be one
/// contiguous array, and exactly one descriptor must count them and point at their header.
pub fn record_array(owner: &[u8], records: &[Modifier]) -> Result<RecordArray, String> {
    use crate::package_payload::{i64_at, relative_offset, u32_at, u64_at};
    let mut offsets = records
        .iter()
        .map(|record| record.owner_offset as usize)
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    let &first = offsets.first().ok_or("No modifier records")?;
    let count = offsets.len();
    if offsets
        .iter()
        .enumerate()
        .any(|(index, offset)| *offset != first + index * RECORD_SIZE)
    {
        return Err("The modifier records are not one array".into());
    }
    let header = first
        .checked_sub(16)
        .ok_or("The modifier records have no array header")?;
    if u64_at(owner, header)? != count as u64
        || u32_at(owner, header + 8)? != modifiers::SETTINGS_SCHEMA
    {
        return Err("The modifier records' array header does not count them".into());
    }
    let mut found = None;
    for descriptor in (0..owner.len().saturating_sub(16)).step_by(8) {
        if u64_at(owner, descriptor)? != count as u64 {
            continue;
        }
        if relative_offset(descriptor, 8, i64_at(owner, descriptor + 8)?).ok() != Some(header) {
            continue;
        }
        if found.replace(descriptor).is_some() {
            return Err("More than one descriptor points at the modifier records".into());
        }
    }
    let descriptor = found.ok_or("No descriptor points at the modifier records")?;
    Ok(RecordArray {
        descriptor,
        header,
        count,
    })
}

/// The bytes of one added row: `template`, a stock record's bytes, with the row's component,
/// ability, input, operation and amount in place of the template's.
pub fn row_bytes(
    template: &[u8],
    row: &crate::sandbox_perk::program::ModifierRow,
) -> Result<Vec<u8>, String> {
    if template.len() != RECORD_SIZE {
        return Err(format!(
            "A modifier record is {RECORD_SIZE} bytes, not {}",
            template.len()
        ));
    }
    let mut bytes = template.to_vec();
    let amount = modifiers::AMOUNT_OFFSET as usize;
    bytes[amount..amount + 4].copy_from_slice(&row.amount_bits.to_le_bytes());
    bytes[modifiers::OPERATION_OFFSET as usize] = row.operation;
    bytes[0x48..0x4A].copy_from_slice(&row.ability.to_le_bytes());
    let input = modifiers::INPUT_OFFSET as usize;
    bytes[input..input + 2].copy_from_slice(&row.input.to_le_bytes());
    bytes[modifiers::COMPONENT_OFFSET as usize] = row.component;
    Ok(bytes)
}
