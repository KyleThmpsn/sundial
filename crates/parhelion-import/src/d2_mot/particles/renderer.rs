//! Coordinate source emitter registers with their consuming material programs.
//! This does not establish renderer scope bindings or serialize native assets.
use super::{Program, program};
use crate::d2_mot::tfx;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod bindings;

const PARTICLE_SCOPE: u8 = 0x1A;

#[derive(Clone, Serialize, Deserialize)]
pub struct Material {
    pub code: Vec<u8>,
    pub constants: Vec<[u8; 16]>,
}

#[derive(Serialize, Deserialize)]
pub struct Compacted {
    pub program: Program,
    pub materials: Vec<Material>,
    /// Old vector indices to new indices. None denotes a renderer expression.
    pub registers: Vec<Option<u8>>,
    pub expressions: BTreeMap<u8, Vec<u8>>,
}

struct Expression {
    start: usize,
    end: usize,
    slot: u8,
    code: Vec<u8>,
    inputs: BTreeMap<u8, u8>,
}

fn lane_mask(selector: u8) -> Result<u8> {
    [15, 7, 3, 12, 1, 2, 4, 8]
        .get(usize::from(selector))
        .copied()
        .context("particle register selector")
}

fn effect(op: u8) -> Result<(usize, usize)> {
    Ok(match op {
        7 | 0x18..=0x23 | 0x28..=0x2A | 0x2D..=0x32 | 0x43..=0x46 | 0x48..=0x49 => (1, 1),
        1..=4 | 8..=15 | 0x33 | 0x47 => (2, 1),
        0x13..=0x16 => (3, 1),
        0x35 => (5, 1),
        0x42 | 0x4B | 0x4C | 0x51 | 0x54 | 0x55 => (0, 1),
        0x4D => (1, 0),
        0x4E => (2, 0),
        0x57 | 0x58 => (3, 2),
        0x5A | 0x5D => (6, 2),
        0x5E => (1, 2),
        _ => bail!("unknown particle stack effect {op:02X}"),
    })
}

fn candidates(code: &[u8]) -> Result<Vec<Expression>> {
    let mut result = Vec::new();
    let (mut at, mut start, mut depth) = (0usize, 0usize, 0usize);
    let mut pure = true;
    let mut inputs = BTreeMap::<u8, u8>::new();
    for i in program::parse(code)? {
        let end = at + 1 + i.args.len();
        let (pop, push) = effect(i.op)?;
        ensure!(depth >= pop, "renderer extraction stack underflow");
        depth = depth - pop + push;
        ensure!(depth <= 64, "renderer extraction stack capacity");
        match i.op {
            0x4C if i.args[0] == 5 && matches!(i.args[2], 0 | 4..=7) => {
                *inputs.entry(i.args[1]).or_default() |= lane_mask(i.args[2])?;
            }
            0x4D if depth == 0 && i.args[0] == 5 && i.args[2] == 0 => {
                if pure {
                    result.push(Expression {
                        start,
                        end,
                        slot: i.args[1],
                        code: code[start..at].to_vec(),
                        inputs: inputs.clone(),
                    });
                }
            }
            1..=4
            | 7..=15
            | 0x13..=0x16
            | 0x18..=0x23
            | 0x28..=0x2A
            | 0x2E..=0x32
            | 0x35
            | 0x42..=0x46
            | 0x48..=0x49 => {}
            _ => pure = false,
        }
        if depth == 0 {
            start = end;
            pure = true;
            inputs.clear();
        }
        at = end;
    }
    ensure!(depth == 0, "renderer extraction unfinished expression");
    Ok(result)
}

fn mapped(registers: &[Option<u8>], slot: u8) -> Result<u8> {
    registers
        .get(usize::from(slot))
        .copied()
        .flatten()
        .context("particle register has no simulation storage")
}

