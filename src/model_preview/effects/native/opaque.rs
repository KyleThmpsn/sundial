//! Retain a bounded additive RGB consumer after the reconstructed gear base.
use super::*;
use program::{Instruction, Operand, Program as Code};
mod gain;
pub(super) mod intensity;
mod paint;
pub(super) use gain::Gain;
pub(super) use paint::Paint;
pub(in crate::model_preview) use paint::legacy::Normal as LegacyNormal;

pub(in crate::model_preview) fn load_normal(
    manager: &PackageManager,
    tag: u32,
    objects: ObjectInputs<'_>,
    surface: u8,
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
    let constants = super::super::read::stage_constants(manager, &bytes, 0x2C8)?;
    let Some(normal) = paint::legacy::recover(&code, &constants, surface) else {
        return Ok(None);
    };
    let globals = crate::dyes::material::global_channels(manager);
    let program = Program::material(&bytes, 0x2C8, &globals, objects, surface, constants.len())?;
    Ok(Some(Material {
        kind: Kind::Native,
        constants,
        program,
        normal: Some(normal),
        samplers: texture::material_samplers(manager, tag),
        ..Default::default()
    }))
}

fn read_eq(a: &Operand, x: usize, b: &Operand, y: usize) -> bool {
    a.kind == b.kind
        && a.modifier == b.modifier
        && a.lanes[x] == b.lanes[y]
        && a.indices.len() == b.indices.len()
        && a.indices
            .iter()
            .zip(&b.indices)
            .all(|(a, b)| a.base == b.base && a.relative.is_none() && b.relative.is_none())
}
fn rgb_eq(a: &Operand, b: &Operand) -> bool {
    (0..3).all(|lane| read_eq(a, lane, b, lane))
}
fn literal(v: &Operand, lane: usize, value: f32) -> bool {
    v.kind == 4 && v.modifier == 0 && v.literal[v.lanes[lane]] == value.to_bits()
}

/// The native ten-operation bounded sum, matched by operand dependencies, not registers.
fn base_at(rows: &[Instruction]) -> Option<&Operand> {
    if rows.len() != 10
        || rows
            .iter()
            .map(|i| i.code)
            .ne([0, 52, 52, 0, 0, 50, 52, 52, 52, 14])
        || rows
            .iter()
            .enumerate()
            .any(|(at, i)| i.saturate != (at == 3))
    {
        return None;
    }
    let v: Vec<_> = rows.iter().map(|i| i.operands.as_slice()).collect();
    let sum = &v[0][0];
    let base = &v[0][1];
    let extra = &v[0][2];
    let scalar = &v[1][0];
    let lane = scalar.mask.trailing_zeros() as usize;
    if lane >= 4
        || scalar.mask.count_ones() != 1
        || base.kind != 0
        || base.modifier != 0
        || [sum, &v[5][0], &v[9][0]]
            .iter()
            .any(|d| d.kind != 0 || d.mask != 7)
        || [1, 2, 3, 4, 6, 7, 8]
            .iter()
            .any(|&at| !read_eq(&v[at][0], lane, scalar, lane) || v[at][0].mask != scalar.mask)
        || !read_eq(&v[1][1], lane, sum, 1)
        || !read_eq(&v[1][2], lane, sum, 0)
        || !read_eq(&v[2][1], lane, sum, 2)
        || !read_eq(&v[2][2], lane, scalar, lane)
        || !read_eq(&v[3][1], lane, scalar, lane)
        || !literal(&v[3][2], lane, -1.0)
        || !literal(&v[4][2], lane, 1.0)
        || v[4][1].modifier != 1
        || {
            let mut positive = v[4][1].clone();
            positive.modifier = 0;
            !read_eq(&positive, lane, scalar, lane)
        }
        || !rgb_eq(&v[5][1], base)
        || !rgb_eq(&v[5][3], extra)
        || !(0..3).all(|i| read_eq(&v[5][2], i, scalar, lane))
        || !read_eq(&v[6][1], lane, &v[5][0], 1)
        || !read_eq(&v[6][2], lane, &v[5][0], 0)
        || !read_eq(&v[7][1], lane, &v[5][0], 2)
        || !read_eq(&v[7][2], lane, scalar, lane)
        || !read_eq(&v[8][1], lane, scalar, lane)
        || !literal(&v[8][2], lane, 1.0)
        || !rgb_eq(&v[9][1], &v[5][0])
        || !(0..3).all(|i| read_eq(&v[9][2], i, scalar, lane))
    {
        return None;
    }
    Some(base)
}

