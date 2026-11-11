//! Rise of Iron selector 4 sends a named event to the sequence host's entity, as native kind 9
//! does.
//!
//! Source start callback `00650390` reads only the name at `+20`. It does nothing for the empty
//! name, and otherwise calls method `3274A36B` of the host entity's interface `F5BDFEAB` with the
//! name, the integer -1 and no parameters. The one implementation, `004BFB40`, looks the name up
//! in its component's response table and falls back to the parent component. Native kind 9,
//! `8080658E`, keeps that interface and component and adds a four-argument method, which start
//! `00ED9730` calls through `00B32250` with the name at `+44`. Byte `+38` `FF` targets the host
//! entity, the empty `+40` keeps it rather than a named child, byte `+48` `FF` passes -1, `+4C`
//! `FFFFFFFF` takes the interface rather than an explicit response resource, and the parameter
//! array at `+50` stays empty. Stock kind 9 events that name `CFC7F883` store exactly these
//! values. Native passes an empty parameter block where the source passes none, taken as
//! equivalent. The name is an event key, not a resource, so it carries unchanged. What answers it
//! on a native host was not found.
use super::*;

pub struct Signal {
    source_offset: usize,
    name: u32,
}

impl Signal {
    pub fn read(source: &Payload, event: usize) -> Result<Self> {
        ensure!(
            source.u32(event.checked_sub(4).context("Source signal marker")?)? == 0x8080050E
                && source.u32(event + 0x1C)? == 4,
            "Source signal event class or kind differs"
        );
        source.bytes::<0x24>(event)?;
        Ok(Self {
            source_offset: event,
            name: source.u32(event + 0x20)?,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x8080050E,
                source_kind: 4,
                class: 0x8080658E,
            },
        )
    }

    pub fn write(&self, target: &mut Payload, event: usize) -> Result<()> {
        ensure!(
            target.u32(event.checked_sub(4).context("Native signal marker")?)? == 0x8080658E
                && target.u32(event + 0x30)? == 9,
            "Native signal event class or kind differs"
        );
        let instance = usize::try_from(target.u64(event + 8)?)?;
        ensure!(
            target.u32(
                instance
                    .checked_sub(4)
                    .context("Native signal instance marker")?
            )? == 0x8080658D
                && target.u32(instance + 4)? == 0x8080658E
                && target.u64(instance + 8)? == event as u64
                && target.bytes::<4>(event + 0x38)? == [0xFF, 0, 0, 0]
                && target.bytes::<0x24>(event + 0x3C)? == [0; 0x24],
            "Native signal event pair or body differs"
        );
        let mut output = target.clone();
        put(&mut output.0, event + 0x40, &0x811C_9DC5u32.to_le_bytes())?;
        put(&mut output.0, event + 0x44, &self.name.to_le_bytes())?;
        put(&mut output.0, event + 0x48, &[0xFF])?;
        put(&mut output.0, event + 0x4C, &u32::MAX.to_le_bytes())?;
        *target = output;
        Ok(())
    }
}
