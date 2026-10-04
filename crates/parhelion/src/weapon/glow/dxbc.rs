//! Bounded SM5 operands and container rebuilding for native gear material edits.
use crate::{AuthoringResult, error::invalid};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Operand(pub Vec<u32>);

impl Operand {
    pub fn kind(&self) -> u32 {
        (self.0[0] >> 12) & 255
    }
    pub fn indices(&self) -> Option<&[u32]> {
        let token = self.0[0];
        let count = ((token >> 20) & 3) as usize;
        if token >> 31 != 0 || (token >> 22) & ((1 << (count * 3)) - 1) != 0 {
            return None;
        }
        self.0.get(1..1 + count)
    }
    pub fn register(&self, kind: u32, index: u32) -> bool {
        self.kind() == kind && self.indices() == Some(&[index][..])
    }
    pub fn cb(&self, slot: u32, vector: u32) -> bool {
        // Negated operands retain their index after a single modifier token.
        if self.kind() != 8 {
            return false;
        }
        let start = 1 + usize::from(self.0[0] >> 31 != 0);
        self.0.get(start..) == Some(&[slot, vector][..])
    }
    pub fn literal(&self, value: f32) -> bool {
        self.kind() == 4 && self.0[1..].iter().all(|word| *word == value.to_bits())
    }
    pub fn mask(&self) -> Option<u32> {
        (self.0[0] & 15 == 2).then_some((self.0[0] >> 4) & 15)
    }
    pub fn lanes(&self) -> Option<[u32; 4]> {
        let token = self.0[0];
        if token & 3 != 2 {
            return None;
        }
        match (token >> 2) & 3 {
            1 => Some(std::array::from_fn(|lane| (token >> (4 + 2 * lane)) & 3)),
            2 => Some([(token >> 4) & 3; 4]),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(super) struct Instruction {
    pub words: Vec<u32>,
    pub args: Vec<Operand>,
    prefix: usize,
}

impl Instruction {
    pub fn opcode(&self) -> u32 {
        self.words[0] & 0x7ff
    }
    pub fn replace(&self, args: Vec<Operand>) -> AuthoringResult<Self> {
        let mut words = self.words[..self.prefix].to_vec();
        words.extend(args.iter().flat_map(|arg| arg.0.iter().copied()));
        if words.len() > 127 {
            return Err(invalid("Glow instruction exceeds the SM5 length limit"));
        }
        words[0] = (words[0] & !0x7f00_0000) | ((words.len() as u32) << 24);
        Ok(Self {
            words,
            args,
            prefix: self.prefix,
        })
    }
}

fn operand(words: &[u32], at: &mut usize, depth: usize) -> AuthoringResult<Operand> {
    if depth > 8 {
        return Err(invalid(
            "Shader operand nesting exceeds the supported limit",
        ));
    }
    let start = *at;
    let token = *words
        .get(*at)
        .ok_or_else(|| invalid("Truncated shader operand"))?;
    *at += 1;
    let mut extended = token;
    while extended >> 31 != 0 {
        extended = *words
            .get(*at)
            .ok_or_else(|| invalid("Truncated shader operand modifier"))?;
        *at += 1;
    }
    let kind = (token >> 12) & 255;
    if kind == 4 || kind == 5 {
        *at += (if token & 3 == 1 { 1 } else { 4 }) * (if kind == 5 { 2 } else { 1 });
    }
    for dim in 0..(token >> 20) & 3 {
        match (token >> (22 + 3 * dim)) & 7 {
            0 => *at += 1,
            1 => *at += 2,
            2 => {
                operand(words, at, depth + 1)?;
            }
            3 => {
                *at += 1;
                operand(words, at, depth + 1)?;
            }
            4 => {
                *at += 2;
                operand(words, at, depth + 1)?;
            }
            _ => return Err(invalid("Unsupported shader operand index")),
        }
    }
    Ok(Operand(
        words
            .get(start..*at)
            .ok_or_else(|| invalid("Truncated shader operand data"))?
            .to_vec(),
    ))
}

pub(super) struct Program {
    chunks: Vec<([u8; 4], Vec<u8>)>,
    code_chunk: usize,
    pub instructions: Vec<Instruction>,
}

fn word(bytes: &[u8], at: usize) -> AuthoringResult<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .ok_or_else(|| invalid("Truncated shader container"))?
            .try_into()
            .unwrap(),
    ))
}