fn reads(i: &Instruction, source: usize, lanes: u8, needed: &mut [u8; 32]) -> Result<(), String> {
    let s = &i.operands[source];
    if s.indices.iter().any(|v| v.relative.is_some()) {
        return Err("Opaque color uses a relative shader address".into());
    }
    if s.kind == 2 {
        return Err("Opaque color reads a previously written render target".into());
    }
    if s.kind != 0 {
        return Ok(());
    }
    let lanes = match i.code {
        15..=17 => (1 << (i.code - 13)) - 1,
        69 | 72 | 73 if source == 1 => 3,
        73 if source >= 4 => 3,
        72 if source == 4 => 1,
        _ => lanes,
    };
    for lane in 0..4 {
        if lanes & (1 << lane) != 0 {
            needed[s.indices[0].base as usize] |= 1 << s.lanes[lane];
        }
    }
    Ok(())
}

struct Slice {
    code: Code,
    at: usize,
    base: Operand,
    intensity: bool,
    ambient: bool,
}

fn slice(
    mut code: Code,
    allow_intensity: bool,
    allow_ambient: bool,
) -> Result<Option<Slice>, String> {
    if code.outputs.len() != 3
        || !(0..3).all(|target| {
            code.outputs.iter().any(|s| {
                s.name == "SV_TARGET" && s.index == target && s.register == target as usize
            })
        })
        || code.inputs.iter().any(|s| s.register == 15)
    {
        return Ok(None);
    }
    let Some((at, base)) = code
        .instructions
        .windows(10)
        .enumerate()
        .find_map(|(at, rows)| base_at(rows).map(|base| (at, base.clone())))
    else {
        return Ok(None);
    };
    let mut seed = base.clone();
    seed.lanes = [0, 1, 2, 3];
    seed.mask = base.lanes[..3]
        .iter()
        .fold(0, |mask, lane| mask | 1 << lane);
    // The recovered native base is RGB. Reject a permutation instead of reseeding wrong lanes.
    if base.lanes[..3] != [0, 1, 2] {
        return Err("Opaque base color permutes its RGB lanes".into());
    }
    let input = Operand {
        kind: 1,
        indices: vec![program::Index {
            base: 15,
            relative: None,
        }],
        lanes: [0, 1, 2, 3],
        mask: 15,
        modifier: 0,
        literal: [0; 4],
    };
    code.instructions.insert(
        at,
        Instruction {
            code: 54,
            saturate: false,
            nonzero: false,
            operands: vec![seed, input],
            offset: [0; 3],
        },
    );
    let (mut selected, intensity, ambient) = retain(&code, allow_intensity, allow_ambient)?;
    if !selected
        .iter()
        .flat_map(|i| &i.operands)
        .any(|v| v.kind == 1 && v.indices[0].base == 15)
    {
        return Ok(None);
    }
    if !selected.iter().any(|i| matches!(i.code, 69 | 72 | 73)) {
        return Ok(None);
    }
    selected.push(Instruction {
        code: 62,
        saturate: false,
        nonzero: false,
        operands: Vec::new(),
        offset: [0; 3],
    });
    code.instructions = selected;
    code.outputs.retain(|s| s.index == 0 && s.register == 0);
    code.resources.retain(|r| {
        code.instructions
            .iter()
            .flat_map(|i| &i.operands)
            .any(|v| v.kind == 7 && v.indices[0].base as usize == r.slot)
    });
    code.buffers.retain(|&(buffer, _)| {
        code.instructions
            .iter()
            .flat_map(|i| &i.operands)
            .any(|v| v.kind == 8 && v.indices[0].base as usize == buffer)
    });
    code.recover_derivatives();
    Ok(Some(Slice {
        code,
        at,
        base,
        intensity,
        ambient,
    }))
}

fn retain(
    code: &Code,
    allow_intensity: bool,
    allow_ambient: bool,
) -> Result<(Vec<Instruction>, bool, bool), String> {
    let intensity = allow_intensity && intensity::recover(code);
    if intensity {
        if allow_ambient && let Ok(selected) = select(code, true, true) {
            return Ok((selected, true, true));
        }
        if let Ok(selected) = select(code, true, false) {
            return Ok((selected, true, false));
        }
    }
    Ok((select(code, false, false)?, false, false))
}

struct Needed {
    temps: [u8; 32],
    outputs: [u8; 16],
}

impl Needed {
    fn take(&mut self, i: &Instruction, depth: usize) -> Result<Option<Instruction>, String> {
        if matches!(i.code, 13 | 18 | 21 | 31 | 62) {
            return Ok(None);
        }
        if matches!(i.code, 38 | 77 | 78) {
            if i.operands
                .iter()
                .take(2)
                .any(|d| d.kind == 0 && self.temps[d.indices[0].base as usize] & d.mask != 0)
            {
                return Err("Opaque color requires a multi-output instruction".into());
            }
            return Ok(None);
        }
        let destination = &i.operands[0];
        let index = destination.indices.first().map_or(0, |i| i.base as usize);
        let wanted = match destination.kind {
            0 => &mut self.temps[index],
            2 => &mut self.outputs[index],
            _ => return Ok(None),
        };
        let lanes = *wanted & destination.mask;
        if lanes == 0 {
            return Ok(None);
        }
        if depth != 0 {
            return Err("Opaque color depends on a shader branch".into());
        }
        *wanted &= !lanes;
        let mut kept = i.clone();
        kept.operands[0].mask = lanes;
        for source in 1..i.operands.len() {
            if !matches!(i.operands[source].kind, 6 | 7) {
                reads(i, source, lanes, &mut self.temps)?;
            }
        }
        Ok(Some(kept))
    }
}

