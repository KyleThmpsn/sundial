//! Preserve source SM5 instructions while adapting native binding contracts.
use super::*;
use crate::tiger::shader::dxbc;
pub(super) mod glow;

fn tokens(code: &[u8]) -> Result<Vec<u32>> {
    let range = dxbc::chunk(code, b"SHEX")?
        .or(dxbc::chunk(code, b"SHDR")?)
        .context("Source shader program is absent")?;
    ensure!(
        range.len().is_multiple_of(4),
        "Source shader program alignment differs"
    );
    let tokens = code[range]
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .collect::<Vec<_>>();
    ensure!(
        tokens.len() >= 2 && tokens[1] as usize == tokens.len(),
        "Source shader token extent differs"
    );
    Ok(tokens)
}

pub(super) fn compact(code: &[u8]) -> Result<Vec<u8>> {
    dxbc::rebuild(code, &tokens(code)?, None)
}

/// Translate checked depth and ambient irradiance vectors in the named transparent scope.
/// A declaration can include unused rows. Every actual read must have a qualified producer.
pub(super) fn depth(code: &[u8], binding: &super::expression::Depth) -> Result<Vec<u8>> {
    let words = tokens(code)?;
    let mut used = false;
    let mut occupied = false;
    for instruction in dxbc::walk(&words)? {
        for operand in instruction.operands {
            if operand.kind != 8 {
                continue;
            }
            ensure!(operand.indices.len() == 2, "Depth buffer dimensions differ");
            let slot = operand.indices[0].context("Relative constant-buffer identity")?;
            occupied |= words[slot] == binding.native && binding.source != binding.native;
            if words[slot] != binding.source {
                continue;
            }
            let row = operand.indices[1].context("Relative transparent depth vector")?;
            ensure!(
                if instruction.opcode == 0x59 {
                    (1..=6).contains(&words[row])
                } else {
                    words[row] < 6 && binding.vectors & (1 << words[row]) != 0
                },
                "Unsupported transparent scope vector"
            );
            used = true;
        }
    }
    ensure!(
        !used || !occupied,
        "Native depth buffer slot is already occupied"
    );
    if !used {
        return Ok(code.to_vec());
    }
    Ok(dxbc::patch(
        code,
        &dxbc::Remap {
            constant_buffers: vec![(binding.source, binding.native)],
            ..Default::default()
        },
    )?
    .bytecode)
}