impl Program {
    pub fn read(bytes: &[u8]) -> AuthoringResult<Self> {
        if bytes.get(..4) != Some(b"DXBC")
            || word(bytes, 24)? as usize != bytes.len()
            || word(bytes, 20)? != 1
            || bytes.len() > 4 * 1024 * 1024
        {
            return Err(invalid("Unsupported glow shader container"));
        }
        if bytes.get(4..20) != Some(super::checksum::compute(bytes)?.as_slice()) {
            return Err(invalid("Shader Glow source program checksum differs"));
        }
        let count = word(bytes, 28)? as usize;
        if count == 0 || count > 64 {
            return Err(invalid("Unsupported shader chunk count"));
        }
        let mut chunks = Vec::new();
        let mut code_chunk = None;
        for i in 0..count {
            let at = word(bytes, 32 + i * 4)? as usize;
            let size = word(bytes, at + 4)? as usize;
            let name: [u8; 4] = bytes
                .get(at..at + 4)
                .ok_or_else(|| invalid("Shader chunk name"))?
                .try_into()
                .unwrap();
            let end = at
                .checked_add(8)
                .and_then(|v| v.checked_add(size))
                .ok_or_else(|| invalid("Shader chunk size overflow"))?;
            let data = bytes
                .get(at + 8..end)
                .ok_or_else(|| invalid("Truncated shader chunk"))?
                .to_vec();
            if (&name == b"SHEX" || &name == b"SHDR") && code_chunk.replace(i).is_some() {
                return Err(invalid("Ambiguous shader program"));
            }
            chunks.push((name, data));
        }
        let code_chunk = code_chunk.ok_or_else(|| invalid("Shader program is missing"))?;
        let data = &chunks[code_chunk].1;
        if data.len() % 4 != 0
            || word(data, 0)? != 0x50
            || word(data, 4)? as usize != data.len() / 4
        {
            return Err(invalid("Shader Glow requires an SM5 pixel program"));
        }
        let tokens = data
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect::<Vec<_>>();
        let mut at = 2;
        let mut instructions = Vec::new();
        while at < tokens.len() {
            let op = tokens[at] & 0x7ff;
            let len = if op == 0x35 {
                *tokens
                    .get(at + 1)
                    .ok_or_else(|| invalid("Shader custom data"))? as usize
            } else {
                ((tokens[at] >> 24) & 127) as usize
            };
            if len == 0 {
                return Err(invalid("Empty shader instruction"));
            }
            let words = tokens
                .get(
                    at..at
                        .checked_add(len)
                        .ok_or_else(|| invalid("Shader size overflow"))?,
                )
                .ok_or_else(|| invalid("Truncated shader instruction"))?
                .to_vec();
            let mut cursor = 1;
            let mut extended = words[0];
            while extended >> 31 != 0 {
                extended = *words
                    .get(cursor)
                    .ok_or_else(|| invalid("Shader instruction modifier"))?;
                cursor += 1;
            }
            let prefix = cursor;
            let mut args = Vec::new();
            if op < 0x58 && op != 0x35 {
                while cursor < words.len() {
                    args.push(operand(&words, &mut cursor, 0)?);
                }
                if cursor != words.len() {
                    return Err(invalid("Shader instruction operands differ"));
                }
            }
            instructions.push(Instruction {
                words,
                args,
                prefix,
            });
            at += len;
        }
        Ok(Self {
            chunks,
            code_chunk,
            instructions,
        })
    }

    pub fn emit(mut self) -> AuthoringResult<Vec<u8>> {
        let mut tokens = vec![0x50, 0];
        tokens.extend(
            self.instructions
                .iter()
                .flat_map(|i| i.words.iter().copied()),
        );
        tokens[1] = u32::try_from(tokens.len()).map_err(|_| invalid("Shader length overflow"))?;
        self.chunks[self.code_chunk].1 = tokens.into_iter().flat_map(u32::to_le_bytes).collect();
        // Reflection and statistics describe the old instruction stream. The native
        // runtime uses the retained signatures and declarations instead.
        self.chunks
            .retain(|(name, _)| ![b"RDEF", b"STAT", b"SDBG", b"SPDB"].contains(&name));
        let mut result = vec![0u8; 32 + 4 * self.chunks.len()];
        result[..4].copy_from_slice(b"DXBC");
        result[20..24].copy_from_slice(&1u32.to_le_bytes());
        result[28..32].copy_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        for (index, (name, data)) in self.chunks.into_iter().enumerate() {
            let at = result.len() as u32;
            result[32 + index * 4..36 + index * 4].copy_from_slice(&at.to_le_bytes());
            result.extend(name);
            result.extend((data.len() as u32).to_le_bytes());
            result.extend(data);
        }
        let len = result.len() as u32;
        result[24..28].copy_from_slice(&len.to_le_bytes());
        let hash = super::checksum::compute(&result)?;
        result[4..20].copy_from_slice(&hash);
        Ok(result)
    }
}

pub(super) fn dst(index: u32, mask: u32) -> Operand {
    Operand(vec![2 | (mask << 4) | (1 << 20), index])
}
pub(super) fn temp(index: u32, swizzle: u32) -> Operand {
    Operand(vec![2 | (1 << 2) | (swizzle << 4) | (1 << 20), index])
}
pub(super) fn cb(slot: u32, index: u32, swizzle: u32) -> Operand {
    Operand(vec![
        2 | (1 << 2) | (swizzle << 4) | (8 << 12) | (2 << 20),
        slot,
        index,
    ])
}
pub(super) fn lit(value: f32) -> Operand {
    Operand(vec![1 | (4 << 12), value.to_bits()])
}
pub(super) fn neg(mut source: Operand) -> Operand {
    source.0[0] |= 1 << 31;
    source.0.insert(1, 0x41);
    source
}
pub(super) fn ins(opcode: u32, args: Vec<Operand>) -> Instruction {
    let mut words = vec![opcode];
    words.extend(args.iter().flat_map(|arg| arg.0.iter().copied()));
    words[0] |= (words.len() as u32) << 24;
    Instruction {
        words,
        args,
        prefix: 1,
    }
}
