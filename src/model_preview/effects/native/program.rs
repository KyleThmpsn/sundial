//! Bounded SM5 token reader. No package identities or shipped shader text are embedded.
use crate::package_payload::bytes_at;

#[derive(Clone, Debug)]
pub(super) struct Operand {
    pub kind: u8,
    pub indices: Vec<Index>,
    pub lanes: [usize; 4],
    pub mask: u8,
    pub modifier: u8,
    pub literal: [u32; 4],
}

#[derive(Clone, Debug)]
pub(super) struct Index {
    pub base: u32,
    pub relative: Option<Box<Operand>>,
}

#[derive(Clone, Debug)]
pub(super) struct Instruction {
    pub code: u16,
    pub saturate: bool,
    pub nonzero: bool,
    pub operands: Vec<Operand>,
    pub offset: [i32; 3],
}

#[derive(Clone, Debug)]
pub(super) struct Semantic {
    pub name: String,
    pub index: u32,
    pub register: usize,
    pub system: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Resource {
    pub slot: usize,
    pub dimension: u32,
    pub integer: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Program {
    pub instructions: Vec<Instruction>,
    pub inputs: Vec<Semantic>,
    pub outputs: Vec<Semantic>,
    pub buffers: Vec<(usize, usize)>,
    pub resources: Vec<Resource>,
    pub samplers: Vec<usize>,
    pub temps: usize,
    pub derivatives: Vec<Option<super::derivative::Derivative>>,
    /// Read-only immediate constant rows retain their original integer bit patterns.
    pub immediate: Vec<[u32; 4]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Execute,
    Stored,
    Affine,
}

const MAX_INSTRUCTIONS: usize = 1024;

fn word(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(bytes, at)?))
}

fn chunk_count(bytes: &[u8]) -> Result<usize, String> {
    if bytes.get(..4) != Some(b"DXBC") || word(bytes, 24)? as usize != bytes.len() {
        return Err("Invalid shader container".into());
    }
    let count = word(bytes, 28)? as usize;
    if count > 32 {
        return Err("Shader chunk count exceeds limits".into());
    }
    Ok(count)
}

fn take(words: &[u32], at: &mut usize) -> Result<u32, String> {
    let value = *words.get(*at).ok_or("Truncated shader operand")?;
    *at += 1;
    Ok(value)
}

fn immediate(
    words: &[u32],
    at: &mut usize,
    previous: &[[u32; 4]],
) -> Result<Vec<[u32; 4]>, String> {
    if !previous.is_empty() {
        return Err("Duplicate immediate shader table".into());
    }
    let token = take(words, at)?;
    let length = take(words, at)? as usize;
    if token != (53 | (3 << 11)) || !(6..=1026).contains(&length) || !(length - 2).is_multiple_of(4)
    {
        return Err("Invalid immediate shader table".into());
    }
    let end = at
        .checked_add(length - 2)
        .ok_or("Immediate shader table overflow")?;
    let rows = words
        .get(*at..end)
        .ok_or("Truncated immediate shader table")?
        .chunks_exact(4)
        .map(|row| row.try_into().unwrap())
        .collect();
    *at = end;
    Ok(rows)
}

fn operand(words: &[u32], at: &mut usize, depth: usize) -> Result<Operand, String> {
    if depth > 2 {
        return Err("Shader index nesting exceeds limits".into());
    }
    let token = take(words, at)?;
    let count = token & 3;
    if count == 3 {
        return Err("Unsupported shader component count".into());
    }
    let selection = (token >> 2) & 3;
    let mask = if count == 2 && selection == 0 {
        ((token >> 4) & 15) as u8
    } else {
        15
    };
    let lanes = std::array::from_fn(|i| match (count, selection) {
        (1, _) => 0,
        (2, 1) => ((token >> (4 + i * 2)) & 3) as usize,
        (2, 2) => ((token >> 4) & 3) as usize,
        _ => i,
    });
    if count == 2 && selection == 3 {
        return Err("Invalid shader lane selection".into());
    }
    let mut modifier = 0;
    if token >> 31 != 0 {
        let extension = take(words, at)?;
        if extension & 63 != 1 || extension >> 31 != 0 || extension & 0x0000_3F00 != 0 {
            return Err("Unsupported shader operand extension".into());
        }
        modifier = ((extension >> 6) & 3) as u8;
    }
    let kind = ((token >> 12) & 255) as u8;
    if !matches!(kind, 0 | 1 | 2 | 4 | 6 | 7 | 8 | 9 | 13) {
        return Err(format!("Unsupported shader operand kind {kind}"));
    }
    let dimensions = (token >> 20) & 3;
    let mut indices = Vec::new();
    for axis in 0..dimensions {
        let mode = (token >> (22 + axis * 3)) & 7;
        let base = match mode {
            0 | 3 => take(words, at)?,
            2 => 0,
            _ => return Err("Unsupported shader index representation".into()),
        };
        let relative = if mode == 2 || mode == 3 {
            Some(Box::new(operand(words, at, depth + 1)?))
        } else {
            None
        };
        indices.push(Index { base, relative });
    }
    let mut literal = [0; 4];
    if kind == 4 {
        if dimensions != 0 || !matches!(count, 1 | 2) {
            return Err("Invalid shader literal".into());
        }
        if count == 1 {
            literal.fill(take(words, at)?);
        } else {
            for value in &mut literal {
                *value = take(words, at)?;
            }
        }
    } else if dimensions
        != if kind == 8 {
            2
        } else if kind == 13 {
            0
        } else {
            1
        }
    {
        return Err("Invalid shader register dimensions".into());
    }
    Ok(Operand {
        kind,
        indices,
        lanes,
        mask,
        modifier,
        literal,
    })
}

