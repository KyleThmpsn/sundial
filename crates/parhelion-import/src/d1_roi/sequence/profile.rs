//! Profile events apply an outgoing profile owner, such as a heal, to the sequence's owner.
//!
//! Rise of Iron kind 24, class `80802402` (`B8` bytes), corresponds to Shadowkeep kind 27, class
//! `80804AC4` (definition `1A0`, instance `60` bytes). Six authored event names occur in both
//! builds with the same timing and values:
//!
//! | Role | Source | Native |
//! | --- | --- | --- |
//! | Profile owner | `+28` | `+48` |
//! | First bound program, 0 or 1 in both corpora | `+30` | `+50` |
//! | Second bound program, mostly 1 | `+70` | `+110` |
//!
//! Every paired native event leaves the program at `+90` empty, stores constant 0 at `+D0` and
//! `+150`, zero at `+40` and an empty name at `+194`. Those native-only fields take the same
//! values. The 405 source events and 477 stock events hold the same value populations.
use super::{program::Program, *};
use crate::d1_roi::Source;

pub struct Profile {
    offset: usize,
    owner: u32,
    programs: [Option<Program>; 2],
}

impl Profile {
    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source profile event marker")?)? == 0x80802402
                && owner.u32(at + 0x1C)? == 24,
            "Source profile event class or kind differs"
        );
        ensure!(
            owner.u64(at + 0x20)? == 0 && owner.u32(at + 0x2C)? == 0 && owner.u64(at + 0xB0)? == 0,
            "Source profile event reserved fields differ"
        );
        let profile = owner.u32(at + 0x28)?;
        ensure!(
            source.reference(profile)? == 0x80800861,
            "Source profile event does not name a component owner"
        );
        Ok(Self {
            offset: at,
            owner: profile,
            programs: [
                Program::read(owner, at + 0x30, true, 1)?,
                Program::read(owner, at + 0x70, true, 1)?,
            ],
        })
    }

    pub fn profile(&self) -> u32 {
        self.owner
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.offset,
            NativeEvent {
                source_class: 0x80802402,
                source_kind: 24,
                class: 0x80804AC4,
            },
        )
    }

    pub fn write(&self, target: &mut Payload, event: usize, symbol: &str) -> Result<ResourcePatch> {
        target.bytes::<0x1A0>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x60>(instance)?;
        ensure!(
            target.u32(
                event
                    .checked_sub(4)
                    .context("Native profile event marker")?
            )? == 0x80804AC4
                && target.u32(event + 0x30)? == 27
                && target.u32(event + 4)? == 0x80804AC3
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native profile runtime marker")?
                )? == 0x80804AC3
                && target.u32(instance + 4)? == 0x80804AC4
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x60 <= event,
            "Native profile event pair or kind differs"
        );
        // Every stock runtime record holds 1.0 at `+4C` and zero elsewhere after its links. The
        // common writer gave the event its timing, parent and selector.
        let mut tail = [0u8; 0x48];
        tail[0x34..0x38].copy_from_slice(&1f32.to_le_bytes());
        ensure!(
            target.bytes::<0x48>(instance + 0x18)? == tail,
            "Native profile event runtime defaults differ"
        );
        let mut output = target.clone();
        put(&mut output.0, event + 0x40, &[0; 8])?;
        put(
            &mut output.0,
            event + 0x48,
            &u64::from(u32::MAX).to_le_bytes(),
        )?;
        let zero = Program::constant(0.0, 1);
        for (at, program) in [
            (0x50, self.programs[0].as_ref()),
            (0x90, None),
            (0xD0, Some(&zero)),
            (0x110, self.programs[1].as_ref()),
            (0x150, Some(&zero)),
        ] {
            Program::write(program, &mut output, event + at, true)?;
        }
        put(&mut output.0, event + 0x190, &[0; 4])?;
        put(&mut output.0, event + 0x194, &0x811C9DC5u32.to_le_bytes())?;
        put(&mut output.0, event + 0x198, &[0; 8])?;
        *target = output;
        Ok(ResourcePatch {
            offset: event + 0x48,
            symbol: symbol.into(),
            source: self.owner,
            source_class: 0x80800861,
            native_class: 0x80809C36,
        })
    }
}
