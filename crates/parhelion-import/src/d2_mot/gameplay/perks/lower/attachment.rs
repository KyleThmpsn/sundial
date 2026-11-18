//! Attachment action declarations retain cleanup and creation parameters separately.
use crate::d2_mot::{gameplay::perks::table, payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

const EMPTY: u32 = 0x811C9DC5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleanup {
    Automatic,
    UnlessParentIsHost,
    Independent,
}

pub struct Attachment {
    pub graph: u32,
    pub retained: bool,
    pub target: u8,
    pub cleanup: Cleanup,
    pub removal_name: u32,
    pub removal_value: f32,
    pub stacks_name: u32,
    pub parameters: BTreeMap<u32, [u8; 16]>,
}

/// The value in every lane of the creation parameter row at `row`, as the entity receives it
/// when nothing changes what its creator publishes.
///
/// Slot 0 of a row's program is the action input and each later slot one named value of the
/// creator, in the order of the row's name list. Shadowkeep's spawn and attachment actions hold
/// no parameter rows, so a live value cannot follow the entity. A named value instead reads as
/// its resting value from [`table::resting`], the value the modern publisher holds with no
/// modifier applied, and the row folds to a constant. A name with no established resting
/// value refuses the row.
///
/// A bound action input is the creating action's own value, a count the perk keeps. A row that
/// reads it folds only where its program saturates, giving one result for any large count, and
/// takes that result: the value at a full count. Any other read of a bound input refuses the row.
fn value(source: &Payload, row: usize, name: u32) -> Result<[f32; 4]> {
    if source.u32(row + 80)? == 255 {
        return evaluate(source, row, name, None);
    }
    let full = [1.0e6, 1.0e12].map(|count| evaluate(source, row, name, Some(count)));
    match full {
        [Ok(low), Ok(high)]
            if low.iter().all(|lane| lane.is_finite())
                && low.map(f32::to_bits) == high.map(f32::to_bits) =>
        {
            Ok(low)
        }
        _ => anyhow::bail!("creation parameter {name:08X} reads the action input"),
    }
}

/// How many creation parameter rows at `field` read a bound action input, which
/// [`parameters`] fixes at its full count.
pub fn bound(source: &Payload, field: usize) -> Result<usize> {
    let mut count = 0;
    for row in source.array(field, 96, Some(0x808022E4))? {
        let code = source
            .array(row + 16, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| source.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let mut at = 0;
        let mut reads = false;
        while at < code.len() {
            reads |= code[at] == 0x4A && code.get(at + 1) == Some(&0);
            at += if matches!(code[at], 0x29 | 0x42..=0x4A | 0x4C) {
                2
            } else {
                1
            };
        }
        count += usize::from(reads && source.u32(row + 80)? != 255);
    }
    Ok(count)
}

/// The row's program with `count` as its bound action input, or with none allowed.
fn evaluate(source: &Payload, row: usize, name: u32, count: Option<f32>) -> Result<[f32; 4]> {
    let program = row + 16;
    let code = source
        .array(program, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| source.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let constants = source.array(program + 16, 16, Some(0x80800090))?;
    let names = source.array(program + 48, 4, Some(0x80800070))?;
    ensure!(
        source.u64(program + 32)? == 1 + names.len() as u64
            && source.u64(program + 40)? == 1
            && source.u32(row + 84)? == EMPTY
            && source.u32(row + 92)? == 0,
        "attachment parameter expression shape differs"
    );
    let lanes = |at: usize| -> Result<[f32; 4]> {
        Ok([
            source.f32(at)?,
            source.f32(at + 4)?,
            source.f32(at + 8)?,
            source.f32(at + 12)?,
        ])
    };
    let slot = |index: u8| -> Result<[f32; 4]> {
        if index == 0 {
            // An unbound input, selector 255, reads the row's own number at `+58`. Rows that
            // read slot 0 unbound hold the only nonzero values found there.
            return match count {
                Some(count) => Ok([count; 4]),
                None => Ok([source.f32(row + 88)?; 4]),
            };
        }
        let read = source.u32(
            *names
                .get(usize::from(index) - 1)
                .context("creation parameter reads a name outside its list")?,
        )?;
        table::resting(read)?.with_context(|| {
            format!(
                "creation parameter {name:08X} passes on its creator's value {read:08X}, which has no established resting value"
            )
        })
    };
    let mut stack = Vec::<[f32; 4]>::new();
    let mut result = None;
    let mut at = 0;
    while at < code.len() {
        let op = code[at];
        let operand = || {
            code.get(at + 1)
                .copied()
                .context("creation parameter expression is truncated")
        };
        let mut pop = || {
            stack
                .pop()
                .context("creation parameter expression stack underflow")
        };
        let (length, pushed) = match op {
            0x42 => {
                let constant = *constants
                    .get(usize::from(operand()?))
                    .context("creation parameter constant is outside its table")?;
                (2, Some(lanes(constant)?))
            }
            0x4A => (2, Some(slot(operand()?)?)),
            0x4C => {
                ensure!(
                    operand()? == 0 && result.is_none(),
                    "creation parameter expression output differs"
                );
                result = Some(pop()?);
                (2, None)
            }
            // The arithmetic both runtimes share: add, subtract, multiply, minimum, maximum.
            0x01..=0x03 | 0x05 | 0x06 | 0x08 | 0x09 => {
                let right = pop()?;
                let left = pop()?;
                (
                    1,
                    Some(std::array::from_fn(|lane| {
                        let (a, b) = (left[lane], right[lane]);
                        match op {
                            0x01 | 0x06 => a + b,
                            0x02 => a - b,
                            0x03 | 0x05 => a * b,
                            0x08 => a.min(b),
                            _ => a.max(b),
                        }
                    })),
                )
            }
            // Source 13 to 16 are native 10 to 13: blend, clamped blend, multiply and add, and
            // clamp, their operands in the order they were pushed.
            0x13..=0x16 => {
                let c = pop()?;
                let b = pop()?;
                let a = pop()?;
                (
                    1,
                    Some(std::array::from_fn(|lane| {
                        let (a, b, c) = (a[lane], b[lane], c[lane]);
                        match op {
                            0x13 => a + c * (b - a),
                            0x14 => (a + c * (b - a)).clamp(0.0, 1.0),
                            0x15 => a * b + c,
                            _ => a.max(b).min(c),
                        }
                    })),
                )
            }
            0x29 => {
                let bits = operand()?;
                let value = pop()?;
                (
                    2,
                    Some(std::array::from_fn(|lane| {
                        value[usize::from((bits >> (6 - lane * 2)) & 3)]
                    })),
                )
            }
            0x2A => (1, Some(pop()?.map(|lane| lane.clamp(0.0, 1.0)))),
            // Source 45 is native 37, the four-segment cubic: four coefficient vectors with
            // one segment in each lane, then the knots. The last knot at or below the input
            // selects the segment, and an input below the first knot gives zero.
            0x45 => {
                let first = usize::from(operand()?);
                let rows = constants
                    .get(first..first + 5)
                    .context("creation parameter curve is outside its constants")?;
                let input = pop()?;
                let knots = lanes(rows[4])?;
                ensure!(
                    input
                        .iter()
                        .all(|lane| lane.to_bits() == input[0].to_bits())
                        && knots.windows(2).all(|pair| pair[0] <= pair[1]),
                    "creation parameter curve has differing input lanes or unordered knots"
                );
                let t = input[0];
                let value = match knots.iter().rposition(|knot| *knot <= t) {
                    Some(segment) => {
                        let [a, b, c, d] = [
                            lanes(rows[0])?[segment],
                            lanes(rows[1])?[segment],
                            lanes(rows[2])?[segment],
                            lanes(rows[3])?[segment],
                        ];
                        (a * t + b) * (t * t) + (c * t + d)
                    }
                    None => 0.0,
                };
                (2, Some([value; 4]))
            }
            _ => anyhow::bail!("creation parameter {name:08X} computes with operation {op:02X}"),
        };
        stack.extend(pushed);
        ensure!(
            stack.len() <= 64,
            "creation parameter expression exceeds its stack"
        );
        at += length;
    }
    ensure!(
        stack.is_empty(),
        "creation parameter expression leaves an incomplete result"
    );
    result.context("creation parameter expression has no output")
}

/// Read creation parameters from the row array at `field`.
/// Each value must be a constant or a program that folds to the same finite scalar in all four lanes.
pub fn parameters(source: &Payload, field: usize) -> Result<BTreeMap<u32, [u8; 16]>> {
    let mut parameters = BTreeMap::new();
    for row in source.array(field, 96, Some(0x808022E4))? {
        let name = source.u32(row)?;
        ensure!(
            name != EMPTY && source.u32(row + 4)? == 0 && source.u64(row + 8)? == 0,
            "attachment parameter has a binding or invalid name"
        );
        let value = value(source, row, name)?;
        ensure!(
            value
                .iter()
                .all(|lane| lane.is_finite() && lane.to_bits() == value[0].to_bits()),
            "attachment creation parameter is not a scalar broadcast"
        );
        let mut constant = [0; 16];
        for lane in constant.chunks_exact_mut(4) {
            lane.copy_from_slice(&value[0].to_le_bytes());
        }
        ensure!(
            parameters.insert(name, constant).is_none(),
            "attachment creation parameter is duplicated"
        );
    }
    Ok(parameters)
}

/// Read a complete action before deciding whether the host can express its
/// parent-sensitive lifetime. A nonempty native lifetime key is not this enum.
pub fn read(reader: &Reader, source: &Payload, at: usize) -> Result<Attachment> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080B7C6,
        "attachment action class differs"
    );
    let header = source.bytes::<8>(at)?;
    ensure!(
        header[0] == 1
            && header[1] <= 1
            && header[2] <= 3
            && header[3..].iter().all(|byte| *byte == 0),
        "attachment action header differs"
    );
    // The debug name is a relative string, not a resource reference.
    if source.u64(at + 8)? != 0 {
        let string = source.pointer(at + 8)?;
        let tail = source
            .0
            .get(string..)
            .context("attachment debug string extent")?;
        let length = tail
            .iter()
            .position(|byte| *byte == 0)
            .context("attachment debug string is unterminated")?;
        std::str::from_utf8(&tail[..length])?;
    }
    let graph = reader.ref64(source, at + 16)?;
    ensure!(
        reader.reference(graph)? == 0x80809AD8,
        "attachment action does not reference an entity graph"
    );
    let cleanup = match source.u32(at + 32)? {
        0 => Cleanup::Automatic,
        1 => Cleanup::UnlessParentIsHost,
        2 => Cleanup::Independent,
        value => anyhow::bail!("unsupported attachment cleanup mode {value}"),
    };
    let removal_name = source.u32(at + 36)?;
    let removal_value = source.f32(at + 40)?;
    ensure!(
        removal_value.is_finite() && (removal_name != EMPTY || removal_value == 0.0),
        "unnamed attachment removal has a value"
    );
    let parameters = parameters(source, at + 48)?;
    Ok(Attachment {
        graph,
        retained: header[1] != 0,
        target: header[2],
        cleanup,
        removal_name,
        removal_value,
        stacks_name: source.u32(at + 44)?,
        parameters,
    })
}
