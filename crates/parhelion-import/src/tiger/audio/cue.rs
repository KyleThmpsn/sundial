//! Streamed effect cues, including their secondary event in the same namespace.
use super::{bank::Namespace, unset};
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub struct Cue {
    pub events: [u32; 2],
    pub settings: u32,
    pub bank: u32,
    pub auxiliary: u32,
    pub media: Vec<u32>,
}

/// Typed settings references must come from completed dependency converters.
pub struct Settings {
    pub tag: u32,
    pub class: u32,
}

pub struct Bindings {
    pub banks: BTreeMap<u32, u32>,
    pub settings: BTreeMap<u32, Settings>,
    pub media: BTreeMap<u32, u32>,
}

fn tag(value: u32) -> Result<u32> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&value),
        "invalid sound dependency tag"
    );
    Ok(value)
}

impl Cue {
    /// Read the inspected class-80809738 streamed layout. Embedded media cues
    /// and source-only flags require their own conversion.
    pub fn read(source: &Payload) -> Result<Self> {
        ensure!(
            source.u64(0)? == source.0.len() as u64,
            "sound cue size differs"
        );
        ensure!(
            source.u32(0x10)? == 0,
            "sound cue source flags require translation"
        );
        let count = usize::try_from(source.u64(0x20)?)?;
        ensure!(
            count > 0 && count <= 1024,
            "unsupported sound cue media count"
        );
        ensure!(
            source.0.len() == 0x50 + count * 4
                && source.pointer(0x28)? == 0x40
                && source.bytes::<12>(0x30)? == [0; 12]
                && source.u32(0x3C)? == 0x80809FB8
                && source.u32(0x4C)? == 0,
            "sound cue media envelope differs"
        );
        let media = source
            .array(0x20, 4, Some(0x80800014))?
            .into_iter()
            .map(|at| tag(source.u32(at)?))
            .collect::<Result<Vec<_>>>()?;
        let events = [source.u32(8)?, source.u32(12)?];
        ensure!(!unset(events[0]), "sound cue has no primary event");
        Ok(Self {
            events,
            settings: tag(source.u32(0x14)?)?,
            bank: tag(source.u32(0x18)?)?,
            auxiliary: tag(source.u32(0x1C)?)?,
            media,
        })
    }

    /// Serialize class 80809802 without inheriting another weapon's settings.
    /// The caller owns package allocation and must supply all dependencies.
    pub fn emit(&self, namespace: &Namespace, bindings: &Bindings) -> Result<Vec<u8>> {
        ensure!(
            !self.media.is_empty() && self.media.len() <= 1024 && !unset(self.events[0]),
            "invalid streamed sound cue"
        );
        let setting = |source: u32, class: u32| -> Result<u32> {
            let mapped = bindings
                .settings
                .get(&source)
                .context("sound settings not translated")?;
            ensure!(
                mapped.class == class,
                "translated sound settings class differs"
            );
            tag(mapped.tag)
        };
        let bank = tag(*bindings
            .banks
            .get(&self.bank)
            .context("sound bank not linked")?)?;
        let settings = setting(self.settings, 0x80808E68)?;
        let auxiliary = setting(self.auxiliary, 0x80808E69)?;
        let mut events = self.events;
        for event in &mut events {
            if !unset(*event) {
                *event = namespace.object(*event)?;
            }
        }
        let mut out = vec![0u8; 0x50 + self.media.len() * 4];
        let size = out.len() as u64;
        out[..8].copy_from_slice(&size.to_le_bytes());
        for (at, value) in [
            (8, events[0]),
            (12, events[1]),
            (16, settings),
            (20, bank),
            (40, auxiliary),
            (60, 0x80809FBD),
            (72, 0x80800014),
        ] {
            out[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for at in [0x18, 0x40] {
            out[at..at + 8].copy_from_slice(&(self.media.len() as u64).to_le_bytes());
        }
        out[0x20..0x28].copy_from_slice(&0x20i64.to_le_bytes());
        for (index, source) in self.media.iter().enumerate() {
            let value = tag(*bindings
                .media
                .get(source)
                .context("sound medium not linked")?)?;
            out[0x50 + index * 4..0x54 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(out)
    }
}

/// The empty class-8080BCA8 record has the same two empty array descriptors as
/// native class 80808E69. Nonempty source settings are not copied blindly.
pub fn empty_auxiliary(source: &Payload) -> Result<Vec<u8>> {
    ensure!(
        source.0.len() == 40 && source.u64(0)? == 40 && source.0[8..].iter().all(|v| *v == 0),
        "nonempty auxiliary sound settings require translation"
    );
    Ok(source.0.clone())
}
