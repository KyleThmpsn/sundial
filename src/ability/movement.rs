//! Movement controller values whose native consumers are traced: how far a Blink carries and how
//! many jumps a jump ability allows in the air. The runtime graph holds each inside an opaque
//! byte field of the controller's definition, so a value here is a four-byte lane of that field,
//! read and written through the same overrides as any other value.
//!
//! Evidence, build 86657.20.08.23.1800.d2_rc___release (2026-10-05 ability atlas):
//! - Blink controller definition `0x80804326` `+0x634`, 4.5 for Blink and 4.2 for Ionic Blink.
//!   `FB45CD` copies it to runtime `+0xB80`, and `FB2247` multiplies that by a clamped fraction
//!   at `+0xB84` before building the displacement. Native units, not measured meters.
//! - Jump controller definition `0x8080429C` `+0x988`, 1 for the Hunter jumps. `FB47A6` copies it
//!   to `+0xCF8`. The count of jumps left is refilled on the ground and spent one per airborne
//!   jump. Triple Jump's bank row `0x808042A5` carries a count of its own, 2, which this value
//!   does not change.
//!
//! Lanes whose role is not established are offered too, marked so a page can say so: the four
//! floats of the jump and lift preparation profile `0x808044BF` `+0x10`, 0.8, 0.8, 0.1 and 1, and
//! the four a `0x808044D6` row sets in their place, High Jump 1, 1, 0.1 and 0 and Strafe Jump
//! 0.25, 0.25, 1 and 1. In game on 2026-10-05, High Jump with three airborne jumps gave its third
//! and fourth jumps no height, which per-jump lanes would explain. That is not yet confirmed.
use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeValue,
    WeaponRuntimeValueOverride,
};

/// How a movement value's four bytes read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    /// A 32-bit float.
    Distance,
    /// An unsigned 32-bit count.
    Count,
    /// A 32-bit float multiplier.
    Factor,
}

impl Unit {
    /// Four bytes as the number they hold.
    #[must_use]
    pub fn value(self, bits: u32) -> f32 {
        match self {
            Self::Count => bits as f32,
            Self::Distance | Self::Factor => f32::from_bits(bits),
        }
    }

    /// A number as its four bytes, a count rounded to a whole one.
    #[must_use]
    pub fn bits(self, value: f32) -> u32 {
        match self {
            Self::Count => value.round().max(0.0) as u32,
            Self::Distance | Self::Factor => value.to_bits(),
        }
    }
}

/// The traced lanes of movement bank rows: the row's modifier class, the lane's offset in its
/// block, the name and how it reads. A selected jump, lift or glide key applies its rows.
/// - `0x808044D4` `+0x10` and `+0x14`: the vertical and directional contributions of the jump
///   preparation. `FB1D46` multiplies a program result by the first to build the vertical
///   vector, and `FB1FD7` scales a second result by the other before combining them. High Jump
///   1.18 and 0.75, Strafe Jump 1.1 and 3, Triple Jump 0.85 and 1.1. Multipliers, not meters.
/// - `0x808042A5` `+0x10`: the airborne jump count a row sets, Triple Jump 2.
/// - `0x808044D6` `+0x10` to `+0x1C`: the preparation profile's four lanes a row sets, role
///   unconfirmed.
const ROW_LANES: [(u32, usize, &str, Unit, bool); 7] = [
    (0x8080_44D4, 0x10, "Vertical Impulse", Unit::Factor, true),
    (0x8080_44D4, 0x14, "Directional Impulse", Unit::Factor, true),
    (0x8080_42A5, 0x10, "Airborne Jumps", Unit::Count, true),
    (0x8080_44D6, 0x10, "Profile 1", Unit::Factor, false),
    (0x8080_44D6, 0x14, "Profile 2", Unit::Factor, false),
    (0x8080_44D6, 0x18, "Profile 3", Unit::Factor, false),
    (0x8080_44D6, 0x1C, "Profile 4", Unit::Factor, false),
];

