//! Numeric event bodies are checked before native sequence allocation begins.
use super::*;
use crate::{d1_roi::procedure, presentation::append};

pub(super) enum Numeric {
    Delay,
    Scalar {
        header: [u8; 8],
        code: Vec<u8>,
        constants: Vec<u8>,
    },
}

impl Numeric {
    pub fn read(source: &Payload, event: &Event, outputs: usize) -> Result<Self> {
        if (event.class, event.kind) == (0x808006CE, 0) {
            return Ok(Self::Delay);
        }
        ensure!(
            (event.class, event.kind) == (0x80800507, 2),
            "Unsupported source numeric event {:08X}",
            event.class
        );
        let from = event.offset;
        let ordinal = source.u32(from + 0x24)?;
        ensure!(
            (ordinal as usize) < outputs,
            "Source numeric event output exceeds its table"
        );
        let code = source.array_range(from + 0x28, 1, Some(0x80800009))?;
        let constants = source.array(from + 0x38, 16, Some(0x80800090))?;
        let lanes = constants
            .iter()
            .map(|&at| {
                Ok([
                    source.u32(at)?,
                    source.u32(at + 4)?,
                    source.u32(at + 8)?,
                    source.u32(at + 12)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            lanes
                .iter()
                .flatten()
                .all(|&v| f32::from_bits(v).is_finite()),
            "Nonfinite sequence constant"
        );
        let inputs = source.array(from + 0x58, 4, Some(0x80800007))?;
        ensure!(
            inputs.len() == 1
                && source.u32(inputs[0])? == u32::MAX
                && source.u64(from + 0x48)? == 1
                && source.u32(from + 0x50)? == 1
                && source.u32(from + 0x54)? <= 1,
            "Source numeric event requires additional input translation"
        );
        procedure::validate(&source.0[code.clone()], &lanes, 1)?;
        Ok(Self::Scalar {
            header: source.bytes::<8>(from + 0x20)?,
            code: source.0[code].to_vec(),
            constants: constants
                .into_iter()
                .map(|at| source.bytes::<16>(at))
                .collect::<Result<Vec<_>>>()?
                .concat(),
        })
    }

    pub fn write(&self, target: &mut Payload, event: usize) -> Result<()> {
        let class = target.u32(
            event
                .checked_sub(4)
                .context("Native numeric event marker")?,
        )?;
        match self {
            Self::Delay => ensure!(
                class == 0x808093CB && target.u32(event + 0x30)? == 0,
                "Native delay event pair differs"
            ),
            Self::Scalar {
                header,
                code,
                constants,
            } => {
                ensure!(
                    class == 0x808093C4 && target.u32(event + 0x30)? == 2,
                    "Native scalar event pair differs"
                );
                put(&mut target.0, event + 0x38, header)?;
                append(&mut target.0, event + 0x40, 0x80800009, code, 1)?;
                append(&mut target.0, event + 0x50, 0x80800090, constants, 16)?;
                // Native dimensions are two u64 values. D1 stores a u32 output count
                // followed by a compiler flag, not the high word of a native count.
                put(&mut target.0, event + 0x60, &1u64.to_le_bytes())?;
                put(&mut target.0, event + 0x68, &1u64.to_le_bytes())?;
                // D1 explicitly lists builtin time as -1. Native events supply it implicitly.
                append(&mut target.0, event + 0x70, 0x80800007, &[], 4)?;
            }
        }
        Ok(())
    }
}
