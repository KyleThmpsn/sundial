//! Common contracts for small paired component implementations.
use super::*;
use crate::d2_mot::entity::links::Interface;
use std::collections::BTreeMap;

pub struct Converted {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    pub unbound: Vec<Object>,
}

pub(super) fn envelope(
    source: &Payload,
    template: &Payload,
    classes: [u32; 4],
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Payload> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid component output identities"
    );
    for (p, ic, dc) in [
        (source, classes[0], classes[1]),
        (template, classes[2], classes[3]),
    ] {
        let i = p.pointer(16)?;
        let d = p.pointer(24)?;
        ensure!(
            p.u64(0)? == p.0.len() as u64
                && p.u32(i - 4)? == ic
                && p.u32(d - 4)? == dc
                && p.u32(i)? == p.u32(d)?
                && p.u32(i + 4)? == dc
                && p.u32(d + 4)? == ic
                && p.u64(i + 8)? == d as u64
                && p.u64(d + 8)? == i as u64,
            "component pair or extent differs"
        );
    }
    let mut output = crate::tiger::projectile::retag(template, owner_tag)?;
    put(&mut output.0, 0x44, &allocation_tag.to_le_bytes())?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    ensure!(
        source.bytes::<32>(si + 16)? == output.bytes::<32>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == output.bytes::<56>(nd + 16)?,
        "component base state differs"
    );
    Ok(output)
}

pub(super) fn object(
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

pub(super) fn provider(
    p: &Payload,
    at: usize,
    parent: usize,
    class: u32,
    dispatch: (u32, u32),
    modern: bool,
    metadata: &BTreeMap<u32, Payload>,
) -> Result<()> {
    let (implementation, method) = dispatch;
    let contract = Interface::read(
        p,
        Object {
            owner: p.u32(p.pointer(16)?)?,
            class,
            offset: at as u64,
        },
        metadata
            .get(&p.u32(at + 8)?)
            .context("component provider metadata absent")?,
        modern,
    )?;
    ensure!(
        contract.definition_offset == parent
            && contract.instance_class == class - 1
            && contract.methods.len() == 1
            && contract.methods[0].implementation_class == implementation
            && contract.methods[0].index == method
            && contract.methods[0].arguments == [0, 0],
        "component provider dispatch differs"
    );
    Ok(())
}
