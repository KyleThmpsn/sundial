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

/// Nested relative operand indices deeper than this are refused, as Parhelion's own reader does.
/// Compiled programs nest one or two levels.
const OPERAND_DEPTH: usize = 8;

/// The DXBC container hash the host computes for every shader edit.
pub(crate) fn checksum(container: &[u8]) -> Result<[u8; 16]> {
    sundial::package_authoring::dxbc::checksum(container).map_err(anyhow::Error::msg)
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    sundial::package_authoring::native_payload::bytes_at(bytes, at)
        .map(u32::from_le_bytes)
        .map_err(anyhow::Error::msg)
        .context("shader container extent")
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
pub(crate) struct Operand {
    pub token_at: usize,
    pub kind: u32,
    pub indices: Vec<Option<usize>>,
}

fn operand(tokens: &[u32], at: usize, operands: &mut Vec<Operand>) -> Result<usize> {
    nested_operand(tokens, at, operands, 0)
}

fn nested_operand(
    tokens: &[u32],
    mut at: usize,
    operands: &mut Vec<Operand>,
    depth: usize,
) -> Result<usize> {
    ensure!(
        depth <= OPERAND_DEPTH,
        "shader operand nesting exceeds the supported limit"
    );
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
                at = nested_operand(tokens, at, operands, depth + 1)?;
            }
            3 => {
                indices.push(None);
                at = nested_operand(tokens, at + 1, operands, depth + 1)?;
            }
            4 => {
                indices.push(None);
                at = nested_operand(tokens, at + 2, operands, depth + 1)?;
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

pub(crate) struct Instruction {
    pub start: usize,
    pub end: usize,
    pub opcode: u32,
    pub operands: Vec<Operand>,
}

fn is_declaration(opcode: u32) -> bool {
    (0x58..=0x6A).contains(&opcode)
        || (0x8F..=0xA2).contains(&opcode)
        || (0xAA..=0xB0).contains(&opcode)
}

pub(crate) fn walk(tokens: &[u32]) -> Result<Vec<Instruction>> {
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
                // Extended opcode tokens chain on their top bit and stay inside the instruction.
                loop {
                    ensure!(
                        cursor < at + length,
                        "truncated extended shader opcode {opcode:X}"
                    );
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
        result.push(Instruction {
            start: at,
            end: at + length,
            opcode,
            operands,
        });
        at += length;
    }
    ensure!(
        at == tokens.len(),
        "shader instructions do not end at the chunk end"
    );
    Ok(result)
}

/// Rebuild a source container around edited SM5 tokens and optional output
/// signature. Compiler reflection and statistics no longer describe edited code.
pub(crate) fn rebuild(container: &[u8], tokens: &[u32], outputs: Option<&[u8]>) -> Result<Vec<u8>> {
    ensure!(
        tokens.len() >= 2 && tokens[1] as usize == tokens.len(),
        "Edited shader token extent differs"
    );
    walk(tokens)?;
    let mut chunks = Vec::new();
    for name in [*b"ISGN", *b"OSGN", *b"PCSG"] {
        let data = if name == *b"OSGN"
            && let Some(outputs) = outputs
        {
            outputs.to_vec()
        } else if let Some(range) = chunk(container, &name)? {
            container[range].to_vec()
        } else {
            continue;
        };
        chunks.push((name, data));
    }
    ensure!(
        chunks.iter().any(|(name, _)| name == b"OSGN"),
        "Source shader output signature is absent"
    );
    chunks.push((
        *b"SHEX",
        tokens.iter().flat_map(|word| word.to_le_bytes()).collect(),
    ));
    let mut result = vec![0; 32 + chunks.len() * 4];
    result[..4].copy_from_slice(b"DXBC");
    result[20..24].copy_from_slice(&1u32.to_le_bytes());
    result[28..32].copy_from_slice(&(chunks.len() as u32).to_le_bytes());
    for (index, (name, data)) in chunks.into_iter().enumerate() {
        let offset = u32::try_from(result.len())?;
        result[32 + index * 4..36 + index * 4].copy_from_slice(&offset.to_le_bytes());
        result.extend(name);
        result.extend(u32::try_from(data.len())?.to_le_bytes());
        result.extend(data);
        while !result.len().is_multiple_of(4) {
            result.push(0);
        }
    }
    let length = u32::try_from(result.len())?;
    result[24..28].copy_from_slice(&length.to_le_bytes());
    let digest = checksum(&result)?;
    result[4..20].copy_from_slice(&digest);
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

/// Replaces the program's one four-component immediate operand whose bits equal `from`. Every
/// other token and chunk is kept, and the container checksum is recomputed.
pub(crate) fn replace_immediate(container: &[u8], from: [f32; 4], to: [f32; 4]) -> Result<Vec<u8>> {
    const IMMEDIATE: u32 = 4;
    let range = match chunk(container, b"SHEX")? {
        Some(range) => range,
        None => chunk(container, b"SHDR")?.context("shader has no program chunk")?,
    };
    let tokens: Vec<u32> = container[range.clone()]
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    ensure!(
        tokens.get(1).copied() == Some(tokens.len() as u32),
        "shader program length differs"
    );
    let bits = from.map(f32::to_bits);
    let mut found = Vec::new();
    for instruction in walk(&tokens)? {
        for operand in &instruction.operands {
            let token = tokens[operand.token_at];
            // An extended operand token would sit before the values. Compiled immediates carry none.
            if operand.kind == IMMEDIATE
                && token & 3 == 2
                && token >> 31 == 0
                && tokens.get(operand.token_at + 1..operand.token_at + 5) == Some(&bits[..])
            {
                found.push(operand.token_at + 1);
            }
        }
    }
    let [at] = found[..] else {
        bail!(
            "shader holds {} immediates with the expected value, not one",
            found.len()
        );
    };
    let mut bytecode = container.to_vec();
    for (index, value) in to.into_iter().enumerate() {
        ensure!(value.is_finite(), "shader immediate is not finite");
        let offset = range.start + (at + index) * 4;
        bytecode[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
    }
    let hash = checksum(&bytecode)?;
    bytecode[4..20].copy_from_slice(&hash);
    Ok(bytecode)
}

/// A pixel program whose color output `register` never exceeds `limit` in R, G or B. Every write
/// to the output moves to a new temporary register, and each return, conditional or not, first
/// copies that register to the output with its declared RGB lanes clamped and any other declared
/// lane unchanged. Other outputs and the signatures are kept. A program that does not declare the
/// output, or declares it without all of R, G and B, is returned unchanged.
pub(crate) fn limit_output(container: &[u8], register: u32, limit: f32) -> Result<Vec<u8>> {
    const MIN: u32 = 0x33;
    ensure!(limit.is_finite(), "output limit is not finite");
    redirect_output(container, register, 1, &|temp, mask| {
        // o<register>.xyz = min(r<temp>.xyzw, limit), o<register>.w = r<temp>.w.
        let mut epilogue = vec![
            MIN | (10 << 24),
            0x0010_2072,
            register,
            0x0010_0E46,
            temp,
            0x0000_4002,
            limit.to_bits(),
            limit.to_bits(),
            limit.to_bits(),
            0,
        ];
        epilogue.extend(keep_alpha(register, temp, mask));
        epilogue
    })
}

/// A pixel program whose color output `register` keeps each pixel's luminance in `hue`. The
/// luminance of the RGB the program writes, with Rec. 709 weights, multiplies `hue` scaled to unit
/// luminance, so a texture's or vertex color's pattern and brightness stay and only the hue
/// changes. Writes move to a temporary register as [`limit_output`] moves them, and any other
/// declared lane is copied unchanged. A program that does not declare the output with all of R, G
/// and B is returned unchanged.
pub(crate) fn tint_output(container: &[u8], register: u32, hue: [f32; 3]) -> Result<Vec<u8>> {
    const DP3: u32 = 0x10;
    const MUL: u32 = 0x38;
    const WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];
    let luminance = hue.iter().zip(WEIGHTS).map(|(v, w)| v * w).sum::<f32>();
    ensure!(
        hue.iter().all(|v| v.is_finite() && *v >= 0.) && luminance > 1e-4,
        "output hue has no luminance"
    );
    let scaled = hue.map(|v| v / luminance);
    redirect_output(container, register, 2, &|temp, mask| {
        // r<temp+1>.x = dot(r<temp>.xyz, weights), o<register>.xyz = r<temp+1>.xxxx * scaled.
        let mut epilogue = vec![
            DP3 | (10 << 24),
            0x0010_0012,
            temp + 1,
            0x0010_0E46,
            temp,
            0x0000_4002,
            WEIGHTS[0].to_bits(),
            WEIGHTS[1].to_bits(),
            WEIGHTS[2].to_bits(),
            0,
            MUL | (10 << 24),
            0x0010_2072,
            register,
            0x0010_0006,
            temp + 1,
            0x0000_4002,
            scaled[0].to_bits(),
            scaled[1].to_bits(),
            scaled[2].to_bits(),
            0,
        ];
        epilogue.extend(keep_alpha(register, temp, mask));
        epilogue
    })
}

/// `mov o<register>.w, r<temp>.wwww` when the output declares its alpha lane.
fn keep_alpha(register: u32, temp: u32, mask: u32) -> Vec<u32> {
    const MOV: u32 = 0x36;
    if mask & 8 == 0 {
        return Vec::new();
    }
    vec![MOV | (5 << 24), 0x0010_2082, register, 0x0010_0FF6, temp]
}

/// Moves every write of pixel output `register` to the first of `extra` new temporary registers
/// and places `epilogue(temp, mask)` before each return, conditional or not, where `mask` is the
/// output's declared lanes. A program that does not declare the output with all of R, G and B is
/// returned unchanged.
fn redirect_output(
    container: &[u8],
    register: u32,
    extra: u32,
    epilogue: &dyn Fn(u32, u32) -> Vec<u32>,
) -> Result<Vec<u8>> {
    const OUTPUT: u32 = 2;
    const DCL_OUTPUT: u32 = 0x65;
    const DCL_TEMPS: u32 = 0x68;
    const RET: u32 = 0x3E;
    const RETC: u32 = 0x3F;
    let range = match chunk(container, b"SHEX")? {
        Some(range) => range,
        None => chunk(container, b"SHDR")?.context("shader has no program chunk")?,
    };
    let tokens: Vec<u32> = container[range]
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    ensure!(
        tokens.len() >= 2 && tokens[1] as usize == tokens.len(),
        "shader program length differs"
    );
    ensure!(
        tokens[0] >> 16 == 0,
        "output redirect needs a pixel program"
    );
    ensure!((1..=4).contains(&extra), "output redirect temporary count");
    let instructions = walk(&tokens)?;
    // dcl_output names its register in a one-dimensional immediate operand.
    let mask = instructions
        .iter()
        .filter(|i| i.opcode == DCL_OUTPUT && i.end - i.start == 3)
        .find(|i| (tokens[i.start + 1] >> 12) & 0xFF == OUTPUT && tokens[i.start + 2] == register)
        .map(|i| (tokens[i.start + 1] >> 4) & 0xF);
    let Some(mask) = mask.filter(|mask| mask & 7 == 7) else {
        return Ok(container.to_vec());
    };
    let temps = instructions
        .iter()
        .find(|i| i.opcode == DCL_TEMPS)
        .map_or(0, |i| tokens[i.start + 1]);
    ensure!(temps < 4096, "shader temporary count differs");
    let temp = temps;
    let epilogue = epilogue(temp, mask);
    let mut edited = tokens[..2].to_vec();
    let mut declared_temps = temps > 0;
    let mut moved = 0;
    for instruction in &instructions {
        let body = &tokens[instruction.start..instruction.end];
        if instruction.opcode == DCL_TEMPS {
            edited.extend([body[0], temps + extra]);
            continue;
        }
        if !is_declaration(instruction.opcode) && instruction.opcode != CUSTOM_DATA {
            if !declared_temps {
                edited.extend([DCL_TEMPS | (2 << 24), extra]);
                declared_temps = true;
            }
            if matches!(instruction.opcode, RET | RETC) {
                edited.extend(&epilogue);
            }
        }
        let at = edited.len();
        edited.extend(body);
        for operand in &instruction.operands {
            let token = tokens[operand.token_at];
            if (token >> 12) & 0xFF != OUTPUT {
                continue;
            }
            // A relatively indexed output could be the limited one, so it is refused.
            let [Some(index)] = operand.indices[..] else {
                bail!("output limit needs immediate output indices");
            };
            if tokens[index] != register {
                continue;
            }
            ensure!(
                !is_declaration(instruction.opcode),
                "output limit found an output in a declaration"
            );
            // Same token with the register file changed from output to temporary.
            edited[at + operand.token_at - instruction.start] = token & !(0xFF << 12);
            edited[at + index - instruction.start] = temp;
            moved += 1;
        }
    }
    ensure!(moved > 0, "pixel program never writes its declared output");
    edited[1] = u32::try_from(edited.len())?;
    rebuild(container, &edited, None)
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
