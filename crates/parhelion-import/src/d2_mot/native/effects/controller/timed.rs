//! Timed attachments retain native lifecycle dispatch and translated source tuning.
use super::*;
use crate::d2_mot::entity::links::Interface;
use std::collections::BTreeMap;

mod duration;
mod root;
mod status;
mod timer;

pub struct Templates<'a> {
    pub owner: &'a Payload,
    pub allocation: &'a Payload,
    pub scalar_input: &'a Payload,
    pub metadata: &'a BTreeMap<u32, Payload>,
}

pub struct Bindings {
    pub resources: BTreeMap<usize, crate::d2_mot::entity::sequence::Resource>,
    pub categories: super::super::resources::selectors::Resources,
    pub groups: super::super::resources::selectors::GroupBindings,
    pub resolver: super::super::resources::selectors::ResourceResolver,
}

pub struct Timed {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    pub unbound: Vec<Object>,
    pub stable_inputs: Vec<Object>,
    pub named_conditions: Vec<u32>,
}

pub fn emit(
    source: &Payload,
    templates: Templates<'_>,
    bindings: &Bindings,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Timed> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid timed output identities"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = templates.owner.pointer(16)?;
    let nd = templates.owner.pointer(24)?;
    for (payload, i, d, ic, dc) in [
        (source, si, sd, 0x80803564, 0x8080B11F),
        (templates.owner, ni, nd, 0x80804209, 0x80804205),
    ] {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64
                && payload.u32(i - 4)? == ic
                && payload.u32(d - 4)? == dc,
            "timed root class or extent differs"
        );
        pair(payload, i, d, ic, dc)?;
    }
    let mut owner = templates.owner.clone();
    let old_tag = owner.u32(ni)?;
    for at in (0..owner.0.len().saturating_sub(15)).step_by(4) {
        if owner.u32(at)? == old_tag {
            ensure!(
                owner.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && owner.u64(at + 8)? < owner.0.len() as u64,
                "untyped native timed owner occurrence"
            );
            put(&mut owner.0, at, &owner_tag.to_le_bytes())?;
        }
    }
    let (mut objects, mut unbound, named_conditions) =
        root::emit(source, &mut owner, templates.metadata, bindings)?;
    super::refresh_size(&mut owner)?;
    let status = status::emit(
        source,
        &mut owner,
        templates.scalar_input,
        templates.metadata,
        (si + 0x30, sd + 0x100),
        (ni + 0x30, nd + 0xE0),
    )?;
    objects.extend(status.objects);
    unbound.extend(status.unbound);
    objects.push(object(source, &owner, si, 0x80803564, ni, 0x80804209)?);
    objects.push(object(source, &owner, sd, 0x8080B11F, nd, 0x80804205)?);
    let rows = if status.allocation.is_empty() {
        Vec::new()
    } else {
        vec![allocation::Row::named(
            0xF3ED5513,
            u32::MAX,
            0,
            status.allocation,
        )]
    };
    let mut allocation = templates.allocation.clone();
    allocation::Row::write(&rows, &mut allocation, 0x20)?;
    put(&mut owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    super::seal(&mut owner)?;
    Ok(Timed {
        owner,
        allocation,
        objects,
        unbound,
        stable_inputs: status.stable_inputs,
        named_conditions,
    })
}

fn pair(p: &Payload, i: usize, d: usize, ic: u32, dc: u32) -> Result<()> {
    let owner = p.u32(p.pointer(16)?)?;
    ensure!(
        p.u32(i)? == owner
            && p.u32(d)? == owner
            && p.u32(i + 4)? == dc
            && p.u32(d + 4)? == ic
            && p.u64(i + 8)? == d as u64
            && p.u64(d + 8)? == i as u64,
        "timed component reciprocal records differ"
    );
    Ok(())
}

fn object(
    source: &Payload,
    output: &Payload,
    from: usize,
    sc: u32,
    to: usize,
    nc: u32,
) -> Result<Relocation> {
    Ok(Relocation {
        source: Object {
            owner: source.u32(source.pointer(16)?)?,
            class: sc,
            offset: from as u64,
        },
        target: Object {
            owner: output.u32(output.pointer(16)?)?,
            class: nc,
            offset: to as u64,
        },
    })
}

/// Provider metadata is checked on each side. A class correspondence never
/// authorizes copying the modern compiled method index into the native graph.
fn provider(
    p: &Payload,
    at: usize,
    parent: usize,
    class: u32,
    dispatch: (u32, &[u32]),
    modern: bool,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<()> {
    let (implementation, methods) = dispatch;
    let object = Object {
        owner: p.u32(p.pointer(16)?)?,
        class,
        offset: at as u64,
    };
    let contract = Interface::read(
        p,
        object,
        metadata
            .get(&p.u32(at + 8)?)
            .context("timed interface metadata absent")?,
        modern,
    )?;
    let instance_class = if matches!(class, 0x808098D2 | 0x80809AE1) {
        class + 1
    } else {
        class - 1
    };
    ensure!(
        contract.definition_offset == parent
            && contract.instance_class == instance_class
            && contract.methods.len() == methods.len()
            && contract
                .methods
                .iter()
                .zip(methods)
                .all(
                    |(method, index)| method.implementation_class == implementation
                        && method.index == *index
                        && method.arguments == [0, 0]
                ),
        "timed interface {class:08X} at {at:X} has unexpected dispatch metadata {:08X}",
        contract.metadata_tag
    );
    Ok(())
}

/// The event-source input keeps its name-independent binding policy and its
/// inactive reference. Native linkage populates its runtime handle later.
fn input(
    source: &Payload,
    output: &mut Payload,
    from: (usize, usize, usize),
    to: (usize, usize, usize),
) -> Result<Vec<Relocation>> {
    let (si, sd, sp) = from;
    let (ni, nd, np) = to;
    pair(source, si, sd, 0x8080918B, 0x8080918C)?;
    pair(output, ni, nd, 0x808092F3, 0x808092F4)?;
    ensure!(
        source.pointer(si + 16)? == sp
            && output.pointer(ni + 16)? == np
            && source.bytes::<56>(si + 24)? == output.bytes::<56>(ni + 24)?
            && source.u64(sd + 16)? <= 2
            && source.u64(sd + 24)? == 0x8080918F
            && output.u64(nd + 24)? == 0x808092F7
            && source.u64(sd + 32)? == 0
            && source.u32(sd + 40)? == u32::MAX
            && source.u32(sd + 44)? == 1
            && source.u64(sd + 48)? == 0,
        "timed event-source input has unsupported policy or live state"
    );
    put(&mut output.0, nd + 16, &source.bytes::<8>(sd + 16)?)?;
    put(&mut output.0, nd + 32, &[0; 8])?;
    put(&mut output.0, nd + 40, &u32::MAX.to_le_bytes())?;
    put(&mut output.0, nd + 44, &0u32.to_le_bytes())?;
    Ok(vec![
        object(source, output, si, 0x8080918B, ni, 0x808092F3)?,
        object(source, output, sd, 0x8080918C, nd, 0x808092F4)?,
    ])
}

struct Embedded {
    objects: Vec<Relocation>,
    unbound: Vec<Object>,
    allocation: Vec<allocation::Row>,
    /// Assembly must trace these inputs to values fixed for the attachment's
    /// lifetime before enabling the native remaining-time refresh route.
    stable_inputs: Vec<Object>,
}
