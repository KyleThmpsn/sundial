//! Expressions embedded in sequence transforms, independent of renderer TFX.
use super::controls::{array, put};
use crate::d2_mot::{payload::Payload, tfx::program};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

/// Numeric sequence expressions retain their local builtin inputs and remap
/// every additional input through the containing sequencer's slot table.
pub(super) fn bound(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    builtins: usize,
    inputs: &BTreeMap<u32, u32>,
) -> Result<()> {
    if source.bytes::<64>(from)? == [0; 64] {
        return put(output, to, &[0; 64]);
    }
    let mut slots = Vec::new();
    for row in source.array(from + 48, 4, Some(0x80800007))? {
        let index = source.u32(row)?;
        slots.extend(
            inputs
                .get(&index)
                .with_context(|| format!("unmapped sequence expression input {index}"))?
                .to_le_bytes(),
        );
    }
    let count = builtins
        .checked_add(slots.len() / 4)
        .context("sequence expression input capacity")?;
    ensure!(
        count <= 64 && source.u64(from + 32)? == count as u64 && source.u64(from + 40)? == 1,
        "sequence expression input contract differs"
    );
    program(source, from, output, to, count)?;
    array(output, to + 48, 0x80800007, &slots, 4)?;
    Ok(())
}

/// The query's two distance functions carry raw 48-byte numeric programs.
pub(super) fn program(
    source: &Payload,
    from: usize,
    output: &mut Payload,
    to: usize,
    inputs: usize,
) -> Result<()> {
    let code = source
        .array(from, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| source.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let rows = source.array(from + 16, 16, Some(0x80800090))?;
    let mut constants = Vec::new();
    for row in rows {
        for lane in 0..4 {
            ensure!(
                source.f32(row + lane * 4)?.is_finite(),
                "nonfinite sequence expression constant"
            );
        }
        constants.extend(source.bytes::<16>(row)?);
    }
    ensure!(
        source.u64(from + 32)? == inputs as u64 && source.u64(from + 40)? == 1,
        "raw sequence expression dimensions differ"
    );
    let code = lower(&code, constants.len() / 16, inputs)?;
    array(output, to, 0x80800009, &code, 1)?;
    array(output, to + 16, 0x80800090, &constants, 16)?;
    put(output, to + 32, &(inputs as u64).to_le_bytes())?;
    put(output, to + 40, &1u64.to_le_bytes())?;
    Ok(())
}

fn lower(code: &[u8], constants: usize, inputs: usize) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut cursor = 0;
    let mut depth = 0usize;
    let mut written = false;
    while cursor < code.len() {
        let op = code[cursor];
        if matches!(op, 0x4A | 0x4C) {
            let index = *code
                .get(cursor + 1)
                .context("truncated sequence expression ordinal")?;
            if op == 0x4A {
                ensure!(
                    (index as usize) < inputs,
                    "sequence expression input outside table"
                );
                depth += 1;
                result.extend([0x3C, index]);
            } else {
                ensure!(
                    index == 0 && depth == 1 && !written,
                    "sequence expression result differs"
                );
                written = true;
                depth = 0;
                result.extend([0x3E, index]);
            }
            cursor += 2;
            continue;
        }
        ensure!(!written, "sequence expression continues after its result");
        ensure!(
            matches!(op, 1..=15 | 0x13..=0x23 | 0x28..=0x2A | 0x2E..=0x32 | 0x35 | 0x42..=0x49),
            "unsupported sequence expression opcode {op:02X}"
        );
        let size = if matches!(op, 0x29 | 0x42..=0x49) {
            2
        } else {
            1
        };
        let bytes = code
            .get(cursor..cursor + size)
            .context("truncated sequence expression")?;
        let decoded = program::parse(bytes)?;
        let instruction = &decoded[0];
        if (0x42..=0x49).contains(&op) {
            let width = [1, 2, 2, 5, 10, 10, 6, 11][(op - 0x42) as usize];
            ensure!(
                instruction.args[0] as usize + width <= constants,
                "sequence expression constant outside table"
            );
        }
        ensure!(
            depth >= instruction.arity,
            "sequence expression stack underflow"
        );
        depth = depth - instruction.arity + 1;
        ensure!(depth <= 64, "sequence expression stack exceeds capacity");
        result.push(instruction.native);
        result.extend_from_slice(instruction.args);
        cursor += size;
    }
    ensure!(
        written && depth == 0,
        "incomplete sequence expression result"
    );
    Ok(result)
}

pub(super) fn write(source: &Payload, from: usize, output: &mut Payload, to: usize) -> Result<()> {
    let code = source
        .array(from, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| source.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let constants = source
        .array(from + 16, 16, Some(0x80800090))?
        .into_iter()
        .map(|at| source.bytes::<16>(at))
        .collect::<Result<Vec<_>>>()?;
    // The inspected transform expressions use a single constant and one result.
    // Do not interpret the two capacity words as channel counts or reuse the
    // channel-bank VM's dependency namespace for these embedded expressions.
    ensure!(
        code == [0x42, 0, 0x4C, 0]
            && constants.len() == 1
            && source.u64(from + 32)? == 1
            && source.u64(from + 40)? == 1,
        "sequence transform expression requires an additional native contract"
    );
    // The expression occupies 64 bytes. Translation and rotation slots have
    // eight additional bytes before the next slot, but the final scale slot
    // is immediately followed by the event's identity. Reading 24 bytes here
    // incorrectly interprets that identity as expression state.
    ensure!(
        source.bytes::<16>(from + 48)? == [0; 16],
        "unsupported sequence expression extension"
    );
    let code = lower(&code, constants.len(), 0)?;
    array(output, to, 0x80800009, &code, 1)?;
    let bytes: Vec<_> = constants.into_iter().flatten().collect();
    array(output, to + 16, 0x80800090, &bytes, 16)?;
    put(output, to + 32, &1u64.to_le_bytes())?;
    put(output, to + 40, &1u64.to_le_bytes())?;
    Ok(())
}
