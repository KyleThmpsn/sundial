//! Movement controller values whose native consumers are traced: how far a Blink carries, how
//! many jumps a jump ability allows in the air and how a jump or lift scales the velocity it
//! starts from. The runtime graph holds each inside an opaque byte field of the controller's
//! definition, so a value here is a four-byte lane of that field, read and written through the
//! same overrides as any other value.
//!
//! Evidence, build 86657.20.08.23.1800.d2_rc___release (2026-10-05 ability atlas):
//! - Blink controller definition `0x80804326` `+0x634`, 4.5 for Blink and 4.2 for Ionic Blink.
//!   `FB45CD` copies it to runtime `+0xB80`, and `FB2247` multiplies that by a clamped fraction
//!   at `+0xB84` before building the displacement. Native units, not measured meters.
//! - Jump controller definition `0x8080429C` `+0x988`, 1 for the Hunter jumps. `FB47A6` copies it
//!   to `+0xCF8`. The count of jumps left is refilled on the ground and spent one per airborne
//!   jump. Triple Jump's bank row `0x808042A5` carries a count of its own, 2, which this value
//!   does not change.
//! - Jump and lift preparation profile `0x808044BF` `+0x10`, `+0x14` and `+0x18`, stock 0.8, 0.8
//!   and 0.1 (2026-10-08 survey). Each multiplies the body's velocity at activation, before the
//!   separate impulse: the first two its projections on a horizontal basis and its perpendicular,
//!   the third its vertical part. `FAEC50` reads the velocity and calls `FBB260`, which skips the
//!   change while all four lanes are within 0.0001 of 1 (`FBB288..FBB2C5`). The basis provider's
//!   identity is not resolved, so the first two are not named forward or sideways. A
//!   `0x808044D6` row multiplies each by a lane of its own (`FBFC71`, `FBFC8D`, `FBFCAB`), High
//!   Jump 1, 1 and 0.1 and Strafe Jump 0.25, 0.25 and 1, so one lane alone is not always the
//!   effective factor.
//!
//! The fourth profile lane, stock 1, only takes part in that test, so it is offered marked
//! unconfirmed.
use crate::package_payload::{
    bytes_at, i64_at, native_array_at, relative_offset, rows_fit, u32_at, u64_at,
};
use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimeResource,
    WeaponRuntimeValue, WeaponRuntimeValueOverride,
};
mod bank;
pub use bank::validate_context as validate_bank_context;

/// How a movement value's four bytes read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    /// A 32-bit float.
    Distance,
    /// An unsigned 32-bit count.
    Count,
    /// A 32-bit float multiplier.
    Factor,
    /// A fraction of the ability's energy per second.
    Rate,
    /// A plain floating-point factor with native conversions retained.
    Number,
    /// One native byte selecting direct application or blending.
    Mode,
}

impl Unit {
    /// Four bytes as the number they hold.
    #[must_use]
    pub fn value(self, bits: u32) -> f32 {
        match self {
            Self::Count | Self::Mode => bits as f32,
            Self::Distance | Self::Factor | Self::Rate | Self::Number => f32::from_bits(bits),
        }
    }

    /// A number as its four bytes, a count rounded to a whole one.
    #[must_use]
    pub fn bits(self, value: f32) -> u32 {
        match self {
            Self::Count => value.round().max(0.0) as u32,
            Self::Mode => u32::from(value >= 0.5),
            Self::Distance | Self::Factor | Self::Rate | Self::Number => value.to_bits(),
        }
    }

