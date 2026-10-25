//! Serialized Shadowkeep replication state for a newly assembled entity.
//!
//! The native 9FFE10 and 9FE9D0 load callbacks derive the field lists, component
//! mask and state sizes from each instance schema and its allocation tree.
use super::{Component, array, put};
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};

pub fn emit(entity: u32, components: &[Component<'_>]) -> Result<Payload> {
    ensure!(
        !components.is_empty() && components.len() <= 64,
        "replicated entity exceeds the native component mask"
    );
    let mut rows = Vec::with_capacity(components.len());
    for component in components {
        let instance = component.payload.pointer(16)?;
        let class = component.payload.u32(
            instance
                .checked_sub(4)
                .context("replication instance has no class marker")?,
        )?;
        let template_instance = component.template.pointer(16)?;
        ensure!(
            class == component.template.u32(template_instance - 4)?,
            "replication instance schema differs from its native implementation"
        );
        let allocation = component.payload.u32(0x44)?;
        ensure!(
            ![0, u32::MAX].contains(&allocation),
            "replication component has no allocation"
        );
        let mut row = vec![0; 32];
        row[..4].copy_from_slice(&class.to_le_bytes());
        row[4..8].copy_from_slice(&allocation.to_le_bytes());
        // +8 and +10 are a runtime field array. The opaque trailing word at +18
        // is not consumed by the captured load callbacks or state-size query.
        // Leave it zero rather than copying a donor's allocation-dependent value.
        rows.push(row);
    }
    let mut output = Payload(vec![0; 64]);
    put(&mut output, 8, &entity.to_le_bytes())?;
    array(&mut output, 0x30, 0x80809BB8, 32, &rows)?;
    let size = output.0.len() as u64;
    put(&mut output, 0, &size.to_le_bytes())?;
    Ok(output)
}
