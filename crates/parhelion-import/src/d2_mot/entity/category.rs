//! Immutable category definitions using validated native provider dispatch.
use super::{
    links::{Graph, Interface, Object},
    shared::root,
};
use crate::d2_mot::{
    native::{categories::Namespace, effects::controller::Relocation},
    payload::Payload,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

pub struct Native {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    pub channels: BTreeMap<(Object, u32), u32>,
}

fn empty_allocation(allocation: &Payload) -> Result<()> {
    ensure!(
        allocation.0.len() == 48
            && allocation.u64(0)? == 48
            && allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u64::from(u32::MAX)
            && allocation.bytes::<16>(32)? == [0; 16],
        "category allocation shape differs"
    );
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn interface(
    owner: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    definition: usize,
    offset: usize,
    class: u32,
    instance: u32,
    implementation: u32,
    methods: &[u32],
    state: u64,
    modern: bool,
) -> Result<Object> {
    let at = definition + offset;
    ensure!(
        owner.pointer(at)? == definition
            && owner.u64(at + 16)? == state
            && (!modern || owner.u64(at + 24)? == 0),
        "category provider state differs"
    );
    let object = Object {
        owner: owner.u32(definition)?,
        class,
        offset: at as u64,
    };
    let meta = metadata
        .get(&owner.u32(at + 8)?)
        .context("category provider metadata absent")?;
    let actual = Interface::read(owner, object, meta, modern)?;
    ensure!(
        actual.definition_offset == definition
            && actual.instance_class == instance
            && actual.methods.len() == methods.len(),
        "category provider shape differs"
    );
    for (actual, &index) in actual.methods.iter().zip(methods) {
        ensure!(
            actual.implementation_class == implementation
                && actual.index == index
                && actual.arguments == [0, 0],
            "category provider dispatch differs"
        );
    }
    Ok(object)
}

/// Convert the empty-input category owner with immutable definition bits.
/// Unsupported source provider channels stay absent and block graph lowering.
/// The caller must include every typed consumer in the supplied graph and bind
/// the category namespace separately in all name-based query dependencies.
#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
pub fn emit(
    source: &Payload,
    source_allocation: &Payload,
    template: &Payload,
    allocation: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    graph: &Graph,
    namespace: &Namespace,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Native> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid category private tags"
    );
    let (source_tag, si, sd) = root(source, 0x80809779, 0x80809775)?;
    let (template_tag, ni, nd) = root(template, 0x808094A4, 0x808094A0)?;
    ensure!(
        (si, sd, source.0.len()) == (192, 360, 672)
            && (ni, nd, template.0.len()) == (128, 264, 528),
        "category owner has extra or truncated records"
    );
    ensure!(
        source.u64(8)? == 160
            && template.u64(8)? == 96
            && source.bytes::<32>(32)? == [0; 32]
            && source.bytes::<48>(80)? == [0; 48]
            && template.bytes::<16>(32)? == [0; 16]
            && source.u32(128)? == u32::MAX
            && template.u32(64)? == u32::MAX
            && source.u64(136)? == 164
            && template.u64(72)? == 136
            && source.u64(144)? == 20
            && template.u64(80)? == 24
            && source.u64(152)? == 0x170500080010
            && template.u64(88)? == source.u64(152)?
            && source.u32(160)? == 0
            && source.u32(164)? == 0x80809AF3
            && source.bytes::<20>(168)? == [0; 20]
            && template.u32(96)? == 0
            && template.u32(100)? == 0x80809C28
            && template.bytes::<20>(104)? == [0; 20],
        "category root lifecycle envelope differs"
    );
    for (owner, field, header, class, instance, marker) in [
        (source, 64, 640, 0x8080907Cu32, si, 0x80809FB8),
        (template, 48, 496, 0x808091A4u32, ni, 0x80809FBD),
    ] {
        ensure!(
            owner.u64(field)? == 1
                && owner.pointer(field + 8)? == header
                && owner.u32(header - 4)? == marker
                && owner.u64(header)? == 1
                && owner.u64(header + 8)? == u64::from(class)
                && owner.pointer(header + 16)? == instance
                && owner.u32(header + 24)? == owner.u32(instance - 4)?
                && owner.u32(header + 28)? == 0x00030002,
            "category instance allocation row differs"
        );
    }
    empty_allocation(source_allocation)?;
    empty_allocation(allocation)?;
    ensure!(
        source.bytes::<32>(si + 16)? == template.bytes::<32>(ni + 16)?
            && source.bytes::<56>(sd + 16)? == template.bytes::<56>(nd + 16)?,
        "category base properties differ"
    );
    ensure!(
        source.bytes::<112>(si + 48)? == [0; 112] && template.bytes::<80>(ni + 48)? == [0; 80],
        "initialized category runtime masks require conversion"
    );
    ensure!(
        source.bytes::<16>(sd + 0xA0)? == [0; 16] && template.bytes::<16>(nd + 0x88)? == [0; 16],
        "category definition inputs require conversion"
    );
    ensure!(
        source.bytes::<4>(sd + 0x110)? == [0; 4] && template.bytes::<4>(nd + 0xE0)? == [0; 4],
        "category trailing padding differs"
    );
    ensure!(
        graph
            .components
            .iter()
            .filter(|&&tag| tag == source_tag)
            .count()
            == 1,
        "category source absent or repeated in entity graph"
    );
    let mut owner = template.clone();
    let mut objects = Vec::new();
    let mut channels = BTreeMap::new();
    for (source_class, source_offset, native_class, native_offset) in [
        (0x80809779, si, 0x808094A4, ni),
        (0x80809775, sd, 0x808094A0, nd),
    ] {
        objects.push(Relocation {
            source: Object {
                owner: source_tag,
                class: source_class,
                offset: source_offset as u64,
            },
            target: Object {
                owner: owner_tag,
                class: native_class,
                offset: native_offset as u64,
            },
        });
    }
    for (so, no, sc, sic, nc, nic, sm, nm, state) in [
        (
            0x48,
            0x48,
            0x8080977B,
            0x8080977A,
            0x808094A6,
            0x808094A5,
            &[0, 1][..],
            &[0, 1][..],
            0,
        ),
        (
            0xB0,
            0x98,
            0x8080979B,
            0x8080979A,
            0x80809682,
            0x80809681,
            &[14][..],
            &[11][..],
            0,
        ),
        (
            0xD0,
            0xB0,
            0x8080977F,
            0x8080977E,
            0x808094AA,
            0x808094A9,
            &[5, 6, 7, 8, 9, 10, 11, 12, 13, 15][..],
            &[5, 6, 7, 8, 9, 10, 12][..],
            0,
        ),
        (
            0xF0,
            0xC8,
            0x808098CD,
            0x808098CC,
            0x80809ADE,
            0x80809ADD,
            &[4][..],
            &[4][..],
            13,
        ),
    ] {
        let source_object = interface(
            source, metadata, sd, so, sc, sic, 0x80809779, sm, state, true,
        )?;
        let mut target = interface(
            template, metadata, nd, no, nc, nic, 0x808094A4, nm, state, false,
        )?;
        target.owner = owner_tag;
        objects.push(Relocation {
            source: source_object,
            target,
        });
        for (channel, &method) in sm.iter().enumerate() {
            let mapped = match (sc, method) {
                (0x8080977F, 11..=13) => None,
                (0x8080977F, 15) => Some(12),
                (0x8080979B, 14) => Some(11),
                _ => Some(method),
            };
            if let Some(mapped) = mapped {
                let native_channel = nm
                    .iter()
                    .position(|&method| method == mapped)
                    .context("category native method correspondence absent")?;
                channels.insert((source_object, channel as u32), native_channel as u32);
            }
        }
    }
    for object in graph.objects().filter(|object| object.owner == source_tag) {
        ensure!(
            objects.iter().any(|mapping| mapping.source == object),
            "untranslated category graph endpoint"
        );
    }
    for edge in graph.connections.iter().chain(&graph.named_connections) {
        if let Some(provider) = edge
            .provider
            .object
            .filter(|object| object.owner == source_tag)
        {
            ensure!(
                channels.contains_key(&(provider, edge.channel)),
                "untranslated category provider channel {}",
                edge.channel
            );
        }
    }
    let mask = namespace.definition_mask(&source.bytes::<56>(sd + 0x68)?)?;
    owner.0[nd + 0x60..nd + 0x88].copy_from_slice(&mask);
    let references = (0..template.0.len().saturating_sub(3))
        .step_by(4)
        .filter(|&at| template.u32(at).ok() == Some(template_tag))
        .collect::<Vec<_>>();
    ensure!(
        references == [ni, nd],
        "category template has unrelated owner references"
    );
    for at in references {
        owner.0[at..at + 4].copy_from_slice(&owner_tag.to_le_bytes());
    }
    owner.0[0x44..0x48].copy_from_slice(&allocation_tag.to_le_bytes());
    Ok(Native {
        owner,
        allocation: allocation.clone(),
        objects,
        channels,
    })
}
