//! D1 ribbons duplicate per-vertex records. Native ribbons publish a Float4 buffer instead.
use super::*;
use std::collections::BTreeSet;

/// `token` is the instruction's first word. Custom data is a declaration only in class 3, an
/// immediate constant buffer.
fn declaration(op: u32, token: u32) -> bool {
    (op == 0x35 && token >> 11 == 3)
        || (0x58..=0x6A).contains(&op)
        || (0x8F..=0xA2).contains(&op)
        || (0xAA..=0xB0).contains(&op)
}

/// Comment, debug and opaque custom data are neither declarations nor code. The D1 ribbon
/// vertex shaders place one opaque block before their declarations and three after their code.
/// Each stays where it was, in the copied declarations or the copied body.
fn neutral(op: u32, token: u32) -> bool {
    op == 0x35 && token >> 11 != 3
}

fn words(code: &[u8]) -> Result<Vec<u32>> {
    let range = dxbc::chunk(code, b"SHEX")?
        .or(dxbc::chunk(code, b"SHDR")?)
        .context("Particle vertex program is absent")?;
    ensure!(
        range.len().is_multiple_of(4),
        "Particle shader alignment differs"
    );
    let words = code[range]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().expect("four-byte word")))
        .collect::<Vec<_>>();
    ensure!(
        words.len() >= 2 && words[0] == 0x10050 && words[1] as usize == words.len(),
        "Particle shader is not a complete SM5 vertex program"
    );
    Ok(words)
}

fn load_inputs(
    code: &[u8],
    rows: u8,
    values: usize,
    inputs: &BTreeMap<u32, u32>,
) -> Result<Vec<u8>> {
    let signature = dxbc::chunk(code, b"ISGN")?.context("Particle input signature is absent")?;
    let signature_data = Payload(code[signature].to_vec());
    let mut vertex_id = None;
    for index in 0..usize::try_from(signature_data.u32(0)?)? {
        let at = 8 + index * 24;
        let register = signature_data.u32(at + 16)?;
        if inputs.contains_key(&register) {
            ensure!(
                signature_data.u32(at + 8)? == 0 && signature_data.u32(at + 12)? == 3,
                "Particle appearance input is not an ordinary Float4"
            );
        } else {
            ensure!(
                signature_data.u32(at + 8)? == 6
                    && signature_data.u32(at + 12)? == 1
                    && signature_data.u8(at + 20)? == 1
                    && vertex_id.replace(register).is_none(),
                "Ribbon needs one vertex ID and only appearance inputs"
            );
        }
    }
    let vertex_id = vertex_id.context("Ribbon vertex ID is absent")?;
    let mut source = words(code)?;
    let instructions = dxbc::walk(&source)?;
    let first = instructions
        .iter()
        .position(|i| {
            !declaration(i.opcode, source[i.start]) && !neutral(i.opcode, source[i.start])
        })
        .context("Particle vertex program is empty")?;
    ensure!(
        instructions[first..]
            .iter()
            .all(|i| !declaration(i.opcode, source[i.start])),
        "Particle shader has a late declaration"
    );
    let temps = instructions
        .iter()
        .filter(|i| i.opcode == 0x68)
        .collect::<Vec<_>>();
    let [temps] = temps.as_slice() else {
        anyhow::bail!("Particle shader temporary declaration differs");
    };
    ensure!(
        temps.end - temps.start == 2,
        "Particle temporary declaration width differs"
    );
    let begin = source[temps.start + 1];
    let extra = u32::try_from(inputs.len())?;
    let index = begin + extra;
    ensure!(index < 4096, "Ribbon exceeds native temporary capacity");
    ensure!(
        values < 256,
        "Ribbon draw offset exceeds material vector capacity"
    );
    let mapping = inputs
        .keys()
        .enumerate()
        .map(|(i, reg)| (*reg, begin + i as u32))
        .collect::<BTreeMap<_, _>>();
    let mut cb = false;
    for instruction in &instructions {
        ensure!(
            !matches!(instruction.opcode, 0x58 | 0x5A | 0x5B),
            "Ribbon source uses a resource or indexed-input declaration"
        );
        for operand in &instruction.operands {
            ensure!(
                !matches!(operand.kind, 6 | 7),
                "Ribbon source already consumes a texture or sampler"
            );
            if operand.kind == 8 && instruction.opcode == 0x59 {
                ensure!(
                    operand.indices.len() == 2,
                    "Ribbon buffer dimensions differ"
                );
                let slot = operand.indices[0].context("Relative ribbon buffer identity")?;
                if source[slot] == 0 {
                    let count = operand.indices[1].context("Relative ribbon buffer capacity")?;
                    ensure!(
                        !cb && source[count] as usize <= values,
                        "Ribbon initial constants differ"
                    );
                    source[count] = u32::try_from(values + 1)?;
                    cb = true;
                }
            }
            if operand.kind != 1 {
                continue;
            }
            ensure!(operand.indices.len() == 1, "Ribbon input dimensions differ");
            let at = operand.indices[0].context("Relative ribbon input")?;
            let register = source[at];
            ensure!(
                inputs.contains_key(&register) || register == vertex_id,
                "Unknown ribbon input"
            );
            if !declaration(instruction.opcode, source[instruction.start])
                && let Some(&temporary) = mapping.get(&register)
            {
                source[operand.token_at] &= !(0xFF << 12);
                source[at] = temporary;
            }
        }
    }
    ensure!(cb, "Ribbon material constant buffer is absent");
    source[temps.start + 1] = index + 1;
    let mut output = source[..2].to_vec();
    for instruction in &instructions[..first] {
        if instruction.opcode == 0x5F {
            ensure!(
                instruction.operands.len() == 1
                    && instruction.operands[0].kind == 1
                    && inputs.contains_key(
                        &source[instruction.operands[0].indices[0]
                            .context("Relative appearance declaration")?]
                    ),
                "Ribbon appearance declaration differs"
            );
            continue;
        }
        output.extend_from_slice(&source[instruction.start..instruction.end]);
    }
    // Buffer<float4> t0, supplied by extern 26 at resource index zero.
    output.extend([0x04000858, 0x00107000, 0, 0x00005555]);
    // r[index].y = uint(cb0[values].x), the draw's Float4 offset in the native buffer.
    output.extend([0x0600001C, 0x00100022, index, 0x0020800A, 0, values as u32]);
    // r[index].x = vertex_id / 2, preserving the original ID for the source equations.
    output.extend([
        0x07000055, 0x00100012, index, 0x0010100A, vertex_id, 0x00004001, 1,
    ]);
    // Each record has the original base row count, without the source's duplicate copy.
    output.extend([
        0x09000052,
        0x00100012,
        index,
        0x0010000A,
        index,
        0x00004001,
        u32::from(rows),
        0x0010001A,
        index,
    ]);
    for (&register, &row) in inputs {
        output.extend([
            0x0700001E, 0x00100042, index, 0x0010000A, index, 0x00004001, row,
        ]);
        output.extend([
            0x8900002D,
            0x80000042,
            0x00155543,
            0x001000F2,
            mapping[&register],
            0x00100AA6,
            index,
            0x00107E46,
            0,
        ]);
    }
    output.extend_from_slice(&source[instructions[first].start..]);
    output[1] = u32::try_from(output.len())?;
    let mut result = dxbc::rebuild(code, &output, None)?;
    // Retain semantic identities for the native dynamic layout, marking ordinary inputs
    // unused just as native buffer-fed ribbon signatures do. Outputs keep their exact bytes.
    let signature = dxbc::chunk(&result, b"ISGN")?.context("Rebuilt ribbon signature")?;
    for position in 0..usize::try_from(signature_data.u32(0)?)? {
        let at = 8 + position * 24;
        if inputs.contains_key(&signature_data.u32(at + 16)?) {
            result[signature.start + at + 21] = 0;
        }
    }
    let digest = dxbc::checksum(&result)?;
    result[4..20].copy_from_slice(&digest);
    Ok(result)
}

