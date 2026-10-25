//! A private action controller hosted by an attached entity.
use super::{component::*, *};
use std::collections::BTreeMap;

pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    action: u32,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Converted> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&action),
        "invalid hosted action identity"
    );
    let mut owner = envelope(
        source,
        template,
        [0x80803140, 0x8080313F, 0x80803E51, 0x80803E52],
        owner_tag,
        allocation_tag,
    )?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = owner.pointer(16)?;
    let nd = owner.pointer(24)?;
    ensure!(
        source.bytes::<16>(si + 0x30)? == [0xFF; 16]
            && source.u64(si + 0x40)? == 0
            && source.u64(si + 0x48)? == u64::MAX
            && source.bytes::<16>(si + 0x50)? == [0; 16]
            && source.u64(si + 0x60)? == 0x1FFFFFFFF
            && source.u64(si + 0x68)? == 0
            && source.u64(sd + 0x48)? == 0,
        "hosted action has initialized or unsupported state"
    );
    provider(
        source,
        sd + 0x60,
        sd,
        0x808098CD,
        (0x80803140, 2),
        true,
        metadata,
    )?;
    provider(
        &owner,
        nd + 0x58,
        nd,
        0x80809ADE,
        (0x80803E51, 0),
        false,
        metadata,
    )?;
    let auxiliary = crate::d2_mot::entity::links::Interface::read(
        source,
        Object {
            owner: source.u32(si)?,
            class: 0x80803142,
            offset: (sd + 0x80) as u64,
        },
        metadata
            .get(&source.u32(sd + 0x88)?)
            .context("hosted action auxiliary metadata absent")?,
        true,
    )?;
    ensure!(
        auxiliary.definition_offset == sd
            && auxiliary.instance_class == 0x80803141
            && auxiliary.methods.len() == 2
            && auxiliary
                .methods
                .iter()
                .enumerate()
                .all(|(index, method)| method.implementation_class == 0x80803140
                    && method.index == index as u32
                    && method.arguments == [0, 0]),
        "hosted action auxiliary dispatch differs"
    );
    put(&mut owner.0, nd + 0x50, &action.to_le_bytes())?;
    let objects = vec![
        object(source, &owner, si, 0x80803140, ni, 0x80803E51)?,
        object(source, &owner, sd, 0x8080313F, nd, 0x80803E52)?,
        object(source, &owner, sd + 0x60, 0x808098CD, nd + 0x58, 0x80809ADE)?,
    ];
    let unbound = vec![Object {
        owner: source.u32(si)?,
        class: 0x80803142,
        offset: (sd + 0x80) as u64,
    }];
    let mut allocation = allocation.clone();
    allocation::Row::write(&[], &mut allocation, 0x20)?;
    seal(&mut owner)?;
    Ok(Converted {
        owner,
        allocation,
        objects,
        unbound,
    })
}