    /// The exact storage width, including the one-byte mode selector.
    pub const fn width(self) -> usize {
        match self {
            Self::Mode => 1,
            _ => 4,
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
/// - `0x808044D6` `+0x10` to `+0x1C`: the multipliers a row applies to the preparation profile's
///   four lanes. The fourth only takes part in the profile's skip test.
const ROW_LANES: [(u32, usize, &str, Unit, bool); 24] = [
    (0x8080_44D2, 0x10, "Velocity Blending", Unit::Mode, true),
    (
        0x8080_4258,
        0x10,
        "Positive X Speed Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x14,
        "Negative X Speed Multiplier",
        Unit::Factor,
        true,
    ),
    (0x8080_4258, 0x18, "Y Speed Multiplier", Unit::Factor, true),
    (
        0x8080_4258,
        0x1C,
        "Vertical Speed Limit Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x24,
        "Acceleration Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x28,
        "Limit Braking Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x2C,
        "Vertical Correction Multiplier",
        Unit::Factor,
        true,
    ),
    (0x8080_4258, 0x30, "Turn Rate Factor", Unit::Number, true),
    (
        0x8080_4258,
        0x34,
        "Falling Gravity Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x38,
        "Rising Gravity Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x3C,
        "Fall Speed Threshold Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x40,
        "Fall Arrest Speed Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x44,
        "Gravity Blend Height Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_4258,
        0x48,
        "Gravity Blend Factor Multiplier",
        Unit::Factor,
        true,
    ),
    (
        0x8080_44CA,
        0x10,
        "Impulse Height Limit",
        Unit::Distance,
        true,
    ),
    (0x8080_452E, 0x10, "Active Energy Rate", Unit::Rate, true),
    (0x8080_44D4, 0x10, "Vertical Impulse", Unit::Factor, true),
    (0x8080_44D4, 0x14, "Directional Impulse", Unit::Factor, true),
    (0x8080_42A5, 0x10, "Airborne Jumps", Unit::Count, true),
    (
        0x8080_44D6,
        0x10,
        "Directional Velocity Multiplier 1",
        Unit::Factor,
        true,
    ),
    (
        0x8080_44D6,
        0x14,
        "Directional Velocity Multiplier 2",
        Unit::Factor,
        true,
    ),
    (
        0x8080_44D6,
        0x18,
        "Vertical Velocity Multiplier",
        Unit::Factor,
        true,
    ),
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
            // These native bodies are independently paired with a source/cache block. Write
            // only the Float32 definition lane, after checking both prefixes and their owners.
            if let Some(body) = match class {
                0x8080_44CA => Some(0x8080_44CB),
                0x8080_452E => Some(0x8080_452F),
                0x8080_44D2 => Some(0x8080_44D3),
                0x8080_4258 => Some(0x8080_4259),
                _ => None,
            } {
                let twin = usize::try_from(u64_at(bank, block + 8)?)
                    .map_err(|_| format!("Movement bank row {index}'s twin overflows"))?;
                bytes_at::<0x18>(bank, block)?;
                if block < 4
                    || twin < 4
                    || u32_at(bank, block - 4)? != body
                    || u32_at(bank, twin - 4)? != class
                    || u32_at(bank, twin + 4)? != body
                    || u32_at(bank, twin)? != u32_at(bank, block)?
                    || u64_at(bank, twin + 8)? != block as u64
                {
                    return Err(format!(
                        "Movement bank row {index} has an invalid {class:08X}/{body:08X} pair"
                    ));
                }
            }
            if class == 0x8080_4258 {
                validate_glide_bank(bank)?;
                bytes_at::<0x50>(bank, block)?;
            }
            let offset = block + lane;
            let stock = if unit == Unit::Mode {
                u32::from(bytes_at::<1>(bank, offset)?[0])
            } else {
                u32_at(bank, offset)?
            };
            if !unit.value(stock).is_finite() {
                return Err(format!("Movement bank row {index}'s {label} is not finite"));
            }
            if matches!(class, 0x8080_44D2 | 0x8080_44CA | 0x8080_452E) {
                // Last writer within a key wins. In particular High Jump's later mode row
                // replaces its earlier mode row. Multiplicative profile lanes are not replaced.
                found.retain(|old: &RowLane| old.key != row.key || old.label != label);
            }
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

fn validate_glide_bank(bank: &[u8]) -> Result<(), String> {
    let source = relative_offset(0x10, 0, i64_at(bank, 0x10)?)?;
    let definition = relative_offset(0x18, 0, i64_at(bank, 0x18)?)?;
    if u32_at(bank, source + 4)? != 0x8080_4252
        || u32_at(bank, definition + 4)? != 0x8080_4251
        || u32_at(bank, source + 0x48)? != 0x8080_4251
        || u32_at(bank, source + 0x4C)? != 1
        || u32_at(bank, source + 0x50)? != u32_at(bank, source)?
        || u32_at(bank, source + 0x54)? != 0x8080_4251
        || u64_at(bank, source + 0x58)? != source as u64
    {
        return Err("Glide bank has an incompatible profile handler".into());
    }
    let descriptor = definition + 0x48;
    bytes_at::<24>(bank, descriptor)?;
    if relative_offset(descriptor, 0, i64_at(bank, descriptor)?)? != definition
        || u32_at(bank, descriptor + 8)? == u32::MAX
        || u32_at(bank, descriptor + 12)? != 0
    {
        return Err("Glide bank has an incompatible movement destination".into());
    }
    Ok(())
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
const VALUES: [(u32, u32, &str, Unit, bool); 9] = [
    (PREPARATION, 0x20, "Velocity Blending", Unit::Mode, true),
    (
        PREPARATION,
        0x260,
        "Impulse Height Limit",
        Unit::Distance,
        true,
    ),
    (PREPARATION, 0x264, "Height Fade Factor", Unit::Factor, true),
    (0x8080_4326, 0x634, "Blink Distance", Unit::Distance, true),
    (0x8080_429C, 0x988, "Airborne Jumps", Unit::Count, true),
    (
        PREPARATION,
        0x10,
        "Directional Velocity Multiplier 1",
        Unit::Factor,
        true,
    ),
    (
        PREPARATION,
        0x14,
        "Directional Velocity Multiplier 2",
        Unit::Factor,
        true,
    ),
    (
        PREPARATION,
        0x18,
        "Vertical Velocity Multiplier",
        Unit::Factor,
        true,
    ),
    (PREPARATION, 0x1C, "Profile 4", Unit::Factor, false),
];

/// The jump and lift preparation definition, whose profile is a movement value only where a
/// controller's activation reaches it.
const PREPARATION: u32 = 0x8080_44BF;

/// The controllers whose activation reaches the preparation they hold: the controller instance's
/// class and the preparation source's offset in it. Build 86657.20.08.23.1800.d2_rc___release,
/// 2026-10-08 survey of the seven stock preparations:
/// - `0x8080429B`, the Hunter jump, `+0xAE0`: method 3 `FB31A0` ends in the base activation
///   `FB2C80`, which passes that preparation to `FAEC50` at `FB2C90`.
/// - `0x808044BB`, a Titan lift, `+0xAE0`: its registered activation is `FB2C80` itself.
/// - `0x8080424B`, a Glide, `+0xCE0`: the `0x808042AE` phase it holds at `+0xCC0` activates its
///   own preparation at `+0x20` through `D26BA0` and `D15010`.
///
/// A Glide's inherited `+0xAE0` preparation is left out, since its method 2 `D18970` selects the
/// phase instead and no reached activation uses that one, and so are the three preparations no
/// controller holds, which only a helper references.
const ACTIVATING: [(u32, u32); 3] = [
    (0x8080_429B, 0xAE0),
    (0x8080_44BB, 0xAE0),
    (0x8080_424B, 0xCE0),
];

/// Whether a controller in `resource`'s owner holds its preparation where its activation reaches.
fn activated(graph: &WeaponRuntimeGraph, resource: &WeaponRuntimeResource) -> bool {
    graph.resources.iter().any(|controller| {
        controller.owner_tag == resource.owner_tag
            && resource
                .instance
                .owner_offset
                .checked_sub(controller.instance.owner_offset)
                .is_some_and(|offset| ACTIVATING.contains(&(controller.instance.schema, offset)))
    })
}

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
/// not wholly inside one opaque field is left out, and so is the profile of a preparation no
/// controller's activation reaches.
#[must_use]
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<MovementValue> {
    let mut found = Vec::new();
    for resource in &graph.resources {
        let Some(root) = resource.definition.as_ref() else {
            continue;
        };
        if root.schema == PREPARATION && !activated(graph, resource) {
            continue;
        }
        for &(schema, offset, label, unit, traced) in &VALUES {
            if root.schema != schema {
                continue;
            }
            if schema == PREPARATION && matches!(offset, 0x260 | 0x264) {
                let height = root.fields.iter().find_map(|field| {
                    let start = 0x260_u32.checked_sub(field.locator.value_offset)? as usize;
                    let WeaponRuntimeValue::Bytes(bytes) = &field.value else {
                        return None;
                    };
                    bytes
                        .get(start..start + 4)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map(f32::from_le_bytes)
                });
                if !height.is_some_and(|height| height.is_finite() && height > 0.0) {
                    continue;
                }
            }
            let mut covering = root.fields.iter().filter(|field| {
                field.source != WeaponRuntimeFieldSource::NativeDeclaration
                    && field.locator.value_offset <= offset
                    && field
                        .locator
                        .value_offset
                        .checked_add(field.locator.byte_size)
                        .is_some_and(|end| end >= offset + unit.width() as u32)
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
        let bytes = bytes.get(self.offset..self.offset + self.unit.width())?;
        if self.unit == Unit::Mode {
            Some(u32::from(bytes[0]))
        } else {
            Some(u32::from_le_bytes(bytes.try_into().ok()?))
        }
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
        let Some(lane) = bytes.get_mut(self.offset..self.offset + self.unit.width()) else {
            return;
        };
        lane.copy_from_slice(&bits.to_le_bytes()[..self.unit.width()]);
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
