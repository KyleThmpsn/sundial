//! Source instruction boundaries and phase roles are qualified by original interpreter execution.
use crate::{
    d1_roi::Source,
    presentation::{append, put},
    tiger::payload::Payload,
};
use anyhow::{Context, Result, ensure};

/// A checked byte program, independent of particle routing, geometry, material and input producers.
/// Writing it into a native definition does not make that definition installable on its own.
pub struct Program {
    sections: [Vec<u8>; 8],
    defaults: Vec<u8>,
    constants: Vec<u8>,
}

impl Source {
    pub fn particle_program(&self, tag: u32) -> Result<Program> {
        ensure!(
            self.reference(tag)? == 0x80801A6A,
            "Source particle program has another resource class"
        );
        let result = Program::read(&self.read(tag)?)?;
        self.unchanged()?;
        Ok(result)
    }
}

fn vectors(source: &Payload, offset: usize) -> Result<Vec<u8>> {
    Ok(source
        .array(offset, 16, Some(0x80800090))?
        .into_iter()
        .map(|row| source.bytes::<16>(row))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

impl Program {
    pub(super) fn validate_storage(&self, state: u8, appearance: u8, workspace: u8) -> Result<()> {
        for section in &self.sections {
            let mut at = 0;
            while let Some(&opcode) = section.get(at) {
                let width = match opcode {
                    0x3E..=0x3F => {
                        let bank = section[at + 1];
                        let slot = usize::from(section[at + 2]);
                        let rows = match bank {
                            0 => usize::from(state),
                            1 => usize::from(appearance),
                            4 => 16,
                            5 => usize::from(workspace) / 16,
                            6 => self.defaults.len() / 16,
                            // Bank two belongs to the containing CPU caller. Its concrete
                            // input region is checked at that caller boundary, not here.
                            2 => 256,
                            _ => anyhow::bail!("Particle program requires another caller bank"),
                        };
                        ensure!(slot < rows, "Particle program exceeds its value storage");
                        ensure!(
                            opcode != 0x3F || bank != 6,
                            "Particle program writes immutable default values"
                        );
                        4
                    }
                    0x22 | 0x34..=0x3B | 0x43 | 0x46..=0x47 => 2,
                    _ => 1,
                };
                at += width;
            }
        }
        Ok(())
    }

    fn read(source: &Payload) -> Result<Self> {
        ensure!(
            source.0.len() >= 0x138 && source.u64(0)? == source.0.len() as u64,
            "Source particle definition extent differs"
        );
        let defaults = vectors(source, 8)?;
        let constants = vectors(source, 0x38)?;
        let code = source
            .array(0x28, 1, Some(0x80800009))?
            .into_iter()
            .map(|row| source.u8(row))
            .collect::<Result<Vec<_>>>()?;
        let lengths = (0..9)
            .map(|index| source.u16(0x48 + index * 2).map(usize::from))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            lengths.iter().sum::<usize>() == code.len()
                && code.len() <= usize::from(u16::MAX)
                && source.bytes::<6>(0x5A)? == [0; 6],
            "Source particle section extents differ"
        );
        // Source section four runs before normalized age is updated. Shadowkeep has no
        // corresponding dispatch there. Its absence is a checked condition, not a dropped phase.
        ensure!(
            lengths[4] == 0,
            "Source particle pre-age phase has no native execution route"
        );
        let mut sections: [Vec<u8>; 8] = std::array::from_fn(|_| Vec::new());
        let mut start = 0usize;
        for (index, length) in lengths.into_iter().enumerate() {
            let end = start
                .checked_add(length)
                .context("Particle phase overflow")?;
            if index != 4 {
                sections[if index < 4 { index } else { index - 1 }] =
                    lower(&code[start..end], defaults.len() / 16, constants.len() / 16)
                        .with_context(|| format!("Source particle phase {index}"))?;
            }
            start = end;
        }
        Ok(Self {
            sections,
            defaults,
            constants,
        })
    }

    /// Replace only the program-owned arrays on a checked native envelope. The caller must
    /// independently translate the definition's routes, controls and runtime input bindings.
    /// All work happens on a copy so a failed append leaves the destination intact.
    pub fn write(&self, target: &mut Payload) -> Result<()> {
        ensure!(
            target.0.len() >= 0x150 && target.u64(0)? == target.0.len() as u64,
            "Native particle definition extent differs"
        );
        let mut result = target.clone();
        append(&mut result.0, 8, 0x80800090, &self.defaults, 16)?;
        append(&mut result.0, 0x60, 0x80800090, &self.constants, 16)?;
        let code = self.sections.iter().flatten().copied().collect::<Vec<_>>();
        append(&mut result.0, 0x50, 0x80800009, &code, 1)?;
        for (index, section) in self.sections.iter().enumerate() {
            put(
                &mut result.0,
                0x70 + index * 2,
                &u16::try_from(section.len())?.to_le_bytes(),
            )?;
        }
        *target = result;
        Ok(())
    }
}

fn lower(source: &[u8], defaults: usize, constants: usize) -> Result<Vec<u8>> {
    let mut result = Vec::with_capacity(source.len());
    let mut cursor = 0;
    let mut depth = 0usize;
    while let Some(&opcode) = source.get(cursor) {
        // Only operations exercised by the complete stored-program comparison are admitted.
        // Three-byte source operations and exit opcodes need separate execution evidence.
        let (width, consumed, produced) = match opcode {
            0x01..=0x04 | 0x08..=0x09 | 0x0B..=0x0F | 0x2C => (1, 2, 1),
            0x10 | 0x12..=0x13 => (1, 3, 1),
            0x15 | 0x17..=0x18 | 0x1A | 0x1C..=0x1F | 0x21 | 0x23 | 0x26..=0x2A => (1, 1, 1),
            0x22 | 0x35 | 0x37..=0x38 | 0x3A..=0x3B => (2, 1, 1),
            0x39 => (2, 2, 1),
            0x34 | 0x43 | 0x46..=0x47 => (2, 0, 1),
            0x3E => (1, 0, 1),
            0x41 => (4, 0, 1),
            0x42 => (4, 1, 0),
            _ => anyhow::bail!("Source particle opcode {opcode:02X} has no qualified native route"),
        };
        let instruction = source
            .get(cursor..cursor + width)
            .context("Source particle instruction crosses a phase boundary")?;
        depth = depth
            .checked_sub(consumed)
            .context("Source particle instruction underflows its phase stack")?
            + produced;
        if matches!(opcode, 0x34..=0x3B) {
            let count = match opcode {
                0x34 => 1,
                0x35 => 2,
                0x37 => 5,
                0x38..=0x39 => 10,
                0x3A => 6,
                0x3B => 11,
                _ => unreachable!(),
            };
            ensure!(
                usize::from(instruction[1]) + count <= constants,
                "Source particle constant span exceeds its array"
            );
        }
        if matches!(opcode, 0x41..=0x42) {
            ensure!(
                instruction[1] <= 6 && instruction[3] <= 7,
                "Source particle register bank or lane selector differs"
            );
            ensure!(
                instruction[1] != 6 || usize::from(instruction[2]) < defaults,
                "Source particle register exceeds its default bank"
            );
        }
        result.push(match opcode {
            0x3E => 0x3D,
            0x41 => 0x3E,
            0x42 => 0x3F,
            other => other,
        });
        result.extend_from_slice(&instruction[1..]);
        cursor += width;
    }
    Ok(result)
}
