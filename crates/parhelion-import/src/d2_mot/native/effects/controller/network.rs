//! Native network components with checked provider dispatch and source tuning.
use super::{Relocation, put};
use crate::d2_mot::{
    entity::links::{Interface, Object},
    payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub struct Network {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    /// Assembly must reject any requested method on this source-only provider.
    pub omitted: Vec<Omitted>,
}

#[derive(Serialize)]
pub struct Omitted {
    pub object: Object,
    pub methods: Vec<u32>,
}

const PROVIDERS: [(usize, usize, u32, u32, u32, u32); 7] = [
    (0x48, 0x48, 0x8080955E, 0x8080974B, 8, 7),
    (0x70, 0x68, 0x8080955E, 0x8080974B, 9, 8),
    (0x98, 0x88, 0x808098CD, 0x80809ADE, 2, 2),
    (0xB8, 0xA0, 0x808098C9, 0x80809ADA, 4, 4),
    (0xD8, 0xB8, 0x80809AB8, 0x80809BF0, 3, 3),
    (0xF8, 0xD0, 0x808091A8, 0x80809494, 5, 5),
    (0x138, 0xE8, 0x808091AB, 0x80809497, 7, 6),
];

fn pair(p: &Payload, instance: usize, definition: usize) -> Result<u32> {
    let tag = p.u32(instance)?;
    ensure!(
        tag != u32::MAX
            && tag == p.u32(definition)?
            && p.u64(instance + 8)? == definition as u64
            && p.u64(definition + 8)? == instance as u64,
        "network reciprocal records differ"
    );
    Ok(tag)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn provider(
    p: &Payload,
    at: usize,
    definition: usize,
    tag: u32,
    class: u32,
    method: u32,
    modern: bool,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<Object> {
    let object = Object {
        owner: tag,
        class,
        offset: at as u64,
    };
    let interface = Interface::read(
        p,
        object,
        metadata
            .get(&p.u32(at + 8)?)
            .context("network provider metadata missing")?,
        modern,
    )?;
    ensure!(
        interface.definition_offset == definition
            && interface.instance_class == class - 1
            && p.u64(at + 16)? == 0
            && (!modern || p.u64(at + 24)? == 0),
        "network provider parent or state differs"
    );
    ensure!(
        interface.methods.len() == 1
            && interface.methods[0].implementation_class
                == if modern { 0x80808EDB } else { 0x808090E6 }
            && interface.methods[0].index == method
            && interface.methods[0].arguments == [0, 0],
        "network provider dispatch differs"
    );
    Ok(object)
}

fn lifecycle(p: &Payload, field: usize, instance: usize, modern: bool) -> Result<()> {
    let rows = p.array(
        field,
        16,
        Some(if modern { 0x8080907C } else { 0x808091A4 }),
    )?;
    ensure!(rows.len() == 1, "network lifecycle count differs");
    let row = rows[0];
    ensure!(
        p.pointer(row)? == instance
            && p.u32(row + 8)? == if modern { 0x80808EDB } else { 0x808090E6 }
            && p.u32(row + 12)? == 0x00010000,
        "network lifecycle dispatch differs"
    );
    Ok(())
}

/// Preserve the source network priority parameters in a native component.
/// Only the independently paired unbound attachment form is supported.
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Network> {
    for p in [source, template, allocation] {
        ensure!(
            p.u64(0)? == p.0.len() as u64,
            "network payload size differs"
        );
    }
    ensure!(
        owner_tag != 0
            && owner_tag != u32::MAX
            && allocation_tag != 0
            && allocation_tag != u32::MAX
            && owner_tag != allocation_tag,
        "invalid private network tags"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = template.pointer(16)?;
    let nd = template.pointer(24)?;
    let source_tag = pair(source, si, sd)?;
    let native_tag = pair(template, ni, nd)?;
    ensure!(
        owner_tag != source_tag && owner_tag != native_tag,
        "network owner was not privately allocated"
    );
    ensure!(
        source.u32(si - 4)? == 0x80808EDB
            && source.u32(sd - 4)? == 0x80808EDD
            && source.u32(si + 4)? == 0x80808EDD
            && source.u32(sd + 4)? == 0x80808EDB,
        "unsupported source network records"
    );
    // Native network records carry dynamic schema tags rather than the base
    // implementation class. Check both schemas before retaining the envelope.
    for (at, base, size) in [(ni, 0x808090E8, 344), (nd, 0x808090E6, 96)] {
        let class = template.u32(at + 4)?;
        let schema = metadata
            .get(&class)
            .context("native network schema missing")?;
        ensure!(
            schema.u64(0)? == schema.0.len() as u64
                && schema.u32(16)? == base
                && schema.u32(20)? == size
                && schema
                    .0
                    .windows(b"network_component_tag".len())
                    .any(|v| v == b"network_component_tag"),
            "native network schema contract differs"
        );
    }
    ensure!(
        template.u32(ni - 4)? == template.u32(nd + 4)?
            && template.u32(nd - 4)? == template.u32(ni + 4)?
            && template.u64(0x48)?
                == nd.checked_sub(ni).context("native network record order")? as u64,
        "native network record extent differs"
    );
    let mut state = [0u8; 80];
    for at in [20, 28, 32] {
        state[at..at + 4].fill(255);
    }
    state[48..64].fill(255);
    state[72] = 255;
    ensure!(
        source.bytes::<80>(si + 16)? == state,
        "initialized source network state"
    );
    ensure!(
        source.bytes::<56>(sd + 16)? == template.bytes::<56>(nd + 16)?,
        "unsupported network definition defaults"
    );
    for (p, class) in [(source, 0x80808EDC), (template, 0x808090E7)] {
        let constructor = p.pointer(8)?;
        ensure!(
            p.bytes::<16>(constructor)? == [0; 16],
            "initialized network constructor state"
        );
        ensure!(
            p.u32(
                constructor
                    .checked_sub(4)
                    .context("network constructor prefix")?
            )? == class,
            "network constructor class differs"
        );
    }
    ensure!(
        source.u64(sd + 0x68)? == 0x80808EE0
            && source.u64(sd + 0x90)? == 0x80808EDF
            && template.u64(nd + 0x60)? == 0x808090EB
            && template.u64(nd + 0x80)? == 0x808090EA,
        "network typed provider kinds differ"
    );
    let mut objects = Vec::new();
    for (from, to) in [(si, ni), (sd, nd)] {
        objects.push(Relocation {
            source: Object {
                owner: source_tag,
                class: source.u32(from + 4)?,
                offset: from as u64,
            },
            target: Object {
                owner: owner_tag,
                class: template.u32(to + 4)?,
                offset: to as u64,
            },
        });
    }
    for (sm, sn, sc, nc, mi, me) in PROVIDERS {
        let from = provider(source, sd + sm, sd, source_tag, sc, mi, true, metadata)?;
        let mut to = provider(template, nd + sn, nd, native_tag, nc, me, false, metadata)?;
        to.owner = owner_tag;
        objects.push(Relocation {
            source: from,
            target: to,
        });
    }
    let omitted = provider(
        source,
        sd + 0x118,
        sd,
        source_tag,
        0x808095C4,
        6,
        true,
        metadata,
    )?;
    // The saved native facet constructor copies precisely 68 bytes. Its source
    // block excludes the following attachment binding mode at +19C.
    let priority = source.bytes::<68>(sd + 0x158)?;
    for at in [0, 4, 8, 12, 16, 20, 24, 28, 32, 44, 48, 52, 56, 64] {
        source.f32(sd + 0x158 + at)?;
    }
    ensure!(
        source.u32(sd + 0x19C)? == 2
            && source.u64(sd + 0x1A0)? == 0
            && source.u32(sd + 0x1A8)? == u32::MAX
            && source.u32(sd + 0x1AC)? == 1
            && source.u64(sd + 0x1B0)? == 0
            && source.bytes::<12>(sd + 0x1B8)? == [0; 12],
        "unsupported active network attachment binding"
    );
    ensure!(
        template.u32(nd + 0x144)? == 0
            && template.u64(nd + 0x148)? == 0
            && template.u64(nd + 0x150)? == u64::from(u32::MAX),
        "native network attachment defaults differ"
    );
    lifecycle(source, 0x40, si, true)?;
    lifecycle(template, 0x30, ni, false)?;
    let refs = source.array(0x60, 8, Some(0x8080906E))?;
    ensure!(
        refs.len() == 1 && source.pointer(refs[0])? == sd + 0x1A8,
        "source network reference enumeration differs"
    );
    ensure!(
        allocation.0.len() == 48
            && allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u64::from(u32::MAX)
            && allocation.u64(32)? == 0
            && allocation.u64(40)? == 0,
        "native network allocation is not empty"
    );
    let mut owner = template.clone();
    for at in [ni, nd] {
        put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
    }
    put(&mut owner.0, nd + 0x100, &priority)?;
    put(&mut owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    Ok(Network {
        owner,
        allocation: allocation.clone(),
        objects,
        omitted: vec![Omitted {
            object: omitted,
            methods: vec![6],
        }],
    })
}
