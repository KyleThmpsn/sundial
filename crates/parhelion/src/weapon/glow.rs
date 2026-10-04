//! Add dye emission to structurally recognized native opaque gear pixel programs.
//! Existing emission consumers and programs outside this material contract stay intact.
use crate::AuthoringResult;
use dxbc::{Program, temp};
mod checksum;
mod dxbc;
mod kernel;
mod plan;

const XYZ: u32 = 0xE4;
const X: u32 = 0;
const Y: u32 = 0x55;
const Z: u32 = 0xAA;
const W: u32 = 0xFF;
const SAT: u32 = 1 << 13;

pub(crate) enum Change {
    NotDyeMaterial,
    AlreadySupported,
    Patched(Vec<u8>),
}

/// Recognize, plan and rewrite only a supported native opaque consumer.
pub(crate) fn patch(bytes: &[u8]) -> AuthoringResult<Change> {
    let mut program = Program::read(bytes)?;
    let plan = match plan::read(&program)? {
        plan::Choice::NotDyeMaterial => return Ok(Change::NotDyeMaterial),
        plan::Choice::AlreadySupported => return Ok(Change::AlreadySupported),
        plan::Choice::Patch(plan) => plan,
    };
    let mut suffix = kernel::instructions(&program.instructions, &plan)?;
    let color = plan.first + 3;
    let factors = plan.first + 4;
    let mut result = Vec::new();
    for (index, mut instruction) in program.instructions.into_iter().enumerate() {
        if index == plan.declaration {
            instruction.words[3] = 27;
        }
        if index == plan.temps {
            instruction.words[1] = plan.first + 5;
        }
        if index == plan.albedo {
            result.append(&mut suffix);
            let mut args = instruction.args.clone();
            args[1] = temp(color, XYZ);
            instruction = instruction.replace(args)?;
        }
        if index == plan.luminance {
            let mut args = instruction.args.clone();
            args[1] = temp(color, 0x24);
            instruction = instruction.replace(args)?;
        }
        if index == plan.intensity {
            let mut args = instruction.args.clone();
            args[3] = temp(factors, W);
            instruction = instruction.replace(args)?;
        }
        result.push(instruction);
    }
    program.instructions = result;
    Ok(Change::Patched(program.emit()?))
}