/// A mapped rigid vertex can change only the packed bone selector in POSITION0.w.
/// Admit the checked integer recovery and refuse other uses of that lane or direct nonroot bones.
pub(super) fn rigid_indices(code: &[u8]) -> Result<()> {
    let signature = dxbc::signature(code, b"ISGN")?;
    ensure!(
        signature.iter().any(
            |(name, index, register, mask)| name.eq_ignore_ascii_case("POSITION")
                && *index == 0
                && *register == 0
                && *mask == 15
        ) && !signature
            .iter()
            .any(|(name, _, _, _)| name.eq_ignore_ascii_case("BLENDINDICES")
                || name.eq_ignore_ascii_case("BLENDWEIGHT")),
        "Mapped weapon shader needs a rigid position input"
    );
    let words = tokens(code)?;
    ensure!(
        words[0] == 0x00010050,
        "Mapped weapon shader is not vertex SM5"
    );
    for instruction in dxbc::walk(&words)? {
        if (0x58..=0x6A).contains(&instruction.opcode) {
            continue;
        }
        // Projection shaders can read a fixed texture texel. It has no vertex-dependent
        // address, unlike a stream carrying additional per-vertex deformation indices.
        ensure!(
            instruction.operands.iter().all(|operand| {
                !matches!(operand.kind, 7 | 30 | 31)
                    || (operand.kind == 7
                        && instruction.opcode == 0x2D
                        && instruction
                            .operands
                            .get(1)
                            .is_some_and(|address| address.kind == 4))
            }),
            "Mapped rigid vertex program reads indexed resource deformation"
        );
        for operand in &instruction.operands {
            let token = words[operand.token_at];
            if operand.kind == 8
                && operand
                    .indices
                    .first()
                    .copied()
                    .flatten()
                    .is_some_and(|at| words[at] == 10)
                && (token >> 25) & 7 == 0
            {
                let row = operand
                    .indices
                    .get(1)
                    .copied()
                    .flatten()
                    .context("Mapped bone row")?;
                ensure!(
                    words[row] < 3,
                    "Mapped shader has a direct nonroot bone read"
                );
            }
            if operand.kind != 1
                || !operand
                    .indices
                    .first()
                    .copied()
                    .flatten()
                    .is_some_and(|at| words[at] == 0)
            {
                continue;
            }
            let uses_w = match (token >> 2) & 3 {
                0 => token & (8 << 4) != 0,
                1 => (0..4).any(|lane| (token >> (4 + lane * 2)) & 3 == 3),
                2 => (token >> 4) & 3 == 3,
                _ => anyhow::bail!("Mapped position input has an unsupported lane selector"),
            };
            if !uses_w {
                continue;
            }
            let scalar = |index: usize, value: f32| -> bool {
                instruction.operands.get(index).is_some_and(|op| {
                    words[op.token_at] == 0x4001
                        && words.get(op.token_at + 1) == Some(&value.to_bits())
                })
            };
            ensure!(
                instruction.opcode == 0x32
                    && instruction.operands.len() == 4
                    && instruction.operands[1].token_at == operand.token_at
                    && token == 0x0010103A
                    && scalar(2, 32767.0)
                    && scalar(3, 0.1),
                "Mapped position.w has a use outside rigid bone-index recovery"
            );
        }
    }
    Ok(())
}

/// D1 bone matrices occupy b10. Native skinning appends the same matrices after
/// the eight model vectors in b11. Input/output signatures remain untouched.
pub(super) fn skinning(code: &[u8]) -> Result<Vec<u8>> {
    let mut words = tokens(code)?;
    let instructions = dxbc::walk(&words)?;
    let mut removed = None;
    let mut target = None;
    let mut count = 0;
    let mut offsets = std::collections::BTreeSet::new();
    for instruction in &instructions {
        for operand in &instruction.operands {
            if operand.kind != 8 {
                continue;
            }
            ensure!(
                operand.indices.len() == 2,
                "Source constant-buffer dimensions differ"
            );
            let index = operand.indices[0].context("Relative constant-buffer identity")?;
            if words[index] == 11 && instruction.opcode == 0x59 {
                ensure!(
                    target
                        .replace((
                            instruction.start,
                            operand.indices[1].context("Native model declaration extent")?
                        ))
                        .is_none(),
                    "Repeated model constant declaration"
                );
            }
            if words[index] != 10 {
                continue;
            }
            let token = words[operand.token_at];
            ensure!(
                (token >> 22) & 7 == 0 && matches!((token >> 25) & 7, 0 | 2 | 3),
                "Unsupported source bone index encoding"
            );
            let vector = index + 1;
            if instruction.opcode == 0x59 {
                ensure!(
                    words[vector] == 216,
                    "Source shader does not use the verified 72-bone human palette"
                );
                count = words[vector] + 8;
                ensure!(
                    removed
                        .replace(instruction.start..instruction.end)
                        .is_none(),
                    "Repeated source bone declaration"
                );
            } else {
                words[index] = 11;
                if (token >> 25) & 7 == 2 {
                    // A relative-only address has no immediate word. Add an
                    // immediate base before the intact relative operand.
                    words[operand.token_at] = (token & !(7 << 25)) | (3 << 25);
                    offsets.insert(vector);
                } else {
                    words[vector] = words[vector]
                        .checked_add(8)
                        .context("Bone constant offset overflow")?;
                }
            }
        }
    }
    if removed.is_some() {
        let (declaration, extent) = target.context("Source skinning has no model constants")?;
        ensure!(
            words[extent] <= 8,
            "Source model prefix exceeds native metadata"
        );
        words[extent] = count;
        words[declaration] |= 1 << 11;
    }
    let mut output = words[..2].to_vec();
    for instruction in instructions {
        if removed
            .as_ref()
            .is_some_and(|r| r.start == instruction.start)
        {
            continue;
        }
        let mut body = words[instruction.start..instruction.end].to_vec();
        let additions = offsets
            .range(instruction.start..instruction.end)
            .copied()
            .collect::<Vec<_>>();
        if !additions.is_empty() {
            for at in additions.into_iter().rev() {
                body.insert(at - instruction.start, 8);
            }
            ensure!(
                body.len() <= 127,
                "Expanded bone instruction exceeds SM5 length"
            );
            body[0] = (body[0] & !(127 << 24)) | ((body.len() as u32) << 24);
        }
        output.extend(body);
    }
    output[1] = u32::try_from(output.len())?;
    dxbc::rebuild(code, &output, None)
}

