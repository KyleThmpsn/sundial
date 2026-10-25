//! Animation events retain source flags and require an explicitly translated action binding.
use super::*;

#[derive(Debug, Serialize)]
pub struct Animation {
    source_offset: usize,
    action: u32,
    flags: [u8; 3],
}

/// The graph producer must resolve this name in the selected translated animation state table.
/// Writing an event does not prove that the action's source clips have been imported and bound.
#[derive(Debug, Serialize)]
pub struct Action {
    pub source: u32,
    pub native: u32,
    pub owner_mode: bool,
    pub lane: bool,
}

impl Animation {
    pub fn action(&self) -> u32 {
        self.action
    }

    pub fn owner_mode(&self) -> bool {
        self.flags[0] != 0
    }

    pub fn lane(&self) -> bool {
        self.flags[1] != 0
    }

    pub fn read(source: &Payload, event: usize) -> Result<Self> {
        ensure!(
            source.u32(
                event
                    .checked_sub(4)
                    .context("Source animation event marker")?
            )? == 0x808025C1
                && source.u32(event + 0x1C)? == 13
                && source.u8(event + 0x27)? == 0
                && source.u32(event + 0x28)? == 0,
            "Source animation event class, kind or extension differs"
        );
        let action = source.u32(event + 0x20)?;
        let flags = source.bytes::<3>(event + 0x24)?;
        ensure!(
            !matches!(action, 0 | 0x811C9DC5 | u32::MAX) && flags.iter().all(|&flag| flag <= 1),
            "Source animation event has no action or has unknown flag values"
        );
        Ok(Self {
            source_offset: event,
            action,
            flags,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x808025C1,
                source_kind: 13,
                class: 0x80803FE9,
            },
        )
    }

    /// Preserve original clip timing. Native +40 is an added duration expression whose empty
    /// form evaluates to zero and bypasses the positive-duration override in all three paths.
    pub fn write(
        &self,
        target: &mut Payload,
        event: usize,
        actions: &BTreeMap<u32, u32>,
    ) -> Result<Action> {
        let action = *actions
            .get(&self.action)
            .context("Source animation action has no native mapping")?;
        ensure!(
            !matches!(action, 0 | 0x811C9DC5 | u32::MAX),
            "Native animation action is absent"
        );
        ensure!(
            target.u32(
                event
                    .checked_sub(4)
                    .context("Native animation event marker")?
            )? == 0x80803FE9
                && target.u32(event + 0x30)? == 18,
            "Native animation event class or kind differs"
        );
        target.bytes::<0x88>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x60>(instance)?;
        ensure!(
            target.u32(
                instance
                    .checked_sub(4)
                    .context("Native animation instance marker")?
            )? == 0x80803FE8
                && target.u32(instance + 4)? == 0x80803FE9
                && target.u32(event + 4)? == 0x80803FE8
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x60 <= event,
            "Native animation event pair differs"
        );
        let mut output = target.clone();
        put(&mut output.0, event + 0x38, &action.to_le_bytes())?;
        // Native +3E is an additional animation flag, not the source +26 owner-state policy.
        put(
            &mut output.0,
            event + 0x3C,
            &[self.flags[0], self.flags[1], 0, self.flags[2]],
        )?;
        put(&mut output.0, event + 0x40, &[0; 72])?;
        // Native package witnesses start with zero timing and owner state, plus an absent
        // cached animation provider. Source integer ticks are not native 64-bit time values.
        put(&mut output.0, instance + 0x18, &[0; 72])?;
        put(&mut output.0, instance + 0x40, &u64::MAX.to_le_bytes())?;
        *target = output;
        Ok(Action {
            source: self.action,
            native: action,
            owner_mode: self.flags[0] != 0,
            lane: self.flags[1] != 0,
        })
    }
}
