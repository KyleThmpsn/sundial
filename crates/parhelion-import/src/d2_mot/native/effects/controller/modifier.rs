//! Incoming damage modifiers with empty authored rules and a literal multiplier.
use super::{component::*, *};
use std::collections::BTreeMap;

pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    scalar_input: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Converted> {
    let mut owner = envelope(
        source,
        template,
        [0x808032E4, 0x808032E5, 0x80803F8B, 0x80803F8C],
        owner_tag,
        allocation_tag,
    )?;
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = owner.pointer(16)?;
    let nd = owner.pointer(24)?;
    // The source-only object input has no native equivalent. Assembly must
    // establish that it has no provider before removing its disconnected row.
    ensure!(
        source.u32(si + 0x50)? == source.u32(si)?
            && source.u32(si + 0x54)? == 0x80809A9F
            && source.u64(si + 0x58)? == (sd + 0xF8) as u64
            && source.pointer(si + 0x60)? == si
            && source.u32(sd + 0xF8)? == source.u32(si)?
            && source.u32(sd + 0xFC)? == 0x80809A9E
            && source.u64(sd + 0x100)? == (si + 0x50) as u64
            && source.u64(sd + 0x108)? == 2
            && source.u64(sd + 0x110)? == u32::MAX as u64,
        "modifier object input state differs"
    );
    for field in [sd + 0x68, sd + 0x78, si + 0xA0] {
        ensure!(
            source.bytes::<16>(field)? == [0; 16],
            "populated modifier rules require translation"
        );
    }
    for field in [nd + 0x60, nd + 0x70, ni + 0x48] {
        put(&mut owner.0, field, &[0; 16])?;
    }
    ensure!(
        source.u64(sd + 0xE8)? == 0x811C9DC5 && source.u64(sd + 0xF0)? == 0,
        "modifier named behavior requires translation"
    );
    put(&mut owner.0, nd + 0xE0, &source.bytes::<16>(sd + 0xE8)?)?;
    let scalar = scalar::emit(source, &mut owner, scalar_input, sd + 0x88, nd + 0x80, 1)?;
    ensure!(
        scalar.extra_inputs == 0
            && scalar.source_code == [0x42, 0, 0x4C, 0]
            && scalar.constants.len() == 16,
        "modifier requires a literal scalar until its input consumer is qualified"
    );
    ensure!(
        scalar
            .constants
            .chunks_exact(4)
            .all(
                |lane| f32::from_le_bytes(lane.try_into().expect("four-byte constant lane"))
                    .is_finite()
            ),
        "modifier scalar is nonfinite"
    );
    provider(
        source,
        sd + 0x48,
        sd,
        0x808098CD,
        (0x808032E4, 1),
        true,
        metadata,
    )?;
    provider(
        &owner,
        nd + 0x48,
        nd,
        0x80809ADE,
        (0x80803F8B, 1),
        false,
        metadata,
    )?;
    let mut objects = scalar.objects;
    objects.extend([
        object(source, &owner, si, 0x808032E4, ni, 0x80803F8B)?,
        object(source, &owner, sd, 0x808032E5, nd, 0x80803F8C)?,
        object(source, &owner, sd + 0x48, 0x808098CD, nd + 0x48, 0x80809ADE)?,
    ]);
    let unbound = [(si + 0x50, 0x80809A9E), (sd + 0xF8, 0x80809A9F)]
        .into_iter()
        .map(|(offset, class)| Object {
            owner: source.u32(si).expect("checked component owner"),
            class,
            offset: offset as u64,
        })
        .collect();
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
