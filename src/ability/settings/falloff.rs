//! Falloff Start: where a sequence query's spatial weight starts to fall.
//!
//! A kind-1D query definition (`80804AB9`) evaluates the inline four-lane program at +0xB0 with a
//! target's normalized distance progress, zero at its shape's center and one at its boundary
//! (`D53875`, `D53884 -> 3B9FE0`). The weight it gives scales that target's second action result
//! (`D5591F`, `D55FA5`). Seven stock queries hold one exact plateau-linear program: weight one
//! through progress `s`, then falling linearly to zero at one. Its five constant rows keep `s` in
//! three Y lanes, `-1 / (1 - s)` in row 2, `1 / (1 - s)` in row 3 and `s` in row 4, so the setting
//! writes the three together and leaves every other lane as it is. Traced in build 86657.
use super::*;

/// The query definition, its kind byte and the operation kind it pairs with, its flags, and the
/// first word of its orientation program, which is zero when it has none.
const QUERY: u32 = 0x8080_4AB9;
const KIND: usize = 0x30;
const QUERY_KIND: u8 = 0x1D;
const FLAGS: usize = 0xA0;
const ORIENTATION: usize = 0x160;
/// The inline program's bytecode and constant arrays, each a count and a relative pointer, then
/// its input count, a second count, its output count and its fast-path index.
const CODE: usize = 0xB0;
const CONSTANTS: usize = 0xC0;
const DESCRIPTOR: usize = 0xD0;
const ONE_IN_ONE_OUT: [u32; 4] = [1, 0, 1, 0];
/// Push input 0, permute, clamp, evaluate the spline, clamp and store.
const PLATEAU_LINEAR: [u8; 14] = [
    0x3C, 0x00, 0x22, 0x00, 0x23, 0x22, 0x00, 0x37, 0x00, 0x23, 0x22, 0x00, 0x3E, 0x00,
];
/// The rows whose Y lane the setting writes: the falling slope, the rising slope and the start.
const FALL: usize = 2;
const RISE: usize = 3;
const START: usize = 4;

/// The Falloff Start of each query in `root` whose program has the exact plateau-linear form, no
/// flags, no orientation program and no fast path.
pub(super) fn find(owner: u32, root: &WeaponRuntimeRoot, payload: Option<&[u8]>) -> Vec<Setting> {
    let Some(payload) = payload else {
        return Vec::new();
    };
    // The structure walk records each query's constant array pointer as a field it declares, so
    // its offset less the pointer's place is where the query starts.
    root.structure
        .fields
        .iter()
        .filter(|field| field.schema == QUERY && field.schema_offset == CONSTANTS + 8)
        .filter_map(|field| field.owner_offset.checked_sub(CONSTANTS + 8))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|query| setting(owner, root, payload, query))
        .collect()
}

