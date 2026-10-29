//! Named HUD Boolean overrides retain resource lifetime and signed merge priority.
use super::*;
use crate::{
    d1_roi::{Source, ability::dependency},
    presentation::append,
};
mod keys;
pub use keys::Keys;

pub struct Hud {
    source_offset: usize,
    resource: u32,
    /// Original name identity and Boolean value. Numeric keys are build-local.
    rows: Vec<(u32, u8)>,
    priority: [u8; 4],
}

impl Hud {
    pub fn resource(&self) -> u32 {
        self.resource
    }

    pub fn read(source: &Source, owner: &Payload, at: usize) -> Result<Self> {
        ensure!(
            owner.u32(at.checked_sub(4).context("Source HUD event marker")?)? == 0x80802573
                && owner.u32(at + 0x1C)? == 8
                && owner.u32(at + 0x24)? == 0,
            "Source HUD event class, kind or padding differs"
        );
        let resource = owner.u32(at + 0x20)?;
        ensure!(
            source.reference(resource)? == 0x80802784,
            "Source HUD resource class differs"
        );
        let data = source.read(resource)?;
        ensure!(
            data.u64(0)? == data.0.len() as u64,
            "Source HUD resource extent differs"
        );
        ensure!(
            data.u32(0x18)? == u32::MAX,
            "Source HUD optional resource override requires separate translation"
        );
        let names = keys::source(source)?;
        let mut previous = None;
        let mut rows = Vec::new();
        for row in data.array(8, 4, Some(0x808022C4))? {
            let key = data.u16(row)?;
            let value = data.u8(row + 2)?;
            ensure!(
                previous.is_none_or(|last| key > last) && value <= 1 && data.u8(row + 3)? == 0,
                "Source HUD rows are unsorted, duplicated or have unsupported values"
            );
            let name = *names
                .get(&key)
                .with_context(|| format!("Source HUD key {key} is not a declared named flag"))?;
            rows.push((name, value));
            previous = Some(key);
        }
        Ok(Self {
            source_offset: at,
            resource,
            rows,
            priority: data.bytes(0x1C)?,
        })
    }

    pub fn mapping(&self) -> (usize, NativeEvent) {
        (
            self.source_offset,
            NativeEvent {
                source_class: 0x80802573,
                source_kind: 8,
                class: 0x80803F24,
            },
        )
    }

    /// Build the private resource from checked source rows. A caller-supplied native table
    /// cannot stand in for this conversion. Optional resource overrides remain unsupported.
    pub fn translate(&self, symbol: &str, keys: &Keys) -> Result<dependency::Asset> {
        dependency::symbol(symbol)?;
        let mut rows = BTreeMap::new();
        for &(name, value) in &self.rows {
            ensure!(
                rows.insert(keys.key(name)?, value).is_none(),
                "Translated HUD keys collide"
            );
        }
        let mut bytes = Vec::new();
        for (key, value) in rows {
            bytes.extend_from_slice(&key.to_le_bytes());
            bytes.extend_from_slice(&[value, 0]);
        }
        let mut payload = Payload(vec![0; 0x20]);
        put(&mut payload.0, 0x18, &u32::MAX.to_le_bytes())?;
        put(&mut payload.0, 0x1C, &self.priority)?;
        append(&mut payload.0, 8, 0x8080619B, &bytes, 4)?;
        let size = u64::try_from(payload.0.len())?;
        put(&mut payload.0, 0, &size.to_le_bytes())?;
        Ok(dependency::Asset {
            symbol: symbol.into(),
            class: 0x80803F26,
            payload,
            patches: Vec::new(),
        })
    }

    pub fn write(&self, target: &mut Payload, event: usize, symbol: &str) -> Result<ResourcePatch> {
        dependency::symbol(symbol)?;
        target.bytes::<0x40>(event)?;
        let instance = usize::try_from(target.u64(event + 8)?)?;
        target.bytes::<0x40>(instance)?;
        ensure!(
            target.u32(event.checked_sub(4).context("Native HUD event marker")?)? == 0x80803F24
                && target.u32(event + 0x30)? == 13
                && target.u32(event + 4)? == 0x80803F23
                && target.u32(
                    instance
                        .checked_sub(4)
                        .context("Native HUD runtime marker")?
                )? == 0x80803F23
                && target.u32(instance + 4)? == 0x80803F24
                && target.u32(instance)? == target.u32(event)?
                && target.u64(instance + 8)? == event as u64
                && instance + 0x40 <= event,
            "Native HUD event pair or kind differs"
        );
        let mut output = target.clone();
        // The common scheduler owns timing and parent state. This event adds no runtime tail.
        put(&mut output.0, instance + 0x18, &[0; 0x28])?;
        put(&mut output.0, event + 0x38, &u32::MAX.to_le_bytes())?;
        put(&mut output.0, event + 0x3C, &[0; 4])?;
        *target = output;
        Ok(ResourcePatch {
            offset: event + 0x38,
            symbol: symbol.into(),
            source: self.resource,
            source_class: 0x80802784,
            native_class: 0x80803F26,
        })
    }
}
