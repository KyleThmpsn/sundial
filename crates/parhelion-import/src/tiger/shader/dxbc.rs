//! Register remapping inside compiled SM5 shaders, with the container checksum recomputed.
//!
//! Constant and resource remapping preserves both signatures. Vertex input remapping updates
//! its signature and operands together, leaving the output contract unchanged. The walker
//! refuses tokens it cannot account for.
use anyhow::{Context, Result, bail, ensure};

const CONSTANT_BUFFER: u32 = 8;
const RESOURCE: u32 = 7;
const CUSTOM_DATA: u32 = 0x35;
const DCL_RESOURCE: u32 = 0x58;
const DCL_CONSTANT_BUFFER: u32 = 0x59;
const DCL_SAMPLER: u32 = 0x5A;
const INPUT: u32 = 1;
const DCL_INPUT: u32 = 0x5F;
const DCL_INPUT_SGV: u32 = 0x60;
const DCL_INPUT_SIV: u32 = 0x61;

fn md5_block(state: &mut [u32; 4], block: &[u8]) {
    const SHIFT: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let words: Vec<u32> = block
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = match i / 16 {
            0 => ((b & c) | (!b & d), i),
            1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
            2 => (b ^ c ^ d, (3 * i + 5) % 16),
            _ => (c ^ (b | !d), (7 * i) % 16),
        };
        let k = ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32;
        let rotated = f
            .wrapping_add(a)
            .wrapping_add(k)
            .wrapping_add(words[g])
            .rotate_left(SHIFT[(i / 16) * 4 + i % 4]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(rotated);
    }
    for (value, add) in state.iter_mut().zip([a, b, c, d]) {
        *value = value.wrapping_add(add);
    }
}

/// The DXBC container hash: MD5 rounds over everything after the hash field, with the bit
/// length stored ahead of the tail and a derived length in the final word.
pub(crate) fn checksum(container: &[u8]) -> Result<[u8; 16]> {
    let data = container.get(20..).context("shader container header")?;
    let bits = u32::try_from(data.len() * 8).context("shader container size")?;
    let tail_bits = (bits >> 2) | 1;
    let mut state = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    let full = data.len() - data.len() % 64;
    for block in data[..full].chunks_exact(64) {
        md5_block(&mut state, block);
    }
    let left = &data[full..];
    let mut block = Vec::with_capacity(128);
    if left.len() < 56 {
        block.extend(bits.to_le_bytes());
        block.extend_from_slice(left);
        block.push(0x80);
        block.resize(60, 0);
        block.extend(tail_bits.to_le_bytes());
        md5_block(&mut state, &block);
    } else {
        block.extend_from_slice(left);
        block.push(0x80);
        block.resize(64, 0);
        md5_block(&mut state, &block);
        let mut last = bits.to_le_bytes().to_vec();
        last.resize(60, 0);
        last.extend(tail_bits.to_le_bytes());
        md5_block(&mut state, &last);
    }
    let mut out = [0; 16];
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(out)
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("shader container extent")?
            .try_into()?,
    ))
}

/// Byte range of one container chunk's data.
pub(crate) fn chunk(container: &[u8], name: &[u8; 4]) -> Result<Option<std::ops::Range<usize>>> {
    ensure!(
        container.starts_with(b"DXBC") && u32_at(container, 24)? as usize == container.len(),
        "shader container envelope differs"
    );
    let count = u32_at(container, 28)? as usize;
    ensure!(count <= 64, "shader container chunk count differs");
    let mut found = None;
    for index in 0..count {
        let at = u32_at(container, 32 + index * 4)? as usize;
        let size = u32_at(container, at + 4)? as usize;
        let range = at + 8..at + 8 + size;
        ensure!(range.end <= container.len(), "shader chunk extent differs");
        if container.get(at..at + 4) == Some(name) {
            ensure!(found.is_none(), "duplicate shader chunk");
            found = Some(range);
        }
    }
    Ok(found)
}

/// An operand's immediate first index and its token position, with the selection bits.
struct Operand {
    token_at: usize,
    kind: u32,
    indices: Vec<Option<usize>>,
}

