//! Health outputs retain their selected region, channel and numeric transform.
use super::*;
use crate::d2_mot::entity::links::Interface;
use std::collections::BTreeMap;

pub(super) struct Providers {
    pub count: usize,
    pub objects: Vec<Relocation>,
}

fn interface(
    owner: &Payload,
    definition: usize,
    metadata: &BTreeMap<u32, Payload>,
    modern: bool,
) -> Result<()> {
    let class = if modern { 0x808098D2 } else { 0x80809AE1 };
    let at = definition + 16;
    let contract = Interface::read(
        owner,
        Object {
            owner: owner.u32(definition)?,
            class,
            offset: at as u64,
        },
        metadata
            .get(&owner.u32(at + 8)?)
            .context("health output metadata missing")?,
        modern,
    )?;
    ensure!(
        contract.definition_offset == definition
            && contract.instance_class == class + 1
            && contract.methods.len() == 1
            && contract.methods[0].implementation_class
                == if modern { 0x80804003 } else { 0x80804B91 }
            && contract.methods[0].index == 0
            && contract.methods[0].arguments == [0, 0],
        "health output dispatch differs"
    );
    let size = if modern { 16 } else { 8 };
    ensure!(
        owner
            .0
            .get(at + 16..at + 16 + size)
            .context("health output state")?
            .iter()
            .all(|&v| v == 0),
        "health output interface state is initialized"
    );
    Ok(())
}

pub(super) fn emit(
    source: &Payload,
    output: &mut Payload,
    metadata: &BTreeMap<u32, Payload>,
    regions: usize,
) -> Result<Providers> {
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ni = output.pointer(16)?;
    let nd = output.pointer(24)?;
    let definitions = source.array(sd + 0x440, 64, Some(0x80804004))?;
    let instances = source.array(si + 0x4D8, 32, Some(0x80804003))?;
    ensure!(
        definitions.len() == instances.len(),
        "health output pair count differs"
    );
    let native_definitions = output.array(nd + 0x398, 56, Some(0x80804B92))?;
    let native_instances = output.array(ni + 0x368, 32, Some(0x80804B91))?;
    ensure!(
        native_definitions.len() == native_instances.len(),
        "native health output pair count differs"
    );
    let template = *native_definitions
        .first()
        .context("native health output envelope absent")?;
    interface(output, template, metadata, false)?;
    let metadata_tag = output.u32(template + 24)?;
    let source_owner = source.u32(si)?;
    for (&d, &i) in definitions.iter().zip(&instances) {
        ensure!(
            source.u32(d)? == source_owner
                && source.u32(i)? == source_owner
                && source.u32(d + 4)? == 0x80804003
                && source.u32(i + 4)? == 0x80804004
                && source.u64(d + 8)? == i as u64
                && source.u64(i + 8)? == d as u64
                && source.pointer(i + 16)? == si
                && source.u64(i + 24)? == 0,
            "health output pair or runtime parent differs"
        );
        interface(source, d, metadata, true)?;
        let kind = source.u8(d + 60)?;
        region::provider_kind(kind)?;
        ensure!(
            source.u32(d + 52)? == 1
                && source.u8(d + 61)? & !7 == 0
                && source.u8(d + 62)? == 4
                && source.u8(d + 63)? == 0,
            "health output transform or extension differs"
        );
        let region = source.u32(d + 56)?;
        ensure!(
            if kind == 5 {
                region == u32::MAX
            } else {
                (region as usize) < regions
            },
            "health output region is outside the translated region array"
        );
    }
    let mut candidate = output.clone();
    append_array(
        &mut candidate.0,
        nd + 0x398,
        0x80804B92,
        &vec![0; definitions.len() * 56],
        56,
    )?;
    append_array(
        &mut candidate.0,
        ni + 0x368,
        0x80804B91,
        &vec![0; definitions.len() * 32],
        32,
    )?;
    let targets = candidate.array(nd + 0x398, 56, Some(0x80804B92))?;
    let runtimes = candidate.array(ni + 0x368, 32, Some(0x80804B91))?;
    let mut objects = Vec::new();
    for (((&from, &source_i), &to), &native_i) in definitions
        .iter()
        .zip(&instances)
        .zip(&targets)
        .zip(&runtimes)
    {
        pair(&mut candidate, native_i, to, 0x80804B91, 0x80804B92, ni)?;
        relative(&mut candidate, to + 16, to)?;
        put(&mut candidate.0, to + 24, &metadata_tag.to_le_bytes())?;
        put(&mut candidate.0, to + 40, &source.bytes::<12>(from + 48)?)?;
        put(
            &mut candidate.0,
            to + 52,
            &[
                region::provider_kind(source.u8(from + 60)?)?,
                source.u8(from + 61)?,
            ],
        )?;
        for (s, sc, n, nc) in [
            (source_i, 0x80804003, native_i, 0x80804B91),
            (from, 0x80804004, to, 0x80804B92),
            (from + 16, 0x808098D2, to + 16, 0x80809AE1),
        ] {
            objects.push(object(source, &candidate, s, sc, n, nc)?);
        }
    }
    *output = candidate;
    Ok(Providers {
        count: definitions.len(),
        objects,
    })
}
