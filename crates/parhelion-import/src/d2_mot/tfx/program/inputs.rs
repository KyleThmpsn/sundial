//! Track the source inputs that feed the constant-buffer lanes a shader reads.
//! This is a dependency analysis, not a mapping of source externs to native ones.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Input {
    /// A four-byte scalar within a source renderer extern. Offset is in bytes.
    External {
        scope: u8,
        offset: u16,
    },
    Object {
        name: u32,
        lane: u8,
    },
    Global {
        index: u8,
        lane: u8,
    },
    /// Numeric metadata of a texture binding. It is constant only after that
    /// binding has been resolved to a proven immutable resource.
    TextureMetadata {
        operation: u8,
        binding: [u8; 2],
        lane: u8,
    },
}

type Lane = BTreeSet<Input>;
type Vector = [Lane; 4];

#[derive(Clone)]
enum Value {
    Vector(Vector),
    Matrix(Box<[Vector; 4]>),
    Resource,
}

fn empty() -> Vector {
    std::array::from_fn(|_| BTreeSet::new())
}

fn vector(value: Value) -> Result<Vector> {
    match value {
        Value::Vector(value) => Ok(value),
        _ => bail!("TFX numeric expression consumes a resource or matrix"),
    }
}

fn take(stack: &mut Vec<Value>) -> Result<Value> {
    stack.pop().context("TFX dependency stack underflow")
}

fn external(scope: u8, offset: u16) -> Vector {
    std::array::from_fn(|lane| {
        BTreeSet::from([Input::External {
            scope,
            offset: offset + lane as u16 * 4,
        }])
    })
}

/// Resolve final output dependencies in source material bytecode. `reads` maps
/// constant-buffer vector indices to XYZW lane masks. Missing output writes
/// retain immutable material constants and have no runtime input dependencies.
/// Curves and procedural operations conservatively retain all input lanes.
pub fn required(data: &[u8], reads: &BTreeMap<u8, u8>) -> Result<BTreeSet<Input>> {
    ensure!(
        reads.values().all(|mask| *mask != 0 && *mask & !15 == 0),
        "invalid shader constant-buffer lane mask"
    );
    let mut stack = Vec::new();
    let mut temps = BTreeMap::<u8, Value>::new();
    let mut outputs = BTreeMap::<u8, Vector>::new();
    for instruction in parse(data)? {
        let i = instruction;
        let value = match i.op {
            0x42 => Value::Vector(empty()),
            0x60..=0x62 => Value::Vector(std::array::from_fn(|lane| {
                BTreeSet::from([Input::TextureMetadata {
                    operation: i.op,
                    binding: [i.args[0], i.args[1]],
                    lane: lane as u8,
                }])
            })),
            0x4A => {
                let input = Input::External {
                    scope: i.args[0],
                    offset: u16::from(i.args[1]) * 4,
                };
                Value::Vector(std::array::from_fn(|_| BTreeSet::from([input.clone()])))
            }
            0x4B => Value::Vector(external(i.args[0], u16::from(i.args[1]) * 16)),
            0x4C => Value::Matrix(Box::new(std::array::from_fn(|row| {
                external(i.args[0], u16::from(i.args[1]) * 16 + row as u16 * 16)
            }))),
            0x4D..=0x4F | 0x5B => Value::Resource,
            0x5C => {
                let name = u32::from_be_bytes(i.args.try_into()?);
                Value::Vector(std::array::from_fn(|lane| {
                    BTreeSet::from([Input::Object {
                        name,
                        lane: lane as u8,
                    }])
                }))
            }
            0x5D => Value::Vector(std::array::from_fn(|lane| {
                BTreeSet::from([Input::Global {
                    index: i.args[0],
                    lane: lane as u8,
                }])
            })),
            0x51 => Value::Vector(outputs.get(&i.args[0]).cloned().unwrap_or_else(empty)),
            0x52 => {
                outputs.insert(i.args[0], vector(take(&mut stack)?)?);
                continue;
            }
            0x53 => {
                let Value::Matrix(rows) = take(&mut stack)? else {
                    bail!("TFX matrix output consumes a non-matrix value")
                };
                ensure!(i.args[0] <= 252, "TFX matrix output exceeds buffer indices");
                for (row, value) in (*rows).into_iter().enumerate() {
                    outputs.insert(i.args[0] + row as u8, value);
                }
                continue;
            }
            0x54 => temps
                .get(&i.args[0])
                .context("TFX temporary read before write")?
                .clone(),
            0x55 => {
                temps.insert(i.args[0], take(&mut stack)?);
                continue;
            }
            0x56..=0x59 => {
                ensure!(
                    matches!(take(&mut stack)?, Value::Resource),
                    "TFX resource output consumes a numeric value"
                );
                continue;
            }
            0x29 => {
                let value = vector(take(&mut stack)?)?;
                Value::Vector(std::array::from_fn(|lane| {
                    value[usize::from((i.args[0] >> (6 - lane * 2)) & 3)].clone()
                }))
            }
            0x28 => {
                let value = vector(take(&mut stack)?)?;
                Value::Vector(std::array::from_fn(|_| value[0].clone()))
            }
            0x0C..=0x0E => {
                let right = vector(take(&mut stack)?)?;
                let left = vector(take(&mut stack)?)?;
                let split = usize::from(i.op - 0x0B);
                Value::Vector(std::array::from_fn(|lane| {
                    if lane < split {
                        left[lane].clone()
                    } else {
                        right[lane - split].clone()
                    }
                }))
            }
            // Component-wise operations retain the matching lanes. All other
            // supported numeric operations retain the union of their inputs.
            1..=0x35 | 0x43..=0x49 => {
                let arity = i.arity;
                ensure!(arity > 0, "TFX numeric operation has no defined operands");
                let operands = (0..arity)
                    .map(|_| vector(take(&mut stack)?))
                    .collect::<Result<Vec<_>>>()?;
                let component_wise = matches!(i.op,
                    1..=4 | 7..=10 | 0x13 | 0x15 | 0x16 | 0x18..=0x1D | 0x20..=0x22 | 0x2A);
                if component_wise {
                    Value::Vector(std::array::from_fn(|lane| {
                        operands
                            .iter()
                            .flat_map(|value| value[lane].iter().cloned())
                            .collect()
                    }))
                } else {
                    let inputs: Lane = operands
                        .iter()
                        .flatten()
                        .flat_map(|lane| lane.iter().cloned())
                        .collect();
                    Value::Vector(std::array::from_fn(|_| inputs.clone()))
                }
            }
            _ => bail!(
                "TFX dependency analysis does not support opcode {:02X}",
                i.op
            ),
        };
        stack.push(value);
        ensure!(stack.len() <= 64, "TFX dependency stack exceeds capacity");
    }
    ensure!(
        stack.is_empty(),
        "TFX dependency expression leaves an unfinished value"
    );
    let mut result = BTreeSet::new();
    for (&index, &mask) in reads {
        if let Some(value) = outputs.get(&index) {
            for (lane, dependencies) in value.iter().enumerate() {
                if mask & (1 << lane) != 0 {
                    result.extend(dependencies.iter().cloned());
                }
            }
        }
    }
    Ok(result)
}
