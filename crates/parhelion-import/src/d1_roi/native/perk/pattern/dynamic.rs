//! Retained scalar drivers preserve their source input and complete value program.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{
    Block, Graph, NodeKind,
    value::{Instruction, Program as ValueProgram},
};

pub(super) fn read(
    source: &Source,
    native: &mut Reader,
    p: &Payload,
    at: usize,
) -> Result<Vec<(Action, NativeAssetPatch)>> {
    ensure!(
        p.bytes::<7>(at + 1)? == [1, 0, 0, 0, 0, 0, 0]
            && p.u32(at + 20)? == 0
            && p.u64(at + 24)? == 0
            && p.u64(at + 64)? == 1
            && p.u64(at + 72)? == 1
            && matches!(p.u8(at + 80)?, 0..=2 | 6)
            && p.u8(at + 81)? <= 1
            && p.bytes::<6>(at + 82)? == [0; 6],
        "Source dynamic attachment needs target, input or lifecycle translation"
    );
    let mut code = p
        .array(at + 32, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let mut constants = p
        .array(at + 48, 16, Some(0x80800090))?
        .into_iter()
        .map(|at| p.bytes::<16>(at))
        .collect::<Result<Vec<_>>>()?;
    let (minimum, mut maximum) = validate(&code, &constants)?;
    let mut rows = modifiers(source, p.u32(at + 16)?)?;
    if minimum < 0.0 {
        ensure!(
            maximum <= 0.0 && constants.len() < 256,
            "Source dynamic weight crosses zero or has no room for sign normalization"
        );
        // The source query consumes amount * weight. Negating both leaves that product
        // unchanged and lets the same reciprocal lowering handle nonpositive curves.
        let index = u8::try_from(constants.len())?;
        constants.push(std::array::from_fn(|i| (-1.0f32).to_le_bytes()[i % 4]));
        code.truncate(code.len() - 2);
        code.extend([0x34, index, 0x03, 0x3E, 0]);
        for row in &mut rows {
            row.change = -row.change;
            row.amount = if row.operation == 0 {
                row.change
            } else if row.change > 0.0 {
                1.0 + row.change
            } else {
                1.0 / (1.0 - row.change)
            };
        }
        maximum = -minimum;
    }
    let curve = |row: &Modifier| {
        if row.operation == 1 && row.change < 0.0 {
            row.change.to_bits()
        } else {
            0
        }
    };
    let split = rows
        .windows(2)
        .any(|pair| curve(&pair[0]) != curve(&pair[1]));
    if split {
        let mut destinations = BTreeSet::new();
        ensure!(
            rows.iter()
                .all(|r| destinations.insert((r.component, r.ability, r.input))),
            "Split dynamic bank repeats a destination whose registration order is unqualified"
        );
        // Each driver reads the input independently. Exclude capacity/stat feedback
        // when the input is derived from magazine state. Reload times do not feed it.
        ensure!(
            !matches!(p.u8(at + 80)?, 1 | 2)
                || rows
                    .iter()
                    .all(|r| r.component != 1 && !(r.component == 14 && r.input != 9)),
            "Split dynamic bank may modify its own magazine input"
        );
    }
    let mut result = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let key = curve(&rows[start]);
        let end = (start + 1..rows.len())
            .find(|&i| curve(&rows[i]) != key)
            .unwrap_or(rows.len());
        let (code, constants) = if key == 0 {
            (code.clone(), constants.clone())
        } else {
            reciprocal(&code, &constants, -f32::from_bits(key), maximum)?
        };
        let (tag, patches) = modifier_patch(native, &rows[start..end])?;
        result.push(emit(p, at, tag, patches, code, constants)?);
        start = end;
    }
    Ok(result)
}

fn reciprocal(
    code: &[u8],
    constants: &[[u8; 16]],
    magnitude: f32,
    maximum: f32,
) -> Result<(Vec<u8>, Vec<[u8; 16]>)> {
    // For a=-b and w>=0, D1 computes x/(1+b*w). With the unit factor
    // 1/(1+b), native lerp needs w*(1+b)/(1+b*w). A pure expression is
    // repeated to avoid changing its input or introducing persistent state.
    ensure!(
        magnitude > 0.0 && magnitude <= 16.0 && maximum <= 16.0,
        "Negative dynamic multiplier exceeds the bounded reciprocal range"
    );
    ensure!(
        code.ends_with(&[0x3E, 0]) && constants.len() <= 253,
        "Negative dynamic multiplier lacks room for reciprocal constants"
    );
    let mut values = constants.to_vec();
    let first = u8::try_from(values.len())?;
    for scalar in [1.0 + magnitude, magnitude, 1.0] {
        let bits = scalar.to_le_bytes();
        values.push(std::array::from_fn(|i| bits[i % 4]));
    }
    let body = &code[..code.len() - 2];
    let mut lowered = body.to_vec();
    // Broadcast lane zero, the scalar actually consumed by the updater.
    lowered.extend([0x22, 0, 0x34, first, 0x03]);
    lowered.extend(body);
    lowered.extend([
        0x22,
        0,
        0x34,
        first + 1,
        0x03,
        0x34,
        first + 2,
        0x01,
        0x04,
        0x3E,
        0,
    ]);
    // Emission validates the enlarged stack, instruction count and constant spans.
    Ok((lowered, values))
}

