//! Modern emission RGB and enable share one vector. Native gear shaders read the
//! enable from a separate vector, so both initial values and live stores must move.
use super::{Payload, append, array};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn translate(scope: &mut Vec<u8>) -> Result<()> {
    let original = Payload(scope.clone());
    let rows = original.array(0x88, 16, Some(0x80800090))?;
    ensure!(rows.len() == 27, "Imported emission vectors differ");
    let code = array(&original, 0x58, 1, 0x80800009)?;
    ensure!(
        code.len() <= 4096,
        "Imported emission program exceeds the native budget"
    );
    let instructions = instructions(&code)?;
    let used = instructions
        .iter()
        .filter(|i| matches!(i[0], 0x45 | 0x46))
        .map(|i| i[1])
        .collect::<BTreeSet<_>>();
    let targets = instructions
        .iter()
        .filter(|i| i[0] == 0x43 && matches!(i[1], 3 | 4))
        .map(|i| i[1])
        .collect::<BTreeSet<_>>();
    // Validate all allocations before changing the caller's private payload.
    let mut constants = array(&original, 0x68, 16, 0x80800090)?;
    let mut translated = Vec::new();
    if !targets.is_empty() {
        let scratch = (0..16u8)
            .find(|i| !used.contains(i))
            .context("No free emission temporary")?;
        let count = constants.len() / 16;
        ensure!(
            count + 1 + targets.len() <= 256,
            "Emission translation exceeds 256 constants"
        );
        let mask = count as u8;
        constants.extend(
            [0.0f32, 1.0, 0.0, 0.0]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        let mut bases = BTreeMap::new();
        for &target in &targets {
            bases.insert(target, (constants.len() / 16) as u8);
            let row = rows[usize::from(target) + 22];
            let mut base = original.0[row..row + 16].to_vec();
            base[4..8].fill(0);
            constants.extend(base);
        }
        for instruction in instructions {
            if instruction[0] == 0x43 && targets.contains(&instruction[1]) {
                let target = instruction[1];
                // Retain the complete source output, then copy its W component
                // to the native selector's Y lane without changing X, Z or W.
                translated.extend([
                    0x46,
                    scratch,
                    0x45,
                    scratch,
                    0x43,
                    target,
                    0x45,
                    scratch,
                    0x22,
                    0xFF,
                    0x34,
                    mask,
                    0x03,
                    0x34,
                    bases[&target],
                    0x01,
                    0x43,
                    target + 22,
                ]);
            } else {
                translated.extend_from_slice(instruction);
            }
        }
        ensure!(
            translated.len() <= 4096,
            "Emission translation exceeds the native program budget"
        );
    }
    for (color, selector) in [(3, 25), (4, 26)] {
        let value = original.bytes::<4>(rows[color] + 12)?;
        scope[rows[selector] + 4..rows[selector] + 8].copy_from_slice(&value);
    }
    if !targets.is_empty() {
        append(scope, 0x68, &constants, 16, 0x80800090)?;
        append(scope, 0x58, &translated, 1, 0x80800009)?;
    }
    Ok(())
}

fn instructions(code: &[u8]) -> Result<Vec<&[u8]>> {
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let op = code[at];
        let size = match op {
            1..=0x21 | 0x23..=0x33 => 0,
            0x22 | 0x34..=0x3B | 0x42..=0x4E => 1,
            0x3C..=0x41 | 0x51..=0x53 => 2,
            _ => anyhow::bail!("Unsupported native emission instruction 0x{op:02X}"),
        };
        let instruction = code
            .get(at..at + 1 + size)
            .context("Truncated emission instruction")?;
        if matches!(op, 0x45 | 0x46) {
            ensure!(
                instruction[1] < 16,
                "Imported emission temporary is out of range"
            );
        }
        if op == 0x43 {
            ensure!(
                instruction[1] < 25,
                "Imported emission already writes native-only vectors"
            );
        }
        instructions.push(instruction);
        at += instruction.len();
    }
    Ok(instructions)
}
