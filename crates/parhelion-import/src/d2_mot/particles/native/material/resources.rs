//! Material resource tables retain their mixed sampler and texture indices.
use super::{Context, Payload, texture, tfx};
use anyhow::{Context as _, Result, bail, ensure};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Resources {
    pub tags: Vec<u32>,
    pub patches: Vec<(usize, String)>,
    textures: BTreeMap<u8, Arc<Payload>>,
}

impl Resources {
    pub fn read(c: &mut Context, source: &Payload, field: usize) -> Result<Self> {
        let rows = source.array(field, 16, None)?;
        ensure!(
            rows.len() <= 256,
            "particle resource table exceeds byte indices"
        );
        let mut result = Self {
            tags: Vec::with_capacity(rows.len()),
            patches: Vec::new(),
            textures: BTreeMap::new(),
        };
        for (index, row) in rows.into_iter().enumerate() {
            let tag = c.source.ref64(source, row)?;
            let entry = c
                .source
                .manager
                .get_entry(tiger_pkg::TagHash(tag))
                .with_context(|| format!("missing particle material resource {tag:08X}"))?;
            match entry.file_type {
                34 => result.tags.push(c.native_sampler(tag)?),
                32 => {
                    result
                        .textures
                        .insert(u8::try_from(index)?, c.source.tag(tag, None)?);
                    result.patches.push((index, texture::convert(c, tag)?));
                    result.tags.push(u32::MAX);
                }
                kind => bail!("particle material resource {tag:08X} has unsupported type {kind}"),
            }
        }
        Ok(result)
    }

    pub fn bind(
        &self,
        code: &[u8],
        bindings: &mut tfx::program::Bindings,
        constants: &mut Vec<u8>,
    ) -> Result<()> {
        for instruction in tfx::program::parse(code)? {
            // Check every sampler read. A later write to the same shader slot can replace
            // it in the lowerer's final binding map without removing the earlier operation.
            if instruction.op == 0x5B {
                ensure!(
                    !self.textures.contains_key(&instruction.args[0]),
                    "particle sampler input references a texture"
                );
                continue;
            }
            if !matches!(instruction.op, 0x60..=0x62) {
                continue;
            }
            let index = instruction.args[0];
            let header = self
                .textures
                .get(&index)
                .context("particle texture metadata resource is not a texture")?;
            if instruction.op == 0x60 {
                bindings.textures.insert(index, index);
                continue;
            }
            let key = [instruction.op, index];
            if let std::collections::btree_map::Entry::Vacant(entry) =
                bindings.texture_metadata.entry(key)
            {
                let value = crate::d2_mot::texture::metadata(header, instruction.op)?;
                entry.insert(u8::try_from(constants.len() / 16)?);
                constants.extend(value);
            }
        }
        bindings.constant_count = constants.len() / 16;
        Ok(())
    }

    pub fn validate_samplers(&self, lowered: &tfx::program::Lowered) -> Result<()> {
        ensure!(
            lowered.samplers.values().all(|index| {
                usize::from(*index) < self.tags.len() && !self.textures.contains_key(index)
            }),
            "particle sampler binding references a texture"
        );
        Ok(())
    }
}
