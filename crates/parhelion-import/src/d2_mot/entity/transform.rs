//! Transform lookup owners with checked package and provider contracts.
use super::{
    links::{Graph, Interface, Object},
    shared::root,
};
use crate::d2_mot::{native::effects::controller::Relocation, payload::Payload};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

pub struct Native {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn interface(
    p: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    d: usize,
    offset: usize,
    class: u32,
    instance_class: u32,
    methods: &[u32],
    modern: bool,
) -> Result<Object> {
    let at = d + offset;
    ensure!(
        p.pointer(at)? == d && p.u64(at + 16)? == 0 && (!modern || p.u64(at + 24)? == 0),
        "transform provider state differs at {offset:X}"
    );
    let object = Object {
        owner: p.u32(d)?,
        class,
        offset: at as u64,
    };
    let contract = Interface::read(
        p,
        object,
        metadata
            .get(&p.u32(at + 8)?)
            .context("transform provider metadata absent")?,
        modern,
    )?;
    ensure!(
        contract.definition_offset == d
            && contract.instance_class == instance_class
            && contract.methods.len() == methods.len(),
        "transform provider interface differs"
    );
    for (entry, &index) in contract.methods.iter().zip(methods) {
        ensure!(
            entry.implementation_class == if modern { 0x80802AB4 } else { 0x808038A3 }
                && entry.index == index
                && entry.arguments == [0, 0],
            "transform provider method differs"
        );
    }
    Ok(object)
}

fn checked_array(
    p: &Payload,
    at: usize,
    class: u32,
    stride: usize,
    marker: u32,
) -> Result<Vec<usize>> {
    let rows = p.array(at, stride, Some(class))?;
    if rows.is_empty() {
        ensure!(p.u64(at + 8)? == 0, "empty transform array has a pointer");
    } else {
        let header = p.pointer(at + 8)?;
        ensure!(
            header >= 8
                && header % 16 == 0
                && p.u32(header - 8)? == 0
                && p.u32(header - 4)? == marker
                && p.u32(header + 12)? == 0,
            "transform array marker or alignment differs"
        );
    }
    Ok(rows)
}

fn zero(p: &Payload, from: usize, to: usize) -> Result<()> {
    ensure!(
        from <= to
            && p.0
                .get(from..to)
                .context("transform padding bounds")?
                .iter()
                .all(|&byte| byte == 0),
        "transform contains unclaimed or initialized bytes"
    );
    Ok(())
}

fn allocation(p: &Payload) -> Result<()> {
    ensure!(
        p.0.len() == 120
            && p.u64(0)? == 120
            && p.u64(8)? == 0x811C9DC5
            && p.u64(16)? == 0
            && p.u64(24)? == u64::from(u32::MAX),
        "transform allocation envelope differs"
    );
    let rows = checked_array(p, 32, 0x80808852, 40, 0x80809FBD)?;
    ensure!(
        rows == [80]
            && p.u64(80)? == 0x128A3E16
            && p.u64(88)? == 0
            && p.u32(96)? == 0x80800070
            && p.u32(100)? == 1
            && p.bytes::<16>(104)? == [0; 16],
        "transform runtime array allocation differs"
    );
    zero(p, 48, 56)
}

/// Convert a single transform input declaration and its name alternatives.
/// Authored transform children and an active modern-only boolean getter refuse.
/// Native lifecycle, allocation and default state come from a checked envelope.
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation_payload: &Payload,
    metadata: &BTreeMap<u32, Payload>,
    graph: &Graph,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Native> {
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid transform private tags"
    );
    allocation(allocation_payload)?;
    let (source_tag, si, sd) = root(source, 0x80802AB4, 0x80802AB8)?;
    let (template_tag, ni, nd) = root(template, 0x808038A3, 0x808038A1)?;
    ensure!(
        (si, sd, ni, nd) == (0x100, 0x178, 0xC0, 0x138) && template.0.len() == 624,
        "unsupported transform owner form"
    );
    ensure!(
        source.u64(sd + 16)? == 0
            && template.u64(nd + 16)? == 2
            && source.bytes::<48>(sd + 24)? == template.bytes::<48>(nd + 24)?,
        "unsupported transform base properties"
    );
    ensure!(
        source.bytes::<56>(si + 16)? == template.bytes::<56>(ni + 16)?
            && source.bytes::<16>(si + 0x48)?
                == [255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
            && template.bytes::<16>(ni + 0x48)? == [0; 16],
        "transform initial state differs"
    );
    ensure!(
        source.bytes::<32>(sd + 0xF8)? == [0; 32]
            && template.bytes::<32>(nd + 0xB8)? == [0; 32]
            && source.u64(sd + 0x118)? == 1
            && template.u64(nd + 0xD8)? == 1,
        "transform children require conversion"
    );
    ensure!(
        source.u64(8)? == 0xA0
            && template.u64(8)? == 0x60
            && source.u64(0x88)? == 0x74
            && source.u64(0x90)? == 0x54
            && template.u64(0x48)? == 0x78
            && template.u64(0x50)? == 0x58
            && source.u64(0x98)? == 0x170500100010
            && template.u64(0x58)? == source.u64(0x98)?
            && source.u32(0x80)? == u32::MAX
            && template.u32(0x40)? == u32::MAX
            && (0x80800001..=0x81FFFFFF).contains(&source.u32(0x84)?),
        "transform lifecycle envelope differs"
    );
    zero(source, 32, 64)?;
    zero(source, 80, 128)?;
    zero(template, 32, 48)?;
    ensure!(
        source.u64(0xA0)? == 0x80802AB200000000
            && template.u64(0x60)? == 0x808038A000000000
            && source.bytes::<16>(0xA8)? == [0; 16]
            && template.bytes::<16>(0x68)? == [0; 16]
            && source.u64(0xC8)? == 0x811C9DC5
            && template.u64(0x88)? == 0x811C9DC5
            && source.u64(0xD0)? == 0
            && template.u64(0x90)? == 0,
        "transform registration envelope differs"
    );
    let registrations = checked_array(source, 0xB8, 0x80802AB1, 8, 0x80809FB8)?;
    let native_registrations = checked_array(template, 0x78, 0x8080389F, 8, 0x80809FBD)?;
    ensure!(
        registrations == [0xF0]
            && native_registrations == [0xB0]
            && source.u32(0xF4)? == 0x811C9DC5
            && template.u32(0xB4)? == 0x811C9DC5,
        "transform input registration differs"
    );
    ensure!(
        source.u32(0xF8)? == 0 && template.u32(0xB8)? == 0,
        "transform record prefix padding differs"
    );
    let runtime = checked_array(source, si + 0x38, 0x80800070, 4, 0x80809FB8)?;
    let native_runtime = checked_array(template, ni + 0x38, 0x80800070, 4, 0x80809FBD)?;
    ensure!(
        runtime == [0x170]
            && native_runtime == [0x130]
            && source.u32(runtime[0])? == 0x811C9DC5
            && template.u32(native_runtime[0])? == 0x811C9DC5,
        "transform runtime input cache differs"
    );
    let rows = checked_array(source, sd + 0xE8, 0x80806CF6, 24, 0x80809FB8)?;
    let targets = checked_array(template, nd + 0xA8, 0x808038AA, 24, 0x80809FBD)?;
    ensure!(
        rows == [0x2B0] && targets == [0x230],
        "unsupported transform declaration count"
    );
    let row = rows[0];
    let name = source.u32(row + 4)?;
    ensure!(
        source.u32(row)? == 0
            && ![0, u32::MAX, 0x811C9DC5].contains(&name)
            && source.u32(0xF0)? == name
            && template.u32(0x230)? == 0
            && template.u32(0x234)? == template.u32(0xB0)?
            && template.bytes::<16>(0x238)? == [0; 16],
        "transform declaration name or index differs"
    );
    let alternatives = checked_array(source, row + 8, 0x80800070, 4, 0x80809FB8)?;
    ensure!(
        alternatives.len() <= 256,
        "transform name alternatives exceed capacity"
    );
    let mut names = BTreeSet::new();
    for &at in &alternatives {
        ensure!(
            names.insert(source.u32(at)?) && ![0, u32::MAX, 0x811C9DC5].contains(&source.u32(at)?),
            "invalid or duplicate transform name alternative"
        );
    }
    zero(source, sd + 0x120, row - 20)?;
    let mut end = row + 24;
    if let Some(&first) = alternatives.first() {
        zero(source, end, first - 24)?;
        end = *alternatives.last().context("transform alternatives")? + 4;
    }
    let life = checked_array(source, 64, 0x8080907C, 16, 0x80809FB8)?;
    let native_life = checked_array(template, 48, 0x808091A4, 16, 0x80809FBD)?;
    ensure!(
        life.len() == 1
            && native_life == [0x260]
            && source.pointer(life[0])? == si
            && source.u64(life[0] + 8)? == 0xFFFF000280802AB4
            && template.pointer(0x260)? == ni
            && template.u64(0x268)? == 0xFFFF0002808038A3
            && source.0.len() == life[0] + 16,
        "transform lifecycle parameters differ"
    );
    zero(source, end, life[0] - 24)?;
    let mut objects = vec![];
    for (so, no, sc, nc) in [
        (si, ni, 0x80802AB4, 0x808038A3),
        (sd, nd, 0x80802AB8, 0x808038A1),
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
    let mut channels = BTreeMap::new();
    for (so, no, sc, sic, nc, nic, methods) in [
        (
            0x48,
            0x48,
            0x80802AB6,
            0x80802AB5,
            0x808038A5,
            0x808038A4,
            &[0, 1][..],
        ),
        (
            0x68,
            0x60,
            0x8080979E,
            0x8080979D,
            0x80809684,
            0x80809683,
            &[3, 4, 5][..],
        ),
        (
            0x88,
            0x78,
            0x80808466,
            0x80808465,
            0x80808861,
            0x80808860,
            &[6][..],
        ),
        (
            0xA8,
            0x90,
            0x808091A8,
            0x808091A7,
            0x80809494,
            0x80809493,
            &[7][..],
        ),
    ] {
        let from = interface(source, metadata, sd, so, sc, sic, methods, true)?;
        let mut to = interface(template, metadata, nd, no, nc, nic, methods, false)?;
        to.owner = owner_tag;
        channels.insert(from, methods.len() as u32);
        objects.push(Relocation {
            source: from,
            target: to,
        });
    }
    interface(
        source,
        metadata,
        sd,
        0xC8,
        0x808098C9,
        0x808098C8,
        &[8],
        true,
    )?;
    ensure!(
        graph
            .components
            .iter()
            .filter(|&&tag| tag == source_tag)
            .count()
            == 1,
        "transform source absent or repeated in entity graph"
    );
    for object in graph.objects().filter(|object| object.owner == source_tag) {
        ensure!(
            objects.iter().any(|entry| entry.source == object),
            "unsupported transform graph interface {:08X} at {:X}",
            object.class,
            object.offset
        );
    }
    for edge in graph.connections.iter().chain(&graph.named_connections) {
        if let Some(object) = edge.provider.object
            && let Some(&count) = channels.get(&object)
        {
            ensure!(
                edge.channel < count,
                "transform provider channel exceeds method count"
            );
        }
    }
    let mut owner = Payload(template.0[..0x248].to_vec());
    owner.0[0xB0..0xB4].copy_from_slice(&name.to_le_bytes());
    owner.0[0x234..0x238].copy_from_slice(&name.to_le_bytes());
    if !alternatives.is_empty() {
        owner
            .0
            .extend_from_slice(&0x80809FBD00000000u64.to_le_bytes());
        owner
            .0
            .extend_from_slice(&(alternatives.len() as u64).to_le_bytes());
        owner.0.extend_from_slice(&0x80800070u64.to_le_bytes());
        owner.0[0x238..0x240].copy_from_slice(&(alternatives.len() as u64).to_le_bytes());
        owner.0[0x240..0x248].copy_from_slice(&16i64.to_le_bytes());
        for &at in &alternatives {
            owner.0.extend_from_slice(&source.bytes::<4>(at)?);
        }
    }
    while !(owner.0.len() + 8).is_multiple_of(16) {
        owner.0.push(0);
    }
    owner
        .0
        .extend_from_slice(&0x80809FBD00000000u64.to_le_bytes());
    let life_header = owner.0.len();
    owner.0.extend_from_slice(&1u64.to_le_bytes());
    owner.0.extend_from_slice(&0x808091A4u64.to_le_bytes());
    owner
        .0
        .extend_from_slice(&(ni as i64 - (life_header + 16) as i64).to_le_bytes());
    owner
        .0
        .extend_from_slice(&0xFFFF0002808038A3u64.to_le_bytes());
    owner.0[0x38..0x40].copy_from_slice(&(life_header as i64 - 0x38).to_le_bytes());
    let occurrences = (0..template.0.len() - 3)
        .step_by(4)
        .filter(|&at| template.u32(at).ok() == Some(template_tag))
        .collect::<Vec<_>>();
    ensure!(
        occurrences == [ni, nd],
        "native transform template has unrelated owner references"
    );
    for at in [ni, nd] {
        owner.0[at..at + 4].copy_from_slice(&owner_tag.to_le_bytes());
    }
    owner.0[0x44..0x48].copy_from_slice(&allocation_tag.to_le_bytes());
    let size = owner.0.len() as u64;
    owner.0[..8].copy_from_slice(&size.to_le_bytes());
    Ok(Native {
        owner,
        allocation: allocation_payload.clone(),
        objects,
    })
}
