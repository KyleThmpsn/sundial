//! A color grade at the end of a pixel program. Every write to a graded render target goes to a
//! fresh temporary instead, and before each return the program writes that color graded, with
//! its alpha as it was. A colorize gives every color one hue and saturation at its own brightness,
//! its largest channel, as a colorized palette or tint does. A turn multiplies the color by a
//! matrix and clamps it at zero.
//!
//! Which targets hold color follows from how many render targets a program declares. A census of
//! the 1,813 distinct pixel programs the stock abilities' particles, models, lights, decals and
//! other effect resources draw with, 2026-10-05, found:
//! - one target: a color
//! - two: lights, their diffuse and specular light, both colors
//! - three: the deferred surface targets, an albedo, a normal encoded by a saturated multiply-add,
//!   and surface parameters, so only the albedo is a color
//! - four: two color layers and two weights, which a colorize would tint
//!
//! A program that writes its first target with the distortion encoding, an offset multiplied by
//! `(1, 1, -1, -1)` and saturated, draws a screen offset rather than a color, and 73 of the
//! census programs do. It is left as it is, as is a program whose graded targets are not 32-bit
//! float colors with red, green and blue.
use super::{Instruction, Operand, Program, dst, ins, lit, neg, temp};
use crate::AuthoringResult;

/// What the program does to its color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grade {
    /// The color every color takes at full brightness, scaled by its largest channel.
    Colorize([f32; 3]),
    /// The hue every color takes, as a color at full saturation and brightness, keeping its own
    /// saturation scaled by `saturation` and its brightness scaled by `brightness`: grays and
    /// white stay as they are. A color whose largest channel is `max` and smallest `min` becomes
    /// `brightness * (max - (max - min) * saturation * (1 - hue))`.
    Hue {
        hue: [f32; 3],
        saturation: f32,
        brightness: f32,
    },
    /// The rows of the matrix each color is multiplied by.
    Matrix([[f32; 3]; 3]),
}

const ADD: u32 = 0x00;
const DP3: u32 = 0x10;
const MAD: u32 = 0x32;
const MIN: u32 = 0x33;
const MAX: u32 = 0x34;
const CUSTOM_DATA: u32 = 0x35;
const MOV: u32 = 0x36;
const MUL: u32 = 0x38;
const RET: u32 = 0x3E;
const RETC: u32 = 0x3F;
const DCL_TEMPS: u32 = 0x68;
/// Operand types.
const TEMP: u32 = 0;
const IMMEDIATE: u32 = 4;
const OUTPUT: u32 = 2;
/// The most temporaries an SM5 program declares.
const TEMP_LIMIT: u32 = 4096;
const XYZW: u32 = 0xE4;
const XXXX: u32 = 0;
const YYYY: u32 = 0x55;
const ZZZZ: u32 = 0xAA;
const WWWW: u32 = 0xFF;
/// Signature component type of a 32-bit float, and the system values of a render target.
const FLOAT: u32 = 3;
const TARGET: [u32; 2] = [0, 64];
/// The distortion encoding's literal.
const DISTORTION: [f32; 4] = [1.0, 1.0, -1.0, -1.0];

/// Whether an opcode at or past the reader's declaration bound declares rather than computes:
/// SM4 declarations, hull shader phase markers and SM5 declarations.
fn declares(opcode: u32) -> bool {
    matches!(opcode, 0x58..=0x6A | 0x70..=0x73 | 0x8E..=0xA1 | 0xD1)
}