fn setting(owner: u32, root: &WeaponRuntimeRoot, payload: &[u8], query: usize) -> Option<Setting> {
    let word = |at: usize| {
        payload
            .get(at..at + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let descriptor = (0..4)
        .map(|index| word(query + DESCRIPTOR + index * 4))
        .collect::<Option<Vec<_>>>()?;
    if word(query.checked_sub(4)?)? != QUERY
        || *payload.get(query + KIND)? != QUERY_KIND
        || word(query + FLAGS)? != 0
        || payload.get(query + ORIENTATION..query + ORIENTATION + 8)? != [0; 8]
        || descriptor != ONE_IN_ONE_OUT
    {
        return None;
    }
    let (code, length) = array(payload, query + CODE, BYTECODE_ROW)?;
    let (rows, count) = array(payload, query + CONSTANTS, CONSTANT_ROW)?;
    if payload.get(code..code + length)? != PLATEAU_LINEAR || count != 5 {
        return None;
    }
    let row = |index: usize| -> Option<[f32; 4]> {
        let mut lanes = [0.0; 4];
        for (lane, value) in lanes.iter_mut().enumerate() {
            *value = f32::from_bits(word(rows + index * 16 + lane * 4)?);
        }
        Some(lanes)
    };
    start(&[row(0)?, row(1)?, row(2)?, row(3)?, row(4)?])?;
    // The constant rows as fields this query's program declares.
    let field = |index: usize| {
        root.fields.iter().find(|field| {
            field.source == WeaponRuntimeFieldSource::NativeDeclaration
                && field.owner_offset as usize == rows + index * 16
                && field.locator.type_handle.get() == CONSTANT_ROW
                && matches!(field.value, WeaponRuntimeValue::Vector4Float32Bits(_))
                && field.locator.path.windows(2).any(|pair| {
                    pair[0].type_handle.get() == QUERY
                        && pair[1].type_handle.get() == ARRAY
                        && pair[1].byte_offset as usize == CONSTANTS + 8
                })
        })
    };
    Some(Setting {
        kind: Kind::FalloffStart,
        owner_tag: owner,
        field: field(START)?.clone(),
        linked: vec![field(FALL)?.clone(), field(RISE)?.clone()],
        lane: None,
        requires_positive_minimum: false,
    })
}

/// The elements of the typed array whose count and relative pointer sit at `at`, and their
/// count. The array's header repeats the count and names its row class.
fn array(payload: &[u8], at: usize, class: u32) -> Option<(usize, usize)> {
    let count = u64::from_le_bytes(payload.get(at..at + 8)?.try_into().ok()?);
    let relative = i64::from_le_bytes(payload.get(at + 8..at + 16)?.try_into().ok()?);
    let header = (at + 8).checked_add_signed(isize::try_from(relative).ok()?)?;
    let bytes = payload.get(header..header + 16)?;
    (u64::from_le_bytes(bytes[..8].try_into().ok()?) == count
        && u32::from_le_bytes(bytes[8..12].try_into().ok()?) == class
        && bytes[12..] == [0; 4])
        .then_some((header + 16, usize::try_from(count).ok()?))
}

/// The start `s` of a plateau-linear fade: rows 0 and 1 zero, row 2 `[0, -1/(1-s), 0, 0]`, row 3
/// `[1, 1/(1-s), 0, 0]` and row 4 `[0, s, 1, 1]`, with `s` from zero up to one.
fn start(rows: &[[f32; 4]; 5]) -> Option<f32> {
    let start = rows[START][1];
    if !(0.0..1.0).contains(&start) {
        return None;
    }
    let slope = slope(start);
    let near =
        |value: f32, wanted: f64| (f64::from(value) - wanted).abs() <= 1e-5 * wanted.abs().max(1.0);
    (rows[0] == [0.0; 4]
        && rows[1] == [0.0; 4]
        && rows[FALL][0] == 0.0
        && near(rows[FALL][1], -slope)
        && rows[FALL][2..] == [0.0; 2]
        && rows[RISE][0] == 1.0
        && near(rows[RISE][1], slope)
        && rows[RISE][2..] == [0.0; 2]
        && rows[START][0] == 0.0
        && rows[START][2..] == [1.0; 2])
        .then_some(start)
}

/// How fast the weight falls after `start`: one over what is left of the shape.
fn slope(start: f32) -> f64 {
    1.0 / (1.0 - f64::from(start))
}

/// The Y lanes for a start of `fraction`, by field: the start row's own, then the falling and
/// rising slopes of its linked rows.
fn lanes(setting: &Setting, fraction: f32) -> Vec<(&WeaponRuntimeField, f32)> {
    let slope = slope(fraction) as f32;
    std::iter::once((&setting.field, fraction))
        .chain(setting.linked.iter().zip([-slope, slope]))
        .collect()
}

/// The Y lane of `value`, a constant row.
pub(super) fn read(value: &WeaponRuntimeValue) -> Option<f32> {
    match value {
        WeaponRuntimeValue::Vector4Float32Bits(lanes) => Some(f32::from_bits(lanes[1])),
        _ => None,
    }
}

/// Sets the three rows for a start of `fraction`, from zero up to one, keeping every other lane of
/// each row as the draft holds it.
pub(super) fn write(
    setting: &Setting,
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    fraction: f32,
) -> Result<(), String> {
    if !(0.0..1.0).contains(&fraction) || setting.linked.len() != 2 {
        return Err("Falloff Start must be at least 0% and below 100%.".into());
    }
    for (field, lane) in lanes(setting, fraction) {
        put(draft, field, Some(lane.to_bits()))?;
    }
    Ok(())
}

/// Restores the stock Y lane of each of its three rows, keeping their other lanes' edits.
pub(super) fn reset(setting: &Setting, draft: &mut Vec<WeaponRuntimeValueOverride>) {
    for field in std::iter::once(&setting.field).chain(&setting.linked) {
        let _ = put(draft, field, None);
    }
}

/// Whether the draft changes the Y lane of any of its three rows.
pub(super) fn is_modified(setting: &Setting, draft: &[WeaponRuntimeValueOverride]) -> bool {
    std::iter::once(&setting.field)
        .chain(&setting.linked)
        .any(|field| {
            let current = draft
                .iter()
                .find(|entry| entry.locator == field.locator)
                .map_or(&field.value, |entry| &entry.value);
            read(current).map(f32::to_bits) != read(&field.value).map(f32::to_bits)
        })
}

/// Writes `bits` into the Y lane of `field` in the draft, or its stock Y lane when `None`. A row
/// that ends up as it shipped leaves the draft.
fn put(
    draft: &mut Vec<WeaponRuntimeValueOverride>,
    field: &WeaponRuntimeField,
    bits: Option<u32>,
) -> Result<(), String> {
    let current = draft
        .iter()
        .find(|entry| entry.locator == field.locator)
        .map_or(&field.value, |entry| &entry.value);
    let (
        WeaponRuntimeValue::Vector4Float32Bits(mut lanes),
        WeaponRuntimeValue::Vector4Float32Bits(stock),
    ) = (current.clone(), &field.value)
    else {
        return Err("Falloff Start has a value of an unexpected kind.".into());
    };
    lanes[1] = bits.unwrap_or(stock[1]);
    let next = WeaponRuntimeValue::Vector4Float32Bits(lanes);
    draft.retain(|entry| entry.locator != field.locator);
    if next != field.value {
        draft.push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: next,
        });
    }
    Ok(())
}

/// Keeps the Falloff Starts whose three rows no other path of the graph reaches, since editing a
/// shared row would change that program too.
pub(super) fn unshared(settings: &mut Vec<Setting>, roots: &[(u32, &WeaponRuntimeRoot)]) {
    let mut paths = BTreeMap::<(u32, u32), BTreeSet<&Vec<WeaponRuntimePathElement>>>::new();
    for (owner, root) in roots {
        for field in &root.fields {
            if field.locator.type_handle.get() == CONSTANT_ROW {
                paths
                    .entry((*owner, field.owner_offset))
                    .or_default()
                    .insert(&field.locator.path);
            }
        }
    }
    settings.retain(|setting| {
        setting.kind != Kind::FalloffStart
            || std::iter::once(&setting.field)
                .chain(&setting.linked)
                .all(|field| {
                    paths
                        .get(&(setting.owner_tag, field.owner_offset))
                        .is_none_or(|paths| paths.len() == 1)
                })
    });
}
