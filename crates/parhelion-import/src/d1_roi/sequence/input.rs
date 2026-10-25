//! Connected sequence-handle inputs have distinct source headers and native cache defaults.
use super::*;

#[derive(Debug, Serialize)]
pub struct Input {
    runtime: usize,
    definition: usize,
    owner: u32,
}

pub struct Written {
    pub runtime: usize,
    pub definition: usize,
    /// Both paired headers must follow the private owner during graph allocation.
    pub patches: Vec<(usize, String)>,
}

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
        ensure!(
            !symbol.is_empty()
                && symbol
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
            "Native sequence input owner symbol differs"
        );
        let owner = target.u32(parent)?;
        ensure!(owner != u32::MAX, "Native sequence input has no owner");
        target.bytes::<80>(runtime)?;
        target.bytes::<48>(definition)?;
        ensure!(
            parent < runtime && runtime + 80 <= definition,
            "Native sequence input storage overlaps its definition or parent"
        );
        // Original 92F3 defaults and serialized common-controller witnesses agree on the
        // inactive cache. Source +3C and +48 differ, so copying its live tail is invalid.
        let mut live = [0u8; 80];
        for field in [0x20, 0x24, 0x30, 0x38, 0x3C] {
            put(&mut live, field, &u32::MAX.to_le_bytes())?;
        }
        let mut settings = [0u8; 48];
        put(&mut settings, 24, &0x808092F7u64.to_le_bytes())?;
        put(&mut settings, 40, &u64::from(u32::MAX).to_le_bytes())?;
        let mut output = target.clone();
        put(&mut output.0, runtime, &live)?;
        put(&mut output.0, definition, &settings)?;
        for (at, twin, class) in [
            (runtime, definition, 0x808092F4u32),
            (definition, runtime, 0x808092F3u32),
        ] {
            put(&mut output.0, at, &owner.to_le_bytes())?;
            put(&mut output.0, at + 4, &class.to_le_bytes())?;
            put(&mut output.0, at + 8, &(twin as u64).to_le_bytes())?;
        }
        native::relative(&mut output, runtime + 16, parent)?;
        *target = output;
        Ok(Written {
            runtime,
            definition,
            patches: vec![(runtime, symbol.into()), (definition, symbol.into())],
        })
    }
}
