//! Ability adjustments retain qualified source curves and input selection.
use super::*;
use sundial::package_authoring::sandbox_perk::action::native::{
    Block, Graph, NodeKind,
    value::{Instruction, Program as ValueProgram},
};

pub(super) fn read(p: &Payload, at: usize) -> Result<Action> {
    ensure!(
        p.u8(at + 1)? == 0 && p.u8(at + 2)? <= 2 && p.u8(at + 3)? <= 2,
        "Source ability adjustment target or activity gate requires translation"
    );
    ensure!(
        p.u32(at + 12)? == 0
            && p.u64(at + 16)? == 0
            && p.u64(at + 0x38)? == 1
            && p.u64(at + 0x40)? == 1
            && matches!(p.u8(at + 0x48)?, 0..=2 | 6 | 11 | 0xFF)
            && p.bytes::<7>(at + 0x49)? == [0; 7],
        "Source ability adjustment has unsupported inputs or metadata"
    );
    let code = p
        .array(at + 0x18, 1, Some(0x80800009))?
        .into_iter()
        .map(|row| p.u8(row))
        .collect::<Result<Vec<_>>>()?;
    let constants = p
        .array(at + 0x28, 16, Some(0x80800090))?
        .into_iter()
        .map(|row| p.bytes::<16>(row))
        .collect::<Result<Vec<_>>>()?;
    let maximum = validate(&code, &constants)?;
    ensure!(
        (maximum * p.f32(at + 4)?.abs()).is_finite(),
        "Source ability curve can overflow its scale"
    );
    let _ = p.f32(at + 8)?;

    // Source 718790 and native EC4D70 select the current ability and pass a
    // non-normalized scalar to the value program. FF supplies 1, not zero.
    // Input 11 uses each engine's target-label value table. Its lookup contract
    // corresponds, but native balance values are not replaced with source globals.
    let mut bytes = vec![0; 0x50];
    bytes[..4].copy_from_slice(&p.bytes::<4>(at)?);
    bytes[0] = 8;
    // Native +4 selects current rather than base. Scale and limit move four bytes.
    put(&mut bytes, 8, &p.bytes::<8>(at + 4)?)?;
    put(&mut bytes, 0x18, &(code.len() as u64).to_le_bytes())?;
    put(&mut bytes, 0x28, &(constants.len() as u64).to_le_bytes())?;
    put(&mut bytes, 0x38, &p.bytes::<24>(at + 0x38)?)?;
    let graph = Graph {
        blocks: vec![
            Block {
                class: 0x80803E4D,
                count: None,
                bytes,
                links: BTreeMap::from([(0x20, 1), (0x30, 2)]),
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
        .validate_node(NodeKind::Effect(8))
        .map_err(anyhow::Error::msg)?;
    Ok(Action::Native {
        node: NativeNode {
            kind: 8,
            bytes: graph.emit().map_err(anyhow::Error::msg)?,
        },
    })
}

fn validate(code: &[u8], constants: &[[u8; 16]]) -> Result<f32> {
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
            .all(|&bits| f32::from_bits(bits).is_finite()),
        "Source ability curve has a nonfinite constant"
    );
    let mut instructions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        let opcode = code[at];
        at += 1;
        let operand = match opcode {
            0x03 | 0x0F | 0x21 | 0x23 => None,
            0x22 | 0x34 | 0x35 | 0x37 | 0x39 | 0x3C | 0x3E => {
                let value = *code
                    .get(at)
                    .context("Truncated ability-curve instruction")?;
                at += 1;
                Some(value)
            }
            _ => anyhow::bail!("Source ability-curve opcode {opcode:02X} requires translation"),
        };
        ensure!(
            opcode != 0x3E || at == code.len(),
            "Source ability-curve output must end its program"
        );
        instructions.push(Instruction { opcode, operand });
    }
    let program = ValueProgram {
        instructions,
        constants,
        fast_path: 0,
    };
    program.validate().map_err(anyhow::Error::msg)?;
    // Native 3B9FE0 clears all lanes if any output lane is nonfinite, while source
    // 416FF0 returns lane zero directly. Only admit curves with finite output bounds.
    // Polynomial intermediates stay unknown until saturation bounds their result.
    let mut stack = Vec::<[Option<(f32, f32)>; 4]>::new();
    for instruction in &program.instructions {
        let operand = usize::from(instruction.operand.unwrap_or(0));
        let value = match instruction.opcode {
            0x34 => program.constants[operand].map(|bits| {
                let value = f32::from_bits(bits);
                Some((value, value))
            }),
            0x3C => [None; 4],
            0x03 => {
                let b = stack.pop().expect("validated product operand");
                let a = stack.pop().expect("validated product operand");
                std::array::from_fn(|i| {
                    let (amin, amax) = a[i]?;
                    let (bmin, bmax) = b[i]?;
                    let values = [amin * bmin, amin * bmax, amax * bmin, amax * bmax];
                    values.iter().all(|v| v.is_finite()).then(|| {
                        (
                            values.into_iter().fold(f32::INFINITY, f32::min),
                            values.into_iter().fold(f32::NEG_INFINITY, f32::max),
                        )
                    })
                })
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
            0x3E => {
                let values = stack.pop().expect("validated output");
                ensure!(
                    values.iter().all(Option::is_some),
                    "Source ability curve has no finite output bound"
                );
                let (minimum, maximum) = values[0].expect("bounded scalar output");
                return Ok(minimum.abs().max(maximum.abs()));
            }
            opcode => {
                stack.pop();
                if matches!(opcode, 0x0F | 0x39) {
                    stack.pop();
                }
                [None; 4]
            }
        };
        stack.push(value);
    }
    anyhow::bail!("Source ability curve has no output")
}
