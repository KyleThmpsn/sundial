//! Bounded Shadowkeep dye-expression interpreter. Opcode numbers differ from
//! modern TFX. Arithmetic semantics reference cohaereo/alkahest expression_vm.
//! Unsupported externs/programs fail as a whole, preserving the static material.
use crate::package_payload::{bytes_at, native_array_at};
type Vector = [f32; 4];

#[derive(Clone)]
pub(super) struct Program {
    ops: Vec<Op>,
    constants: Vec<Vector>,
}
#[derive(Clone)]
enum Op {
    Constant(usize),
    Time,
    Unary(u8),
    Binary(u8),
    Ternary(u8),
    Permute(u8),
    Lerp(usize),
    Gradient(usize),
    Store(usize),
    PushTemp(usize),
    PopTemp(usize),
}

impl Program {
    pub fn read(scope: &[u8]) -> Result<Option<Self>, String> {
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
                0x35 => Op::Lerp(arg()? as usize),
                0x3A => Op::Gradient(arg()? as usize),
                0x3C => {
                    let (source, element) = (arg()?, arg()?);
                    if source != 1 || !matches!(element, 0 | 1) {
                        return Err(format!(
                            "Shader animation requires game state unavailable in the preview ({source}:{element})"
                        ));
                    }
                    Op::Time
                }
                0x43 => Op::Store(arg()? as usize),
                0x45 => Op::PushTemp(arg()? as usize),
                0x46 => Op::PopTemp(arg()? as usize),
                0x22 => Op::Permute(arg()?),
                0x01..=0x06 | 0x08..=0x0E => Op::Binary(code),
                0x10 | 0x12 | 0x13 => Op::Ternary(code),
                0x15..=0x1A | 0x1D..=0x21 | 0x23 | 0x25 | 0x27..=0x29 => Op::Unary(code),
                _ => {
                    return Err(format!(
                        "Unsupported shader animation instruction 0x{code:02X}"
                    ));
                }
            };
            ops.push(op);
        }
        let program = Self { ops, constants };
        // Validate stack discipline, all indices, output ranges and numeric values.
        program.run([[0.0; 4]; 27], 0.0)?;
        Ok(Some(program))
    }

    pub fn run(&self, mut output: [Vector; 27], seconds: f32) -> Result<[Vector; 27], String> {
        if !seconds.is_finite() {
            return Err("Invalid shader preview time".into());
        }
        let mut stack = Vec::<Vector>::new();
        let mut temp = [None; 16];
        for op in &self.ops {
            let value = match *op {
                Op::Constant(i) => self.constant(i)?,
                Op::Time => [seconds; 4],
                Op::Unary(code) => unary(code, pop(&mut stack)?),
                Op::Binary(code) => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    binary(code, a, b)
                }
                Op::Ternary(code) => {
                    let c = pop(&mut stack)?;
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    ternary(code, a, b, c)
                }
                Op::Permute(bits) => {
                    let a = pop(&mut stack)?;
                    std::array::from_fn(|i| a[((bits >> (6 - i * 2)) & 3) as usize])
                }
                Op::Lerp(i) => {
                    let t = pop(&mut stack)?;
                    ternary(0x10, self.constant(i)?, self.constant(i + 1)?, t)
                }
                Op::Gradient(i) => gradient(
                    pop(&mut stack)?,
                    self.constants
                        .get(i..i + 6)
                        .ok_or("Invalid shader gradient index")?,
                ),
                Op::Store(i) => {
                    *output.get_mut(i).ok_or("Invalid shader output index")? = pop(&mut stack)?;
                    continue;
                }
                Op::PushTemp(i) => temp
                    .get(i)
                    .copied()
                    .flatten()
                    .ok_or("Shader temporary is uninitialized")?,
                Op::PopTemp(i) => {
                    *temp.get_mut(i).ok_or("Invalid shader temporary index")? =
                        Some(pop(&mut stack)?);
                    continue;
                }
            };
            if value.iter().any(|v| !v.is_finite() || v.abs() > 1e6) || stack.len() >= 64 {
                return Err("Shader animation exceeds numeric or stack limits".into());
            }
            stack.push(value);
        }
        if !stack.is_empty() {
            return Err("Shader animation leaves an incomplete expression".into());
        }
        Ok(output)
    }

    fn constant(&self, index: usize) -> Result<Vector, String> {
        self.constants
            .get(index)
            .copied()
            .ok_or("Invalid shader constant index".into())
    }
}

