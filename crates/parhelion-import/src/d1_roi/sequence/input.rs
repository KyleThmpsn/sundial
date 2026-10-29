//! Connected sequence-handle inputs have distinct source headers and native cache defaults.
use super::*;

#[derive(Debug, Serialize)]
pub struct Input {
    runtime: usize,
    definition: usize,
    owner: u32,
}

pub use crate::tiger::sequence::input::Written;

impl Input {
    pub fn definition(&self) -> usize {
        self.definition
    }

    pub fn owner(&self) -> u32 {
        self.owner
    }

    /// Only a directly connected, inactive 08AF/0456 helper is admitted. Dynamic binding
    /// policies and initialized source caches require their own lifetime translation.
    pub fn read(source: &Payload, runtime: usize, parent: usize) -> Result<Self> {
        let definition = usize::try_from(source.u64(runtime + 8)?)?;
        let owner = source.u32(parent)?;
        ensure!(
            owner != u32::MAX
                && source.u64(0)? == source.0.len() as u64
                && source.u64(runtime)? == u64::from(owner)
                && source.u64(runtime + 16)? == 0x80800456
                && source.pointer(runtime + 24)? == parent
                && source.u64(definition)? == u64::from(owner)
                && source.u64(definition + 8)? == runtime as u64
                && source.u64(definition + 16)? == 0x808008AF,
            "Source sequence input pair or parent differs"
        );
        ensure!(
            source.u64(definition + 24)? == 0
                && source.u64(definition + 32)? == 0x808003E3
                && source.u64(definition + 40)? == 0
                && source.u64(definition + 48)? == u64::from(u32::MAX),
            "Source sequence input needs another binding policy"
        );
        for (field, expected) in [
            (0x20, u64::from(u32::MAX)),
            (0x28, 0),
            (0x30, u64::from(u32::MAX)),
            (0x38, u64::from(u32::MAX)),
            (0x40, 0),
            (0x48, u64::from(u32::MAX)),
        ] {
            ensure!(
                source.u64(runtime + field)? == expected,
                "Source sequence input has initialized live state"
            );
        }
        Ok(Self {
            runtime,
            definition,
            owner,
        })
    }

    /// Write a fresh pair into existing controller-owned storage. This supplies the consumer
    /// only. The containing graph must connect its definition to a translated native provider.
    pub fn write(
        &self,
        target: &mut Payload,
        runtime: usize,
        definition: usize,
        parent: usize,
        symbol: &str,
    ) -> Result<Written> {
        crate::tiger::sequence::input::write(target, runtime, definition, parent, symbol)
    }
}
