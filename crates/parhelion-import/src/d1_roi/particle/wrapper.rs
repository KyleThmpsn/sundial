//! Source particle wrappers and their native 52-byte system envelope.
use crate::{d1_roi::Source, presentation::put, tiger::payload::Payload};
use anyhow::{Result, ensure};

pub struct WrapperControls {
    words: [[u8; 2]; 4],
}

/// Everything a native system takes from one source wrapper besides its three resource edges.
pub struct Wrapper {
    controls: WrapperControls,
    /// Source `+18`, a hash-like word shared with native `+20` by the same effects across builds.
    identity: [u8; 4],
    /// Source `+1C` and `+20`, stored beside the controls and carried to native `+24` and `+28`.
    stored: [[u8; 2]; 2],
}

impl Source {
    pub fn particle_wrapper_controls(&self, tag: u32) -> Result<WrapperControls> {
        ensure!(
            self.reference(tag)? == 0x80801C40,
            "Particle wrapper controls have another resource class"
        );
        let source = self.read(tag)?;
        ensure!(
            source.0.len() == 0x24 && source.u32(4)? == 0 && source.u32(0x0C)? == u32::MAX,
            "Particle wrapper controls require another envelope or compute route"
        );
        let controls = WrapperControls {
            words: [
                source.bytes(8)?,
                source.bytes(10)?,
                source.bytes(0x1E)?,
                source.bytes(0x22)?,
            ],
        };
        self.unchanged()?;
        Ok(controls)
    }

    pub fn particle_wrapper(&self, tag: u32) -> Result<Wrapper> {
        let controls = self.particle_wrapper_controls(tag)?;
        let source = self.read(tag)?;
        let wrapper = Wrapper {
            controls,
            identity: source.bytes(0x18)?,
            stored: [source.bytes(0x1C)?, source.bytes(0x20)?],
        };
        self.unchanged()?;
        Ok(wrapper)
    }
}

impl WrapperControls {
    /// Both update producers interpret these words as signed values. The first, second and
    /// fourth use a 1/16 scale, and the third uses 1/256. Keep their stored bits. The remaining
    /// wrapper metadata and native-only budget controls require separate preparation.
    pub fn write(&self, target: &mut Payload) -> Result<()> {
        ensure!(
            target.0.len() == 0x34
                && [4, 8, 0x10]
                    .into_iter()
                    .all(|at| target.u32(at).ok() == Some(u32::MAX)),
            "Native particle wrapper requires another envelope or compute route"
        );
        let mut output = target.clone();
        for (offset, word) in [0x0C, 0x0E, 0x26, 0x2A].into_iter().zip(&self.words) {
            put(&mut output.0, offset, word)?;
        }
        *target = output;
        Ok(())
    }
}

impl Wrapper {
    /// Offsets of the definition, render material and geometry container handles. Each holds
    /// the unresolved sentinel for the containing graph's private allocation.
    pub const EDGES: [usize; 3] = [0, 0x14, 0x18];

    /// A CPU system has no compute materials and no compiled GPU identity. The native frame
    /// updater reads the four controls and the cost words at `+2C/+2E`. A zero `+2C` gives
    /// zero cost attenuation and no live-count term, matching the source, which has neither.
    /// No consumer of `+20/+24/+28` was found in the qualified native readers, so the stored
    /// source words move with the footer instead of being invented or cleared.
    pub fn payload(&self) -> Result<Payload> {
        let mut output = Payload(vec![0; 0x34]);
        for at in [0, 4, 8, 0x10, 0x14, 0x18, 0x1C, 0x30] {
            put(&mut output.0, at, &u32::MAX.to_le_bytes())?;
        }
        put(&mut output.0, 0x20, &self.identity)?;
        put(&mut output.0, 0x24, &self.stored[0])?;
        put(&mut output.0, 0x28, &self.stored[1])?;
        put(&mut output.0, 0x2C, &0u16.to_le_bytes())?;
        put(&mut output.0, 0x2E, &1u16.to_le_bytes())?;
        self.controls.write(&mut output)?;
        Ok(output)
    }
}
