//! Checked scalar programs used by projectile mechanics.
use crate::{
    presentation::{append, put},
    tiger::{entity::links::Object, payload::Payload},
};
use anyhow::{Result, ensure};

/// Remove the channel inputs a replacement expression no longer consumes.
pub(super) fn unbind(p: &mut Payload, definition: usize) -> Result<Vec<Object>> {
    let instance = usize::try_from(p.u64(definition + 8)?)?;
    let owner = p.u32(definition)?;
    ensure!(
        matches!(
            (p.u32(definition + 4)?, p.u32(instance + 4)?),
            (0x808089f5, 0x808089f6) | (0x808089f7, 0x808089f8)
        ) && p.u32(instance)? == owner
            && p.u64(instance + 8)? == definition as u64,
        "Projectile expression reciprocal differs"
    );
    let definitions = p.array(definition + 0x40, 40, Some(0x80809789))?;
    let instances = p.array(instance + 0x20, 96, Some(0x80809788))?;
    ensure!(
        definitions.len() == instances.len(),
        "Projectile expression input counts differ"
    );
    let mut objects = Vec::new();
    for (definition, instance) in definitions.into_iter().zip(instances) {
        ensure!(
            p.u32(definition)? == owner
                && p.u32(definition + 4)? == 0x80809788
                && p.u64(definition + 8)? == instance as u64
                && p.u32(instance)? == owner
                && p.u32(instance + 4)? == 0x80809789
                && p.u64(instance + 8)? == definition as u64,
            "Projectile expression input reciprocal differs"
        );
        objects.push(Object {
            owner,
            class: 0x80809789,
            offset: definition as u64,
        });
    }
    put(&mut p.0, definition + 0x40, &[0; 16])?;
    put(&mut p.0, instance + 0x20, &[0; 16])?;
    Ok(objects)
}

/// Replace an 80808E76 expression, preserving its enclosing reciprocal object.
pub(super) fn write(
    p: &mut Payload,
    at: usize,
    code: &[u8],
    values: &[f32],
    inputs: u64,
) -> Result<()> {
    ensure!(
        values.iter().all(|v| v.is_finite()),
        "Nonfinite projectile expression"
    );
    p.bytes::<48>(at)?;
    append(&mut p.0, at, 0x80800009, code, 1)?;
    let constants = values
        .iter()
        .flat_map(|v| [*v; 4])
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    append(&mut p.0, at + 16, 0x80800090, &constants, 16)?;
    put(&mut p.0, at + 32, &inputs.to_le_bytes())?;
    put(&mut p.0, at + 40, &1u64.to_le_bytes())
}

pub(super) fn constant(p: &mut Payload, at: usize, value: f32) -> Result<()> {
    write(p, at, &[0x34, 0, 0x3e, 0], &[value], 0)
}

pub(super) fn first(p: &mut Payload, at: usize, seconds: f32) -> Result<()> {
    // Input zero is the collector's detonation ordinal. Only its first shard waits.
    write(
        p,
        at,
        &[0x34, 0, 0x3c, 0, 0x23, 0x02, 0x34, 1, 0x03, 0x3e, 0],
        &[1., seconds],
        1,
    )
}
