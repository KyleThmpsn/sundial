//! Projectile responses with explicit dependencies and unrepresented categories.
use super::{Relocation, put};
use crate::d2_mot::{
    entity::links::Object, native::effects::resources::Reference, payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

mod read;
mod write;
use crate::d2_mot::native::effects::resources::selectors::{self, GroupBindings};
use read::Read;
use write::Write;

#[cfg(test)]
mod tests;

pub struct Resources {
    pub tags: BTreeMap<u32, u32>,
    pub hashes: BTreeMap<u64, u32>,
    pub categories: Vec<Option<u16>>,
    pub names: Vec<u32>,
}

#[derive(Serialize)]
pub enum Gate {
    Selector {
        source: usize,
    },
    Predicate {
        offset: usize,
        index: usize,
        name: u32,
    },
    Mask {
        index: usize,
        name: u32,
    },
    Index {
        index: usize,
        name: u32,
    },
    Secondary {
        row: [u8; 32],
        dictionary: u32,
        mapped_dictionary: Option<u32>,
    },
}

pub struct Response {
    pub owner: Payload,
    pub allocation: Payload,
    pub objects: Vec<Relocation>,
    pub references: Vec<Reference>,
    pub reference_classes: BTreeMap<usize, u32>,
    pub named_conditions: Vec<u32>,
    pub group_aliases: Vec<selectors::GroupAliasUse>,
    /// These source fields need a native representation before entity assembly.
    pub gates: Vec<Gate>,
}

/// Emit the supported response layouts. The caller must bind every dependency
/// and represent every returned gate before treating the owner as complete.
pub fn emit(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    resources: &Resources,
    owner_tag: u32,
    allocation_tag: u32,
) -> Result<Response> {
    emit_checked(
        source,
        template,
        allocation,
        resources,
        owner_tag,
        allocation_tag,
        None,
        None,
    )
}

pub fn emit_with_groups(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    resources: &Resources,
    owner_tag: u32,
    allocation_tag: u32,
    groups: &GroupBindings,
) -> Result<Response> {
    emit_checked(
        source,
        template,
        allocation,
        resources,
        owner_tag,
        allocation_tag,
        Some(groups),
        None,
    )
}

/// Provide typed external selector dependencies for collision value selectors.
#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
pub fn emit_with_bindings(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    resources: &Resources,
    owner_tag: u32,
    allocation_tag: u32,
    groups: &GroupBindings,
    resolver: &selectors::ResourceResolver,
) -> Result<Response> {
    ensure!(
        resolver
            .hashes
            .iter()
            .all(|(hash, tag)| resources.hashes.get(hash) == Some(tag)),
        "response and selector source hash correspondence differs"
    );
    emit_checked(
        source,
        template,
        allocation,
        resources,
        owner_tag,
        allocation_tag,
        Some(groups),
        Some(resolver),
    )
}

fn category_gates(
    source: &Payload,
    resources: &Resources,
    read: &mut Read<'_>,
    write: &mut Write,
    sd: usize,
    nd: usize,
) -> Result<Vec<Gate>> {
    let mut gates = Vec::new();
    for bit in 0..448 {
        if source.u8(sd + 0x24 + bit / 8)? & (1 << (bit % 8)) == 0 {
            continue;
        }
        let name = *resources
            .names
            .get(bit)
            .context("response mask category outside dictionary")?;
        if let Some(index) = resources.categories[bit] {
            let index = usize::from(index);
            ensure!(index < 320, "response native category outside mask");
            write.owner.0[nd + 0x24 + index / 8] |= 1 << (index % 8);
        } else {
            gates.push(Gate::Mask { index: bit, name });
        }
    }
    ensure!(
        source.u32(sd + 0x5C)? == 0 && source.u32(sd + 0x7C)? == 0,
        "response category padding differs"
    );
    for row in read.array(sd + 0x60, 0x80809784, 4)? {
        let index = usize::try_from(source.u32(row)?)?;
        let name = *resources
            .names
            .get(index)
            .context("response index category outside dictionary")?;
        gates.push(Gate::Index { index, name });
    }
    Ok(gates)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Native conversion keeps independent payload, identity and dispatch contracts explicit"
)]
fn emit_checked(
    source: &Payload,
    template: &Payload,
    allocation: &Payload,
    resources: &Resources,
    owner_tag: u32,
    allocation_tag: u32,
    groups: Option<&GroupBindings>,
    resolver: Option<&selectors::ResourceResolver>,
) -> Result<Response> {
    for p in [source, template, allocation] {
        ensure!(
            p.u64(0)? == p.0.len() as u64,
            "response payload size differs"
        );
    }
    ensure!(
        (0x80800001..=0x81FFFFFF).contains(&owner_tag)
            && (0x80800001..=0x81FFFFFF).contains(&allocation_tag)
            && owner_tag != allocation_tag,
        "invalid response output tags"
    );
    ensure!(
        resources.names.len() == resources.categories.len() && resources.names.len() <= 448,
        "response category correspondence differs"
    );
    let si = source.pointer(16)?;
    let sd = source.pointer(24)?;
    let ti = template.pointer(16)?;
    let td = template.pointer(24)?;
    ensure!(
        si >= 4
            && sd >= 4
            && ti >= 4
            && td >= 4
            && si % 16 == 0
            && sd % 16 == 0
            && ti % 16 == 0
            && td % 16 == 0,
        "response root alignment differs"
    );
    let source_tag = source.u32(si)?;
    for (p, i, d, ic, dc) in [
        (source, si, sd, 0x80803F6F, 0x80803F70),
        (template, ti, td, 0x80804B01, 0x80804B02),
    ] {
        ensure!(
            p.u32(i - 4)? == ic
                && p.u32(d - 4)? == dc
                && p.u32(i + 4)? == dc
                && p.u32(d + 4)? == ic
                && p.u32(i)? == p.u32(d)?
                && p.u32(i)? != u32::MAX
                && p.u64(i + 8)? == d as u64
                && p.u64(d + 8)? == i as u64
                && p.bytes::<16>(i + 16)? == [0; 16],
            "response reciprocal records or state differ"
        );
    }
    ensure!(
        source.u64(8)? == 0 && template.u64(8)? == 0,
        "unsupported response constructor block"
    );
    for field in [0x20, 0x30, 0x40, 0x50, 0x70] {
        ensure!(
            source.bytes::<16>(field)? == [0; 16],
            "unsupported response header array"
        );
    }
    ensure!(
        allocation.0.len() == 48
            && allocation.u64(8)? == 0x811C9DC5
            && allocation.u64(16)? == 0
            && allocation.u64(24)? == u64::from(u32::MAX)
            && allocation.bytes::<16>(32)? == [0; 16],
        "response allocation is not empty"
    );
    let mut read = Read::new(source, resources);
    read.groups = groups;
    read.resolver = resolver;
    read.claim(0, si - 4)?;
    read.claim(si - 4, 36)?;
    read.claim(sd - 4, 308)?;
    let mut write = Write {
        owner: Payload(
            template
                .0
                .get(..96)
                .context("response native envelope")?
                .to_vec(),
        ),
        references: Vec::new(),
        gates: Vec::new(),
        reference_classes: BTreeMap::new(),
        named_conditions: Vec::new(),
        group_aliases: Vec::new(),
    };
    let ni = write.prefix(0x80804B01, 16)?;
    write.reserve(32)?;
    let nd = write.prefix(0x80804B02, 16)?;
    write.reserve(272)?;
    for (at, class, twin) in [(ni, 0x80804B02u32, nd), (nd, 0x80804B01, ni)] {
        put(&mut write.owner.0, at, &owner_tag.to_le_bytes())?;
        put(&mut write.owner.0, at + 4, &class.to_le_bytes())?;
        put(&mut write.owner.0, at + 8, &(twin as u64).to_le_bytes())?;
    }
    write.relative(16, ni)?;
    write.relative(24, nd)?;
    put(&mut write.owner.0, 0x44, &allocation_tag.to_le_bytes())?;
    write.copy(&read, sd + 16, nd + 16, 20)?;
    let mut gates = category_gates(source, resources, &mut read, &mut write, sd, nd)?;
    write.copy(&read, sd + 0x70, nd + 0x4C, 12)?;
    if source.u64(sd + 0x80)? != 0 {
        let record = write.record(&mut read, source.pointer(sd + 0x80)?)?;
        write.relative(nd + 0x58, record)?;
    }
    let rows = read.array(sd + 0x88, 0x80809787, 32)?;
    let start = write.array(nd + 0x60, 0x808094B3, rows.len(), 24)?;
    for (index, row) in rows.into_iter().enumerate() {
        let at = start + index * 24;
        write.copy(&read, row, at, 16)?;
        write.reference(&mut read, row + 16, at + 16)?;
    }
    for row in read.array(sd + 0x98, 0x80809787, 32)? {
        let dictionary = read.reference(row + 16)?;
        gates.push(Gate::Secondary {
            row: source.bytes::<32>(row)?,
            dictionary,
            mapped_dictionary: resources.tags.get(&dictionary).copied(),
        });
    }
    write.copy(&read, sd + 0xA8, nd + 0x70, 8)?;
    let modifiers = read.array(sd + 0xB0, 0x80803FAD, 8)?;
    let start = write.array(nd + 0x78, 0x80804B3D, modifiers.len(), 8)?;
    for (index, row) in modifiers.into_iter().enumerate() {
        let record = write.record(&mut read, source.pointer(row)?)?;
        write.relative(start + index * 8, record)?;
    }
    ensure!(
        source.u64(sd + 0xC0)? == 0x100,
        "response selector flags differ"
    );
    let conditions = read.array(sd + 0xC8, 0x808091B7, 16)?;
    let start = write.array(nd + 0x90, 0x80809316, conditions.len(), 16)?;
    for (index, row) in conditions.into_iter().enumerate() {
        let condition = source.pointer(row + 8)?;
        ensure!(
            source.u64(row)? == 0
                && condition >= 4
                && matches!(source.u32(condition - 4)?, 0x808042CE | 0x808042CB),
            "unsupported response selector predicate"
        );
        let condition = write.record(&mut read, condition)?;
        write.relative(start + index * 16 + 8, condition)?;
    }
    write.copy(&read, sd + 0xD8, nd + 0xA0, 8)?;
    let defaults = read.array(sd + 0xE0, 0x8080211E, 24)?;
    ensure!(
        defaults.len() <= 1,
        "response selector default count differs"
    );
    if let Some(&row) = defaults.first() {
        ensure!(
            source.u64(row)? == 0,
            "response selector default kind differs"
        );
        write.reference(&mut read, row + 8, nd + 0xB0)?;
    }
    let modifiers = read.array(sd + 0xF0, 0x80803F7B, 8)?;
    let start = write.array(nd + 0xB8, 0x80804B0D, modifiers.len(), 8)?;
    for (index, row) in modifiers.into_iter().enumerate() {
        let record = write.record(&mut read, source.pointer(row)?)?;
        write.relative(start + index * 8, record)?;
    }
    write.copy(&read, sd + 0x100, nd + 0xD0, 16)?;
    let rows = read.array(sd + 0x110, 0x80803F73, 12)?;
    let start = write.array(nd + 0xF0, 0x80804B05, rows.len(), 12)?;
    for (index, row) in rows.into_iter().enumerate() {
        write.copy(&read, row, start + index * 12, 12)?;
    }
    write.copy(&read, sd + 0x120, nd + 0x100, 4)?;
    ensure!(
        source.bytes::<12>(sd + 0x124)? == [0; 12],
        "response root padding differs"
    );
    read.finish()?;
    gates.extend(write.gates);
    let len = write.owner.0.len() as u64;
    put(&mut write.owner.0, 0, &len.to_le_bytes())?;
    let objects = [(si, ni), (sd, nd)]
        .into_iter()
        .map(|(from, to)| {
            Ok(Relocation {
                source: Object {
                    owner: source_tag,
                    class: source.u32(from + 4)?,
                    offset: from as u64,
                },
                target: Object {
                    owner: owner_tag,
                    class: write.owner.u32(to + 4)?,
                    offset: to as u64,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Response {
        owner: write.owner,
        allocation: allocation.clone(),
        objects,
        references: write.references,
        reference_classes: write.reference_classes,
        named_conditions: write.named_conditions,
        group_aliases: write.group_aliases,
        gates,
    })
}