/// Source weighted signature paired with native layout 9, which stores no detail scale.
const WEIGHTED: [(&str, u32, u8); 7] = [
    ("POSITION", 0, 15),
    ("BLENDWEIGHT", 0, 15),
    ("BLENDINDICES", 0, 15),
    ("TEXCOORD", 0, 3),
    ("NORMAL", 0, 7),
    ("TANGENT", 0, 15),
    ("TEXCOORD", 2, 3),
];

/// Source unweighted signature paired with native layout 123, which stores no detail scale.
const UNWEIGHTED: [(&str, u32, u8); 5] = [
    ("POSITION", 0, 15),
    ("TEXCOORD", 0, 3),
    ("NORMAL", 0, 7),
    ("TANGENT", 0, 15),
    ("TEXCOORD", 2, 3),
];

/// A vertex program specialized for a stored layout without the detail scale its
/// source signature names.
pub(super) struct Specialized {
    pub code: Vec<u8>,
    /// The validated detail multiply became a base-UV move. An unused signature
    /// entry changes only the signature.
    pub approximated: bool,
}

/// Use the source convention for programs without a per-vertex detail scale when
/// the stored layout has none. Weighted layout 9 follows the six-input armor
/// program and unweighted layout 123 the four-input weapon program. Both copy the
/// transformed base UV to both output pairs. Specialize only this exact data flow,
/// never arbitrary reads of the absent register.
pub(super) fn without_detail_scale(code: &[u8], layout: u16) -> Result<Specialized> {
    let signature = dxbc::signature(code, b"ISGN")?;
    if !signature
        .iter()
        .any(|(name, index, _, _)| name.eq_ignore_ascii_case("TEXCOORD") && *index == 2)
    {
        return Ok(Specialized {
            code: code.to_vec(),
            approximated: false,
        });
    }
    let (expected, uv): (&[(&str, u32, u8)], u32) = match layout {
        9 => (&WEIGHTED[..], 3),
        123 => (&UNWEIGHTED[..], 1),
        _ => anyhow::bail!("Layout {layout} has no convention without a detail scale"),
    };
    let last = expected.len() - 1;
    let detail = u32::try_from(last)?;
    ensure!(
        signature.len() == expected.len()
            && signature
                .iter()
                .zip(expected.iter().copied())
                .enumerate()
                .all(
                    |(register, ((name, index, slot, mask), (wanted, semantic, lanes)))| {
                        name.eq_ignore_ascii_case(wanted)
                            && *index == semantic
                            && *slot == register as u32
                            && *mask == lanes
                    }
                ),
        "Missing detail scale requires the layout {layout} source vertex signature"
    );
    let range = dxbc::chunk(code, b"ISGN")?.context("Source input signature")?;
    let declaration = Payload(code[range].to_vec());
    for (register, (name, _, _)) in expected.iter().enumerate() {
        ensure!(
            declaration.u32(8 + register * 24 + 8)? == 0
                && declaration.u32(8 + register * 24 + 12)?
                    == if *name == "BLENDINDICES" { 1 } else { 3 },
            "Source vertex input component type differs for layout {layout}"
        );
    }
    let words = tokens(code)?;
    ensure!(
        words[0] == 0x00010050,
        "Source vertex program profile differs"
    );
    let instructions = dxbc::walk(&words)?;
    let mut output = words[..2].to_vec();
    let mut declared = 0;
    let mut reads = 0;
    for (index, instruction) in instructions.iter().enumerate() {
        let mut selected = false;
        for operand in &instruction.operands {
            if operand.kind != 1 {
                continue;
            }
            ensure!(
                operand.indices.len() == 1 && operand.indices[0].is_some(),
                "Relative source vertex input cannot omit detail scale"
            );
            selected |= words[operand.indices[0].unwrap()] == detail;
        }
        let body = &words[instruction.start..instruction.end];
        if !selected {
            output.extend_from_slice(body);
            continue;
        }
        if body == [0x0300005F, 0x00101032, detail] {
            declared += 1;
            continue;
        }
        ensure!(
            body.len() == 7
                && body[0] == 0x07000038
                && body[1] == 0x001020C2
                && body[3] == 0x00100406
                && body[5..] == [0x00101406, detail],
            "Missing detail scale has an unsupported shader consumer"
        );
        let previous = index.checked_sub(1).context("Missing base UV producer")?;
        let previous = &words[instructions[previous].start..instructions[previous].end];
        ensure!(
            previous
                == [
                    0x0B000032, 0x00100032, body[4], 0x00101046, uv, 0x00208046, 11, 6, 0x00208AE6,
                    11, 6,
                ],
            "Missing detail scale does not multiply transformed base UV"
        );
        ensure!(
            dxbc::signature(code, b"OSGN")?
                .iter()
                .any(
                    |(name, index, register, mask)| name.eq_ignore_ascii_case("TEXCOORD")
                        && *index == 3
                        && *register == body[2]
                        && *mask == 15
                ),
            "Missing detail scale output contract differs"
        );
        output.extend([0x05000036, body[1], body[2], body[3], body[4]]);
        reads += 1;
    }
    ensure!(
        matches!((declared, reads), (0, 0) | (1, 1))
            && declaration.u8(8 + last * 24 + 21)? == if reads == 0 { 0 } else { 3 },
        "Missing detail scale declaration or use count differs"
    );
    output[1] = u32::try_from(output.len())?;
    let mut result = dxbc::rebuild(code, &output, None)?;
    let range = dxbc::chunk(&result, b"ISGN")?.context("Rebuilt input signature")?;
    result[range.start..range.start + 4].copy_from_slice(&detail.to_le_bytes());
    // Keep the original string offsets, but remove the last signature record.
    result[range.start + 8 + last * 24..range.start + 8 + (last + 1) * 24].fill(0);
    let checksum = dxbc::checksum(&result)?;
    result[4..20].copy_from_slice(&checksum);
    Ok(Specialized {
        code: result,
        approximated: reads != 0,
    })
}

