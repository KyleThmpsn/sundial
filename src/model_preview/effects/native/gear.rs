//! Recover the affine plate placement of a bounded per-pixel gear dye program.
use super::*;
use program::{Operand, Program};

type Affine = [f32; 3];

fn value(
    p: &Program,
    constants: &[[f32; 4]],
    operand: &Operand,
    lane: usize,
    before: usize,
    depth: usize,
) -> Option<Affine> {
    if depth > 64 || operand.modifier > 1 {
        return None;
    }
    let lane = operand.lanes[lane];
    let index = operand.indices.first().map_or(0, |i| i.base as usize);
    let result = match operand.kind {
        1 if lane < 2 && texcoord_input(p, index) => {
            let mut result = [0.0; 3];
            result[lane] = 1.0;
            result
        }
        4 => [0.0, 0.0, f32::from_bits(operand.literal[lane])],
        8 if index == 0 && operand.indices.get(1)?.relative.is_none() => [
            0.0,
            0.0,
            constants.get(operand.indices[1].base as usize)?[lane],
        ],
        8 if index == 11
            && operand.indices.get(1)?.relative.is_none()
            && operand.indices[1].base == 6 =>
        {
            [0.0, 0.0, if lane < 2 { 1.0 } else { 0.0 }]
        }
        0 | 2 => written(p, constants, (operand, index, lane), before, depth)?,
        _ => return None,
    };
    let result = if operand.modifier == 1 {
        result.map(|v| -v)
    } else {
        result
    };
    result.iter().all(|v| v.is_finite()).then_some(result)
}

/// Whether input register `index` is the texture coordinate the program reads: the fourth set
/// when it declares one, the first otherwise.
fn texcoord_input(p: &Program, index: usize) -> bool {
    let set = if p
        .inputs
        .iter()
        .any(|s| s.name == "TEXCOORD" && s.index == 3)
    {
        3
    } else {
        0
    };
    p.inputs
        .iter()
        .any(|s| s.register == index && s.name == "TEXCOORD" && s.index == set)
}

/// The affine value the last instruction before `before` that writes `lane` of a temporary or
/// output register left there, outside any loop.
fn written(
    p: &Program,
    constants: &[[f32; 4]],
    (operand, index, lane): (&Operand, usize, usize),
    before: usize,
    depth: usize,
) -> Option<Affine> {
    let (at, instruction) = p.instructions[..before]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, i)| {
            i.operands.first().is_some_and(|d| {
                d.kind == operand.kind
                    && d.indices[0].base as usize == index
                    && d.mask & (1 << lane) != 0
            })
        })?;
    let nesting = p.instructions[..at]
        .iter()
        .fold(0usize, |depth, i| match i.code {
            31 => depth + 1,
            21 => depth.saturating_sub(1),
            _ => depth,
        });
    if nesting != 0 {
        return None;
    }
    let read = |n| value(p, constants, &instruction.operands[n], lane, at, depth + 1);
    let result = match instruction.code {
        54 => read(1)?,
        0 => add(read(1)?, read(2)?),
        56 => multiply(read(1)?, read(2)?)?,
        50 => add(multiply(read(1)?, read(2)?)?, read(3)?),
        _ => return None,
    };
    let saturated = result[2] + result[0].min(0.0) + result[1].min(0.0) < 0.0
        || result[2] + result[0].max(0.0) + result[1].max(0.0) > 1.00001;
    (!(instruction.saturate && saturated)).then_some(result)
}

pub(super) fn stored_vertex_uv(p: &Program) -> Option<[f32; 4]> {
    let output = p
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 3)?;
    let lane = |lane| {
        let operand = Operand {
            kind: 2,
            indices: vec![program::Index {
                base: output.register as u32,
                relative: None,
            }],
            lanes: [0, 1, 2, 3],
            mask: 15,
            modifier: 0,
            literal: [0; 4],
        };
        value(p, &[], &operand, lane, p.instructions.len(), 0)
    };
    let x = lane(0)?;
    let y = lane(1)?;
    (x[1] == 0.0 && y[0] == 0.0 && x[0] > 0.0 && y[1] > 0.0).then_some([x[0], y[1], x[2], y[2]])
}