/// One traced lane of a bank property row: the row, the key that applies it, the lane's offset
/// in the row's modifier block and in the bank, its name, how it reads and its stock bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowLane {
    pub row: u16,
    pub key: u32,
    pub lane: u16,
    pub offset: usize,
    pub label: &'static str,
    pub unit: Unit,
    /// Whether a native consumer establishes its role.
    pub traced: bool,
    pub stock: u32,
}

/// The traced lanes of `bank`'s rows that `keys` apply, leaving out rows an inner key gates,
/// which apply only beside another selection.
pub fn row_lanes(bank: &[u8], keys: &[u32]) -> Result<Vec<RowLane>, String> {
    let rows = crate::ability::bank::property_rows(bank)?;
    let modifiers = crate::ability::bank::row_modifiers(bank)?;
    let mut found = Vec::new();
    for (index, (row, &(block, gated))) in rows.iter().zip(&modifiers).enumerate() {
        if gated || !keys.contains(&row.key) {
            continue;
        }
        let row_index = u16::try_from(index).map_err(|_| "The bank has too many rows")?;
        for &(class, lane, label, unit, traced) in &ROW_LANES {
            if row.modifier_class != class {
                continue;
            }
            let offset = block + lane;
            let stock = bank
                .get(offset..offset + 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
                .ok_or_else(|| format!("Bank row {index}'s lane +0x{lane:X} leaves the bank"))?;
            found.push(RowLane {
                row: row_index,
                key: row.key,
                lane: lane as u16,
                offset,
                label,
                unit,
                traced,
                stock,
            });
        }
    }
    Ok(found)
}

/// Where a script parameter's row holds its reset value: the value the bank's script reads while
/// no applied key sets it. A row of the parameter table, by its place and the parameter's name.
pub const PARAMETER_RESET: u16 = 4;

/// The reset value of script parameter `name`, the `index`th row of `bank`'s parameter table, as
/// a lane a bank value edit can write.
pub fn parameter_lane(bank: &[u8], (name, index): (u32, u16)) -> Result<RowLane, String> {
    let parameters = crate::ability::bank::parameters(bank)?;
    let rows = crate::ability::bank::parameter_rows(bank)?;
    let (parameter, &row) = parameters
        .iter()
        .zip(&rows)
        .nth(usize::from(index))
        .ok_or_else(|| format!("The bank has no parameter row {index}"))?;
    if parameter.name != name {
        return Err(format!(
            "Parameter row {index} is 0x{:08X}, not 0x{name:08X}",
            parameter.name
        ));
    }
    Ok(RowLane {
        row: index,
        key: name,
        lane: PARAMETER_RESET,
        offset: row + usize::from(PARAMETER_RESET),
        label: "Default",
        unit: Unit::Distance,
        traced: true,
        stock: parameter.reset.to_bits(),
    })
}

/// The block offset of the lane `lane` of row `row` of `bank` when the row has key `key` and a
/// traced lane there, which a bank value edit checks before it writes.
pub fn row_lane(bank: &[u8], (key, row, lane): (u32, u16, u16)) -> Result<RowLane, String> {
    let rows = crate::ability::bank::property_rows(bank)?;
    let found = rows
        .get(usize::from(row))
        .ok_or_else(|| format!("The bank has no property row {row}"))?;
    if found.key != key {
        return Err(format!(
            "Bank row {row} has key 0x{:08X}, not 0x{key:08X}",
            found.key
        ));
    }
    row_lanes(bank, &[key])?
        .into_iter()
        .find(|each| each.row == row && each.lane == lane)
        .ok_or_else(|| format!("Bank row {row} has no editable lane +0x{lane:X}"))
}

/// The values: the definition class, the offset in it, the name, how it reads and whether its
/// role is traced.
const VALUES: [(u32, u32, &str, Unit, bool); 6] = [
    (0x8080_4326, 0x634, "Blink Distance", Unit::Distance, true),
    (0x8080_429C, 0x988, "Airborne Jumps", Unit::Count, true),
    (0x8080_44BF, 0x10, "Profile 1", Unit::Factor, false),
    (0x8080_44BF, 0x14, "Profile 2", Unit::Factor, false),
    (0x8080_44BF, 0x18, "Profile 3", Unit::Factor, false),
    (0x8080_44BF, 0x1C, "Profile 4", Unit::Factor, false),
];

/// One movement value of a graph: its name, how it reads, and the opaque field holding it with
/// its byte offset there.
#[derive(Clone, Debug, PartialEq)]
pub struct MovementValue {
    pub label: &'static str,
    pub unit: Unit,
    /// Whether a native consumer establishes its role.
    pub traced: bool,
    pub field: WeaponRuntimeField,
    offset: usize,
}

/// Every traced movement value in `graph`'s component definitions. A value whose four bytes are
/// not wholly inside one opaque field is left out.
#[must_use]
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<MovementValue> {
    let mut found = Vec::new();
    for root in graph
        .resources
        .iter()
        .filter_map(|resource| resource.definition.as_ref())
    {
        for &(schema, offset, label, unit, traced) in &VALUES {
            if root.schema != schema {
                continue;
            }
            let mut covering = root.fields.iter().filter(|field| {
                field.source != WeaponRuntimeFieldSource::NativeDeclaration
                    && field.locator.value_offset <= offset
                    && field
                        .locator
                        .value_offset
                        .checked_add(field.locator.byte_size)
                        .is_some_and(|end| end >= offset + 4)
            });
            let Some(field) = covering.next() else {
                continue;
            };
            if covering.next().is_some() || !matches!(field.value, WeaponRuntimeValue::Bytes(_)) {
                continue;
            }
            let value = MovementValue {
                label,
                unit,
                traced,
                field: field.clone(),
                offset: (offset - field.locator.value_offset) as usize,
            };
            if value.read(&field.value).is_some() && !found.contains(&value) {
                found.push(value);
            }
        }
    }
    found
}

impl MovementValue {
    fn read(&self, value: &WeaponRuntimeValue) -> Option<u32> {
        let WeaponRuntimeValue::Bytes(bytes) = value else {
            return None;
        };
        let bytes = bytes.get(self.offset..self.offset + 4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn own<'a>(
        &self,
        draft: &'a [WeaponRuntimeValueOverride],
    ) -> Option<&'a WeaponRuntimeValueOverride> {
        draft
            .iter()
            .find(|entry| entry.locator == self.field.locator)
    }

    /// The stock bits.
    #[must_use]
    pub fn stock(&self) -> u32 {
        self.read(&self.field.value).unwrap_or_default()
    }

    /// The bits `draft` gives it, else the stock ones.
    #[must_use]
    pub fn bits(&self, draft: &[WeaponRuntimeValueOverride]) -> u32 {
        self.own(draft)
            .and_then(|entry| self.read(&entry.value))
            .unwrap_or_else(|| self.stock())
    }

    /// Whether `draft` changes it.
    #[must_use]
    pub fn is_modified(&self, draft: &[WeaponRuntimeValueOverride]) -> bool {
        self.bits(draft) != self.stock()
    }

    /// Sets its four bytes in `draft`, keeping the rest of the field as `draft` has it. The
    /// override goes once the field is stock again.
    pub fn write(&self, draft: &mut Vec<WeaponRuntimeValueOverride>, bits: u32) {
        let mut value = self
            .own(draft)
            .map_or_else(|| self.field.value.clone(), |entry| entry.value.clone());
        let WeaponRuntimeValue::Bytes(bytes) = &mut value else {
            return;
        };
        let Some(lane) = bytes.get_mut(self.offset..self.offset + 4) else {
            return;
        };
        lane.copy_from_slice(&bits.to_le_bytes());
        draft.retain(|entry| entry.locator != self.field.locator);
        if value != self.field.value {
            draft.push(WeaponRuntimeValueOverride {
                locator: self.field.locator.clone(),
                value,
            });
        }
    }

    /// Restores its stock bytes in `draft`.
    pub fn reset(&self, draft: &mut Vec<WeaponRuntimeValueOverride>) {
        self.write(draft, self.stock());
    }
}
