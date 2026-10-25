//! Shared entity context interfaces backed by their native implementation.
pub mod effect;
use super::links::{Interface, Object};
use crate::d2_mot::{native::effects::controller::Relocation, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

fn root(
    payload: &Payload,
    instance_class: u32,
    definition_class: u32,
) -> Result<(u32, usize, usize)> {
    ensure!(
        payload.u64(0)? == payload.0.len() as u64,
        "context owner size differs"
    );
    let instance = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    let owner = payload.u32(instance)?;
    ensure!(
        payload.u32(instance.checked_sub(4).context("context instance class")?)? == instance_class
            && payload.u32(
                definition
                    .checked_sub(4)
                    .context("context definition class")?
            )? == definition_class
            && payload.u32(instance + 4)? == definition_class
            && payload.u32(definition + 4)? == instance_class
            && payload.u32(definition)? == owner
            && payload.u64(instance + 8)? == definition as u64
            && payload.u64(definition + 8)? == instance as u64,
        "context component pair differs"
    );
    Ok((owner, instance, definition))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn provider(
    payload: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner: u32,
    definition: usize,
    offset: usize,
    name: u32,
    class: u32,
    instance_class: u32,
    method_class: u32,
    method: u32,
    modern: bool,
) -> Result<Object> {
    let object = Object {
        owner,
        class,
        offset: (definition + offset) as u64,
    };
    let at = definition + offset;
    let width = if modern { 32 } else { 24 };
    ensure!(
        payload.u64(at + width)? == name as u64
            && payload.u64(at + 16)? == 0
            && (!modern || payload.u64(at + 24)? == 0),
        "context provider state or name differs"
    );
    let tag = payload.u32(at + 8)?;
    let interface = Interface::read(
        payload,
        object,
        metadata
            .get(&tag)
            .context("context provider metadata is absent")?,
        modern,
    )?;
    ensure!(
        interface.definition_offset == definition
            && interface.instance_class == instance_class
            && interface.methods.len() == 1,
        "context provider interface differs"
    );
    let entry = &interface.methods[0];
    ensure!(
        entry.implementation_class == method_class
            && entry.index == method
            && entry.arguments == [0, 0],
        "context provider method differs"
    );
    Ok(object)
}

/// Resolve the two inspected named context outputs and the attachment input.
/// The caller retains the native component and its native allocation metadata.
/// This emits no guessed implementation class or copied modern method index.
pub fn interfaces(
    source: &Payload,
    native: &Payload,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<Vec<Relocation>> {
    let (source_owner, si, sd) = root(source, 0x808040FF, 0x80803F64)?;
    let (native_owner, ni, nd) = root(native, 0x80804C7A, 0x80804AF6)?;
    // These records are interface-only definitions. No authored component
    // property can disappear in the intervals preceding their providers.
    ensure!(
        source.bytes::<56>(sd + 0x10)? == native.bytes::<56>(nd + 0x10)?,
        "context base properties differ"
    );
    for (source_offset, native_offset, source_name, native_name) in [
        (0xE8, 0xC0, 0x80809445, 0x8080971A),
        (0x110, 0xE0, 0x8080943E, 0x80809713),
        (0x138, 0x100, 0x80809772, 0x8080949D),
    ] {
        ensure!(
            source.pointer(sd + source_offset)? == sd
                && native.pointer(nd + native_offset)? == nd
                && source.u64(sd + source_offset + 16)? == 0
                && source.u64(sd + source_offset + 24)? == 0
                && source.u64(sd + source_offset + 32)? == source_name
                && native.u64(nd + native_offset + 16)? == 0
                && native.u64(nd + native_offset + 24)? == native_name,
            "context fixed interface contract differs"
        );
    }
    let input =
        |p: &Payload, owner, instance, definition, offset, ic, dc, required| -> Result<Object> {
            let at = definition + offset;
            let state = instance + 0x30;
            ensure!(
                p.u32(at)? == owner
                    && p.u32(at + 4)? == ic
                    && p.u64(at + 8)? == state as u64
                    && p.u64(at + 16)? == 1
                    && p.u64(at + 24)? == required
                    && p.u64(at + 32)? == u32::MAX as u64
                    && p.u32(state)? == owner
                    && p.u32(state + 4)? == dc
                    && p.u64(state + 8)? == at as u64
                    && p.pointer(state + 16)? == instance,
                "context attachment input differs"
            );
            Ok(Object {
                owner,
                class: dc,
                offset: at as u64,
            })
        };
    let mut result = vec![Relocation {
        source: input(
            source,
            source_owner,
            si,
            sd,
            0x1B0,
            0x80809A9E,
            0x80809A9F,
            0x8080977E,
        )?,
        target: input(
            native,
            native_owner,
            ni,
            nd,
            0x160,
            0x80809BD8,
            0x80809BD9,
            0x808094A9,
        )?,
    }];
    ensure!(
        source.bytes::<56>(si + 0x48)? == native.bytes::<56>(ni + 0x48)?,
        "context attachment input is initialized differently"
    );
    for (so, no, name, sm, nm) in [
        (0x160, 0x120, 0x49FCE899, 14, 13),
        (0x188, 0x140, 0xDC44E70B, 15, 14),
    ] {
        result.push(Relocation {
            source: provider(
                source,
                metadata,
                source_owner,
                sd,
                so,
                name,
                0x808098D2,
                0x808098D3,
                0x808040FF,
                sm,
                true,
            )?,
            target: provider(
                native,
                metadata,
                native_owner,
                nd,
                no,
                name,
                0x80809AE1,
                0x80809AE2,
                0x80804C7A,
                nm,
                false,
            )?,
        });
    }
    Ok(result)
}