pub(super) fn arity(code: u16) -> Option<usize> {
    Some(match code {
        0 | 1 | 14..=17 | 29 | 30 | 41 | 49 | 51 | 52 | 56 | 57 => 3,
        13 | 31 => 1,
        32 | 60 | 61 | 80 => 3,
        18 | 21 | 62 => 0,
        25..=28 | 43 | 47 | 54 | 64..=68 | 75 | 86 | 122 | 124 => 2,
        35 | 38 | 50 | 55 | 78 | 108 => 4,
        45 | 77 => 3,
        69 | 72 => {
            if code == 69 {
                4
            } else {
                5
            }
        }
        73 => 6,
        140 => 5,
        _ => return None,
    })
}

fn signature(bytes: &[u8]) -> Result<Vec<Semantic>, String> {
    let count = word(bytes, 0)? as usize;
    if count > 16 || 8 + count * 24 > bytes.len() {
        return Err("Invalid shader signature".into());
    }
    (0..count)
        .map(|i| {
            let at = 8 + i * 24;
            let start = word(bytes, at)? as usize;
            let name = bytes.get(start..).ok_or("Invalid shader semantic name")?;
            let end = name
                .iter()
                .position(|v| *v == 0)
                .filter(|n| *n <= 64)
                .ok_or("Invalid shader semantic name")?;
            let name = std::str::from_utf8(&name[..end])
                .map_err(|_| "Invalid shader semantic encoding")?
                .to_owned();
            let register = word(bytes, at + 16)? as usize;
            if register >= 16 {
                return Err("Shader signature register exceeds limits".into());
            }
            Ok(Semantic {
                name,
                index: word(bytes, at + 4)?,
                system: word(bytes, at + 8)?,
                register,
            })
        })
        .collect()
}