fn emit(
    p: &Payload,
    at: usize,
    tag: u32,
    patches: Vec<NativeAssetResourcePatch>,
    code: Vec<u8>,
    constants: Vec<[u8; 16]>,
) -> Result<(Action, NativeAssetPatch)> {
    let mut bytes = vec![0; 88];
    bytes[..3].copy_from_slice(&[2, 1, 0]);
    put(&mut bytes, 16, &tag.to_le_bytes())?;
    put(&mut bytes, 32, &(code.len() as u64).to_le_bytes())?;
    put(&mut bytes, 48, &(constants.len() as u64).to_le_bytes())?;
    bytes[64..88].copy_from_slice(&p.bytes::<24>(at + 64)?);
    let graph = Graph {
        blocks: vec![
            Block {
                class: 0x80803E44,
                count: None,
                bytes,
                links: BTreeMap::from([(40, 1), (56, 2)]),
            },
            Block {
                class: 0x80800009,
                count: Some(code.len()),
                bytes: code,
                links: BTreeMap::new(),
            },
            Block {
                class: 0x80800090,
                count: Some(constants.len()),
                bytes: constants.concat(),
                links: BTreeMap::new(),
            },
        ],
    };
    graph
        .validate_node(NodeKind::Effect(2))
        .map_err(anyhow::Error::msg)?;
    Ok((
        Action::Native {
            node: NativeNode {
                kind: 2,
                bytes: graph.emit().map_err(anyhow::Error::msg)?,
            },
        },
        NativeAssetPatch {
            action_index: 0,
            source_graph: tag,
            patches,
            appends: Vec::new(),
            remove_owners: Vec::new(),
            programs: Vec::new(),
        },
    ))
}

