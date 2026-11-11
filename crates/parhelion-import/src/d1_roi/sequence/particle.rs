//! Source particle events bind two ordered locators to one private native system.
use super::*;
use crate::d1_roi::{Source, dependency};

pub struct Particle {
    source_offset: usize,
    system: u32,
    locators: [u8; 2],
}

impl Particle {
    pub fn system(&self) -> u32 {
        self.system
    }

    pub fn locators(&self) -> [u8; 2] {
        self.locators
    }

    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source particle event marker")?)? == 0x8080037A
                && owner.u32(at + 0x1C)? == 0
                && owner.u32(at + 0x24)? == u32::MAX
                && owner.u16(at + 0x2A)? == 0,
            "Source particle event needs another class, override or extension policy"
        );
        let system = owner.u32(at + 0x20)?;
        ensure!(
            source.reference(system)? == 0x80801C40,
            "Source particle event does not name a particle system"
        );
        let locators = owner.bytes::<2>(at + 0x28)?;
        let count = owner
            .array(owner.pointer(24)? + 0x1B0, 5, Some(0x80800429))?
            .len();
        ensure!(
            locators.iter().all(|&index| usize::from(index) < count),
            "Source particle event locator is undeclared"
        );
        Ok(Self {
            source_offset: at,
            system,
            locators,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x8080037A,
                source_kind: 0,
                class: 0x80806CC6,
            },
        )
    }

    /// The system must include its translated definition, material and geometry closure.
    /// That definition must address the primary and secondary locator at indices zero and
    /// one respectively. The event does not infer those roles from matching locator values.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        symbol: &str,
        locators: &BTreeMap<u8, u8>,
    ) -> Result<ResourcePatch> {
        dependency::symbol(symbol)?;
        target.bytes::<0x128>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x50>(instance)?;
        ensure!(
            target.u32(
                event
                    .checked_sub(4)
                    .context("Native particle event marker")?
            )? == 0x80806CC6
                && target.u32(event + 0x30)? == 4
                && target.u32(event + 4)? == 0x80806CC5
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native particle runtime marker")?
                )? == 0x80806CC5
                && target.u32(instance + 4)? == 0x80806CC6
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x50 <= event,
            "Native particle event pair or kind differs"
        );
        let bindings = self
            .locators
            .iter()
            .map(|index| {
                locators
                    .get(index)
                    .copied()
                    .context("Particle locator has no native mapping")
            })
            .collect::<Result<Vec<_>>>()?;
        let mut output = target.clone();
        let resources =
            native::particle::write_bound(&mut output, event, &[(symbol.to_owned(), bindings)])?;
        // Keep scheduler state and parent links. Only this event's playback state is reset.
        put(&mut output.0, instance + 0x40, &[0xFF; 8])?;
        put(&mut output.0, instance + 0x48, &[0; 8])?;
        let [(offset, _)] = resources.as_slice() else {
            anyhow::bail!("Native particle event did not emit exactly one system");
        };
        let resource = ResourcePatch {
            offset: *offset,
            symbol: symbol.into(),
            source: self.system,
            source_class: 0x80801C40,
            native_class: 0x80806E28,
        };
        *target = output;
        Ok(resource)
    }
}
