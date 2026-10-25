//! Spatial getters hosted by checked native lifecycle and dispatch metadata.
use super::{
    links::{Graph, Object},
    shared::{provider, root},
};
use crate::d2_mot::{native::effects::controller::Relocation, payload::Payload};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub struct Native {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
}

/// Convert the empty spatial component form. The native implementation owns
/// initial state and metadata. Unsupported getters must have neither typed
/// entity edges nor dynamic named consumers in the caller's complete closure.
#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    graph: &Graph,
    requested_names: &BTreeSet<u32>,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Native> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid spatial private tags"
    );
    let (source_tag, si, sd) = root(source, 0x80802938, 0x80802939)?;
    let (template_tag, ni, nd) = root(template, 0x8080375F, 0x80803760)?;
    ensure!(
        source.0.len() == sd + 0x1B8 && template.0.len() == nd + 0x130,
        "spatial owner has extra or truncated records"
    );
    source.bytes::<96>(si)?;
    template.bytes::<80>(ni)?;
    ensure!(
        source.bytes::<56>(sd + 16)? == template.bytes::<56>(nd + 16)?
            && source.bytes::<64>(si + 16)? == template.bytes::<64>(ni + 16)?,
        "spatial base properties or initial state differ"
    );
    ensure!(
        source.u32(si + 0x50)? == 0x100 && source.bytes::<12>(si + 0x54)? == [0; 12],
        "unsupported spatial initial state extension"
    );
    ensure!(
        source.bytes::<24>(sd + 0x1A0)? == [0; 24]
            && template.bytes::<24>(nd + 0x118)? == [0; 24]
            && template.bytes::<16>(ni + 0x30)? == [0; 16],
        "spatial component children require conversion"
    );
    ensure!(
        graph
            .components
            .iter()
            .filter(|&&tag| tag == source_tag)
            .count()
            == 1,
        "spatial source absent or repeated in entity graph"
    );
    ensure!(
        allocation.0.len() == 48
            && allocation.u64(0)? == 48
            && allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u64::from(u32::MAX)
            && allocation.bytes::<16>(32)? == [0; 16],
        "spatial allocation shape differs"
    );
    let mut owner = template.clone();
    let mut objects = Vec::new();
    for (sc, so, nc, no) in [
        (0x80802938, si, 0x8080375F, ni),
        (0x80802939, sd, 0x80803760, nd),
    ] {
        objects.push(Relocation {
            source: Object {
                owner: source_tag,
                class: sc,
                offset: so as u64,
            },
            target: Object {
                owner: owner_tag,
                class: nc,
                offset: no as u64,
            },
        });
    }
    for (so, no, sc, sic, nc, nic, method) in [
        (
            0x48, 0x48, 0x808098CD, 0x808098CC, 0x80809ADE, 0x80809ADD, 0,
        ),
        (
            0x68, 0x60, 0x808095C6, 0x808095C5, 0x808097BB, 0x808097BA, 1,
        ),
    ] {
        let source = provider(
            source, metadata, sd, so, None, sc, sic, 0x80802938, method, true,
        )?;
        let mut target = provider(
            template, metadata, nd, no, None, nc, nic, 0x8080375F, method, false,
        )?;
        target.owner = owner_tag;
        objects.push(Relocation { source, target });
    }
    let mut names = BTreeSet::new();
    for (so, method) in [
        (0x88, 2),
        (0xB0, 3),
        (0xD8, 4),
        (0x100, 5),
        (0x128, 6),
        (0x150, 7),
        (0x178, 8),
    ] {
        let name = source.u32(sd + so + 32)?;
        ensure!(
            name != 0 && name != 0x811C9DC5 && names.insert(name),
            "invalid or duplicate spatial getter name"
        );
        provider(
            source,
            metadata,
            sd,
            so,
            Some(name),
            0x808098D2,
            0x808098D3,
            0x80802938,
            method,
            true,
        )?;
        if matches!(method, 6 | 8) {
            ensure!(
                !requested_names.contains(&name),
                "unsupported spatial getter requested by name {name:08X}"
            );
        }
    }
    for (so, no, sm, nm) in [
        (0x88, 0x78, 2, 2),
        (0xB0, 0x98, 3, 3),
        (0xD8, 0xB8, 4, 4),
        (0x100, 0xD8, 5, 5),
        (0x150, 0xF8, 7, 6),
    ] {
        let name = source.u32(sd + so + 32)?;
        let source_object = provider(
            source,
            metadata,
            sd,
            so,
            Some(name),
            0x808098D2,
            0x808098D3,
            0x80802938,
            sm,
            true,
        )?;
        let mut target = provider(
            template, metadata, nd, no, None, 0x80809AE1, 0x80809AE2, 0x8080375F, nm, false,
        )?;
        ensure!(
            template.u64(nd + no + 24)? <= u64::from(u32::MAX),
            "native spatial getter name padding differs"
        );
        owner.0[nd + no + 24..nd + no + 32].copy_from_slice(&u64::from(name).to_le_bytes());
        target.owner = owner_tag;
        objects.push(Relocation {
            source: source_object,
            target,
        });
    }
    for object in graph.objects().filter(|object| object.owner == source_tag) {
        ensure!(
            objects.iter().any(|mapping| mapping.source == object),
            "untranslated spatial graph interface {:08X} at {:X}",
            object.class,
            object.offset
        );
    }
    let occurrences = (0..template.0.len().saturating_sub(3))
        .step_by(4)
        .filter(|&at| template.u32(at).ok() == Some(template_tag))
        .collect::<Vec<_>>();
    ensure!(
        occurrences == [ni, nd],
        "native spatial template has unrelated owner references"
    );
    for at in occurrences {
        owner.0[at..at + 4].copy_from_slice(&owner_tag.to_le_bytes());
    }
    owner.0[0x44..0x48].copy_from_slice(&allocation_tag.to_le_bytes());
    Ok(Native {
        owner,
        allocation: allocation.clone(),
        objects,
    })
}