fn validate(code: &[u8], constants: &[[u8; 16]]) -> Result<(f32, f32)> {
    let constants = constants
        .iter()
        .map(|row| {
            std::array::from_fn(|i| {
                u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("four-byte lane"))
            })
        })
        .collect::<Vec<[u32; 4]>>();
    ensure!(
        constants
            .iter()
            .flatten()
            .all(|v| f32::from_bits(*v).is_finite()),
        "Nonfinite source dynamic constant"
    );
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let operand = match opcode {
            0x01 | 0x03 | 0x0F | 0x23 => None,
            0x22 | 0x34 | 0x35 | 0x37 | 0x38 | 0x3A | 0x3C | 0x3E => {
                let value = *code.get(at).context("Truncated dynamic instruction")?;
                at += 1;
                Some(value)
            }
            _ => anyhow::bail!("Source dynamic opcode {opcode:02X} requires translation"),
        };
        ensure!(
            opcode != 0x3E || at == code.len(),
            "Dynamic output must end its program"
        );
        instructions.push(Instruction { opcode, operand });
    }
    let program = ValueProgram {
        instructions,
        constants,
        fast_path: 0,
    };
    program.validate().map_err(anyhow::Error::msg)?;

    // Original 719B60 and 108C1C0 write the scalar result to every numeric row's
    // weight. A signed source multiplier changes branches at zero. Prove a bounded
    // one-sided weight from operations before normalizing its sign, never from samples.
    // Curves are unknown until an explicit saturation establishes their sign.
    let mut stack = Vec::<[Option<(f32, f32)>; 4]>::new();
    let mut uniform = Vec::<bool>::new();
    let mut bounds = None;
    for instruction in &program.instructions {
        let operand = usize::from(instruction.operand.unwrap_or(0));
        // Equal intervals do not imply equal lanes. A spline can XOR several
        // polynomial lanes when its input is nonuniform, defeating scalar bounds.
        let same = match instruction.opcode {
            0x34 => program.constants[operand].windows(2).all(|p| p[0] == p[1]),
            0x3C => true,
            0x01 | 0x03 | 0x0F => {
                let right = uniform.pop().expect("validated binary operand");
                let left = uniform.pop().expect("validated binary operand");
                left && (instruction.opcode == 0x0F || right)
            }
            0x22 => {
                let input = uniform.pop().expect("validated swizzle operand");
                input
                    || [0, 2, 4, 6]
                        .into_iter()
                        .all(|shift| (operand >> shift) & 3 == operand & 3)
            }
            0x23 => uniform.pop().expect("validated saturation operand"),
            0x35 => {
                uniform.pop().expect("validated interpolation operand")
                    && program.constants[operand..operand + 2]
                        .iter()
                        .all(|row| row.windows(2).all(|p| p[0] == p[1]))
            }
            0x37 => *uniform.last().expect("validated spline operand"),
            0x38 => {
                uniform.pop();
                true
            }
            0x3A => {
                uniform.pop();
                false
            }
            0x3E => {
                uniform.pop();
                false
            }
            _ => unreachable!(),
        };
        let value = match instruction.opcode {
            0x34 => program.constants[operand].map(|bits| {
                let value = f32::from_bits(bits);
                Some((value, value))
            }),
            0x3C => [None; 4],
            0x01 | 0x03 => {
                let b = stack.pop().expect("validated binary operand");
                let a = stack.pop().expect("validated binary operand");
                std::array::from_fn(|i| {
                    let (amin, amax) = a[i]?;
                    let (bmin, bmax) = b[i]?;
                    let values = if instruction.opcode == 0x01 {
                        [amin + bmin, amin + bmax, amax + bmin, amax + bmax]
                    } else {
                        [amin * bmin, amin * bmax, amax * bmin, amax * bmax]
                    };
                    values.iter().all(|v| v.is_finite()).then(|| {
                        (
                            values.into_iter().fold(f32::INFINITY, f32::min),
                            values.into_iter().fold(f32::NEG_INFINITY, f32::max),
                        )
                    })
                })
            }
            0x0F => {
                stack.pop();
                stack.pop();
                [None; 4]
            }
            0x22 => {
                let a = stack.pop().expect("validated swizzle operand");
                std::array::from_fn(|i| a[(operand >> (6 - 2 * i)) & 3])
            }
            0x23 => {
                stack.pop();
                [Some((0.0, 1.0)); 4]
            }
            0x35 => {
                let time = stack.pop().expect("validated interpolation operand");
                std::array::from_fn(|i| {
                    let (minimum, maximum) = time[i]?;
                    let start = f32::from_bits(program.constants[operand][i]);
                    let end = f32::from_bits(program.constants[operand + 1][i]);
                    let a = start + (end - start) * minimum;
                    let b = start + (end - start) * maximum;
                    (a.is_finite() && b.is_finite()).then_some((a.min(b), a.max(b)))
                })
            }
            0x37 => {
                uniform.pop();
                let time = stack.pop().expect("validated curve input");
                let rows = &program.constants[operand..operand + 5];
                let knots = rows[4].map(f32::from_bits);
                let bounds = time[0].and_then(|(minimum, maximum)| {
                    // For linear segments the rounded multiply and add are monotone.
                    // Endpoints bound every finite input, including either side of a
                    // discontinuity. Cubic segments still need a later saturation.
                    if !same
                        || knots.windows(2).any(|p| p[0] > p[1])
                        || rows[..2]
                            .iter()
                            .flatten()
                            .any(|&v| f32::from_bits(v) != 0.0)
                        || minimum.abs().max(maximum.abs()) > 1e6
                    {
                        return None;
                    }
                    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
                    if minimum < knots[0] {
                        low = 0.0;
                        high = 0.0;
                    }
                    for i in 0..4 {
                        let start = minimum.max(knots[i]);
                        let end = maximum.min(if i == 3 { maximum } else { knots[i + 1] });
                        if start > end {
                            continue;
                        }
                        let slope = f32::from_bits(rows[2][i]);
                        let intercept = f32::from_bits(rows[3][i]);
                        for value in [slope * start + intercept, slope * end + intercept] {
                            if !value.is_finite() {
                                return None;
                            }
                            low = low.min(value);
                            high = high.max(value);
                        }
                    }
                    (low.is_finite() && high.is_finite()).then_some((low, high))
                });
                [bounds; 4]
            }
            0x3A => {
                let time = stack.pop().expect("validated vector ramp operand");
                // Each ramp saturates to [0,1], including its guarded step branch.
                // Bound all intermediate arithmetic, then sum signed contributions
                // in the original pairwise order. This proves the final scalar sign.
                let values = &program.constants[operand..operand + 6];
                ensure!(
                    time.iter()
                        .all(|v| v.is_some_and(|(a, b)| a >= -1e6 && b <= 1e6))
                        && values
                            .iter()
                            .flatten()
                            .all(|&v| f32::from_bits(v).abs() <= 1e6),
                    "Source vector ramp exceeds bounded finite arithmetic"
                );
                std::array::from_fn(|i| {
                    let initial = f32::from_bits(values[0][i]);
                    let delta = values[i + 1].map(f32::from_bits);
                    let low = delta.map(|v| v.min(0.0));
                    let high = delta.map(|v| v.max(0.0));
                    Some((
                        initial + ((low[0] + low[2]) + (low[1] + low[3])),
                        initial + ((high[0] + high[2]) + (high[1] + high[3])),
                    ))
                })
            }
            0x3E => {
                let result = stack.pop().expect("validated output");
                // The native scalar wrapper clears the whole vector when any
                // lane is nonfinite. Require finite bounds in every output lane.
                ensure!(
                    result.iter().all(Option::is_some)
                        && result[0]
                            .is_some_and(|(minimum, maximum)| minimum >= 0.0 || maximum <= 0.0),
                    "Source dynamic output has no bounded one-sided weight contract"
                );
                bounds = result[0];
                continue;
            }
            _ => {
                stack.pop();
                [None; 4]
            }
        };
        stack.push(value);
        uniform.push(instruction.opcode == 0x37 || same);
    }
    bounds.context("Source dynamic program has no bounded output")
}