/// The reticle mask is an integer screen texture whose bit pattern selects the sight pixels
/// to keep. Source sight programs read its first channel, while every Shadowkeep sight
/// program reads the second (stock `81532F5D`, `81529F7A` and `8161EE21` load `t6.yxzw`
/// before the same frac-and-discard test). The native mask keeps its selection bits in
/// that channel, so each load from the mask resource swaps its first two components.
pub(super) fn reticle_mask(code: &[u8], slot: u32) -> Result<Vec<u8>> {
    let mut words = tokens(code)?;
    let mut rewritten = 0;
    for instruction in dxbc::walk(&words)? {
        if instruction.opcode != 45 {
            continue;
        }
        let resource = instruction
            .operands
            .get(2)
            .context("Source mask load has no resource operand")?;
        if resource.kind != 7 {
            continue;
        }
        let index = resource.indices[0].context("Relative mask resource")?;
        if words[index] != slot {
            continue;
        }
        let token = words[resource.token_at];
        ensure!(
            (token >> 2) & 3 == 1,
            "Source mask load selects its resource without a swizzle"
        );
        let swizzle = (token >> 4) & 0xFF;
        let swapped = (swizzle & 0xF0) | ((swizzle & 3) << 2) | ((swizzle >> 2) & 3);
        words[resource.token_at] = (token & !(0xFF << 4)) | (swapped << 4);
        rewritten += 1;
    }
    ensure!(
        rewritten > 0,
        "Source sight program never loads its reticle mask"
    );
    dxbc::rebuild(code, &words, None)
}