fn select(code: &Code, intensity: bool, ambient: bool) -> Result<Vec<Instruction>, String> {
    let mut branch = 0usize;
    let mut depths = Vec::new();
    for i in &code.instructions {
        if i.code == 21 {
            branch -= 1;
        }
        depths.push(branch);
        if i.code == 31 {
            branch += 1;
        }
    }
    let mut needed = Needed {
        temps: [0; 32],
        outputs: [0; 16],
    };
    needed.outputs[0] = 7;
    if intensity {
        needed.outputs[2] = if ambient { 10 } else { 2 };
    }
    let mut selected = Vec::new();
    for (at, i) in code.instructions.iter().enumerate().rev() {
        if let Some(kept) = needed.take(i, depths[at])? {
            selected.push(kept);
        }
    }
    if needed.temps.iter().chain(&needed.outputs).any(|v| *v != 0) {
        return Err("Opaque color has an unwritten dependency".into());
    }
    selected.reverse();
    Ok(selected)
}

pub(in crate::model_preview) fn load_opaque(
    manager: &PackageManager,
    tag: u32,
    objects: ObjectInputs<'_>,
    surface: u8,
    uv: [f32; 4],
    model: &mut Model,
) -> Result<Option<Material>, String> {
    let bytes = checked(manager, tag, 0x8080_71E8)?;
    let pixel = u32_at(&bytes, 0x2C8)?;
    if matches!(pixel, 0 | u32::MAX) {
        return Ok(None);
    }
    let raw = super::super::read::shader_bytes(manager, pixel, 0)?;
    let Ok(code) = Code::read(&raw, 0) else {
        return Ok(None);
    };
    let power = intensity::ambient_power(manager);
    let Some(Slice {
        code: mut sliced,
        at,
        base,
        mut intensity,
        mut ambient,
    }) = slice(code.clone(), true, power.is_some())?
    else {
        return Ok(None);
    };
    let constants = super::super::read::stage_constants(manager, &bytes, 0x2C8)?;
    let gain = gain::recover(&code, at, &base, &constants, uv)?;
    let paint = paint::recover(&code, at, &base, &constants)?;
    if ambient && sliced.resources.len() > 3 {
        let Some(emission) = slice(code.clone(), true, false)? else {
            return Ok(None);
        };
        sliced = emission.code;
        intensity = emission.intensity;
        ambient = false;
    }
    if intensity && sliced.resources.len() > 3 {
        let Some(rgb) = slice(code.clone(), false, false)? else {
            return Ok(None);
        };
        sliced = rgb.code;
        intensity = false;
    }
    if sliced.resources.len() > 3 {
        return Err("Opaque color exceeds its separate texture budget".into());
    }
    if sliced
        .resources
        .iter()
        .any(|r| r.dimension != 3 || r.integer)
    {
        return Err(
            "Opaque color requires an unavailable image dimension or numeric format".into(),
        );
    }
    if uv.iter().any(|v| !v.is_finite()) || uv[..2].iter().any(|v| v.abs() < 1e-8) {
        return Err("Opaque color UV placement is not invertible".into());
    }
    let mut material = load_program(
        manager,
        tag,
        &bytes,
        objects,
        surface,
        model,
        (sliced, true, false),
    )?;
    let native = material.native.as_mut().unwrap();
    native.intensity = intensity;
    native.ambient_power = power.filter(|_| ambient);
    if intensity && power.is_some() && !ambient {
        model.notices.push(format!(
            "Material 0x{tag:08X}: Native ambient shading uses an unsupported material recipe."
        ));
    }
    if !intensity && intensity::present(&code) {
        model.notices.push(format!(
            "Material 0x{tag:08X}: Native emission uses an unsupported material recipe."
        ));
    }
    native.opaque_uv = Some(uv);
    native.base_gain = gain;
    if paint.as_ref().is_some_and(Paint::metal_unavailable) {
        model.notices.push(format!(
            "Material 0x{tag:08X}: Opaque metalness uses an unsupported material recipe."
        ));
    }
    native.paint = paint;
    if native.paint.as_ref().is_some_and(Paint::normal_unavailable) {
        model.notices.push(format!(
            "Material 0x{tag:08X}: Native normal decoding uses an unsupported material recipe."
        ));
    }
    if native.paint.as_ref().is_some_and(Paint::grain_unavailable) {
        model.notices.push(format!(
            "Material 0x{tag:08X}: Native normal grain uses an unsupported material recipe."
        ));
    }
    Ok(Some(material))
}
