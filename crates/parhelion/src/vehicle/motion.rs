//! Native hover motion expressions. The supported shape is established by clean Shadowkeep
//! Sparrow owners and original interpreter execution, rather than an asset-specific hash.
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{append_native_array, array_at, read_u32, write_u64},
};

const CODE: u32 = 0x8080_0009;
const CONSTANT: u32 = 0x8080_0090;
const CHANNEL: u32 = 0x8080_9789;

fn expression(payload: &mut Vec<u8>, owner: u32, root: usize, percent: u16) -> AuthoringResult<()> {
    if read_u32(payload, root)? != owner || read_u32(payload, root + 4)? != 0x8080_89F5 {
        return Err(invalid(
            "Driving Speed found an unsupported motion expression reference",
        ));
    }
    let (code_count, _, code_rows, code_class) = array_at(payload, root + 16)?;
    let (constant_count, _, constant_rows, constant_class) = array_at(payload, root + 32)?;
    let code = payload
        .get(code_rows..code_rows.saturating_add(code_count))
        .ok_or_else(|| invalid("Driving Speed bytecode is truncated"))?
        .to_vec();
    let constants = payload
        .get(constant_rows..constant_rows.saturating_add(constant_count.saturating_mul(16)))
        .ok_or_else(|| invalid("Driving Speed constants are truncated"))?
        .to_vec();
    let inputs = read_u32(payload, root + 48)?;
    let fixed = matches!(code.as_slice(), [0x34, _, 0x3E, 0]);
    let drive = matches!(code.as_slice(), [0x34, _, 0x3C, 1, 1, 0x3E, 0]);
    if code_class != CODE
        || constant_class != CONSTANT
        || constant_count == 0
        || constant_count >= 256
        || !(fixed || drive)
        || usize::from(code[1]) >= constant_count
        || inputs != if fixed { 1 } else { 2 }
        || read_u32(payload, root + 52)? != 0
        || read_u32(payload, root + 56)? != 1
        || read_u32(payload, root + 60)? != 0
    {
        return Err(invalid(
            "Driving Speed supports constant or Engine-additive hover motion programs with one output",
        ));
    }
    let channel_count = crate::tag_payload::read_u64(payload, root + 64)? as usize;
    if channel_count != usize::from(drive) {
        return Err(invalid(
            "Driving Speed motion inputs do not match their provider channels",
        ));
    }
    if drive {
        let (_, _, rows, class) = array_at(payload, root + 64)?;
        if class != CHANNEL || payload.get(rows..rows.saturating_add(40)).is_none() {
            return Err(invalid(
                "Driving Speed found an unsupported provider channel",
            ));
        }
    }
    if constants.chunks_exact(4).any(|lane| {
        !f32::from_le_bytes(lane.try_into().expect("four-byte constant lane")).is_finite()
    }) {
        return Err(invalid("Driving Speed requires finite motion constants"));
    }
    // Multiply the full result, retaining the Engine input and both original metadata and
    // channel descriptors. The polynomial fast path is explicitly rejected above.
    let mut edited_code = code[..code.len() - 2].to_vec();
    edited_code.extend_from_slice(&[0x34, constant_count as u8, 3, 0x3E, 0]);
    let mut edited_constants = constants;
    for _ in 0..4 {
        edited_constants.extend_from_slice(&(f32::from(percent) / 100.0).to_le_bytes());
    }
    append_native_array(payload, root + 16, CODE, edited_code.len(), &edited_code)?;
    append_native_array(
        payload,
        root + 32,
        CONSTANT,
        constant_count + 1,
        &edited_constants,
    )
}

pub(super) fn scale(
    payload: &mut Vec<u8>,
    owner: u32,
    roots: impl IntoIterator<Item = usize>,
    percent: u16,
) -> AuthoringResult<()> {
    for root in roots {
        // Class 0x80803171 has the two checked definition-side expressions at these offsets.
        expression(payload, owner, root + 0x210, percent)?;
        expression(payload, owner, root + 0x270, percent)?;
    }
    let size = payload.len() as u64;
    write_u64(payload, 0, size)
}
