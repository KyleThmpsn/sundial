//! Validate the native opaque material contract before allocating any resources.
use super::{SAT, dxbc::Program};
use crate::{AuthoringResult, error::invalid};

pub(super) enum Choice {
    NotDyeMaterial,
    AlreadySupported,
    Patch(Plan),
}

pub(super) struct Plan {
    pub declaration: usize,
    pub slot: u32,
    pub sample: usize,
    pub albedo: usize,
    pub luminance: usize,
    pub intensity: usize,
    pub temps: usize,
    pub first: u32,
}

pub(super) fn read(program: &Program) -> AuthoringResult<Choice> {
    let code = &program.instructions;
    let dye = code
        .iter()
        .enumerate()
        .filter(|(_, op)| {
            op.opcode() == 0x59
                && op.words.len() == 4
                && (5..=7).contains(&op.words[2])
                && (23..=27).contains(&op.words[3])
        })
        .map(|(i, op)| (i, op.words[2]))
        .collect::<Vec<_>>();
    if dye.is_empty() {
        return Ok(Choice::NotDyeMaterial);
    }
    let [(declaration, slot)] = dye[..] else {
        return Err(invalid(
            "Shader Glow cannot identify one native dye buffer in this material",
        ));
    };
    if code
        .iter()
        .flat_map(|i| &i.args)
        .any(|a| a.cb(slot, 3) || a.cb(slot, 4) || a.cb(slot, 25) || a.cb(slot, 26))
    {
        return Ok(Choice::AlreadySupported);
    }
    let failure = || invalid("Shader Glow does not yet support this material's pixel program");
    // These families are straight-line opaque gear shaders. Do not splice into
    // cutouts, loops, helper functions or materials with alternate return paths.
    if code
        .iter()
        .any(|i| matches!(i.opcode(),2..=10 | 13 | 18 | 20..=23 | 31 | 44 | 63 | 76))
        || code.last().is_none_or(|i| i.opcode() != 62)
        || code.iter().filter(|i| i.opcode() == 62).count() != 1
    {
        return Err(failure());
    }
    let surface = code.windows(2).any(|pair| {
        pair[0].opcode() == 0
            && pair[0].args.len() == 3
            && pair[0].args[1].cb(slot, 9)
            && pair[0].args[2].cb(slot, 13)
            && pair[1].opcode() == 50
            && pair[1].args.len() == 4
            && pair[1].args[1].cb(0, 0)
            && pair[1].args[1].lanes() == Some([0; 4])
            && pair[1].args[3].cb(slot, 9)
    });
    if !surface {
        return Err(failure());
    }
    let sample = unique(
        code.iter()
            .enumerate()
            .filter(|(_, i)| {
                i.opcode() == 69
                    && i.args.len() == 4
                    && i.args[2].register(7, 2)
                    && i.args[1].register(1, 3)
            })
            .map(|(i, _)| i),
    )?;
    let albedo = unique(
        code.iter()
            .enumerate()
            .filter(|(_, i)| {
                i.opcode() == 54
                    && i.args.len() == 2
                    && i.args[0].register(2, 0)
                    && i.args[0].mask().is_some_and(|m| m & 7 == 7)
                    && i.args[1].kind() == 0
            })
            .map(|(i, _)| i),
    )?;
    let base = code[albedo].args[1].clone();
    let luminance = unique(
        code.iter()
            .enumerate()
            .filter(|(_, i)| {
                i.opcode() == 16
                    && i.args.len() == 3
                    && i.args[1].kind() == 0
                    && i.args[1].indices() == base.indices()
                    && i.args[1]
                        .lanes()
                        .is_some_and(|lanes| lanes[..3] == [0, 1, 2])
                    && i.args[2].kind() == 4
                    && i.args[2].0.len() == 5
                    && (f32::from_bits(i.args[2].0[1]) - 0.3).abs() < 1e-6
                    && (f32::from_bits(i.args[2].0[2]) - 0.59).abs() < 1e-6
                    && (f32::from_bits(i.args[2].0[3]) - 0.11).abs() < 1e-6
            })
            .map(|(i, _)| i),
    )?;
    let intensity = unique(
        code.windows(3)
            .enumerate()
            .filter(|(_, p)| {
                p[0].opcode() == 50
                    && p[0].args.len() == 4
                    && p[0].args[3].literal(0.0078125)
                    && p[1].opcode() == 47
                    && p[2].opcode() == 50
                    && p[2].words[0] & SAT != 0
                    && p[2].args.len() == 4
                    && p[2].args[2].literal(1.0 / 13.0)
                    && p[2].args[3].literal(7.0 / 13.0)
            })
            .map(|(i, _)| i),
    )?;
    if !(sample < albedo && albedo < luminance && luminance < intensity) {
        return Err(failure());
    }
    let temps = unique(
        code.iter()
            .enumerate()
            .filter(|(_, i)| i.opcode() == 104 && i.words.len() == 2)
            .map(|(i, _)| i),
    )?;
    let first = code[temps].words[1];
    if first > 4090 {
        return Err(invalid(
            "Shader Glow exceeds the native temporary register budget",
        ));
    }
    Ok(Choice::Patch(Plan {
        declaration,
        slot,
        sample,
        albedo,
        luminance,
        intensity,
        temps,
        first,
    }))
}

fn unique(values: impl Iterator<Item = usize>) -> AuthoringResult<usize> {
    let values = values.collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Ok(*value),
        _ => Err(invalid(
            "Shader Glow found an unsupported or ambiguous material operation",
        )),
    }
}
