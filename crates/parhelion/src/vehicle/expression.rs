//! Multiply a checked native numeric expression's output, retaining its inputs and curves.
use crate::{
    AuthoringResult,
    error::invalid,
    tag_payload::{append_native_array, array_at, read_u32, read_u64},
};
use sundial::package_authoring::sandbox_perk::action::native::value::{Instruction, Program};

pub(super) fn scale(
    payload: &mut Vec<u8>,
    owner: u32,
    root: usize,
    factor: f32,
    label: &str,
) -> AuthoringResult<()> {
    let context = |error| invalid(format!("{label}: {error}"));
    if read_u32(payload, root)? != owner || read_u32(payload, root + 4)? != 0x8080_89F5 {
        return Err(context("unsupported numeric expression reference"));
    }
    let (count, _, code_at, class) = array_at(payload, root + 16)?;
    let (constant_count, _, constants_at, constants_class) = array_at(payload, root + 32)?;
    let code = payload
        .get(
            code_at
                ..code_at
                    .checked_add(count)
                    .ok_or_else(|| context("bytecode size overflow"))?,
        )
        .ok_or_else(|| context("truncated bytecode"))?
        .to_vec();
    let constants = payload
        .get(
            constants_at
                ..constants_at
                    .checked_add(
                        constant_count
                            .checked_mul(16)
                            .ok_or_else(|| context("constant size overflow"))?,
                    )
                    .ok_or_else(|| context("constant range overflow"))?,
        )
        .ok_or_else(|| context("truncated constants"))?
        .to_vec();
    let inputs = read_u32(payload, root + 48)?;
    let channels = usize::try_from(read_u64(payload, root + 64)?)
        .map_err(|_| context("provider count overflow"))?;
    if class != 0x8080_0009
        || constants_class != 0x8080_0090
        || constant_count == 0
        || constant_count >= 256
        || !(1..=32).contains(&inputs)
        || channels != inputs.saturating_sub(1) as usize
        || read_u32(payload, root + 52)? != 0
        || read_u32(payload, root + 56)? != 1
        || read_u32(payload, root + 60)? != 0
        || !code.ends_with(&[0x3E, 0])
        || !factor.is_finite()
        || factor <= 0.0
    {
        return Err(context(
            "requires a finite one-output numeric program without a compiled fast path",
        ));
    }
    if channels != 0 {
        let (_, _, at, class) = array_at(payload, root + 64)?;
        if class != 0x8080_9789
            || payload
                .get(
                    at..at
                        .checked_add(
                            channels
                                .checked_mul(40)
                                .ok_or_else(|| context("provider size overflow"))?,
                        )
                        .ok_or_else(|| context("provider range overflow"))?,
                )
                .is_none()
        {
            return Err(context("unsupported provider channels"));
        }
    }
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let operand = if opcode == 34 || opcode >= 52 {
            let value = *code
                .get(at)
                .ok_or_else(|| context("truncated instruction operand"))?;
            at += 1;
            Some(value)
        } else {
            None
        };
        if opcode == 60 && operand.is_none_or(|index| u32::from(index) >= inputs) {
            return Err(context("input index exceeds metadata"));
        }
        if opcode == 62 && (operand != Some(0) || at != code.len()) {
            return Err(context("requires one final output store"));
        }
        // Reuse the native stack and constant-span validator. Only its perk-specific input
        // identity restriction is normalized here, after checking the real input above.
        instructions.push(Instruction {
            opcode,
            operand: if opcode == 60 { Some(0) } else { operand },
        });
    }
    let constants_lanes = constants
        .chunks_exact(16)
        .map(|row| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("four-byte lane"))
            })
        })
        .collect::<Vec<_>>();
    if constants_lanes
        .iter()
        .flatten()
        .any(|bits| !f32::from_bits(*bits).is_finite())
    {
        return Err(context("non-finite constant"));
    }
    let mut program = Program {
        instructions,
        constants: constants_lanes,
        fast_path: 0,
    };
    program
        .validate()
        .map_err(|e| invalid(format!("{label}: {e}")))?;
    program.instructions.pop();
    program.instructions.extend([
        Instruction {
            opcode: 52,
            operand: Some(constant_count as u8),
        },
        Instruction {
            opcode: 3,
            operand: None,
        },
        Instruction {
            opcode: 62,
            operand: Some(0),
        },
    ]);
    program.constants.push([factor.to_bits(); 4]);
    program
        .validate()
        .map_err(|e| invalid(format!("{label}: {e}")))?;
    let mut edited = code[..code.len() - 2].to_vec();
    edited.extend_from_slice(&[0x34, constant_count as u8, 3, 0x3E, 0]);
    let mut scaled_constants = constants;
    for _ in 0..4 {
        scaled_constants.extend_from_slice(&factor.to_le_bytes());
    }
    append_native_array(payload, root + 16, 0x8080_0009, edited.len(), &edited)?;
    append_native_array(
        payload,
        root + 32,
        0x8080_0090,
        constant_count + 1,
        &scaled_constants,
    )
}