/// The initial-vector row a sight vertex program subtracts from the quad's z before the
/// reticle projection (`add r0.z, r0.z, -cb0[row].x`). Stock sight programs do the same with
/// their own per-weapon constant (`81532F60` row 11).
pub(super) fn sight_depth_row(code: &[u8]) -> Result<u8> {
    let words = tokens(code)?;
    let mut rows = Vec::new();
    for instruction in dxbc::walk(&words)? {
        if instruction.opcode != 0 || instruction.operands.len() != 3 {
            continue;
        }
        let (target, source) = (&instruction.operands[0], &instruction.operands[2]);
        if source.kind != 8 || target.kind != 0 || (words[target.token_at] >> 4) & 15 != 4 {
            continue;
        }
        let token = words[source.token_at];
        let negated = token >> 31 != 0 && (words[source.token_at + 1] >> 6) & 0xFF == 1;
        let [Some(buffer), Some(row)] = source.indices[..] else {
            continue;
        };
        if negated && words[buffer] == 0 {
            rows.push(words[row]);
        }
    }
    ensure!(
        rows.len() == 1,
        "Source sight program has {} depth subtractions",
        rows.len()
    );
    u8::try_from(rows[0]).context("Sight depth row exceeds the vector table")
}

/// Whether a pixel program writes exactly the two full gbuffer targets of a D1 pass.
fn two_targets(code: &[u8]) -> Result<bool> {
    let signatures = dxbc::signature(code, b"OSGN")?;
    Ok(signatures.len() == 2
        && signatures
            .iter()
            .enumerate()
            .all(|(i, (name, semantic, register, mask))| {
                name.eq_ignore_ascii_case("SV_Target")
                    && *semantic == i as u32
                    && *register == i as u32
                    && *mask == 15
            }))
}

/// D1 decals write premultiplied color with its inverse coverage, and two-target decals a
/// premultiplied packed normal as well, which is the target layout of native decals, so their
/// outputs stay as they are. A replacing decal comes from an unblended source state. Its zero
/// inverse coverage makes the native premultiplied blend replace the color.
pub(super) fn decal(code: &[u8], replace: bool) -> Result<Vec<u8>> {
    ensure!(two_targets(code)?, "Source decal output contract differs");
    let words = tokens(code)?;
    if !replace {
        return dxbc::rebuild(code, &words, None);
    }
    let instructions = dxbc::walk(&words)?;
    ensure!(
        instructions.iter().filter(|i| i.opcode == 0x3E).count() == 1,
        "Multiple decal returns need separate control-flow verification"
    );
    let mut output = words[..2].to_vec();
    for instruction in &instructions {
        if instruction.opcode == 0x3E {
            // mov o0.w, l(0)
            output.extend([0x05000036, 0x00102082, 0, 0x00004001, 0]);
        }
        output.extend_from_slice(&words[instruction.start..instruction.end]);
    }
    output[1] = u32::try_from(output.len())?;
    dxbc::rebuild(code, &output, None)
}

