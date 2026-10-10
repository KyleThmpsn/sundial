//! Bounded Shadowkeep material-expression interpreter. Arithmetic uses the shared,
//! independently captured native prefix. The binding opcodes differ from particle bytecode
//! and modern TFX. Global channels use render-global defaults, the two frame clocks use
//! preview time, and other frame fields remain zero. Unsupported inputs reject the whole scope.
use crate::{
    expression::{binary, math, ternary, unary},
    package_payload::{bytes_at, native_array_at},
};
type Vector = [f32; 4];
pub(crate) type ObjectInput = Result<Vector, String>;
pub(crate) type ObjectInputs<'a> = Result<&'a [ObjectInput], &'a str>;

#[derive(Clone)]
pub(crate) struct Program {
    ops: Vec<Op>,
    constants: Vec<Vector>,
    outputs: usize,
}
#[derive(Clone)]
enum Op {
    Constant(usize),
    Time,
    Unary(u8),
    Binary(u8),
    Ternary(u8),
    Permute(u8),
    Stack(u8),
    Curve {
        code: u8,
        index: usize,
    },
    /// A value the game supplies, held at the preview's stand-in for it.
    Value(Vector),
    Output(usize),
    Store(usize),
    PushTemp(usize),
    PopTemp(usize),
}

/// The frame extern, whose first two fields are the game and render clocks.
const FRAME: u8 = 1;

