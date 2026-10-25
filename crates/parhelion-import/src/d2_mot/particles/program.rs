use super::*;

pub(super) struct Instruction<'a> {
    pub op: u8,
    pub args: &'a [u8],
}

pub(super) fn parse(code: &[u8]) -> Result<Vec<Instruction<'_>>> {
    let mut at = 0;
    let mut result = Vec::new();
    while at < code.len() {
        let op = code[at];
        let size = match op {
            0x4C | 0x4D => 3, // Register bank, vector slot and selector.
            0x55 => 4,        // A named object channel, unlike material PopTemp.
            0x29 | 0x42..=0x49 | 0x4E..=0x54 => 1,
            0x01..=0x23
            | 0x28
            | 0x2A..=0x35
            | 0x3B..=0x41
            | 0x4B
            | 0x57
            | 0x58
            | 0x5A
            | 0x5B
            | 0x5D
            | 0x5E
            | 0x5F
            | 0x60 => 0,
            _ => bail!("unknown particle opcode {op:02X} at {at}"),
        };
        let end = at + 1 + size;
        let args = code
            .get(at + 1..end)
            .context("particle instruction crosses phase boundary")?;
        result.push(Instruction { op, args });
        at = end;
    }
    Ok(result)
}

pub(super) fn lower(
    code: &[u8],
    constants: usize,
    defaults: usize,
    channels: &BTreeMap<u32, u8>,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut stack = 0usize;
    for i in parse(code)? {
        let (native, consumed, produced) = match i.op {
            // Preview VM and material dialect disagree on 05/06 arity.
            // Neither is present in the source corpus. Require engine evidence.
            7 => (i.op, 1, 1),
            1..=4 | 8..=15 => (i.op, 2, 1),
            0x13..=0x16 => (i.op - 3, 3, 1),
            0x18..=0x23 => (i.op - 3, 1, 1),
            0x28 => (0x21, 1, 1),
            0x29 => (0x22, 1, 1),
            0x2A => (0x23, 1, 1),
            0x2D => (0x26, 1, 1), // Four-lane length.
            0x2E..=0x32 => (i.op - 7, 1, 1),
            0x33 => (0x2C, 2, 1), // Axis-angle rotation, angle in turns.
            0x35 => (0x2E, 5, 1),
            0x42..=0x49 => {
                let width = [1, 2, 2, 5, 10, 10, 6, 11][usize::from(i.op - 0x42)];
                ensure!(
                    usize::from(i.args[0]) + width <= constants,
                    "particle constant curve exceeds table"
                );
                (
                    i.op - 14,
                    if i.op == 0x42 {
                        0
                    } else if i.op == 0x47 {
                        2
                    } else {
                        1
                    },
                    1,
                )
            }
            0x4B => (0x3D, 0, 1), // One advance of the owning runtime's random seed.
            0x4C | 0x4D => {
                let [bank, slot, selector]: [u8; 3] = i.args.try_into()?;
                ensure!(
                    bank <= 6 && selector <= 7,
                    "particle register dialect differs"
                );
                ensure!(
                    if bank == 6 {
                        usize::from(slot) < defaults
                    } else {
                        slot < 64
                    },
                    "particle register exceeds bank"
                );
                ensure!(
                    i.op != 0x4D || bank != 6,
                    "particle program writes immutable defaults"
                );
                (
                    i.op - 14,
                    usize::from(i.op == 0x4D),
                    usize::from(i.op == 0x4C),
                )
            }
            0x51 => {
                // This is a scalar input index, independent of the 56 output routes.
                (0x43, 0, 1)
            }
            // Indexed values from the external runtime at context +0x28.
            // The owning emitter still has to provide this input family.
            0x54 => (0x46, 0, 1),
            // Particle transform operations do not follow the material dialect's
            // offset. Native 49 reads a translation/quaternion pair from a runtime
            // transform index. 4C adds a world offset, 4D first rotates the offset
            // by the pair's quaternion, and 40 stores the pair. Corresponding
            // shipped programs establish source 57 -> native 4C and 58 -> 4D.
            // Identity rotations cannot distinguish these two operations.
            0x5E => (0x49, 1, 2),
            0x57 => (0x4C, 3, 2),
            0x58 => (0x4D, 3, 2),
            0x4E => (0x40, 2, 0),
            // Transform operations present in corresponding shipped programs.
            // Both consume two transform pairs and two parameter vectors. The
            // native handlers retain one transform pair, a four-vector drop.
            0x5A => (0x4F, 6, 2),
            0x5D => (0x52, 6, 2),
            0x55 => {
                let name = u32::from_be_bytes(i.args.try_into()?);
                let slot = channels
                    .get(&name)
                    .with_context(|| format!("particle channel {name:08X} is not bound"))?;
                result.extend([0x47, *slot]);
                stack += 1;
                ensure!(stack <= 64, "particle stack exceeds capacity");
                continue;
            }
            _ => bail!(
                "particle opcode {:02X} needs a validated native operation",
                i.op
            ),
        };
        ensure!(
            stack >= consumed,
            "particle stack underflow at opcode {:02X}",
            i.op
        );
        stack = stack - consumed + produced;
        ensure!(stack <= 64, "particle stack exceeds capacity");
        result.push(native);
        result.extend(i.args);
    }
    ensure!(stack == 0, "particle phase leaves an unfinished result");
    Ok(result)
}