/// D1's two deferred outputs encode color/AO and normal/roughness/material code.
/// Native D2 retains the color and normal encodings in three render targets.
/// Supported source emissive profiles need a separate native emission value.
pub(super) fn deferred(code: &[u8], glow: &[glow::Profile]) -> Result<Vec<u8>> {
    ensure!(
        two_targets(code)?,
        "Source deferred output contract differs"
    );
    let mut words = tokens(code)?;
    ensure!(
        words[0] == 0x50,
        "Deferred translation requires an SM5 pixel program"
    );
    let instructions = dxbc::walk(&words)?;
    let declaration = instructions
        .iter()
        .find(|i| i.opcode == 0x68)
        .context("Source deferred temporary declaration")?;
    ensure!(
        declaration.end == declaration.start + 2,
        "Source temporary declaration size differs"
    );
    let first = words[declaration.start + 1];
    ensure!(first <= 4092, "Native shader temporary capacity exceeded");
    words[declaration.start + 1] = first + 4;
    for instruction in &instructions {
        for operand in &instruction.operands {
            if operand.kind != 2 {
                continue;
            }
            ensure!(
                operand.indices.len() == 1,
                "Source output dimensions differ"
            );
            let index = operand.indices[0].context("Relative deferred output")?;
            ensure!(words[index] <= 1, "Undeclared source deferred output");
            words[index] += first;
            words[operand.token_at] &= !(0xFF << 12);
        }
    }
    let mut output = words[..2].to_vec();
    let mut declared = false;
    let mut returned = false;
    for instruction in &instructions {
        if instruction.opcode == 0x65 {
            if !declared {
                for slot in 0..3 {
                    output.extend([0x03000065, 0x001020F2, slot]);
                }
                declared = true;
            }
            continue;
        }
        if instruction.opcode == 0x3E {
            ensure!(
                !returned,
                "Multiple deferred returns need separate control-flow verification"
            );
            returned = true;
            // The native colour target's alpha is an iridescence selector, not D1's material
            // code: stock gear `81532FC3` writes `(id + 128.5) / 256` for a selected
            // iridescence row and `0.2` scaled by its gearstack emission mask otherwise. A D1
            // code passed through (Iron Gjallarhorn 139 and 140, 0.546 and 0.55) selects rows
            // 11 and 12 of the iridescence lookup `80B35E2B`, the second a magenta
            // placeholder. The code stays in the register for the glow profiles below.
            output.extend([0x05000036, 0x00102082, 0, 0x00004001, 0x3E4CCCCD]);
            for (slot, mask, register, swizzle) in [
                (0, 7, first, 0xE4),
                (1, 7, first + 1, 0xE4),
                (2, 8, first, 0xFF),
            ] {
                output.extend([
                    0x05000036,
                    0x00102002 | (mask << 4),
                    slot,
                    0x00100006 | (swizzle << 4),
                    register,
                ]);
            }
            output.extend([0x05000036, 0x00102082, 1, 0x00004001, 0]);
            output.extend([0x08000036, 0x00102072, 2, 0x00004002, 0, 0x3F000000, 0, 0]);
            glow::append(&mut output, first, glow);
        }
        output.extend_from_slice(&words[instruction.start..instruction.end]);
    }
    ensure!(
        declared && returned,
        "Source deferred declarations or return are absent"
    );
    output[1] = u32::try_from(output.len())?;
    let mut signature = vec![3, 0, 0, 0, 8, 0, 0, 0];
    for slot in 0..3u32 {
        for word in [80, slot, 64, 3, slot, 15] {
            signature.extend(word.to_le_bytes());
        }
    }
    signature.extend(b"SV_Target\0");
    while !signature.len().is_multiple_of(4) {
        signature.push(0);
    }
    dxbc::rebuild(code, &output, Some(&signature))
}
