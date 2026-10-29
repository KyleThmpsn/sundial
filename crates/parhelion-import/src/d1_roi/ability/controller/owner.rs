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
    callbacks: Vec<usize>,
    pub initializers: Vec<lifecycle::Method>,
}

impl Owner {
    /// Continue construction on a native envelope whose callback and lifecycle tables have
    /// been cleared. The caller must supply every retained private identity before sealing.
    pub fn adopt(payload: Payload, symbol: &str) -> Result<Self> {
        crate::d1_roi::ability::dependency::symbol(symbol)?;
        let runtime = payload.pointer(16)?;
        let definition = payload.pointer(24)?;
        let id = payload.u32(runtime)?;
        ensure!(
            payload.u64(0)? == payload.0.len() as u64
                && runtime >= 4
                && runtime < definition
                && !matches!(id, 0 | u32::MAX)
                && payload.u32(definition)? == id
                && payload.u64(runtime + 8)? == definition as u64
                && payload.u64(definition + 8)? == runtime as u64
                && payload.u32(runtime - 4)? == payload.u32(definition + 4)?
                && payload.u32(definition - 4)? == payload.u32(runtime + 4)?
                && payload.bytes::<32>(0x20)? == [0; 32],
            "Retained owner cannot adopt an inconsistent or initialized envelope"
        );
        Ok(Self {
            payload,
            patches: vec![(runtime, symbol.into()), (definition, symbol.into())],
            runtime,
            definition,
            id,
            symbol: symbol.into(),
            callbacks: Vec::new(),
            initializers: Vec::new(),
        })
    }

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

    /// The callback may address the root or a constructed embedded runtime object. The
    /// reference table points inside this record, after its implementation and method words.
    pub fn callback(
        &mut self,
        offset: usize,
        implementation: u32,
        index: u32,
        target: usize,
    ) -> Result<()> {
        self.payload.bytes::<24>(offset)?;
        let end = offset.checked_add(24).context("Callback extent overflow")?;
        ensure!(
            offset >= self.runtime
                && self
                    .callbacks
                    .iter()
                    .all(|&reference| { end <= reference - 8 || offset >= reference + 16 }),
            "Retained callback slots overlap or precede the runtime"
        );
        let class = self.callback_target(target, self.payload.0.len())?;
        ensure!(
            index != u32::MAX
                && sundial::package_authoring::runtime::native_type_inherits(class, implementation),
            "Retained callback implementation does not belong to its target"
        );
        put(&mut self.payload.0, offset, &implementation.to_le_bytes())?;
        put(&mut self.payload.0, offset + 4, &index.to_le_bytes())?;
        put(&mut self.payload.0, offset + 8, &self.id.to_le_bytes())?;
        put(&mut self.payload.0, offset + 12, &class.to_le_bytes())?;
        put(
            &mut self.payload.0,
            offset + 16,
            &u64::try_from(target)?.to_le_bytes(),
        )?;
        self.patches.push((offset + 8, self.symbol.clone()));
        self.callbacks.push(offset + 8);
        Ok(())
    }

    fn callback_target(&self, target: usize, mutable_end: usize) -> Result<u32> {
        ensure!(
            target >= self.runtime
                && target.checked_add(16).is_some_and(|end| end <= mutable_end)
                && self
                    .patches
                    .iter()
                    .any(|(at, symbol)| *at == target && symbol == &self.symbol),
            "Callback target is not a private object in the mutable owner"
        );
        let definition = usize::try_from(self.payload.u64(target + 8)?)?;
        self.payload.bytes::<16>(definition)?;
        let class = self.payload.u32(definition + 4)?;
        ensure!(
            self.payload.u32(target)? == self.id
                && self.payload.u32(definition)? == self.id
                && self.payload.u64(definition + 8)? == target as u64
                && !matches!(class, 0 | u32::MAX),
            "Callback target does not have reciprocal owner headers"
        );
        Ok(class)
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
        for &reference in &self.callbacks {
            let target = usize::try_from(self.payload.u64(reference + 8)?)?;
            let class = self.callback_target(target, mutable_end)?;
            ensure!(
                reference + 16 <= mutable_end
                    && self.payload.u32(reference)? == self.id
                    && class == self.payload.u32(reference + 4)?
                    && self.payload.u32(reference - 4)? != u32::MAX
                    && sundial::package_authoring::runtime::native_type_inherits(
                        class,
                        self.payload.u32(reference - 8)?,
                    ),
                "Retained callback is outside mutable storage or its target changed"
            );
        }
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
        // Entries identify the object reference at callback +8. The loader resolves its
        // owner/class/offset tuple, with implementation and method immediately before it.
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
