//! Empty damage-event tables retain their native dispatch and registration.
use super::{component::*, *};
use std::collections::BTreeMap;

pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Converted> {
    let mut owner = envelope(
        source,
        template,
        [0x8080433D, 0x8080433E, 0x80804DD1, 0x80804DD2],
        owner_tag,
        allocation_tag,
    )?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = owner.pointer(16)?;
    let nd = owner.pointer(24)?;
    for field in (0x48..0xB8).step_by(16) {
        ensure!(
            source.bytes::<16>(sd + field)? == [0; 16],
            "populated damage-event table requires translation"
        );
        put(&mut owner.0, nd + field, &[0; 16])?;
    }
    ensure!(
        source.u64(sd + 0xB8)? == 0,
        "damage-event extension differs"
    );
    let mut objects = vec![
        object(source, &owner, si, 0x8080433D, ni, 0x80804DD1)?,
        object(source, &owner, sd, 0x8080433E, nd, 0x80804DD2)?,
    ];
    for (index, (sc, nc)) in [
        (0x808040D8, 0x80804C57),
        (0x808040D7, 0x80804C56),
        (0x808040D6, 0x80804C55),
        (0x808040FC, 0x80804C54),
        (0x808040D5, 0x80804C53),
        (0x808040D4, 0x80802A4D),
        (0x808040D3, 0x80804C52),
    ]
    .into_iter()
    .enumerate()
    {
        let from = sd + 0xC0 + index * 40;
        let to = nd + 0xB8 + index * 32;
        provider(
            source,
            from,
            sd,
            0x8080955E,
            (0x8080433D, index as u32 + 1),
            true,
            metadata,
        )?;
        provider(
            &owner,
            to,
            nd,
            0x8080974B,
            (0x80804DD1, index as u32 + 1),
            false,
            metadata,
        )?;
        ensure!(
            source.u64(from + 32)? == sc && owner.u64(to + 24)? == nc,
            "damage-event registration class differs"
        );
        objects.push(object(source, &owner, from, 0x8080955E, to, 0x8080974B)?);
    }
    let mut allocation = allocation.clone();
    allocation::Row::write(&[], &mut allocation, 0x20)?;
    seal(&mut owner)?;
    Ok(Converted {
        owner,
        allocation,
        objects,
        unbound: Vec::new(),
    })
}