fn operand(tokens: &[u32], mut at: usize, operands: &mut Vec<Operand>) -> Result<usize> {
    let token = *tokens.get(at).context("truncated shader operand")?;
    let start = at;
    at += 1;
    if token >> 31 != 0 {
        loop {
            let extended = *tokens.get(at).context("truncated extended operand")?;
            at += 1;
            if extended >> 31 == 0 {
                break;
            }
        }
    }
    let kind = (token >> 12) & 0xFF;
    let components = token & 3;
    match kind {
        4 => at += if components == 1 { 1 } else { 4 },
        5 => at += if components == 1 { 2 } else { 8 },
        _ => {}
    }
    let mut indices = Vec::new();
    for dimension in 0..(token >> 20) & 3 {
        match (token >> (22 + 3 * dimension)) & 7 {
            0 => {
                indices.push(Some(at));
                at += 1;
            }
            1 => {
                indices.push(None);
                at += 2;
            }
            2 => {
                indices.push(None);
                at = operand(tokens, at, operands)?;
            }
            3 => {
                indices.push(None);
                at = operand(tokens, at + 1, operands)?;
            }
            4 => {
                indices.push(None);
                at = operand(tokens, at + 2, operands)?;
            }
            other => bail!("unknown shader index representation {other}"),
        }
    }
    operands.push(Operand {
        token_at: start,
        kind,
        indices,
    });
    Ok(at)
}

struct Instruction {
    opcode: u32,
    operands: Vec<Operand>,
}

fn is_declaration(opcode: u32) -> bool {
    (0x58..=0x6A).contains(&opcode)
        || (0x8F..=0xA2).contains(&opcode)
        || (0xAA..=0xB0).contains(&opcode)
}

fn walk(tokens: &[u32]) -> Result<Vec<Instruction>> {
    let mut at = 2;
    let mut result = Vec::new();
    while at < tokens.len() {
        let token = tokens[at];
        let opcode = token & 0x7FF;
        let length = if opcode == CUSTOM_DATA {
            *tokens.get(at + 1).context("truncated custom data")? as usize
        } else {
            ((token >> 24) & 0x7F) as usize
        };
        ensure!(
            length > 0 && at + length <= tokens.len(),
            "shader instruction extent differs"
        );
        let mut operands = Vec::new();
        let declaration = is_declaration(opcode);
        if opcode != CUSTOM_DATA
            && (!declaration
                || matches!(
                    opcode,
                    DCL_RESOURCE
                        | DCL_CONSTANT_BUFFER
                        | DCL_SAMPLER
                        | DCL_INPUT
                        | DCL_INPUT_SGV
                        | DCL_INPUT_SIV
                ))
        {
            let mut cursor = at + 1;
            if token >> 31 != 0 {
                loop {
                    let extended = tokens[cursor];
                    cursor += 1;
                    if extended >> 31 == 0 {
                        break;
                    }
                }
            }
            // Resource and system-input declarations carry a trailing type token.
            let end = at + length
                - usize::from(matches!(
                    opcode,
                    DCL_RESOURCE | DCL_INPUT_SGV | DCL_INPUT_SIV
                ));
            while cursor < end {
                cursor = operand(tokens, cursor, &mut operands)?;
            }
            ensure!(
                cursor == end,
                "shader operands overrun instruction {opcode:X}"
            );
        }
        result.push(Instruction { opcode, operands });
        at += length;
    }
    ensure!(
        at == tokens.len(),
        "shader instructions do not end at the chunk end"
    );
    Ok(result)
}

/// Register changes applied to one compiled shader.
#[derive(Default)]
pub(crate) struct Remap {
    /// Vertex input registers, declarations and signature included. Include every input.
    pub inputs: Vec<(u32, u32)>,
    /// Constant buffer slots, declarations included.
    pub constant_buffers: Vec<(u32, u32)>,
    /// Resource slots, declarations included.
    pub resources: Vec<(u32, u32)>,
    /// Resource slots whose reads move while their declaration stays.
    pub reads: Vec<(u32, u32)>,
    /// Read `cb[buffer][vector].x` broadcasts as `.z` when the previous instruction reads the
    /// same vector's `.w` broadcast: the emissive exposure product.
    pub exposure: Option<(u32, u32)>,
}

