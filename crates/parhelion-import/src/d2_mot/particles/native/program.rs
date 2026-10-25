//! Native particle programs (class 80806E2C) from the source dialect (80806927).
//!
//! The header and route maps were voted over 1,749 shipped programs whose defaults, constants
//! and phases one through seven are identical in both games. Header bytes move between fixed
//! offsets, the routes keep their indices except for the emission pair, and the arrays keep their
//! order. Named inputs the weapon fixes become constants, so the program reads no input table.
use super::super::Program;
use super::array;
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

const HEADER: usize = 0x150;
const UNSET: [u8; 2] = [0xFF, 0];

/// Source header bytes and their native offsets, outside the arrays, sections and routes.
fn header_bytes() -> impl Iterator<Item = (usize, usize)> {
    (0x38..=0x45)
        .map(|at| (at, at))
        .chain([
            (0x48, 0x46),
            (0x49, 0x47),
            (0x4C, 0x48),
            (0x4D, 0x49),
            (0x4E, 0x4A),
            (0x4F, 0x4B),
            (0x50, 0x4C),
            (0x51, 0x4D),
            (0x52, 0x4E),
            (0x53, 0x4F),
        ])
        .chain((0x118..=0x147).map(|at| (at, at - 8)))
        .chain([
            (0x14A, 0x140),
            (0x14B, 0x141),
            (0x14C, 0x142),
            (0x14D, 0x143),
            (0x14E, 0x144),
            (0x14F, 0x145),
            (0x150, 0x146),
            (0x151, 0x148),
            (0x152, 0x149),
            (0x153, 0x14A),
            (0x154, 0x14B),
            (0x54, 0x14C),
        ])
}

/// Source route 32 is native route 2. Source route 40 is native 32, or native 51 when source
/// route 51 is live, which then takes native 32. Native 55 repeats route 42. Source route 2 has
/// no native output.
fn routes(source: &[[u8; 2]]) -> Result<Vec<[u8; 2]>> {
    ensure!(source.len() == 56, "particle route table differs");
    ensure!(
        source[55] == UNSET || source[55] == source[42],
        "particle route 55 differs from route 42"
    );
    for index in [1, 3, 4, 53, 54] {
        ensure!(
            source[index] == UNSET,
            "particle route {index} has no voted native counterpart"
        );
    }
    let mut native = source.to_vec();
    native[2] = source[32];
    if source[51] == UNSET {
        native[32] = source[40];
        native[51] = UNSET;
    } else {
        native[32] = source[51];
        native[51] = source[40];
    }
    native[40] = UNSET;
    native[55] = source[42];
    Ok(native)
}