pub(super) fn bind(material: &mut Material, definition: &Payload) -> Result<()> {
    let rows = definition.u8(0x10A)?;
    let mode = definition.u8(0x114)?;
    let copies = definition.u8(0x10F)?;
    ensure!(
        (1..=16).contains(&rows) && matches!(mode, 0..=3 | 5),
        "Unqualified particle appearance geometry {mode} with {rows} rows"
    );
    ensure!(
        usize::from(definition.u8(0x10B)?) == usize::from(rows) * usize::from(copies),
        "Source appearance stride does not match its copies"
    );
    let shader_index = material
        .shaders
        .iter()
        .position(|v| v.stage == Stage::Vertex)
        .context("Particle vertex shader is absent")?;
    let shader = &material.shaders[shader_index];
    let signature = dxbc::signature(&shader.bytecode, b"ISGN")?;
    let mut inputs = BTreeMap::new();
    for (name, row, register, mask) in &signature {
        if name.eq_ignore_ascii_case("TEXCOORD") {
            ensure!(
                *row < u32::from(rows) && *mask == 15,
                "Particle shader exceeds its appearance rows"
            );
            ensure!(
                inputs.insert(*register, *row).is_none(),
                "Particle input register is duplicated"
            );
        } else {
            ensure!(
                name.to_ascii_uppercase().starts_with("SV_")
                    || mode == 1 && name.eq_ignore_ascii_case("POSITION") && *row < 3,
                "Unqualified particle geometry input"
            );
        }
    }
    ensure!(
        inputs.values().copied().collect::<BTreeSet<_>>() == (0..u32::from(rows)).collect(),
        "Particle shader appearance inputs differ from the definition"
    );
    // Trail geometry 5 draws its copied records per instance, as ordinary particles do.
    if !matches!(mode, 2 | 3) {
        ensure!(
            copies == 1,
            "Ordinary particles cannot duplicate appearance records"
        );
        material.appearance_rows = Some(rows);
        return Ok(());
    }
    ensure!(
        copies == 2,
        "Ribbon source does not have paired vertex records"
    );
    let mut payload = material.payload.clone();
    let mut values = vectors(&payload, 0x98)?;
    let row =
        u8::try_from(values.len() / 16).context("Ribbon material has no free constant row")?;
    let bytecode = load_inputs(&shader.bytecode, rows, values.len() / 16, &inputs)?;
    let mut code = payload.0[payload.array_range(0x68, 1, Some(0x80800009))?].to_vec();
    code.extend([0x3F, 26, 0, 0x47, 2 << 5, 0x3C, 26, 76, 0x43, row]);
    values.extend([0; 16]);
    append(&mut payload.0, 0x68, 0x80800009, &code, 1)?;
    append(&mut payload.0, 0x98, 0x80800090, &values, 16)?;
    material.payload = payload;
    material.shaders[shader_index].bytecode = bytecode;
    material.appearance_rows = Some(rows);
    material.ribbon = true;
    Ok(())
}