impl Program {
    pub(crate) fn animated(&self) -> bool {
        self.ops.iter().any(|op| matches!(op, Op::Time))
    }
    /// `channels` holds each global channel's default value.
    pub fn read(scope: &[u8], channels: &[Vector]) -> Result<Option<Self>, String> {
        if u64::from_le_bytes(bytes_at(scope, 0x58)?) == 0 {
            return Ok(None);
        }
        let code = rows(scope, 0x58, 0x8080_0009, 1, 4096)?;
        let constants = rows(scope, 0x68, 0x8080_0090, 16, 256)?;
        let constants = constants
            .chunks_exact(16)
            .map(|row| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap())
                })
            })
            .collect::<Vec<Vector>>();
        Self::decode(code, constants, channels, Ok(&[]), None, 27)
    }

    /// A material stage uses the same arithmetic but owns its output length and bindings.
    /// Object inputs retain unavailable slots, resolved only when the stage reads them.
    pub(crate) fn material(
        bytes: &[u8],
        stage: usize,
        channels: &[Vector],
        objects: ObjectInputs<'_>,
        surface: u8,
        outputs: usize,
    ) -> Result<Option<Self>, String> {
        if u64::from_le_bytes(bytes_at(bytes, stage + 0x20)?) == 0 {
            return Ok(None);
        }
        let code = rows(bytes, stage + 0x20, 0x8080_0009, 1, 4096)?;
        let constants = rows(bytes, stage + 0x30, 0x8080_0090, 16, 256)?
            .chunks_exact(16)
            .map(|row| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap())
                })
            })
            .collect();
        Self::decode(code, constants, channels, objects, Some(surface), outputs)
    }

    fn decode(
        code: &[u8],
        constants: Vec<Vector>,
        channels: &[Vector],
        objects: ObjectInputs<'_>,
        surface: Option<u8>,
        outputs: usize,
    ) -> Result<Option<Self>, String> {
        if outputs > 256 {
            return Err("Shader output buffer exceeds preview limits".into());
        }
        if constants
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Invalid shader animation constants".into());
        }
        let mut bytes = code.iter().copied();
        let mut ops = Vec::new();
        while let Some(code) = bytes.next() {
            let mut arg = || {
                bytes
                    .next()
                    .ok_or("Truncated shader animation instruction".to_owned())
            };
            let op = match code {
                0x34 => Op::Constant(arg()? as usize),
                0x35..=0x3B => Op::Curve {
                    code,
                    index: arg()? as usize,
                },
                0x3C => match (arg()?, arg()?) {
                    (FRAME, 0 | 1) => Op::Time,
                    (FRAME, _) => Op::Value([0.0; 4]),
                    (source, element) => {
                        return Err(format!(
                            "Shader animation requires game state unavailable in the preview ({source}:{element})"
                        ));
                    }
                },
                0x4E => {
                    let channel = arg()?;
                    Op::Value(*channels.get(usize::from(channel)).ok_or_else(|| {
                        format!(
                            "Shader animation reads global channel {channel}, which is not defined"
                        )
                    })?)
                }
                0x4D if surface.is_some() => {
                    let index = usize::from(arg()?);
                    Op::Value(
                        *objects
                            .map_err(str::to_owned)?
                            .get(index)
                            .ok_or("The effect's object input is unavailable")?
                            .as_ref()
                            .map_err(Clone::clone)?,
                    )
                }
                0x3D if surface.is_some() => match (arg()?, arg()?) {
                    (86, 0) => Op::Value([f32::from(surface.unwrap() % 2); 4]),
                    // The studio has no temporal dither or camera jitter.
                    (FRAME, 26) => Op::Value([0.0; 4]),
                    (source, element) => {
                        return Err(format!(
                            "The effect needs an unsupported vector input ({source}:{element})"
                        ));
                    }
                },
                // These bind samplers and scene textures. They cannot enter numeric expressions.
                0x4C if surface.is_some() => {
                    let sampler = arg()?;
                    if arg()? != 0x49 {
                        return Err("Invalid effect sampler binding".into());
                    }
                    let slot = arg()?;
                    if sampler > 5 || slot != 0x21 + sampler {
                        return Err("The effect remaps an unsupported sampler".into());
                    }
                    continue;
                }
                0x3F if surface.is_some() => {
                    let source = arg()?;
                    let element = arg()?;
                    if arg()? != 0x47 {
                        return Err("Invalid effect texture binding".into());
                    }
                    let slot = arg()?;
                    if !matches!((source, element, slot), (3, 7, 0x25) | (3, 15, 0x23)) {
                        return Err("The effect needs an unsupported scene texture".into());
                    }
                    continue;
                }
                0x42 => Op::Output(arg()? as usize),
                0x43 => Op::Store(arg()? as usize),
                0x45 => Op::PushTemp(arg()? as usize),
                0x46 => Op::PopTemp(arg()? as usize),
                0x22 => Op::Permute(arg()?),
                0x01..=0x06 | 0x08..=0x0F => Op::Binary(code),
                0x10..=0x14 => Op::Ternary(code),
                0x07 | 0x15..=0x21 | 0x23..=0x2B => Op::Unary(code),
                0x2C..=0x33 => Op::Stack(code),
                _ => {
                    return Err(format!(
                        "Unsupported shader animation instruction 0x{code:02X}"
                    ));
                }
            };
            ops.push(op);
        }
        let program = Self {
            ops,
            constants,
            outputs,
        };
        // Validate structure without evaluating an arbitrary time. A well-formed animation
        // may be undefined at time zero but produce valid values on later frames.
        program.validate()?;
        Ok(Some(program))
    }

    fn validate(&self) -> Result<(), String> {
        let mut depth = 0_usize;
        let mut temp = [false; 16];
        for op in &self.ops {
            let (inputs, outputs) = match *op {
                Op::Constant(index) => {
                    self.constant(index)?;
                    (0, 1)
                }
                Op::Time => (0, 1),
                Op::Value(value) => {
                    if value.iter().any(|v| !v.is_finite()) {
                        return Err("Invalid shader animation input".into());
                    }
                    (0, 1)
                }
                Op::Unary(_) | Op::Permute(_) => (1, 1),
                Op::Binary(_) => (2, 1),
                Op::Ternary(_) => (3, 1),
                Op::Stack(code) => match code {
                    0x2C => (2, 1),
                    0x2D => (8, 4),
                    0x2E => (5, 1),
                    0x2F => (6, 1),
                    0x30 => (11, 1),
                    0x31 => (12, 1),
                    0x32 => (7, 1),
                    0x33 => (12, 1),
                    _ => unreachable!(),
                },
                Op::Curve { code, index } => {
                    self.curve(index, curve_count(code))?;
                    (if code == 0x39 { 2 } else { 1 }, 1)
                }
                Op::Output(index) | Op::Store(index) => {
                    if index >= self.outputs {
                        return Err("Invalid shader output index".into());
                    }
                    if matches!(op, Op::Output(_)) {
                        (0, 1)
                    } else {
                        (1, 0)
                    }
                }
                Op::PushTemp(index) => {
                    if !temp.get(index).copied().unwrap_or(false) {
                        return Err("Shader temporary is uninitialized".into());
                    }
                    (0, 1)
                }
                Op::PopTemp(index) => {
                    *temp
                        .get_mut(index)
                        .ok_or("Invalid shader temporary index")? = true;
                    (1, 0)
                }
            };
            depth = depth
                .checked_sub(inputs)
                .ok_or("Shader animation stack underflow")?
                + outputs;
            if depth > 64 {
                return Err("Shader animation exceeds stack limits".into());
            }
        }
        if depth != 0 {
            return Err("Shader animation leaves an incomplete expression".into());
        }
        Ok(())
    }

    pub fn run(&self, mut output: [Vector; 27], seconds: f32) -> Result<[Vector; 27], String> {
        self.run_into(&mut output, seconds)?;
        Ok(output)
    }

    pub(crate) fn run_into(&self, output: &mut [Vector], seconds: f32) -> Result<(), String> {
        self.evaluate(output, seconds, true)
    }

    /// Native GPU stages use IEEE infinities as clamp bounds and retain raw register values.
    pub(crate) fn run_shader_into(
        &self,
        output: &mut [Vector],
        seconds: f32,
    ) -> Result<(), String> {
        self.evaluate(output, seconds, false)
    }

    fn evaluate(&self, output: &mut [Vector], seconds: f32, finite: bool) -> Result<(), String> {
        if output.len() != self.outputs {
            return Err("Shader output buffer length differs".into());
        }
        if !seconds.is_finite() {
            return Err("Invalid shader preview time".into());
        }
        let mut stack = Vec::<Vector>::new();
        let mut temp = [None; 16];
        for op in &self.ops {
            match *op {
                Op::Constant(i) => stack.push(self.constant(i)?),
                Op::Time => stack.push([seconds; 4]),
                Op::Unary(code) => {
                    let value = pop(&mut stack)?;
                    stack.push(unary(code, value));
                }
                Op::Binary(code) => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(binary(code, a, b));
                }
                Op::Ternary(code) => {
                    let c = pop(&mut stack)?;
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(ternary(code, a, b, c));
                }
                Op::Permute(bits) => {
                    let a = pop(&mut stack)?;
                    stack.push(std::array::from_fn(|i| {
                        a[((bits >> (6 - i * 2)) & 3) as usize]
                    }));
                }
                Op::Stack(code) => stack_operation(code, &mut stack)?,
                Op::Curve { code, index } => {
                    let constants = self.curve(index, curve_count(code))?;
                    let input = pop(&mut stack)?;
                    let value = match code {
                        0x35 | 0x36 => ternary(
                            if code == 0x35 { 0x10 } else { 0x11 },
                            constants[0],
                            constants[1],
                            input,
                        ),
                        0x37..=0x39 => {
                            let fallback = if code == 0x39 {
                                Some(pop(&mut stack)?)
                            } else {
                                None
                            };
                            math::spline(input, constants, fallback)?
                        }
                        0x3A | 0x3B => math::gradient(input, constants)?,
                        _ => unreachable!(),
                    };
                    stack.push(value);
                }
                Op::Value(value) => stack.push(value),
                Op::Output(i) => stack.push(*output.get(i).ok_or("Invalid shader output index")?),
                Op::Store(i) => {
                    *output.get_mut(i).ok_or("Invalid shader output index")? = pop(&mut stack)?;
                }
                Op::PushTemp(i) => stack.push(
                    temp.get(i)
                        .copied()
                        .flatten()
                        .ok_or("Shader temporary is uninitialized")?,
                ),
                Op::PopTemp(i) => {
                    *temp.get_mut(i).ok_or("Invalid shader temporary index")? =
                        Some(pop(&mut stack)?);
                }
            }
            // Dye edits remain finite. Native stages retain IEEE clamp bounds for DXBC.
            // In either case the caller owns a local frame until evaluation completes.
            if (finite && stack.iter().flatten().any(|v| !v.is_finite())) || stack.len() > 64 {
                return Err("Shader animation exceeds numeric or stack limits".into());
            }
        }
        if !stack.is_empty() {
            return Err("Shader animation leaves an incomplete expression".into());
        }
        Ok(())
    }

    fn constant(&self, index: usize) -> Result<Vector, String> {
        self.constants
            .get(index)
            .copied()
            .ok_or("Invalid shader constant index".into())
    }

    fn curve(&self, index: usize, count: usize) -> Result<&[Vector], String> {
        self.constants
            .get(index..index + count)
            .ok_or("Invalid shader curve index".into())
    }
}

