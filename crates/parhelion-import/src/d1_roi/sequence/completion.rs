//! Completion events receive their selected ability slot from the controller at startup.
use super::*;

#[derive(Debug, Serialize)]
pub struct Completion {
    source_offset: usize,
}

impl Completion {
    pub fn read(source: &Payload, event: usize) -> Result<Self> {
        ensure!(
            source.u32(event.checked_sub(4).context("Source completion marker")?)? == 0x808027EA
                && source.u32(event + 0x1C)? == 14,
            "Source completion event class or kind differs"
        );
        source.bytes::<0x20>(event)?;
        Ok(Self {
            source_offset: event,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x808027EA,
                source_kind: 14,
                class: 0x80804595,
            },
        )
    }

    /// The containing graph must retain the controller's slot-assignment dispatch and connected
    /// sequence inputs. Native FAEB50 initializes kind 19 through those inputs, just as source
    /// 486B70 initializes kind 14. Serializing a selected slot here would bypass that lifecycle.
    pub fn write(&self, target: &mut Payload, event: usize) -> Result<()> {
        ensure!(
            target.u32(event.checked_sub(4).context("Native completion marker")?)? == 0x80804595
                && target.u32(event + 0x30)? == 19,
            "Native completion event class or kind differs"
        );
        target.bytes::<0x38>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x50>(instance)?;
        ensure!(
            target.u32(
                instance
                    .checked_sub(4)
                    .context("Native completion instance marker")?
            )? == 0x80804594
                && target.u32(instance + 4)? == 0x80804595
                && target.u32(event + 4)? == 0x80804594
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x50 <= event,
            "Native completion event pair differs"
        );
        let mut output = target.clone();
        // Preserve reciprocal ownership and the parent pointer. Reset native timing state.
        put(&mut output.0, instance + 0x18, &[0; 0x38])?;
        put(&mut output.0, instance + 0x40, &[u8::MAX])?;
        *target = output;
        Ok(())
    }
}
