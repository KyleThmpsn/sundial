//! Interface-only context owners reused through individually checked contracts.
use super::links::{Interface, Object};
use crate::d2_mot::{native::effects::controller::Relocation, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub(super) fn root(
    p: &Payload,
    instance_class: u32,
    definition_class: u32,
) -> Result<(u32, usize, usize)> {
    ensure!(p.u64(0)? == p.0.len() as u64, "shared owner size differs");
    let i = p.pointer(16)?;
    let d = p.pointer(24)?;
    let tag = p.u32(i)?;
    ensure!(
        i >= 4
            && d >= 4
            && p.u32(i - 4)? == instance_class
            && p.u32(d - 4)? == definition_class
            && p.u32(i + 4)? == definition_class
            && p.u32(d)? == tag
            && p.u32(d + 4)? == instance_class
            && p.u64(i + 8)? == d as u64
            && p.u64(d + 8)? == i as u64,
        "shared component pair differs"
    );
    Ok((tag, i, d))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
pub(super) fn provider(
    p: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    root: usize,
    offset: usize,
    name: Option<u32>,
    class: u32,
    instance_class: u32,
    implementation_class: u32,
    method: u32,
    modern: bool,
) -> Result<Object> {
    let at = root + offset;
    let width = if modern { 32 } else { 24 };
    ensure!(
        p.pointer(at)? == root && p.u64(at + 16)? == 0 && (!modern || p.u64(at + 24)? == 0),
        "shared provider state differs at {offset:X}"
    );
    if let Some(name) = name {
        ensure!(
            p.u64(at + width)? == name as u64,
            "shared provider name differs"
        );
    }
    let object = Object {
        owner: p.u32(root)?,
        class,
        offset: at as u64,
    };
    let interface = Interface::read(
        p,
        object,
        metadata
            .get(&p.u32(at + 8)?)
            .context("shared provider metadata absent")?,
        modern,
    )?;
    ensure!(
        interface.definition_offset == root
            && interface.instance_class == instance_class
            && interface.methods.len() == 1,
        "shared provider interface differs"
    );
    let entry = &interface.methods[0];
    ensure!(
        entry.implementation_class == implementation_class
            && entry.index == method
            && entry.arguments == [0, 0],
        "shared provider getter contract differs"
    );
    Ok(object)
}

/// Map named global-context values. The modern-only provider is validated but
/// deliberately has no relocation. Graph lowering must reject a connection to
/// it instead of substituting an unrelated native method.
pub fn globals(
    source: &Payload,
    native: &Payload,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<Vec<Relocation>> {
    let (_, si, sd) = root(source, 0x808032A9, 0x808032AA)?;
    let (_, ni, nd) = root(native, 0x80803F6D, 0x80803F6E)?;
    ensure!(
        source.bytes::<56>(sd + 16)? == native.bytes::<56>(nd + 16)?
            && source.bytes::<64>(si + 16)? == native.bytes::<64>(ni + 16)?
            && source.bytes::<8>(si + 0x50)? == [0, 0, 128, 191, 0, 0, 0, 0],
        "shared context contains authored properties or initialized state"
    );
    // Validate every source definition field, including the newer interface
    // which the older runtime cannot supply.
    provider(
        source, metadata, sd, 0xC0, None, 0x8080B785, 0x8080B784, 0x808032A9, 7, true,
    )?;
    for (so, no, sn, nn, method) in [
        (0xE0, 0xA8, 0x80809445, 0x8080971A, 2),
        (0x108, 0xC8, 0x80803F62, 0x80804AF4, 3),
    ] {
        provider(
            source,
            metadata,
            sd,
            so,
            Some(sn),
            0x8080955E,
            0x8080955D,
            0x808032A9,
            method,
            true,
        )?;
        provider(
            native,
            metadata,
            nd,
            no,
            Some(nn),
            0x8080974B,
            0x8080974A,
            0x80803F6D,
            method,
            false,
        )?;
    }
    let mut result = Vec::new();
    for (so, no, name, method) in [
        (0x48, 0x48, 0xE2F20335, 4),
        (0x70, 0x68, 0xE729F6E8, 5),
        (0x98, 0x88, 0x0F654FFA, 6),
    ] {
        result.push(Relocation {
            source: provider(
                source,
                metadata,
                sd,
                so,
                Some(name),
                0x808098D2,
                0x808098D3,
                0x808032A9,
                method,
                true,
            )?,
            target: provider(
                native,
                metadata,
                nd,
                no,
                Some(name),
                0x80809AE1,
                0x80809AE2,
                0x80803F6D,
                method,
                false,
            )?,
        });
    }
    Ok(result)
}