fn curve_count(code: u8) -> usize {
    match code {
        0x35 | 0x36 => 2,
        0x37 => 5,
        0x38 | 0x39 => 10,
        0x3A => 6,
        0x3B => 11,
        _ => unreachable!(),
    }
}

fn stack_operation(code: u8, stack: &mut Vec<Vector>) -> Result<(), String> {
    match code {
        0x2C => {
            let axis = pop(stack)?;
            let value = pop(stack)?;
            stack.push(math::rotate_axis(value, axis));
        }
        0x2D => {
            let right: [Vector; 4] = take(stack, 4)?.try_into().unwrap();
            let left: [Vector; 4] = take(stack, 4)?.try_into().unwrap();
            stack.extend(right.map(|row| math::matrix(left, row)));
        }
        0x2E => {
            let value = pop(stack)?;
            let matrix: [Vector; 4] = take(stack, 4)?.try_into().unwrap();
            stack.push(math::matrix(matrix, value));
        }
        0x2F..=0x33 => {
            let input = pop(stack)?;
            let count = match code {
                0x2F => 5,
                0x30 | 0x31 => 10,
                0x32 => 6,
                _ => 11,
            };
            let constants = take(stack, count)?;
            let value = if code <= 0x31 {
                let fallback = if code == 0x31 {
                    Some(pop(stack)?)
                } else {
                    None
                };
                math::spline(input, &constants, fallback)?
            } else {
                math::gradient(input, &constants)?
            };
            stack.push(value);
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn rows(
    bytes: &[u8],
    offset: usize,
    expected: u32,
    stride: usize,
    limit: usize,
) -> Result<&[u8], String> {
    if u64::from_le_bytes(bytes_at(bytes, offset)?) == 0 {
        return Ok(&[]);
    }
    let (count, _, start, class) = native_array_at(bytes, offset)?;
    if class != expected || count > limit {
        return Err("Unsupported shader animation array".into());
    }
    bytes
        .get(start..start + count * stride)
        .ok_or("Truncated shader animation array".into())
}
fn pop(stack: &mut Vec<Vector>) -> Result<Vector, String> {
    stack.pop().ok_or("Shader animation stack underflow".into())
}
fn take(stack: &mut Vec<Vector>, count: usize) -> Result<Vec<Vector>, String> {
    let start = stack
        .len()
        .checked_sub(count)
        .ok_or("Shader animation stack underflow")?;
    Ok(stack.split_off(start))
}

#[cfg(test)]
mod tests;