pub(crate) struct Patched {
    pub bytecode: Vec<u8>,
    pub changes: usize,
}

// Four-component operands select by a broadcast swizzle or by a single component.
fn exposure_swizzle(token: &mut u32, previous_w: bool) -> (bool, usize) {
    let (mask, w, z) = match (*token & 3, (*token >> 2) & 3) {
        (2, 1) => (0xFF, 0xFF, 0xAA),
        (2, 2) => (3, 3, 2),
        _ => return (false, 0),
    };
    let component = (*token >> 4) & mask;
    if component == 0 && previous_w {
        *token = (*token & !(mask << 4)) | (z << 4);
        (false, 1)
    } else {
        (component == w, 0)
    }
}

/// Moves one vertex input operand to its remapped register. Returns 1 when it moved.
fn remap_input(tokens: &mut [u32], operand: &Operand, inputs: &[(u32, u32)]) -> Result<usize> {
    ensure!(operand.indices.len() == 1, "vertex input dimensions differ");
    let first = operand
        .indices
        .first()
        .copied()
        .flatten()
        .context("relative vertex input cannot be remapped")?;
    let value = tokens[first];
    let (_, to) = inputs
        .iter()
        .find(|(from, _)| *from == value)
        .context("vertex input has no signature element")?;
    tokens[first] = *to;
    Ok(usize::from(value != *to))
}

pub(crate) fn patch(container: &[u8], remap: &Remap) -> Result<Patched> {
    let range = match chunk(container, b"SHEX")? {
        Some(range) => range,
        None => chunk(container, b"SHDR")?.context("shader has no program chunk")?,
    };
    ensure!(
        range.len().is_multiple_of(4),
        "shader program chunk alignment"
    );
    let mut tokens: Vec<u32> = container[range.clone()]
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    ensure!(
        tokens.get(1).copied() == Some(tokens.len() as u32),
        "shader program length differs"
    );
    let instructions = walk(&tokens)?;
    let mut changes = 0;
    let mut previous_w = false;
    for instruction in &instructions {
        let declaration = matches!(instruction.opcode, DCL_RESOURCE | DCL_CONSTANT_BUFFER);
        let mut reads_w = false;
        for operand in &instruction.operands {
            let first = operand.indices.first().copied().flatten();
            if operand.kind == INPUT && !remap.inputs.is_empty() {
                changes += remap_input(&mut tokens, operand, &remap.inputs)?;
                continue;
            }
            let Some(first) = first else {
                continue;
            };
            let value = tokens[first];
            if operand.kind == CONSTANT_BUFFER {
                if let Some((_, to)) = remap
                    .constant_buffers
                    .iter()
                    .find(|(from, _)| *from == value)
                {
                    tokens[first] = *to;
                    changes += 1;
                }
                if let Some((buffer, vector)) = remap.exposure
                    && value == buffer
                    && let Some(Some(second)) = operand.indices.get(1)
                    && tokens[*second] == vector
                {
                    let (uses_w, changed) =
                        exposure_swizzle(&mut tokens[operand.token_at], previous_w);
                    reads_w |= uses_w;
                    changes += changed;
                }
            } else if operand.kind == RESOURCE {
                if let Some((_, to)) = remap.resources.iter().find(|(from, _)| *from == value) {
                    tokens[first] = *to;
                    changes += 1;
                } else if !declaration
                    && let Some((_, to)) = remap.reads.iter().find(|(from, _)| *from == value)
                {
                    tokens[first] = *to;
                    changes += 1;
                }
            }
        }
        previous_w = reads_w;
    }
    let mut bytecode = container.to_vec();
    for (i, token) in tokens.iter().enumerate() {
        bytecode[range.start + i * 4..range.start + i * 4 + 4]
            .copy_from_slice(&token.to_le_bytes());
    }
    if !remap.inputs.is_empty() {
        remap_signature(container, &mut bytecode, tokens[0], &remap.inputs)?;
    }
    let hash = checksum(&bytecode)?;
    bytecode[4..20].copy_from_slice(&hash);
    Ok(Patched { bytecode, changes })
}