/// A translated color buffer can feed one constant texel to TEXCOORD8. Recover
/// that exact load without running the unrelated metadata and skeleton program.
/// Variable color lookups remain unavailable rather than receiving a made-up color.
pub(super) fn stored_vertex_color(
    manager: &PackageManager,
    bytes: &[u8],
    p: &Program,
) -> Result<Option<[f32; 4]>, String> {
    let Some(output) = p
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 8)
    else {
        return Ok(None);
    };
    let writes: Vec<_> = p
        .instructions
        .iter()
        .filter(|i| {
            i.operands
                .first()
                .is_some_and(|d| d.kind == 2 && d.indices[0].base as usize == output.register)
        })
        .collect();
    let unavailable = "The stored vertex color lookup is unavailable";
    if writes.len() != 1 {
        return Err(unavailable.into());
    }
    let instruction = writes[0];
    if instruction.code != 45 || instruction.saturate || instruction.offset != [0; 3] {
        return Err(unavailable.into());
    }
    let [dest, coordinates, resource] = instruction.operands.as_slice() else {
        return Err(unavailable.into());
    };
    if dest.mask != 15
        || coordinates.kind != 4
        || coordinates.literal != [0; 4]
        || coordinates.modifier != 0
        || resource.kind != 7
        || resource.indices[0].base != 0
        || resource.lanes != [0, 1, 2, 3]
        || resource.modifier != 0
        || !p
            .resources
            .iter()
            .any(|r| r.slot == 0 && !r.integer && r.dimension == 3)
    {
        return Err(unavailable.into());
    }
    let mut nesting = 0usize;
    for i in &p.instructions {
        if std::ptr::eq(i, instruction) {
            if nesting != 0 {
                return Err(unavailable.into());
            }
            break;
        }
        if i.code == 62 {
            return Err(unavailable.into());
        }
        match i.code {
            31 => nesting += 1,
            21 => nesting = nesting.saturating_sub(1),
            _ => {}
        }
    }
    let (count, rows) = super::super::vertex::table(bytes, 0x50, 0x8080_7211, 8, 32)?;
    let mut tag = None;
    for row in (0..count).map(|i| rows + i * 8) {
        if u32_at(bytes, row)? == 0 && tag.replace(u32_at(bytes, row + 4)?).is_some() {
            return Err("The vertex color texture slot is bound more than once".into());
        }
    }
    let tag = tag.ok_or("The vertex color texture is missing")?;
    let header = manager.read_tag(tag)?;
    if u32_at(&header, 4)? != 28 || u16_at(&header, 0x0E)? > 2048 || u16_at(&header, 0x10)? > 2048 {
        return Err("The vertex color texture format is unavailable".into());
    }
    let image = texture::load(manager, tag)?;
    // This is numeric vertex data, so no color-space transform is applied.
    Ok(Some(std::array::from_fn(|lane| {
        image.rgba[lane] as f32 / 255.0
    })))
}

fn add(a: Affine, b: Affine) -> Affine {
    std::array::from_fn(|i| a[i] + b[i])
}
fn multiply(a: Affine, b: Affine) -> Option<Affine> {
    if a[0] == 0.0 && a[1] == 0.0 {
        Some(b.map(|v| v * a[2]))
    } else if b[0] == 0.0 && b[1] == 0.0 {
        Some(a.map(|v| v * b[2]))
    } else {
        None
    }
}

pub(in crate::model_preview) fn map_transform(
    manager: &PackageManager,
    bytes: &[u8],
) -> Option<([f32; 4], Option<[f32; 4]>)> {
    let pixel = super::super::read::shader_bytes(manager, u32_at(bytes, 0x2C8).ok()?, 0).ok()?;
    let program = Program::read(&pixel, 0).ok()?;
    // The translated family selects six surfaces from three merged modern dye banks.
    if !program
        .buffers
        .iter()
        .any(|&(slot, size)| slot == 0 && size >= 63)
        || !program
            .instructions
            .iter()
            .flat_map(|i| &i.operands)
            .any(|v| v.kind == 8 && v.indices.get(1).is_some_and(|i| i.relative.is_some()))
    {
        return None;
    }
    // Translated placements are literals. Surface selection from the dye constant bank
    // is evaluated separately from these coordinate coefficients.
    let constants = super::super::read::stage_constants(manager, bytes, 0x2C8).unwrap_or_default();
    let coordinates = |slot| {
        let samples: Vec<_> = program
            .instructions
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                matches!(i.code, 69 | 70 | 72 | 73 | 74)
                    && i.operands
                        .get(2)
                        .is_some_and(|r| r.kind == 7 && r.indices[0].base == slot)
            })
            .collect();
        if samples.len() != 1 {
            return None;
        }
        let (at, sample) = samples[0];
        Some([
            value(&program, &constants, &sample.operands[1], 0, at, 0)?,
            value(&program, &constants, &sample.operands[1], 1, at, 0)?,
        ])
    };
    let plate = coordinates(0)?;
    if coordinates(1)? != plate
        || coordinates(2)? != plate
        || coordinates(3)? != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    {
        return None;
    }
    let [x, y] = plate;
    if x[1] != 0.0
        || y[0] != 0.0
        || x[0] <= 0.0
        || y[1] <= 0.0
        || x[2] < 0.0
        || y[2] < 0.0
        || x[0] + x[2] > 1.00001
        || y[1] + y[2] > 1.00001
    {
        return None;
    }
    let map = [1.0 / x[0], 1.0 / y[1], -x[2] / x[0], -y[2] / y[1]];
    // The compiled vertex equations consume raw native UVs. Decoding has already
    // applied the model's atlas placement, which need not be this material's plate.
    let vertex = super::super::read::shader_bytes(manager, u32_at(bytes, 0x48).ok()?, 1)
        .ok()
        .and_then(|bytes| Program::read_stored(&bytes).ok())
        .filter(super::motion::stored);
    let uv = match vertex {
        Some(vertex) => {
            let v = stored_vertex_uv(&vertex)?;
            Some([
                x[0] * v[0],
                y[1] * v[1],
                x[0] * v[2] + x[2],
                y[1] * v[3] + y[2],
            ])
        }
        None => None,
    };
    Some((map, uv))
}
