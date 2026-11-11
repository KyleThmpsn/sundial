//! Checked scalar programs with builtin sequence context.
use super::*;
use crate::{d1_roi::procedure, presentation::append};

/// A numeric program. Rise of Iron lists each builtin input as `FFFFFFFF`. Native supplies
/// builtins implicitly and keeps only additional inputs in its slot array.
#[derive(Clone, PartialEq)]
pub(super) struct Program {
    code: Vec<u8>,
    constants: Vec<u8>,
    inputs: u64,
    /// Sequence input indices after the builtins, source or native as the program was mapped.
    additional: Vec<u32>,
}

impl Program {
    /// Read a 48-byte raw program, or a 64-byte bound program with its builtin slot list.
    pub(super) fn read(
        owner: &Payload,
        at: usize,
        bound: bool,
        builtins: u64,
    ) -> Result<Option<Self>> {
        Self::read_inputs(owner, at, bound, builtins, false)
    }

    /// Read a bound program that may also read sequence inputs after its builtins. Rise of Iron
    /// lists those inputs by sequence input index after the builtin slots, and `mapped` turns
    /// them into native indices before writing.
    pub(super) fn read_with_inputs(
        owner: &Payload,
        at: usize,
        builtins: u64,
    ) -> Result<Option<Self>> {
        Self::read_inputs(owner, at, true, builtins, true)
    }

    fn read_inputs(
        owner: &Payload,
        at: usize,
        bound: bool,
        builtins: u64,
        additional: bool,
    ) -> Result<Option<Self>> {
        let size = if bound { 64 } else { 48 };
        if owner
            .0
            .get(at..at + size)
            .context("Source query program extent")?
            .iter()
            .all(|b| *b == 0)
        {
            return Ok(None);
        }
        let code = owner.array_range(at, 1, Some(0x80800009))?;
        let rows = owner.array(at + 16, 16, Some(0x80800090))?;
        let lanes = rows
            .iter()
            .map(|&row| {
                Ok([
                    owner.u32(row)?,
                    owner.u32(row + 4)?,
                    owner.u32(row + 8)?,
                    owner.u32(row + 12)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            lanes
                .iter()
                .flatten()
                .all(|&v| f32::from_bits(v).is_finite()),
            "Nonfinite source query constant"
        );
        let inputs = owner.u64(at + 32)?;
        ensure!(
            (inputs == builtins || (additional && inputs > builtins))
                && owner.u32(at + 40)? == 1
                && owner.u32(at + 44)? <= 1,
            "Source query program dimensions differ"
        );
        let mut extra = Vec::new();
        if bound {
            let slots = owner
                .array(at + 48, 4, Some(0x80800007))?
                .into_iter()
                .map(|slot| owner.u32(slot))
                .collect::<Result<Vec<_>>>()?;
            let builtin = usize::try_from(builtins)?;
            ensure!(
                slots.len() as u64 == inputs
                    && slots[..builtin].iter().all(|&slot| slot == u32::MAX)
                    && slots[builtin..].iter().all(|&slot| slot != u32::MAX),
                "Source query program needs additional input translation"
            );
            extra = slots[builtin..].to_vec();
        }
        procedure::validate(&owner.0[code.clone()], &lanes, usize::try_from(inputs)?)?;
        Ok(Some(Self {
            code: owner.0[code].to_vec(),
            constants: rows
                .into_iter()
                .map(|row| owner.bytes::<16>(row))
                .collect::<Result<Vec<_>>>()?
                .concat(),
            inputs,
            additional: extra,
        }))
    }

    pub(super) fn constant(value: f32, inputs: u64) -> Self {
        Self {
            code: vec![0x34, 0, 0x3E, 0],
            constants: value.to_le_bytes().repeat(4),
            inputs,
            additional: Vec::new(),
        }
    }

    /// The program with its sequence inputs renumbered by the translated sequence's inputs.
    pub(super) fn mapped(&self, inputs: &BTreeMap<u32, u32>) -> Result<Self> {
        let mut program = self.clone();
        for slot in &mut program.additional {
            *slot = *inputs
                .get(slot)
                .context("Program sequence input has no native mapping")?;
        }
        Ok(program)
    }

    pub(super) fn write(
        program: Option<&Self>,
        target: &mut Payload,
        at: usize,
        bound: bool,
    ) -> Result<()> {
        let Some(program) = program else {
            return put(&mut target.0, at, &vec![0; if bound { 64 } else { 48 }]);
        };
        append(&mut target.0, at, 0x80800009, &program.code, 1)?;
        append(&mut target.0, at + 16, 0x80800090, &program.constants, 16)?;
        put(&mut target.0, at + 32, &program.inputs.to_le_bytes())?;
        put(&mut target.0, at + 40, &1u64.to_le_bytes())?;
        // Native supplies builtins implicitly and lists only the additional inputs.
        if bound {
            let slots = program
                .additional
                .iter()
                .flat_map(|slot| slot.to_le_bytes())
                .collect::<Vec<_>>();
            append(&mut target.0, at + 48, 0x80800007, &slots, 4)?;
        } else {
            ensure!(
                program.additional.is_empty(),
                "Raw program cannot list sequence inputs"
            );
        }
        Ok(())
    }
}