// Phase 0 may run again between update and rendering. Prove that it is an
// idempotent initialization from immutable defaults and persistent lanes.
// Track lanes because initialization commonly writes xyz while retaining w.
fn stable_initialization(source: &Program) -> Result<bool> {
    let mut mutable = BTreeMap::<u8, u8>::new();
    for section in &source.sections[..7] {
        for i in program::parse(section)? {
            if i.op == 0x4D && i.args[0] == 5 {
                *mutable.entry(i.args[1]).or_default() |= lane_mask(i.args[2])?;
            }
        }
    }
    let mut assigned = BTreeMap::<u8, u8>::new();
    for i in program::parse(&source.sections[0])? {
        match i.op {
            0x4C if i.args[0] == 6 => {}
            0x4C if i.args[0] == 5 => {
                let changed = mutable.get(&i.args[1]).copied().unwrap_or(0);
                let stable = assigned.get(&i.args[1]).copied().unwrap_or(0);
                if lane_mask(i.args[2])? & changed & !stable != 0 {
                    return Ok(false);
                }
            }
            0x4D if i.args[0] == 5 => {
                *assigned.entry(i.args[1]).or_default() |= lane_mask(i.args[2])?;
            }
            0x4D if i.args[0] == 4 => {}
            1..=4
            | 7..=15
            | 0x13..=0x16
            | 0x18..=0x23
            | 0x28..=0x2A
            | 0x2E..=0x32
            | 0x35
            | 0x42..=0x46
            | 0x48..=0x49 => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn width(op: u8) -> usize {
    [1, 2, 2, 5, 10, 10, 6, 11][usize::from(op - 0x42)]
}

fn material_expression(
    expression: &[u8],
    source: &Program,
    registers: &[Option<u8>],
    material: &mut Material,
    constants: &mut BTreeMap<(u8, usize), u8>,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    for i in program::parse(expression)? {
        match i.op {
            0x4C => {
                ensure!(i.args[0] == 5, "renderer expression reads another VM bank");
                let slot = mapped(registers, i.args[1])?;
                if i.args[2] == 0 {
                    result.extend([0x4B, PARTICLE_SCOPE, slot]);
                } else {
                    ensure!((4..=7).contains(&i.args[2]), "renderer expression selector");
                    let scalar = slot
                        .checked_mul(4)
                        .and_then(|v| v.checked_add(i.args[2] - 4))
                        .context("renderer scalar index exceeds byte operand")?;
                    result.extend([0x4A, PARTICLE_SCOPE, scalar]);
                }
            }
            0x42..=0x49 => {
                let count = width(i.op);
                let key = (i.args[0], count);
                let index = if let Some(&index) = constants.get(&key) {
                    index
                } else {
                    let start = usize::from(i.args[0]);
                    let values = source
                        .constants
                        .get(start..start + count)
                        .context("renderer expression constant extent")?;
                    ensure!(
                        material.constants.len() + count <= 256,
                        "material constants exceed byte indices"
                    );
                    let index = u8::try_from(material.constants.len())?;
                    material.constants.extend_from_slice(values);
                    constants.insert(key, index);
                    index
                };
                result.extend([i.op, index]);
            }
            _ => {
                result.push(i.op);
                result.extend(i.args);
            }
        }
    }
    Ok(result)
}

fn material_consumers(materials: &[Material], slots: usize) -> Result<BTreeSet<u8>> {
    let mut consumed = BTreeSet::new();
    for material in materials {
        ensure!(
            material.constants.len() <= 256,
            "source material constant capacity"
        );
        for i in tfx::program::parse(&material.code)? {
            if (0x4A..=0x4F).contains(&i.op) && i.args[0] == PARTICLE_SCOPE {
                ensure!(
                    matches!(i.op, 0x4A | 0x4B),
                    "particle scope has matrix or resource consumers"
                );
                let slot = if i.op == 0x4A {
                    i.args[1] / 4
                } else {
                    i.args[1]
                };
                ensure!(
                    usize::from(slot) < slots,
                    "material reads outside emitter workspace"
                );
                consumed.insert(slot);
            }
        }
    }
    Ok(consumed)
}

struct RegisterAccess {
    reads: BTreeSet<u8>,
    writes: BTreeMap<u8, Vec<(usize, usize, u8)>>,
}

fn register_access(source: &Program, slots: usize) -> Result<RegisterAccess> {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeMap::<u8, Vec<(usize, usize, u8)>>::new();
    for (phase, section) in source.sections.iter().enumerate() {
        let mut at = 0;
        for i in program::parse(section)? {
            if matches!(i.op, 0x4C | 0x4D) && i.args[0] == 5 {
                ensure!(
                    usize::from(i.args[1]) < slots,
                    "VM register outside workspace"
                );
                if i.op == 0x4C {
                    reads.insert(i.args[1]);
                } else {
                    writes
                        .entry(i.args[1])
                        .or_default()
                        .push((phase, at, lane_mask(i.args[2])?));
                }
            }
            at += 1 + i.args.len();
        }
    }
    for &[bank, scalar] in &source.routes {
        if bank == 5 {
            ensure!(usize::from(scalar / 4) < slots, "route outside workspace");
            reads.insert(scalar / 4);
        }
    }
    Ok(RegisterAccess { reads, writes })
}

fn rewrite_material(
    original: &Material,
    source: &Program,
    registers: &[Option<u8>],
    expressions: &BTreeMap<u8, Vec<u8>>,
) -> Result<Material> {
    let mut material = Material {
        code: Vec::new(),
        constants: original.constants.clone(),
    };
    let mut constants = BTreeMap::new();
    let mut cache = BTreeMap::<u8, Vec<u8>>::new();
    for i in tfx::program::parse(&original.code)? {
        if matches!(i.op, 0x4A | 0x4B) && i.args[0] == PARTICLE_SCOPE {
            let slot = if i.op == 0x4A {
                i.args[1] / 4
            } else {
                i.args[1]
            };
            if let Some(expression) = expressions.get(&slot) {
                if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(slot) {
                    entry.insert(material_expression(
                        expression,
                        source,
                        registers,
                        &mut material,
                        &mut constants,
                    )?);
                }
                material.code.extend(&cache[&slot]);
                if i.op == 0x4A {
                    material.code.extend([0x29, (i.args[1] % 4) * 0x55]);
                }
            } else {
                let index = mapped(registers, slot)?;
                material.code.extend([
                    i.op,
                    PARTICLE_SCOPE,
                    if i.op == 0x4A {
                        index * 4 + i.args[1] % 4
                    } else {
                        index
                    },
                ]);
            }
        } else {
            material.code.push(i.op);
            material.code.extend(i.args);
        }
    }
    // Numeric dependencies and stack effects remain checked independently
    // of later renderer extern and texture binding resolution.
    tfx::program::inputs::required(&material.code, &BTreeMap::new())?;
    Ok(material)
}

/// Repack the class-80806927 source dialect and the complete set of material
/// expression programs consuming its particle scope. The caller must establish
/// that renderer binding and completeness before linking the result.
///
/// Only whole-vector writes from emitter update phase 1 can move. Their inputs
/// must remain stable from the original write through rendering. Engine routes,
/// all VM reads, all other phase writes and later writes in phase 1 veto a move.
/// No random or engine input operation is moved or replayed.
pub fn compact_source(source: &Program, materials: &[Material]) -> Result<Compacted> {
    source
        .lower_declared()
        .context("validate source particle dialect")?;
    ensure!(
        !materials.is_empty(),
        "renderer compaction needs all consuming materials"
    );
    let bytes = source
        .workspace_bytes
        .context("missing source workspace size")?;
    ensure!(
        bytes != 0 && bytes.is_multiple_of(16),
        "source workspace alignment"
    );
    let slots = usize::from(bytes / 16);
    ensure!(slots <= 64, "source workspace exceeds register operands");
    let consumed = material_consumers(materials, slots)?;
    let RegisterAccess { reads, writes } = register_access(source, slots)?;
    let stable_initialization = stable_initialization(source)?;
    let expressions = candidates(&source.sections[1])?
        .into_iter()
        .filter(|e| {
            consumed.contains(&e.slot)
                && !reads.contains(&e.slot)
                && writes
                    .get(&e.slot)
                    .is_some_and(|w| w.len() == 1 && w[0].0 == 1)
                && e.inputs.iter().all(|(input, mask)| {
                    writes.get(input).is_none_or(|w| {
                        (0..4).all(|lane| {
                            let bit = 1 << lane;
                            if *mask & bit == 0 {
                                return true;
                            }
                            let relevant =
                                || w.iter().filter(|&&(_, _, written)| written & bit != 0);
                            (stable_initialization && relevant().all(|&(phase, _, _)| phase == 0))
                                || relevant().all(|&(phase, at, _)| phase == 1 && at < e.start)
                        })
                    })
                })
        })
        .collect::<Vec<_>>();
    let moved = expressions.iter().map(|e| e.slot).collect::<BTreeSet<_>>();
    let mut next = 0u8;
    let registers = (0..slots)
        .map(|slot| {
            if moved.contains(&(slot as u8)) {
                None
            } else {
                let index = next;
                next += 1;
                Some(index)
            }
        })
        .collect::<Vec<_>>();
    ensure!(
        next <= 10,
        "renderer compaction retains {} bytes, native capacity is 160",
        u16::from(next) * 16
    );
    // Native headers require a nonempty vector allocation even for a material
    // whose complete particle scope was replaced by constant expressions.
    let mut program = source.clone();
    program.workspace_bytes = Some(u16::from(next.max(1)) * 16);
    for (phase, section) in source.sections.iter().enumerate() {
        let mut code = Vec::new();
        let mut at = 0;
        for i in program::parse(section)? {
            let end = at + 1 + i.args.len();
            if phase != 1 || !expressions.iter().any(|e| at >= e.start && end <= e.end) {
                code.push(i.op);
                if matches!(i.op, 0x4C | 0x4D) && i.args[0] == 5 {
                    code.extend([5, mapped(&registers, i.args[1])?, i.args[2]]);
                } else {
                    code.extend(i.args);
                }
            }
            at = end;
        }
        program.sections[phase] = code;
    }
    for route in &mut program.routes {
        if route[0] == 5 {
            route[1] = mapped(&registers, route[1] / 4)? * 4 + route[1] % 4;
        }
    }
    let expressions = expressions
        .into_iter()
        .map(|e| (e.slot, e.code))
        .collect::<BTreeMap<_, _>>();
    let mut rewritten = Vec::new();
    for original in materials {
        let material = rewrite_material(original, source, &registers, &expressions)?;
        rewritten.push(material);
    }
    program
        .lower_declared()
        .context("validate compacted particle dialect")?;
    program.native_workspace_bytes()?;
    Ok(Compacted {
        program,
        materials: rewritten,
        registers,
        expressions,
    })
}
