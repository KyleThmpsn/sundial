//! Keep edited lanes authoritative while leaving the source expression alive on every other lane.
use crate::{AuthoringResult, error::invalid, tag_payload::*};
use std::collections::BTreeMap;

pub(crate) fn apply(scope: &mut Vec<u8>, writes: &[(usize, usize, f32)]) -> AuthoringResult<()> {
    if writes.is_empty() {
        return Ok(());
    }
    let (count, _, rows, class) = array_at(scope, 0x88)?;
    if count != 27 || class != 0x80800090 {
        return Err(invalid("Source dye vectors have an unsupported layout"));
    }
    let mut lanes = BTreeMap::<usize, ([f32; 4], [f32; 4])>::new();
    for &(vector, lane, value) in writes {
        if vector >= 27 || lane >= 4 || !value.is_finite() {
            return Err(invalid("Source dye edit is outside its material vectors"));
        }
        write_bytes(scope, rows + vector * 16 + lane * 4, &value.to_le_bytes())?;
        let (mask, values) = lanes.entry(vector).or_insert(([1.0; 4], [0.0; 4]));
        mask[lane] = 0.0;
        values[lane] = value;
    }
    if read_u64(scope, 0x58)? == 0 {
        return Ok(());
    }
    let (count, _, start, class) = array_at(scope, 0x58)?;
    if class != 0x80800009 || count > 4096 {
        return Err(invalid("Source expression layout differs"));
    }
    let code = scope
        .get(start..start + count)
        .ok_or_else(|| invalid("Source expression is truncated"))?
        .to_vec();
    let mut constants = if read_u64(scope, 0x68)? == 0 {
        Vec::new()
    } else {
        let (count, _, start, class) = array_at(scope, 0x68)?;
        if class != 0x80800090 || count > 256 {
            return Err(invalid("Source expression constants differ"));
        }
        scope
            .get(start..start + count * 16)
            .ok_or_else(|| invalid("Source expression constants are truncated"))?
            .to_vec()
    };
    let mut masks = BTreeMap::new();
    let mut result = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let op = code[at];
        let size = match op {
            1..=0x13 | 0x15..=0x21 | 0x23 | 0x25 | 0x27..=0x2B | 0x2E => 0,
            0x22 | 0x34..=0x3B | 0x42..=0x4C | 0x4E => 1,
            0x3C..=0x41 | 0x51..=0x53 => 2,
            0x4D => 4,
            _ => {
                return Err(invalid(format!(
                    "Unknown source native expression opcode 0x{op:02X}"
                )));
            }
        };
        let instruction = code
            .get(at..at + 1 + size)
            .ok_or_else(|| invalid("Source expression instruction is truncated"))?;
        if op == 0x43
            && let Some(&(mask, values)) = lanes.get(&(instruction[1] as usize))
        {
            let vector = instruction[1];
            let index = match masks.get(&vector) {
                Some(&index) => index,
                None => {
                    let index = u8::try_from(constants.len() / 16)
                        .map_err(|_| invalid("Edited expression exceeds 256 constants"))?;
                    if index == 255 {
                        return Err(invalid("Edited expression exceeds 256 constants"));
                    }
                    constants.extend(mask.into_iter().chain(values).flat_map(f32::to_le_bytes));
                    masks.insert(vector, index);
                    index
                }
            };
            result.extend([0x34, index, 0x03, 0x34, index + 1, 0x01]);
        }
        result.extend_from_slice(instruction);
        at += 1 + size;
    }
    if result.len() > 4096 {
        return Err(invalid(
            "Edited expression exceeds the native program budget",
        ));
    }
    append_native_array(scope, 0x68, 0x80800090, constants.len() / 16, &constants)?;
    append_native_array(scope, 0x58, 0x80800009, result.len(), &result)?;
    let len = scope.len() as u64;
    write_u64(scope, 0, len)
}
