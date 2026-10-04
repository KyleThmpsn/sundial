//! Instruction availability is separate from engine bindings and particle playback.
use super::{Program, vm};
use std::collections::BTreeSet;

pub(crate) struct Coverage {
    pub instructions: usize,
    pub available: usize,
    pub unsupported: BTreeSet<u8>,
    pub runtime_inputs: bool,
}

impl Program {
    pub fn coverage(&self) -> Result<Coverage, String> {
        let mut coverage = Coverage {
            instructions: 0,
            available: 0,
            unsupported: BTreeSet::new(),
            runtime_inputs: false,
        };
        for section in 0..self.sections.len() {
            let code = self
                .section(section)
                .ok_or("Particle section exceeds bytecode")?;
            let mut at = 0;
            while at < code.len() {
                let opcode = code[at];
                let length = instruction_len(opcode).ok_or_else(|| {
                    format!(
                        "Unknown particle opcode 0x{opcode:02X} in section {section} at byte {at}"
                    )
                })?;
                if at + length > code.len() {
                    return Err(format!(
                        "Particle instruction crosses section {section} at byte {at}"
                    ));
                }
                coverage.instructions += 1;
                if vm::supported(opcode) {
                    coverage.available += 1;
                } else {
                    coverage.unsupported.insert(opcode);
                }
                coverage.runtime_inputs |= matches!(opcode, 0x3D | 0x40..=0x47 | 0x49..=0x4B);
                at += length;
            }
        }
        Ok(coverage)
    }
}

pub(super) fn instruction_len(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0x3E | 0x3F => 4,
        0x22 | 0x34..=0x3B | 0x40..=0x47 => 2,
        0x01..=0x33 | 0x3D | 0x49..=0x52 => 1,
        _ => return None,
    })
}
