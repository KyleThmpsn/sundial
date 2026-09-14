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
        if self
            .constants
            .iter()
            .flatten()
            .any(|bits| !f32::from_bits(*bits).is_finite())
        {
            return Err("Value constants must be finite.".into());
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
