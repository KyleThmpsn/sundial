//! Rise of Iron feedback events become native kind-12 feedback events.
//!
//! Source `80802891` (`310` bytes) and native `80803D8D` (`370` bytes) store the same `2E8`-byte
//! program block. Source start `5ADEA0` and native start `E7D930` read its feedback resource at
//! block `+08` and evaluate ten two-input scale programs from block `+68` with stride `40`, through
//! `5AD140` and `E77940`. Both stop callbacks, `5AE0E0` and `E7D8E0`, end the owner's feedback
//! unless the stop byte is set.
//!
//! | Role | Source | Native |
//! | --- | --- | --- |
//! | Shared `808093DB` byte, unread by both callbacks | None | `+38`, `FF` |
//! | Program block | `+20..+307` | `+40..+327` |
//! | Fallback feedback with unit scales | `+308` | `+328` |
//! | Target mode, 0 or 1 | `+30C` | `+32C` |
//! | Keep feedback when stopped | `+30D` | `+32D` |
//! | Transform from the event owner rather than the target | None | `+32E`, zero as in source |
//! | Native-only program `5ADC7388` | None | `+330`, the stock constant one |
use super::{program::Program, *};
use crate::d1_roi::{Source, dependency};

const PROGRAMS: usize = 10;

pub struct Feedback {
    offset: usize,
    front: [u8; 8],
    feedback: u32,
    programs: Vec<Option<Program>>,
    fallback: Option<u32>,
    mode: u8,
    stop: u8,
}

fn resource(source: &Source, tag: u32) -> Result<u32> {
    ensure!(
        source.reference(tag)? == 0x80802637,
        "Source feedback event does not name a feedback resource"
    );
    Ok(tag)
}

impl Feedback {
    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source feedback marker")?)? == 0x80802891
                && owner.u32(at + 0x1C)? == 7,
            "Source feedback event class or kind differs"
        );
        owner.bytes::<0x310>(at)?;
        // Source `+78` is an array descriptor, block `+58`. An empty one still stores a relative
        // offset at `+80`, which the Necrochasm shot's feedback event points at a count-zero
        // header past the event. Read by count alone it has nothing to carry, and the native
        // writer leaves the block's descriptor empty. A nonempty one has no translation yet.
        ensure!(
            owner.0[at + 0x2C..at + 0x78].iter().all(|b| *b == 0)
                && owner.array_range(at + 0x78, 1, None)?.is_empty()
                && owner.0[at + 0x30E..at + 0x310].iter().all(|b| *b == 0),
            "Source feedback event has fields without a native counterpart"
        );
        let (mode, stop) = (owner.u8(at + 0x30C)?, owner.u8(at + 0x30D)?);
        ensure!(
            mode <= 1 && stop <= 1,
            "Source feedback target mode or stop byte differs"
        );
        // A scale program can read sequence inputs after its two builtins, as the Necrochasm shot's
        // programs 5 and 7 read input 0. Stock native feedback events do the same, such as
        // `80B58D96` and `80BBDA95` reading input 0 or 1 through `3C 02 3E 00`.
        let programs = (0..PROGRAMS)
            .map(|i| Program::read_with_inputs(owner, at + 0x88 + i * 0x40, 2))
            .collect::<Result<Vec<_>>>()?;
        let fallback = match owner.u32(at + 0x308)? {
            u32::MAX => None,
            tag => Some(resource(source, tag)?),
        };
        source.unchanged()?;
        Ok(Self {
            offset: at,
            front: owner.bytes::<8>(at + 0x20)?,
            feedback: resource(source, owner.u32(at + 0x28)?)?,
            programs,
            fallback,
            mode,
            stop,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.offset,
            NativeEvent {
                source_class: 0x80802891,
                source_kind: 7,
                class: 0x80803D8D,
            },
        )
    }

    /// Source feedback resources this event names.
    pub fn resources(&self) -> impl Iterator<Item = u32> + '_ {
        std::iter::once(self.feedback).chain(self.fallback)
    }

    /// Fill a freshly allocated native event. Feedback resources stay unresolved until the
    /// containing graph allocates their private translations. Programs that read sequence
    /// inputs take the translated sequence's input indices from `inputs`.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        feedback: &BTreeMap<u32, String>,
        inputs: &BTreeMap<u32, u32>,
    ) -> Result<Vec<ResourcePatch>> {
        target.bytes::<0x370>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        ensure!(
            target.u32(event.checked_sub(4).context("Native feedback marker")?)? == 0x80803D8D
                && target.u32(event + 0x30)? == 12
                && target.u32(event + 4)? == 0x80803D8C
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native feedback runtime marker")?
                )? == 0x80803D8C
                && target.u64(instance + 8)? == event as u64,
            "Native feedback event pair, kind or class differs"
        );
        let mut output = target.clone();
        put(&mut output.0, event + 0x38, &[0; 0x338])?;
        put(&mut output.0, event + 0x38, &0xFFu32.to_le_bytes())?;
        put(&mut output.0, event + 0x40, &self.front)?;
        let mut patches = Vec::new();
        for (offset, tag) in [(0x48, Some(self.feedback)), (0x328, self.fallback)] {
            put(&mut output.0, event + offset, &u32::MAX.to_le_bytes())?;
            let Some(tag) = tag else { continue };
            let symbol = feedback
                .get(&tag)
                .context("Feedback event resource has no private translation")?;
            dependency::symbol(symbol)?;
            patches.push(ResourcePatch {
                offset: event + offset,
                symbol: symbol.clone(),
                source: tag,
                source_class: 0x80802637,
                native_class: 0x80803EB0,
            });
        }
        for (i, program) in self.programs.iter().enumerate() {
            let mapped = program
                .as_ref()
                .map(|program| program.mapped(inputs))
                .transpose()?;
            Program::write(mapped.as_ref(), &mut output, event + 0xA8 + i * 0x40, true)?;
        }
        output.0[event + 0x32C] = self.mode;
        output.0[event + 0x32D] = self.stop;
        Program::write(
            Some(&Program::constant(1.0, 1)),
            &mut output,
            event + 0x330,
            true,
        )?;
        *target = output;
        Ok(patches)
    }
}