/// Replace reads of fixed named inputs with constants appended to the program.
fn specialize(program: &mut Program, inputs: &BTreeMap<u32, f32>) -> Result<()> {
    let mut slots = BTreeMap::new();
    for section in &mut program.sections {
        let mut code = Vec::with_capacity(section.len());
        for instruction in super::super::program::parse(section)? {
            if instruction.op != 0x55 {
                code.push(instruction.op);
                code.extend_from_slice(instruction.args);
                continue;
            }
            let name = u32::from_be_bytes(instruction.args.try_into()?);
            let value = *inputs
                .get(&name)
                .with_context(|| format!("particle program reads unfixed input {name:08X}"))?;
            ensure!(value.is_finite(), "fixed particle input is not finite");
            let slot = match slots.get(&name) {
                Some(slot) => *slot,
                None => {
                    let slot = u8::try_from(program.constants.len())
                        .context("fixed particle input exceeds the constant table")?;
                    let mut vector = [0; 16];
                    for lane in 0..4 {
                        vector[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
                    }
                    program.constants.push(vector);
                    slots.insert(name, slot);
                    slot
                }
            };
            code.extend([0x42, slot]);
        }
        *section = code;
    }
    program.channel_names.clear();
    Ok(())
}

/// Resolve the importing weapon's fixed inputs before planning workspace and material changes.
pub(super) fn prepare(source: &[u8], inputs: &BTreeMap<u32, f32>) -> Result<Program> {
    let mut program = Program::read(source)?;
    let p = Payload(source.to_vec());
    ensure!(
        p.u64(0x28)? != 0 || p.u64(0x30)? == 0,
        "empty particle transform table has a pointer"
    );
    specialize(&mut program, inputs)?;
    Ok(program)
}

/// Serialize the planned program while retaining the source header's other contracts.
/// `identity` fills +0xF8, which the native runtime copies to each emitter as its seed identity.
pub(super) fn native(source: &[u8], program: &Program, identity: u32) -> Result<Vec<u8>> {
    let p = Payload(source.to_vec());
    let lowered = program.lower(&BTreeMap::new())?;
    let workspace = lowered.native_workspace_bytes()?;
    let routes = routes(&program.routes)?;
    // The per-emitter byte array at +0x18 (two entries in every inspected program) is copied
    // as it stands. Its native array keeps the same element class.
    let bindings = p
        .array(0x18, 1, Some(0x80800009))?
        .into_iter()
        .map(|at| p.u8(at))
        .collect::<Result<Vec<_>>>()?;
    let mut out = vec![0; HEADER];
    for (from, to) in header_bytes() {
        out[to] = p.u8(from)?;
    }
    out[0x147] = workspace;
    let code = lowered.sections.concat();
    for (index, section) in lowered.sections.iter().enumerate() {
        let length = u16::try_from(section.len()).context("particle phase exceeds capacity")?;
        out[0x70 + index * 2..0x72 + index * 2].copy_from_slice(&length.to_le_bytes());
    }
    for (index, route) in routes.iter().enumerate() {
        out[0x80 + index * 2..0x82 + index * 2].copy_from_slice(route);
    }
    out[0xF8..0xFC].copy_from_slice(&identity.to_le_bytes());
    let vectors = |rows: &[[u8; 16]]| rows.concat();
    array(&mut out, 0x08, 0x80800090, &vectors(&lowered.defaults), 16)?;
    array(&mut out, 0x18, 0x80800009, &bindings, 1)?;
    // The source 6928 and native 6E2D records are two-byte transform bindings.
    // Keep their indices and flags. Their count is not the runtime transform
    // capacity, which also includes inputs supplied by the containing sequence.
    array(
        &mut out,
        0x28,
        0x80806E2D,
        &lowered.transform_bindings.concat(),
        2,
    )?;
    array(&mut out, 0x50, 0x80800009, &code, 1)?;
    array(&mut out, 0x60, 0x80800090, &vectors(&lowered.constants), 16)?;
    let size = out.len() as u64;
    out[..8].copy_from_slice(&size.to_le_bytes());
    validate(&out, &lowered)?;
    Ok(out)
}

/// Read the written program back in the native layout before it leaves the converter.
fn validate(bytes: &[u8], lowered: &Program) -> Result<()> {
    let p = Payload(bytes.to_vec());
    ensure!(
        p.u64(0)? == bytes.len() as u64,
        "native particle program size differs"
    );
    let defaults = p.array(0x08, 16, Some(0x80800090))?.len();
    let constants = p.array(0x60, 16, Some(0x80800090))?.len();
    let code = p.array(0x50, 1, Some(0x80800009))?.len();
    ensure!(
        defaults == lowered.defaults.len()
            && constants == lowered.constants.len()
            && code == lowered.sections.iter().map(Vec::len).sum::<usize>(),
        "native particle program arrays differ"
    );
    ensure!(
        p.u64(0x100)? == 0,
        "native particle program reads named inputs"
    );
    for index in 0..56 {
        let [bank, scalar] = p.bytes::<2>(0x80 + index * 2)?;
        match bank {
            0xFF => ensure!(scalar == 0, "unset native particle route is not cleared"),
            6 => ensure!(
                usize::from(scalar) < defaults * 4,
                "native particle route is outside the defaults"
            ),
            5 => ensure!(
                (u16::from(scalar) + 1) * 4 <= u16::from(p.u8(0x147)?),
                "native particle route is outside the workspace"
            ),
            0..=4 => {}
            _ => bail!("native particle route names an unknown bank"),
        }
    }
    Ok(())
}