fn rows(
    bytes: &[u8],
    offset: usize,
    expected: u32,
    stride: usize,
    limit: usize,
) -> Result<&[u8], String> {
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

fn binary(code: u8, a: Vector, b: Vector) -> Vector {
    match code {
        0x0B => [a.into_iter().zip(b).map(|(a, b)| a * b).sum(); 4],
        0x0C => [a[0], b[0], b[1], b[2]],
        0x0D => [a[0], a[1], b[0], b[1]],
        0x0E => [a[0], a[1], a[2], b[0]],
        _ => std::array::from_fn(|i| match code {
            1 | 6 => a[i] + b[i],
            2 => a[i] - b[i],
            3 | 5 => a[i] * b[i],
            4 => a[i] / b[i],
            8 => a[i].min(b[i]),
            9 => a[i].max(b[i]),
            10 => u8::from(a[i] < b[i]) as f32,
            _ => unreachable!(),
        }),
    }
}
fn ternary(code: u8, a: Vector, b: Vector, c: Vector) -> Vector {
    std::array::from_fn(|i| match code {
        0x10 => a[i] + (b[i] - a[i]) * c[i],
        0x12 => a[i] * b[i] + c[i],
        _ => a[i].max(b[i]).min(c[i]),
    })
}
fn unary(code: u8, a: Vector) -> Vector {
    match code {
        0x21 => [a[0]; 4],
        0x28 => [jitter(a[0]); 4],
        0x29 => [wander(a[0]); 4],
        _ => std::array::from_fn(|i| {
            let v = a[i];
            match code {
                0x15 => v.abs(),
                0x16 => v.signum(),
                0x17 => v.floor(),
                0x18 => v.ceil(),
                0x19 => v.round(),
                0x1A => v - v.floor(),
                0x1D => -v,
                0x1E => sine(v),
                0x1F => sine(v + 0.25),
                0x20 => sine(v + if i % 2 == 1 { 0.25 } else { 0.0 }),
                0x23 => v.clamp(0.0, 1.0),
                0x25 => {
                    let bits = v.to_bits();
                    let mantissa = (bits & 0x007f_ffff) as f32 / 8_388_608.0;
                    let exponent = ((bits >> 23) & 255) as f32 - 127.0;
                    exponent
                        + mantissa * 1.4232545
                        + mantissa * mantissa * (-0.585_421_1 + mantissa * 0.16216666)
                }
                0x27 => (v - v.round()).abs() * 2.0,
                _ => unreachable!(),
            }
        }),
    }
}
fn pseudo_sine(v: f32) -> f32 {
    let v = v - v.round();
    v * (-16.0 * v.abs() + 8.0)
}
fn sine(v: f32) -> f32 {
    let v = pseudo_sine(v);
    v * (0.225 * v.abs() + 0.775)
}
fn jitter(v: f32) -> f32 {
    let sum: f32 = [4.67, 2.99, 1.08, 1.35]
        .into_iter()
        .zip([0.52, 0.37, 0.16, 0.79])
        .map(|(a, b)| pseudo_sine(v * a + b) * 0.25)
        .sum();
    let x = sum + 0.5;
    x * x * (3.0 - 2.0 * x)
}
fn wander(v: f32) -> f32 {
    let a = [4.08, 1.02, 3.0 / 5.37, 3.0 / 9.67];
    let b = [0.92, 0.33, 0.26, 0.54];
    let c = [1.83, 3.09, 0.39, 0.87];
    let d = [0.12, 0.37, 0.16, 0.79];
    let w = [0.02, 0.02, 0.28, 0.28];
    0.5 + (0..4)
        .map(|i| pseudo_sine(v * a[i] + b[i]) * pseudo_sine(v * c[i] + d[i]) * w[i])
        .sum::<f32>()
}
fn gradient(input: Vector, constants: &[Vector]) -> Vector {
    let bounds = constants[5];
    let weights: Vector = std::array::from_fn(|i| {
        let end = if i == 3 { 1.0 } else { bounds[i + 1] };
        let width = end - bounds[i];
        if width.abs() < 1e-6 {
            u8::from(input[i] > bounds[i]) as f32
        } else {
            ((input[i] - bounds[i]) / width).clamp(0.0, 1.0)
        }
    });
    std::array::from_fn(|i| {
        constants[0][i]
            + (0..4)
                .map(|j| constants[i + 1][j] * weights[j])
                .sum::<f32>()
    })
}

#[cfg(test)]
mod tests;