/// Rewrites the input signature's registers in `bytecode` to match the remapped inputs.
fn remap_signature(
    container: &[u8],
    bytecode: &mut [u8],
    version: u32,
    inputs: &[(u32, u32)],
) -> Result<()> {
    ensure!(
        version >> 16 == 1,
        "input remapping requires a vertex program"
    );
    let range = chunk(container, b"ISGN")?.context("vertex input signature missing")?;
    let count = u32_at(container, range.start)? as usize;
    ensure!(
        count == inputs.len() && count <= 32,
        "vertex input signature count differs"
    );
    let mut records = Vec::with_capacity(count);
    for index in 0..count {
        let at = range.start + 8 + index * 24;
        let mut record: [u8; 24] = container
            .get(at..at + 24)
            .filter(|_| at + 24 <= range.end)
            .context("vertex input signature extent")?
            .try_into()?;
        let from = u32_at(&record, 16)?;
        let (_, to) = inputs
            .iter()
            .find(|(old, _)| *old == from)
            .context("unmapped vertex signature register")?;
        record[16..20].copy_from_slice(&to.to_le_bytes());
        records.push((*to, record));
    }
    records.sort_by_key(|(register, _)| *register);
    ensure!(
        records.windows(2).all(|rows| rows[0].0 != rows[1].0),
        "vertex input register collision"
    );
    for (index, (_, record)) in records.iter().enumerate() {
        let at = range.start + 8 + index * 24;
        bytecode[at..at + 24].copy_from_slice(record);
    }
    Ok(())
}

/// Declared constant buffer and resource slots, from the program's declarations.
pub(crate) fn declarations(container: &[u8]) -> Result<(Vec<u32>, Vec<u32>)> {
    let range = match chunk(container, b"SHEX")? {
        Some(range) => range,
        None => chunk(container, b"SHDR")?.context("shader has no program chunk")?,
    };
    let tokens: Vec<u32> = container[range]
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    let mut buffers = Vec::new();
    let mut resources = Vec::new();
    for instruction in walk(&tokens)? {
        let Some(first) = instruction
            .operands
            .first()
            .and_then(|o| o.indices.first().copied().flatten())
        else {
            continue;
        };
        match instruction.opcode {
            DCL_CONSTANT_BUFFER => buffers.push(tokens[first]),
            DCL_RESOURCE => resources.push(tokens[first]),
            _ => {}
        }
    }
    Ok((buffers, resources))
}

/// Input or output signature elements: (semantic, index, register, mask). `OSG5` elements
/// carry a leading stream index.
pub(crate) fn signature(container: &[u8], name: &[u8; 4]) -> Result<Vec<(String, u32, u32, u8)>> {
    let (range, stride, base) = match chunk(container, name)? {
        Some(range) => (range, 24, 0),
        None if name == b"OSGN" => match chunk(container, b"OSG5")? {
            Some(range) => (range, 28, 4),
            None => return Ok(Vec::new()),
        },
        None => return Ok(Vec::new()),
    };
    let data = &container[range];
    let count = u32_at(data, 0)? as usize;
    ensure!(count <= 64, "shader signature count differs");
    let mut result = Vec::with_capacity(count);
    for index in 0..count {
        let at = 8 + index * stride + base;
        let name_at = u32_at(data, at)? as usize;
        let tail = data.get(name_at..).context("shader semantic name")?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .context("unterminated shader semantic")?;
        result.push((
            String::from_utf8(tail[..end].to_vec())?,
            u32_at(data, at + 4)?,
            u32_at(data, at + 16)?,
            *data.get(at + 20).context("shader signature mask")?,
        ));
    }
    Ok(result)
}
