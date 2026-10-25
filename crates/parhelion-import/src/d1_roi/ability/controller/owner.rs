//! Typed component ownership for native implementations without a stock owner.
use crate::{
    presentation::{append, put},
    tiger::{entity::lifecycle, instance, payload::Payload},
};
use anyhow::{Context, Result, ensure};
use serde_json::json;

pub(super) struct Owner {
    pub payload: Payload,
    pub patches: Vec<(usize, String)>,
    pub runtime: usize,
    pub definition: usize,
    id: u32,
    symbol: String,
    runtime_class: u32,
    callbacks: Vec<usize>,
    pub initializers: Vec<lifecycle::Method>,
}

impl Owner {
    pub fn new(
        template: &Payload,
        id: u32,
        symbol: &str,
        classes: (u32, u32),
        runtime: Vec<u8>,
        definition: Vec<u8>,
    ) -> Result<Self> {
        ensure!(
            !symbol.is_empty()
                && symbol
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
                && id != u32::MAX,
            "Invalid retained ability component identity"
        );
        ensure!(
            template.u64(0)? == template.0.len() as u64
                && template.pointer(8)? == 0x68
                && template.u32(0x64)? == 0x80809C28
                && template.pointer(16)? == 0x80
                && template.u32(0x40)? == u32::MAX
                && runtime.len() >= 0x30
                && definition.len() >= 0x48,
            "Native component envelope differs"
        );
        let instance = 0x80usize;
        let settings = instance
            .checked_add(runtime.len())
            .and_then(|n| n.checked_add(11))
            .context("Ability instance size overflow")?
            & !7;
        let end = settings
            .checked_add(definition.len())
            .context("Ability definition size overflow")?;
        let mut result = Self {
            payload: Payload(vec![0; end]),
            patches: vec![(0x44, format!("{symbol}-allocation"))],
            runtime: instance,
            definition: settings,
            id,
            symbol: symbol.to_owned(),
            runtime_class: classes.0,
            callbacks: Vec::new(),
            initializers: Vec::new(),
        };
        result.payload.0[..instance].copy_from_slice(&template.0[..instance]);
        result.payload.0[instance..instance + runtime.len()].copy_from_slice(&runtime);
        result.payload.0[settings..end].copy_from_slice(&definition);
        for (field, at) in [(16, instance), (24, settings)] {
            put(
                &mut result.payload.0,
                field,
                &(at as i64 - field as i64).to_le_bytes(),
            )?;
        }
        put(&mut result.payload.0, 32, &[0; 32])?;
        put(&mut result.payload.0, 0x44, &u32::MAX.to_le_bytes())?;
        put(
            &mut result.payload.0,
            0x48,
            &((settings - instance) as u64).to_le_bytes(),
        )?;
        put(
            &mut result.payload.0,
            instance - 4,
            &classes.0.to_le_bytes(),
        )?;
        put(
            &mut result.payload.0,
            settings - 4,
            &classes.1.to_le_bytes(),
        )?;
        result.pair(instance, settings, classes, None)?;
        result.size()?;
        Ok(result)
    }

    pub fn pair(
        &mut self,
        runtime: usize,
        definition: usize,
        classes: (u32, u32),
        parent: Option<usize>,
    ) -> Result<()> {
        for (at, twin, class) in [
            (runtime, definition, classes.1),
            (definition, runtime, classes.0),
        ] {
            put(&mut self.payload.0, at, &self.id.to_le_bytes())?;
            put(&mut self.payload.0, at + 4, &class.to_le_bytes())?;
            put(&mut self.payload.0, at + 8, &(twin as u64).to_le_bytes())?;
            self.patches.push((at, self.symbol.clone()));
        }
        if let Some(parent) = parent {
            put(
                &mut self.payload.0,
                runtime + 16,
                &(parent as i64 - (runtime + 16) as i64).to_le_bytes(),
            )?;
        }
        Ok(())
    }

    pub fn callback(&mut self, offset: usize, implementation: u32, index: u32) -> Result<()> {
        ensure!(
            !self.callbacks.contains(&offset),
            "Duplicate retained callback slot"
        );
        put(&mut self.payload.0, offset, &implementation.to_le_bytes())?;
        put(&mut self.payload.0, offset + 4, &index.to_le_bytes())?;
        put(&mut self.payload.0, offset + 8, &self.id.to_le_bytes())?;
        put(
            &mut self.payload.0,
            offset + 12,
            &self.runtime_class.to_le_bytes(),
        )?;
        put(
            &mut self.payload.0,
            offset + 16,
            &(self.runtime as u64).to_le_bytes(),
        )?;
        self.patches.push((offset + 8, self.symbol.clone()));
        self.callbacks.push(offset);
        Ok(())
    }

    pub fn size(&mut self) -> Result<()> {
        let size = self.payload.0.len() as u64;
        put(&mut self.payload.0, 0, &size.to_le_bytes())
    }

    pub fn finish(&mut self, mutable_end: usize) -> Result<()> {
        ensure!(
            mutable_end <= self.payload.0.len(),
            "Retained mutable span exceeds its owner"
        );
        if self.payload.bytes::<16>(0x30)? == [0; 16] {
            lifecycle::write(&mut self.payload, &self.initializers, mutable_end)?;
        } else {
            // Controller lowering can install its expanded helper plan before final sealing.
            // Preserve that checked table, while rejecting a second conflicting call list.
            let existing = lifecycle::read(&self.payload)?;
            ensure!(
                self.initializers.is_empty() || self.initializers == existing,
                "Retained owner has conflicting initializer plans"
            );
        }
        // Entries identify callback records, not their component roots. The native loader
        // resolves each callback's original implementation and method index independently.
        append(
            &mut self.payload.0,
            32,
            0x8080919F,
            &vec![0; self.callbacks.len() * 8],
            8,
        )?;
        for (&at, &target) in self
            .payload
            .array(32, 8, Some(0x8080919F))?
            .iter()
            .zip(&self.callbacks)
        {
            put(
                &mut self.payload.0,
                at,
                &(target as i64 - at as i64).to_le_bytes(),
            )?;
        }
        let mut patches = self
            .patches
            .iter()
            .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
            .collect::<Vec<_>>();
        instance::seal_span(&mut self.payload, &mut patches, mutable_end)?;
        self.patches = patches
            .into_iter()
            .map(|patch| {
                Ok((
                    usize::try_from(
                        patch["offset"]
                            .as_u64()
                            .context("Retained owner patch offset")?,
                    )?,
                    patch["symbol"]
                        .as_str()
                        .context("Retained owner patch symbol")?
                        .to_owned(),
                ))
            })
            .collect::<Result<_>>()?;
        self.size()
    }
}