impl Program {
    pub(super) fn recover_derivatives(&mut self) {
        self.derivatives = self
            .instructions
            .iter()
            .enumerate()
            .map(|(at, i)| {
                matches!(i.code, 69 | 108 | 122 | 124)
                    .then(|| super::derivative::Derivative::read(self, at))
                    .flatten()
            })
            .collect();
    }
    fn container_code<'a>(&mut self, bytes: &'a [u8], count: usize) -> Result<&'a [u8], String> {
        let mut code = None;
        for i in 0..count {
            let at = word(bytes, 32 + i * 4)? as usize;
            let length = word(bytes, at.checked_add(4).ok_or("Invalid shader chunk")?)? as usize;
            let end = at
                .checked_add(8)
                .and_then(|v| v.checked_add(length))
                .ok_or("Invalid shader chunk")?;
            let chunk = bytes.get(at + 8..end).ok_or("Truncated shader chunk")?;
            match bytes.get(at..at + 4) {
                Some(b"ISGN") => self.inputs = signature(chunk)?,
                Some(b"OSGN") => self.outputs = signature(chunk)?,
                Some(b"SHEX" | b"SHDR") if code.replace(chunk).is_some() => {
                    return Err("Duplicate shader code".into());
                }
                _ => {}
            }
        }
        code.ok_or_else(|| "Shader instructions are missing".into())
    }
    pub(super) fn read(bytes: &[u8], stage: u32) -> Result<Self, String> {
        Self::read_impl(bytes, stage, Mode::Execute)
    }

    /// Read the translated stored-vertex envelope for dependency slicing. Instructions
    /// outside the evaluated slice never reach the interpreter or GLSL backend.
    pub(super) fn read_stored(bytes: &[u8]) -> Result<Self, String> {
        Self::read_impl(bytes, 1, Mode::Stored)
    }

    /// Read for affine dependency recovery. The affine walker independently checks
    /// every dependency before accepting a recovered transform.
    pub(super) fn read_affine(bytes: &[u8], stage: u32) -> Result<Self, String> {
        Self::read_impl(bytes, stage, Mode::Affine)
    }

    fn read_impl(bytes: &[u8], stage: u32, mode: Mode) -> Result<Self, String> {
        let count = chunk_count(bytes)?;
        let mut program = Self {
            instructions: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            buffers: Vec::new(),
            resources: Vec::new(),
            samplers: Vec::new(),
            temps: 0,
            derivatives: Vec::new(),
            immediate: Vec::new(),
        };
        let code = program.container_code(bytes, count)?;
        if code.len() % 4 != 0
            || code.len() > 64 * 1024
            || word(code, 0)? != (stage << 16 | 0x50)
            || word(code, 4)? as usize != code.len() / 4
        {
            return Err("Unsupported shader version or instruction length".into());
        }
        let words: Vec<u32> = code
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect();
        let mut at = 2;
        let mut branches = Vec::new();
        while at < words.len() {
            let token = words[at];
            if token & 0x7FF == 53 {
                program.immediate = immediate(&words, &mut at, &program.immediate)?;
                continue;
            }
            let length = ((token >> 24) & 127) as usize;
            let end = at + length;
            if length == 0 || end > words.len() || program.instructions.len() >= MAX_INSTRUCTIONS {
                return Err("Invalid shader instruction length".into());
            }
            let code = (token & 0x7FF) as u16;
            if code == 61 && (token >> 11) & 3 != 2 {
                return Err("Only integer resource dimensions are supported".into());
            }
            let instruction = &words[at..end];
            let mut p = 1;
            let mut extension = token;
            let mut offset = [0; 3];
            while extension >> 31 != 0 {
                extension = take(instruction, &mut p)?;
                match extension & 63 {
                    1 => {
                        for (i, v) in offset.iter_mut().enumerate() {
                            *v = ((extension >> (9 + i * 4)) as i32 & 15) << 28 >> 28;
                        }
                    }
                    2 | 3 => {}
                    _ => return Err("Unsupported shader instruction extension".into()),
                }
            }
            if matches!(code, 88..=90 | 95..=106) {
                match code {
                    104 => {
                        program.temps = take(instruction, &mut p)? as usize;
                        if program.temps > 32 {
                            return Err("Shader temporary count exceeds limits".into());
                        }
                    }
                    106 => {}
                    _ => {
                        let value = operand(instruction, &mut p, 0)?;
                        if value.indices.iter().any(|v| v.relative.is_some()) {
                            return Err("Relative shader declaration".into());
                        }
                        let slot = value
                            .indices
                            .first()
                            .ok_or("Missing shader declaration index")?
                            .base as usize;
                        match code {
                            88 => {
                                let format = take(instruction, &mut p)?;
                                let dimension = (token >> 11) & 31;
                                if value.kind != 7
                                    || slot > 31
                                    || !matches!(dimension, 3 | 5 | 6 | 8)
                                    || !matches!(format, 0x4444 | 0x5555)
                                {
                                    return Err("Unsupported shader resource declaration".into());
                                }
                                program.resources.push(Resource {
                                    slot,
                                    dimension,
                                    integer: format == 0x4444,
                                });
                            }
                            89 => {
                                if value.kind != 8 {
                                    return Err("Invalid shader constant buffer declaration".into());
                                }
                                let count = value.indices[1].base as usize;
                                if slot > 13 || count == 0 || count > 256 {
                                    return Err("Shader constant buffer exceeds limits".into());
                                }
                                program.buffers.push((slot, count));
                            }
                            90 => {
                                if value.kind != 6 || slot > 15 {
                                    return Err("Invalid shader sampler".into());
                                }
                                program.samplers.push(slot);
                            }
                            95..=103 => {
                                if slot >= 16 || !matches!(value.kind, 1 | 2) {
                                    return Err("Invalid shader input or output".into());
                                }
                                if matches!(code, 96 | 97 | 99 | 100 | 102 | 103) {
                                    take(instruction, &mut p)?;
                                }
                            }
                            _ => return Err("Unsupported shader declaration".into()),
                        }
                    }
                }
            } else {
                let count = arity(code)
                    .or_else(|| {
                        (mode != Mode::Execute)
                            .then_some(match code {
                                32..=34 | 36 | 37 | 39 | 42 | 60 | 85 => Some(3),
                                131 => Some(2),
                                139 => Some(4),
                                _ => None,
                            })
                            .flatten()
                    })
                    .ok_or_else(|| format!("Unsupported shader instruction {code}"))?;
                let mut operands = Vec::new();
                for _ in 0..count {
                    operands.push(operand(instruction, &mut p, 0)?);
                }
                match code {
                    31 => {
                        branches.push(false);
                        if branches.len() > 8 {
                            return Err("Shader branch nesting exceeds limits".into());
                        }
                    }
                    18 => {
                        let seen = branches.last_mut().ok_or("Shader else has no branch")?;
                        if *seen {
                            return Err("Duplicate shader else".into());
                        }
                        *seen = true;
                    }
                    21 => {
                        branches.pop().ok_or("Shader end has no branch")?;
                    }
                    _ => {}
                }
                program.instructions.push(Instruction {
                    code,
                    saturate: token & (1 << 13) != 0,
                    nonzero: token & (1 << 18) != 0,
                    operands,
                    offset,
                });
            }
            if p != length {
                return Err(format!("Shader instruction {code} has trailing operands"));
            }
            at = end;
        }
        if !branches.is_empty() || program.instructions.last().is_none_or(|i| i.code != 62) {
            return Err("Incomplete shader program".into());
        }
        program.validate(program.immediate.len())?;
        program.recover_derivatives();
        Ok(program)
    }

    fn validate(&self, immediate_count: usize) -> Result<(), String> {
        fn check(p: &Program, v: &Operand, immediate_count: usize) -> Result<(), String> {
            for (axis, index) in v.indices.iter().enumerate() {
                if let Some(relative) = &index.relative {
                    check(p, relative, immediate_count)?;
                    if relative.kind != 0 || !matches!((v.kind, axis), (8, 1) | (9, 0)) {
                        return Err("Unsupported relative shader index".into());
                    }
                }
            }
            let index = v.indices.first().map_or(0, |v| v.base as usize);
            let valid = match v.kind {
                0 => index < p.temps,
                1 | 2 => index < 16,
                4 | 13 => true,
                6 => p.samplers.contains(&index),
                7 => p.resources.iter().any(|r| r.slot == index),
                8 => p.buffers.iter().any(|&(slot, count)| {
                    slot == index
                        && (v.indices[1].relative.is_some() || (v.indices[1].base as usize) < count)
                }),
                9 => {
                    immediate_count > 0
                        && (v.indices[0].relative.is_some() || index < immediate_count)
                }
                _ => false,
            };
            if valid {
                Ok(())
            } else {
                Err("Shader register is outside its declaration".into())
            }
        }
        for instruction in &self.instructions {
            for value in &instruction.operands {
                check(self, value, immediate_count)?;
            }
            let destinations = match instruction.code {
                13 | 18 | 21 | 31 | 62 => 0,
                38 | 77 | 78 => 2,
                _ => 1,
            };
            for value in instruction.operands.iter().take(destinations) {
                if !matches!(value.kind, 0 | 2 | 13) || value.modifier != 0 || value.mask == 0 {
                    return Err("Invalid shader destination register".into());
                }
            }
            if matches!(instruction.code, 45 | 61 | 69 | 72 | 73 | 108)
                && instruction.operands[2].kind != 7
            {
                return Err("Shader texture operation has no resource operand".into());
            }
            if matches!(instruction.code, 69 | 72 | 73 | 108) && instruction.operands[3].kind != 6 {
                return Err("Shader texture operation has no sampler operand".into());
            }
            for (index, value) in instruction.operands.iter().enumerate().skip(destinations) {
                let binding = (index == 2
                    && matches!(instruction.code, 45 | 61 | 69 | 72 | 73 | 108))
                    || (index == 3 && matches!(instruction.code, 69 | 72 | 73 | 108));
                if !binding && !matches!(value.kind, 0 | 1 | 2 | 4 | 8 | 9) {
                    return Err("Invalid numeric shader operand".into());
                }
            }
        }
        Ok(())
    }
}
