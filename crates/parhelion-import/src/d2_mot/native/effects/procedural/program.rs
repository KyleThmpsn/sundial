//! Numeric channel sequencers share arithmetic with TFX, but their input and
//! output instructions have one-byte ordinals instead of renderer externs.
use anyhow::{Context, Result, ensure};

pub(super) fn lower(code: &[u8], constants: usize, inputs: usize) -> Result<Vec<u8>> {
    ensure!(
        constants <= 256 && inputs <= 64,
        "sequencer tables exceed byte indices"
    );
    let mut at = 0;
    let mut stack = 0usize;
    let mut output = false;
    let mut result = Vec::new();
    while at < code.len() {
        let op = code[at];
        if matches!(op, 0x4A | 0x4C) {
            let index = *code.get(at + 1).context("truncated sequencer ordinal")?;
            if op == 0x4A {
                ensure!((index as usize) < inputs, "sequencer input outside table");
                stack += 1;
                result.extend([0x3C, index]);
            } else {
                ensure!(
                    index == 0 && stack > 0 && !output,
                    "sequencer output differs"
                );
                output = true;
                stack -= 1;
                result.extend([0x3E, index]);
            }
            at += 2;
            continue;
        }
        ensure!(
            matches!(op, 1..=15 | 0x13..=0x23 | 0x28..=0x2A | 0x2E..=0x32 | 0x35 | 0x42..=0x49),
            "unsupported sequencer opcode {op:02X}"
        );
        let length = if matches!(op, 0x29 | 0x42..=0x49) {
            2
        } else {
            1
        };
        let bytes = code
            .get(at..at + length)
            .context("truncated sequencer instruction")?;
        let instructions = crate::d2_mot::tfx::program::parse(bytes)?;
        let instruction = &instructions[0];
        if (0x42..=0x49).contains(&op) {
            let width = [1, 2, 2, 5, 10, 10, 6, 11][(op - 0x42) as usize];
            ensure!(
                instruction.args[0] as usize + width <= constants,
                "sequencer constants outside table"
            );
        }
        ensure!(stack >= instruction.arity, "sequencer stack underflow");
        stack = stack - instruction.arity + 1;
        ensure!(stack <= 64, "sequencer stack exceeds capacity");
        result.push(instruction.native);
        result.extend(instruction.args);
        at += length;
    }
    ensure!(
        output && stack == 0,
        "sequencer leaves an incomplete result"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constants_inputs_and_splines_retain_their_native_equations() {
        assert_eq!(
            lower(&[0x42, 0, 0x4C, 0], 1, 0).unwrap(),
            [0x34, 0, 0x3E, 0]
        );
        assert_eq!(
            lower(&[0x4A, 0, 0x4C, 0], 0, 1).unwrap(),
            [0x3C, 0, 0x3E, 0]
        );
        let source = hex::decode("4a004a010329002a2a290045002a29004c00").unwrap();
        assert_eq!(
            hex::encode(lower(&source, 5, 2).unwrap()),
            "3c003c010322002323220037002322003e00"
        );
        assert!(lower(&source, 4, 2).is_err());
        assert!(lower(&source, 5, 1).is_err());
        for bad in [
            &[0x4A][..],
            &[0x4A, 0],
            &[0x4A, 0, 0x4C, 1],
            &[0x03, 0x4C, 0],
            &[0x4D, 0, 0],
            &[0x42, 0, 0x4C, 0, 0x4C, 0],
        ] {
            assert!(lower(bad, 1, 1).is_err());
        }
    }
}
