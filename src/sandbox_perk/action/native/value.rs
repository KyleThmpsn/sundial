//! Native four-lane value programs, their operands, constants and execution bounds.
use super::*;
use crate::package_payload::u32_at;

pub const CLASS: u32 = 0x80808E76;

pub fn validate(graph: &Graph, block: usize, offset: usize) -> Result<(), String> {
    Program::read(graph, block, offset)?.validate()?;
    let bytes = &graph.blocks[block].bytes;
    let counts = [
        u32_at(bytes, offset + 32)?,
        u32_at(bytes, offset + 36)?,
        u32_at(bytes, offset + 40)?,
    ];
    if counts != [1, 0, 1] {
        return Err(
            "The native scalar value program has unsupported input or output metadata.".into(),
        );
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instruction {
    pub opcode: u8,
    pub operand: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Program {
    pub instructions: Vec<Instruction>,
    pub constants: Vec<[u32; 4]>,
    pub fast_path: u32,
}

impl Program {
    pub fn read(graph: &Graph, block: usize, offset: usize) -> Result<Self, String> {
        let owner = graph
            .blocks
            .get(block)
            .ok_or("Missing value-program owner.")?;
        let code = array(graph, owner, offset, 0x80800009)?;
        let constants = array(graph, owner, offset + 16, 0x80800090)?
            .chunks_exact(16)
            .map(|row| {
                std::array::from_fn(|i| {
                    u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("four-byte lane"))
                })
            })
            .collect();
        let mut instructions = Vec::new();
        let mut at = 0;
        while at < code.len() {
            let opcode = code[at];
            at += 1;
            if opcode > 62 {
                return Err(format!("Unknown value instruction 0x{opcode:02X}."));
            }
            let operand = if opcode == 34 || opcode >= 52 {
                let value = *code.get(at).ok_or("Truncated value instruction operand.")?;
                at += 1;
                Some(value)
            } else {
                None
            };
            instructions.push(Instruction { opcode, operand });
        }
        Ok(Self {
            instructions,
            constants,
            fast_path: u32_at(&owner.bytes, offset + 44)?,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.constants.len() > 256 || self.instructions.len() > 4096 || self.fast_path > 1 {
            return Err("The value program exceeds its native limits.".into());
        }
        if self.fast_path == 1 && self.constants.is_empty() {
            return Err("The polynomial fast path needs a constant vector.".into());
        }
        let mut stack = 0usize;
        let mut output = false;
        let mut stopped = false;
        for instruction in &self.instructions {
            if stopped {
                return Err("Instructions after Stop would never execute.".into());
            }
            let op = instruction.opcode;
            if (op == 34 || op >= 52) != instruction.operand.is_some() {
                return Err("A value instruction has an invalid operand width.".into());
            }
            let span = match op {
                52 => 1,
                53 | 54 => 2,
                55 => 5,
                56 | 57 => 10,
                58 => 6,
                _ => 0,
            };
            if span > 0
                && instruction
                    .operand
                    .is_none_or(|index| usize::from(index) + span > self.constants.len())
            {
                return Err("A value instruction exceeds the constant table.".into());
            }
            let (required, removed, added) = match op {
                0 => {
                    stopped = true;
                    (0, 0, 0)
                }
                52 | 60 => (0, 0, 1),
                62 => {
                    output = true;
                    (1, 1, 0)
                }
                1..=6 | 8..=11 | 15 | 57 => (2, 2, 1),
                16..=20 => (3, 3, 1),
                7 | 21..=29 | 33..=35 | 53..=56 | 58 => (1, 1, 1),
                _ => {
                    return Err(format!(
                        "The stack contract for instruction 0x{op:02X} is not recovered."
                    ));
                }
            };
            if matches!(op, 60 | 62) && instruction.operand != Some(0) {
                return Err("Perk value programs have one input and one output.".into());
            }
            if stack < required {
                return Err("The value program would underflow its stack.".into());
            }
            stack = stack - removed + added;
            if stack > 64 {
                return Err("The value program exceeds its stack limit.".into());
            }
        }
        if stack != 0 || !output {
            return Err("The value program must store its output and leave an empty stack.".into());
        }
        Ok(())
    }

    /// Publish instructions, constants and the selected execution mode together.
    pub fn write(&self, graph: &mut Graph, block: usize, offset: usize) -> Result<(), String> {
        let original = Self::read(graph, block, offset)?;
        for (row, constant) in self.constants.iter().enumerate() {
            for (lane, bits) in constant.iter().enumerate() {
                if !f32::from_bits(*bits).is_finite()
                    && original.constants.get(row).map(|value| value[lane]) != Some(*bits)
                {
                    return Err("Edited value constants must be finite.".into());
                }
            }
        }
        let program = self.clone();
        program.validate()?;
        let mut changed = graph.clone();
        let code = program
            .instructions
            .iter()
            .flat_map(|i| std::iter::once(i.opcode).chain(i.operand))
            .collect::<Vec<_>>();
        let constants = program
            .constants
            .iter()
            .flatten()
            .flat_map(|bits| bits.to_le_bytes())
            .collect::<Vec<_>>();
        replace_array(&mut changed, block, offset, 0x80800009, code)?;
        replace_array(&mut changed, block, offset + 16, 0x80800090, constants)?;
        changed.blocks[block]
            .bytes
            .get_mut(offset + 32..offset + 48)
            .ok_or("Truncated value-program header.")?
            .copy_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        changed.blocks[block].bytes[offset + 44..offset + 48]
            .copy_from_slice(&program.fast_path.to_le_bytes());
        changed.validate()?;
        *graph = changed;
        Ok(())
    }

    /// The program's result for one input, as the scalar wrapper 3B9FE0 returns it: the input
    /// fills every lane of input 0 and the result is lane 0 of output 0. Covers the traced
    /// instructions whose stock results check out: Multikill Clip gives 1/6, 1/3 and 1/2 and
    /// Swashbuckler 1/15 a stack up to 1/3, the values both perks are known for, and stock
    /// piecewise cubics meet at their knots. `None` for anything else, such as the comparison
    /// at 0x0A, whose direction the perk and particle traces disagree on, or a result that is
    /// not a finite number.
    #[must_use]
    pub fn evaluate(&self, input: f32) -> Option<f32> {
        let rows = self
            .constants
            .iter()
            .map(|row| row.map(f32::from_bits))
            .collect::<Vec<_>>();
        if self.fast_path == 1 {
            // 3B66F0 clamps the input, evaluates the cubic in row 0 and clamps the result.
            let value = polynomial(*rows.first()?, input.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            return value.is_finite().then_some(value);
        }
        let mut stack = Vec::<[f32; 4]>::new();
        let mut output = None;
        for instruction in &self.instructions {
            let at = usize::from(instruction.operand.unwrap_or(0));
            let row = |offset: usize| rows.get(at + offset).copied();
            let value = match instruction.opcode {
                0 => break,
                1..=3 | 5 | 6 | 8 | 9 | 15 => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    match instruction.opcode {
                        1 | 6 => lanes(|i| a[i] + b[i]),
                        2 => lanes(|i| a[i] - b[i]),
                        3 | 5 => lanes(|i| a[i] * b[i]),
                        8 => lanes(|i| a[i].min(b[i])),
                        9 => lanes(|i| a[i].max(b[i])),
                        // The newer vector holds the coefficients and the older one the input.
                        _ => a.map(|time| polynomial(b, time)),
                    }
                }
                18 => {
                    let c = stack.pop()?;
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    lanes(|i| a[i] * b[i] + c[i])
                }
                33 => [stack.pop()?[0]; 4],
                34 => {
                    let value = stack.pop()?;
                    lanes(|i| value[(at >> (6 - 2 * i)) & 3])
                }
                35 => stack.pop()?.map(|value| value.clamp(0.0, 1.0)),
                52 => row(0)?,
                53 => {
                    let time = stack.pop()?;
                    let (start, end) = (row(0)?, row(1)?);
                    lanes(|i| start[i] + (end[i] - start[i]) * time[i])
                }
                55 => {
                    // Cubic, quadratic, linear and constant rows hold one segment per lane,
                    // and the fifth row holds where each segment starts.
                    let time = stack.pop()?[0];
                    let knots = row(4)?;
                    if knots.windows(2).any(|pair| pair[0] > pair[1]) {
                        return None;
                    }
                    let segment = (1..4).take_while(|&i| time >= knots[i]).count();
                    let coefficients = [
                        row(0)?[segment],
                        row(1)?[segment],
                        row(2)?[segment],
                        row(3)?[segment],
                    ];
                    [polynomial(coefficients, time); 4]
                }
                60 => [input; 4],
                62 => {
                    output = Some(stack.pop()?[0]);
                    continue;
                }
                _ => return None,
            };
            stack.push(value);
        }
        output.filter(|value| value.is_finite())
    }
}

fn lanes(lane: impl FnMut(usize) -> f32) -> [f32; 4] {
    std::array::from_fn(lane)
}

/// A cubic whose coefficients run from the highest power down, so `[0, 0, 1, 0]` is identity.
fn polynomial(coefficients: [f32; 4], time: f32) -> f32 {
    ((coefficients[0] * time + coefficients[1]) * time + coefficients[2]) * time + coefficients[3]
}

fn array<'a>(
    graph: &'a Graph,
    owner: &Block,
    offset: usize,
    class: u32,
) -> Result<&'a [u8], String> {
    let count = crate::package_payload::u64_at(&owner.bytes, offset)?;
    let Some(target) = owner.links.get(&(offset + 8)) else {
        return if count == 0 {
            Ok(&[])
        } else {
            Err("A value array has no allocation.".into())
        };
    };
    let block = graph.blocks.get(*target).ok_or("Missing value array.")?;
    if block.class != class || block.count.map(|n| n as u64) != Some(count) {
        return Err("A value array has an invalid class or length.".into());
    }
    Ok(&block.bytes)
}

pub(super) fn replace_array(
    graph: &mut Graph,
    block: usize,
    offset: usize,
    class: u32,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let stride = schema::record(class)?.size;
    if stride == 0 || bytes.len() % stride != 0 {
        return Err("Invalid native array stride.".into());
    }
    let count = bytes.len() / stride;
    let index = graph.blocks.len();
    graph.blocks.push(Block {
        class,
        count: Some(count),
        bytes,
        links: BTreeMap::new(),
    });
    let owner = graph.blocks.get_mut(block).ok_or("Missing array owner.")?;
    owner
        .bytes
        .get_mut(offset..offset + 8)
        .ok_or("Invalid array descriptor.")?
        .copy_from_slice(&(count as u64).to_le_bytes());
    owner
        .bytes
        .get_mut(offset + 8..offset + 16)
        .ok_or("Invalid array descriptor.")?
        .fill(0);
    owner.links.insert(offset + 8, index);
    Ok(())
}

pub fn instruction_name(op: u8) -> &'static str {
    match op {
        0 => "Stop",
        1 | 6 => "Add",
        2 => "Subtract",
        3 | 5 => "Multiply",
        4 => "Guarded Divide",
        7 => "Is Zero",
        8 => "Minimum",
        9 => "Maximum",
        10 => "Greater or Equal",
        11 => "Dot Product",
        15 => "Cubic Polynomial",
        16 => "Interpolate",
        17 => "Interpolate and Clamp",
        18 => "Multiply and Add",
        19 => "Clamp",
        20 => "Smoothstep",
        21 => "Absolute",
        22 => "Sign",
        23 => "Floor",
        24 => "Ceiling",
        25 => "Round",
        26 => "Fraction",
        27 | 28 => "Normalize",
        29 => "Negate",
        33 => "Splat First Lane",
        34 => "Swizzle",
        35 => "Saturate",
        52 => "Push Constant",
        53 => "Interpolate Constant Pair",
        54 => "Interpolate and Clamp Constant Pair",
        55 => "Piecewise Cubic",
        56 => "Eight-Segment Cubic",
        57 => "Eight-Segment Cubic with Fallback",
        58 => "Piecewise Linear Vector",
        60 => "Push Input",
        62 => "Store Output",
        _ => "Unmapped Instruction",
    }
}
