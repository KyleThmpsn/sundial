//! Evaluate structurally validated Shadowkeep opaque surface programs before studio lighting.
//! Explicit image roles and material UV equations stay in the original pixel dependency graph.
use super::*;
use program::{Instruction, Operand, Program as Code};
mod decal;

fn literal(operand: &Operand, lane: usize, value: f32) -> bool {
    operand.kind == 4
        && operand.modifier == 0
        && operand.literal[operand.lanes[lane]] == value.to_bits()
}

fn output(code: &Code, target: u32, lane: usize) -> Option<(usize, &Instruction)> {
    code.instructions.iter().enumerate().rev().find(|(_, row)| {
        row.operands.first().is_some_and(|o| {
            o.kind == 2 && o.indices[0].base == target && o.mask & (1 << lane) != 0
        })
    })
}

fn producer<'a>(
    code: &'a Code,
    source: &Operand,
    lane: usize,
    before: usize,
) -> Option<(usize, &'a Instruction, usize)> {
    if source.kind != 0
        || source.modifier != 0
        || source.indices.len() != 1
        || source.indices[0].relative.is_some()
    {
        return None;
    }
    let lane = source.lanes[lane];
    code.instructions[..before]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, row)| {
            let d = row.operands.first()?;
            (d.kind == 0
                && d.indices[0].base == source.indices[0].base
                && d.mask & (1 << lane) != 0)
                .then_some((at, row, lane))
        })
}

fn normal_contract(code: &Code) -> bool {
    let Some((at, row)) = output(code, 1, 0) else {
        return false;
    };
    normal_vector(code, at, row)
}

fn normal_vector(code: &Code, at: usize, row: &Instruction) -> bool {
    if row.code != 50
        || !row.saturate
        || row.operands[0].mask != 7
        || !(0..3).all(|lane| literal(&row.operands[3], lane, 0.5))
    {
        return false;
    }
    let Some((radius_at, radius, lane)) = producer(code, &row.operands[2], 0, at) else {
        return false;
    };
    (1..3).all(|axis| {
        producer(code, &row.operands[2], axis, at)
            .is_some_and(|(other_at, _, other_lane)| other_at == radius_at && other_lane == lane)
    }) && radius.code == 50
        && !radius.saturate
        && literal(&radius.operands[2], lane, 0.125)
        && literal(&radius.operands[3], lane, 0.375)
}

fn visibility_contract(code: &Code) -> bool {
    let Some((_, y)) = output(code, 2, 1) else {
        return false;
    };
    let Some((_, z)) = output(code, 2, 2) else {
        return false;
    };
    z.code == 54
        && literal(&z.operands[1], 2, 0.0)
        && ((y.code == 56
            && !y.saturate
            && (literal(&y.operands[1], 1, 0.5) || literal(&y.operands[2], 1, 0.5)))
            || opaque::intensity::recover_surface(code))
}

fn supported(code: &Code, decal: bool) -> bool {
    code.outputs.len() == 3
        && (0..3).all(|target| {
            code.outputs.iter().any(|s| {
                s.name == "SV_TARGET" && s.index == target && s.register == target as usize
            })
        })
        && code.buffers.iter().all(|&(slot, _)| slot == 0)
        && (1..=9).contains(&code.resources.len())
        && code
            .resources
            .iter()
            .all(|r| r.dimension == 3 && !r.integer)
        && !code.instructions.iter().any(|i| {
            (!decal && matches!(i.code, 13 | 18 | 21 | 22 | 31 | 48))
                || i.operands
                    .iter()
                    .any(|o| o.indices.iter().any(|i| i.relative.is_some()))
        })
        && (decal || normal_contract(code) && visibility_contract(code))
}