/// `bytes` graded, or `None` for a program the grade does not apply to: one that is not an SM5
/// pixel program, whose first target is not a float color with red, green and blue, that draws a
/// distortion, declares no temporaries or writes a graded target in a way it cannot follow.
pub(crate) fn grade(bytes: &[u8], grade: Grade) -> AuthoringResult<Option<Vec<u8>>> {
    let Ok(mut program) = Program::read(bytes) else {
        return Ok(None);
    };
    let Some(targets) = graded_targets(&program) else {
        return Ok(None);
    };
    let Some(temps) = program
        .instructions
        .iter()
        .position(|each| each.opcode() == DCL_TEMPS)
    else {
        return Ok(None);
    };
    let Some(&count) = program.instructions[temps].words.get(1) else {
        return Ok(None);
    };
    let added = u32::try_from(targets.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    if count.saturating_add(added) > TEMP_LIMIT {
        return Ok(None);
    }
    // Each graded target's color, then one temporary the grade works in.
    let colors = targets
        .iter()
        .zip(count..)
        .map(|(&(register, mask), color)| (register, mask, color))
        .collect::<Vec<_>>();
    let scratch = count + added - 1;
    let mut result = Vec::with_capacity(program.instructions.len() + 8 * colors.len());
    let mut returns = 0;
    for (index, mut instruction) in std::mem::take(&mut program.instructions)
        .into_iter()
        .enumerate()
    {
        let opcode = instruction.opcode();
        if index == temps {
            instruction.words[1] = count + added;
            result.push(instruction);
            continue;
        }
        if opcode == CUSTOM_DATA || declares(opcode) {
            result.push(instruction);
            continue;
        }
        if matches!(opcode, RET | RETC) {
            for &(register, mask, color) in &colors {
                result.extend(finish(grade, (register, mask), (color, scratch)));
            }
            returns += 1;
        }
        let parsed = if opcode < 0x58 {
            instruction
        } else {
            match instruction.parsed() {
                Ok(parsed) => parsed,
                Err(_) => return Ok(None),
            }
        };
        if distorts(&parsed) {
            return Ok(None);
        }
        let mut args = Vec::with_capacity(parsed.args.len());
        let mut moved = false;
        for arg in &parsed.args {
            match redirect(arg, &colors) {
                Redirect::Keep => args.push(arg.clone()),
                Redirect::To(arg) => {
                    args.push(arg);
                    moved = true;
                }
                Redirect::Unsupported => return Ok(None),
            }
        }
        result.push(if moved { parsed.replace(args)? } else { parsed });
    }
    if returns == 0 {
        return Ok(None);
    }
    program.instructions = result;
    program.emit().map(Some)
}

/// Whether `instruction` writes render target 0 with the distortion encoding's literal.
fn distorts(instruction: &Instruction) -> bool {
    let writes_first = instruction
        .args
        .first()
        .is_some_and(|arg| arg.kind() == OUTPUT && arg.indices() == Some(&[0][..]));
    let literal = DISTORTION.map(f32::to_bits);
    writes_first
        && instruction.args[1..]
            .iter()
            .any(|arg| arg.kind() == IMMEDIATE && arg.0.get(1..) == Some(&literal[..]))
}

enum Redirect {
    Keep,
    To(Operand),
    Unsupported,
}

/// An operand naming a graded render target, made to name that target's color temporary.
fn redirect(arg: &Operand, colors: &[(u32, u32, u32)]) -> Redirect {
    if arg.kind() != OUTPUT {
        return Redirect::Keep;
    }
    let Some(&[register]) = arg.indices() else {
        return Redirect::Unsupported;
    };
    match colors.iter().find(|(each, ..)| *each == register) {
        Some(&(_, _, color)) => {
            let mut words = arg.0.clone();
            words[0] = (words[0] & !(0xFF << 12)) | (TEMP << 12);
            words[1] = color;
            Redirect::To(Operand(words))
        }
        None => Redirect::Keep,
    }
}

/// The render targets holding color, with the components each declares, by how many targets the
/// output signature declares. None when target 0 is not a 32-bit float color with red, green and
/// blue.
fn graded_targets(program: &Program) -> Option<Vec<(u32, u32)>> {
    let chunk = program.chunk(b"OSGN")?;
    let word = |at: usize| {
        chunk
            .get(at..at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    };
    let count = word(0)? as usize;
    let mut targets = Vec::new();
    for element in 0..count {
        let at = 8 + element * 24;
        let (system, component, register) = (word(at + 8)?, word(at + 12)?, word(at + 16)?);
        if TARGET.contains(&system) && register != u32::MAX {
            let mask = u32::from(*chunk.get(at + 20)?);
            targets.push((register, mask, component == FLOAT && mask & 7 == 7));
        }
    }
    let colors: &[u32] = match targets.len() {
        2 | 4 => &[0, 1],
        _ => &[0],
    };
    let graded = targets
        .iter()
        .filter(|(register, _, color)| *color && colors.contains(register))
        .map(|&(register, mask, _)| (register, mask))
        .collect::<Vec<_>>();
    graded
        .iter()
        .any(|(register, _)| *register == 0)
        .then_some(graded)
}

/// A four-component literal.
fn vector([x, y, z, w]: [f32; 4]) -> Operand {
    Operand(vec![
        2 | (IMMEDIATE << 12),
        x.to_bits(),
        y.to_bits(),
        z.to_bits(),
        w.to_bits(),
    ])
}

/// Render target `register` with the components in `mask`.
fn output(register: u32, mask: u32) -> Operand {
    Operand(vec![2 | (mask << 4) | (OUTPUT << 12) | (1 << 20), register])
}

/// What writes render target `register` graded, from temporary `color`, through temporary
/// `scratch`.
fn finish(
    grade: Grade,
    (register, mask): (u32, u32),
    (color, scratch): (u32, u32),
) -> Vec<Instruction> {
    let rgb = output(register, mask & 7);
    let mut code = match grade {
        Grade::Colorize([r, g, b]) => vec![
            ins(
                MAX,
                vec![dst(scratch, 1), temp(color, XXXX), temp(color, YYYY)],
            ),
            ins(
                MAX,
                vec![dst(scratch, 1), temp(scratch, XXXX), temp(color, ZZZZ)],
            ),
            ins(MAX, vec![dst(scratch, 1), temp(scratch, XXXX), lit(0.0)]),
            ins(MUL, vec![rgb, temp(scratch, XXXX), vector([r, g, b, 0.0])]),
        ],
        Grade::Hue {
            hue: [r, g, b],
            saturation,
            brightness,
        } => vec![
            // The largest channel in x, the smallest in y.
            ins(
                MAX,
                vec![dst(scratch, 1), temp(color, XXXX), temp(color, YYYY)],
            ),
            ins(
                MAX,
                vec![dst(scratch, 1), temp(scratch, XXXX), temp(color, ZZZZ)],
            ),
            ins(
                MIN,
                vec![dst(scratch, 2), temp(color, XXXX), temp(color, YYYY)],
            ),
            ins(
                MIN,
                vec![dst(scratch, 2), temp(scratch, YYYY), temp(color, ZZZZ)],
            ),
            // Their difference scaled, and the largest scaled.
            ins(
                ADD,
                vec![
                    dst(scratch, 2),
                    temp(scratch, XXXX),
                    neg(temp(scratch, YYYY)),
                ],
            ),
            ins(
                MUL,
                vec![
                    dst(scratch, 2),
                    temp(scratch, YYYY),
                    lit(saturation * brightness),
                ],
            ),
            ins(
                MUL,
                vec![dst(scratch, 1), temp(scratch, XXXX), lit(brightness)],
            ),
            ins(
                MAD,
                vec![
                    dst(scratch, 7),
                    neg(temp(scratch, YYYY)),
                    vector([1.0 - r, 1.0 - g, 1.0 - b, 0.0]),
                    temp(scratch, XXXX),
                ],
            ),
            ins(MAX, vec![rgb, temp(scratch, XYZW), vector([0.0; 4])]),
        ],
        Grade::Matrix(rows) => {
            let mut code = rows
                .iter()
                .zip([1, 2, 4])
                .map(|(&[a, b, c], lane)| {
                    ins(
                        DP3,
                        vec![
                            dst(scratch, lane),
                            temp(color, XYZW),
                            vector([a, b, c, 0.0]),
                        ],
                    )
                })
                .collect::<Vec<_>>();
            code.push(ins(MAX, vec![rgb, temp(scratch, XYZW), vector([0.0; 4])]));
            code
        }
    };
    if mask & 8 != 0 {
        code.push(ins(MOV, vec![output(register, 8), temp(color, WWWW)]));
    }
    code
}