/// Geometry decoding already applies the model placement. Compose a material's original
/// vertex UV equations with its inverse, so the pixel stage receives the original varying.
fn vertex_uv(
    manager: &PackageManager,
    bytes: &[u8],
    model_uv: [f32; 4],
) -> Result<Option<[f32; 4]>, String> {
    let tag = u32_at(bytes, 0x48)?;
    if matches!(tag, 0 | u32::MAX) {
        return Ok(Some([1.0, 1.0, 0.0, 0.0]));
    }
    if model_uv.iter().any(|v| !v.is_finite()) || model_uv[..2].iter().any(|v| v.abs() < 1e-8) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, tag, 1)?;
    let Ok(mut code) = Code::read_stored(&raw) else {
        return Ok(None);
    };
    let constants = super::super::read::stage_constants(manager, bytes, 0x48)?;
    gear::bind_model_uv(&mut code, model_uv);
    let Some(output) = code
        .outputs
        .iter()
        .find(|s| s.name == "TEXCOORD" && s.index == 3)
    else {
        return Ok(None);
    };
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
    let Some(x) = gear::value(&code, &constants, &operand, 0, code.instructions.len(), 0) else {
        return Ok(None);
    };
    let Some(y) = gear::value(&code, &constants, &operand, 1, code.instructions.len(), 0) else {
        return Ok(None);
    };
    if x[1] != 0.0 || y[0] != 0.0 {
        return Ok(None);
    }
    let scale = [x[0] / model_uv[0], y[1] / model_uv[1]];
    let uv = [
        scale[0],
        scale[1],
        x[2] - scale[0] * model_uv[2],
        y[2] - scale[1] * model_uv[3],
    ];
    Ok(uv.iter().all(|v| v.is_finite()).then_some(uv))
}

pub(in crate::model_preview) fn load(
    manager: &PackageManager,
    tag: u32,
    objects: ObjectInputs<'_>,
    dye: u8,
    uv: [f32; 4],
    model: &mut Model,
) -> Result<Option<Material>, String> {
    if matches!(tag, 0 | u32::MAX) {
        return Ok(None);
    }
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let pixel = u32_at(&bytes, 0x2C8)?;
    if matches!(pixel, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, pixel, 0)?;
    let Ok(code) = Code::read(&raw, 0) else {
        return Ok(None);
    };
    let decal = matches!(bytes[0x20], 0x96 | 0x97) && decal::supported(&code);
    if !supported(&code, decal) {
        return Ok(None);
    }
    let explicit = resource::explicit(&bytes, 0x2D0)?;
    if code
        .resources
        .iter()
        .any(|r| !explicit.contains_key(&r.slot))
    {
        return Ok(None);
    }
    let Some(uv) = vertex_uv(manager, &bytes, uv)? else {
        return Ok(None);
    };
    let intensity = opaque::intensity::recover_surface(&code);
    let mut material = load_program(
        manager,
        tag,
        &bytes,
        objects,
        dye,
        model,
        (code, true, true),
    )?;
    let native = material.native.as_mut().unwrap();
    native.opaque_uv = Some(uv);
    native.intensity = intensity;
    native.decal = decal;
    native.ambient_power = opaque::intensity::ambient_power(manager);
    Ok(Some(material))
}

pub(in crate::model_preview) struct Sample {
    pub surface: shader::Sample,
    pub normal: [f32; 3],
    pub ambient: f32,
    pub coverage: f32,
}

pub(in crate::model_preview) fn sample(
    model: &Model,
    material: &Material,
    constants: &Frame,
    gear: &shader::Bindings<'_>,
    pixel: Pixel<'_>,
) -> Option<Sample> {
    let native = material.native.as_ref().filter(|n| n.deferred)?;
    let mut targets = shade::outputs(model, material, constants, gear, pixel)?;
    if targets[..3].iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let coverage = if native.decal {
        let coverage = (1.0 - targets[0][3]).clamp(0.0, 1.0);
        if coverage <= 1e-8 {
            return Some(decal::empty());
        }
        decal::unpack(&mut targets, pixel, coverage)?;
        coverage
    } else {
        1.0
    };
    let packed = std::array::from_fn::<_, 3, _>(|i| targets[1][i] - 0.5);
    let radius = packed.iter().map(|v| v * v).sum::<f32>().sqrt();
    if radius < 1e-8 {
        return None;
    }
    let ambient = native.ambient_power.map_or_else(
        || (2.0 * targets[2][1]).clamp(0.0, 1.0) * targets[2][3].clamp(0.0, 1.0),
        |power| ambient_visibility(targets[2][1], targets[2][3], power),
    );
    Some(Sample {
        surface: shader::Sample {
            albedo: std::array::from_fn(|i| targets[0][i].max(0.0)),
            roughness: 1.0 - ((radius - 0.375) * 8.0).clamp(0.0, 1.0),
            metal: targets[2][0].clamp(0.0, 1.0),
            ao: ambient,
            emission: if native.intensity {
                let intensity = opaque::intensity::decode(targets[2][1]);
                std::array::from_fn(|i| targets[0][i].max(0.0) * intensity)
            } else {
                [0.0; 3]
            },
        },
        normal: packed.map(|v| v / radius),
        ambient,
        coverage,
    })
}
