//! Grouped effect resources, modern class 8080873F to native class 80808BCD.
//!
//! The three branches retain their group ids, name hashes and flags. Leaf arrays
//! shrink from 32-byte modern references to native 32-bit tags. Their resources
//! include sound cues and effect entities. Caller-owned mappings must name
//! converted dependencies. Missing mappings produce unset slots and explicit
//! dependency records for assembly.
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

pub mod programs;
pub mod selectors;

#[cfg(test)]
mod tests;

pub const MODERN_CLASS: u32 = 0x8080873F;
pub const NATIVE_CLASS: u32 = 0x80808BCD;

#[derive(Serialize)]
pub struct Reference {
    pub offset: usize,
    pub source: u32,
    pub target: Option<u32>,
}

/// A resource identity and its byte offset in the checked Source table.
/// Package metadata can classify these dependencies before Native emission.
#[derive(Clone, Copy, Serialize)]
pub struct SourceReference {
    pub offset: usize,
    pub source: u32,
}

pub struct Table {
    pub payload: Payload,
    /// Every set source resource, including targets that still need conversion.
    pub references: Vec<Reference>,
}

struct Row {
    name: [u8; 8],
    flags: [u8; 8],
    resources: Vec<SourceReference>,
}

struct Group {
    kind: [u8; 8],
    rows: Vec<Row>,
}

struct Branch {
    flags: [u8; 8],
    groups: Vec<Group>,
}

/// Track inspected array extents so aliases and overlapping descriptors cannot
/// expand a small input into unbounded output or hide initialized extra fields.
struct Read<'a> {
    p: &'a Payload,
    spans: BTreeMap<usize, usize>,
}

impl Read<'_> {
    fn array(&mut self, descriptor: usize, stride: usize, class: u32) -> Result<Vec<usize>> {
        let rows = self.p.array(descriptor, stride, Some(class))?;
        if rows.is_empty() {
            ensure!(
                self.p.u64(descriptor + 8)? == 0,
                "empty resource table array has a pointer"
            );
            return Ok(rows);
        }
        let header = self.p.pointer(descriptor + 8)?;
        ensure!(
            header >= 0xD0
                && header % 16 == 0
                && self.p.u32(header - 4)? == 0x80809FB8
                && self.p.u64(header + 8)? == u64::from(class),
            "resource table array envelope differs at {descriptor:X}"
        );
        let start = header - 4;
        let end = header + 16 + rows.len() * stride;
        ensure!(
            self.spans
                .range(..=start)
                .next_back()
                .is_none_or(|(_, previous_end)| *previous_end <= start)
                && self
                    .spans
                    .range(start..)
                    .next()
                    .is_none_or(|(next_start, _)| *next_start >= end),
            "resource table arrays overlap at {descriptor:X}"
        );
        self.spans.insert(start, end);
        Ok(rows)
    }

    fn finish(&self) -> Result<()> {
        let mut end = 0xC8;
        for (&start, &next_end) in &self.spans {
            ensure!(
                self.p.0[end..start].iter().all(|byte| *byte == 0),
                "resource table has unparsed data at {end:X}"
            );
            end = next_end;
        }
        ensure!(
            end == self.p.0.len(),
            "resource table has an unparsed trailer at {end:X}"
        );
        Ok(())
    }
}

fn reference(p: &Payload, at: usize, hashes: &BTreeMap<u64, u32>) -> Result<u32> {
    ensure!(
        p.bytes::<16>(at + 16)? == [0; 16],
        "resource table reference extension at {at:X} requires translation"
    );
    let (tag, form, hash) = (p.u32(at)?, p.u32(at + 4)?, p.u64(at + 8)?);
    let tag = if tag == u32::MAX && form == 0 && hash != 0 {
        *hashes
            .get(&hash)
            .with_context(|| format!("resource table reference {hash:016X} is unresolved"))?
    } else {
        ensure!(
            form <= 2 && hash == 0,
            "resource table reference at {at:X} has an unknown form"
        );
        tag
    };
    valid_tag(tag)?;
    Ok(tag)
}

fn valid_tag(tag: u32) -> Result<()> {
    ensure!(
        tag == 0 || tag == u32::MAX || (0x80800001..=0x81FFFFFF).contains(&tag),
        "invalid resource table resource tag {tag:08X}"
    );
    Ok(())
}

fn read(source: &Payload, hashes: &BTreeMap<u64, u32>) -> Result<Vec<Branch>> {
    ensure!(
        source.0.len() >= 0xC8 && source.u64(0)? == source.0.len() as u64,
        "resource table size differs"
    );
    let mut reader = Read {
        p: source,
        spans: BTreeMap::new(),
    };
    let mut branches = Vec::new();
    for descriptor in [0x80, 0x98, 0xB0] {
        let mut groups = Vec::new();
        for group in reader.array(descriptor, 24, 0x80808747)? {
            let mut rows = Vec::new();
            for row in reader.array(group + 8, 32, 0x80808749)? {
                let resources = reader
                    .array(row + 8, 32, 0x8080BC7B)?
                    .into_iter()
                    .map(|at| {
                        Ok(SourceReference {
                            offset: at,
                            source: reference(source, at, hashes)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                rows.push(Row {
                    name: source.bytes(row)?,
                    flags: source.bytes(row + 24)?,
                    resources,
                });
            }
            groups.push(Group {
                kind: source.bytes(group)?,
                rows,
            });
        }
        branches.push(Branch {
            flags: source.bytes(descriptor + 16)?,
            groups,
        });
    }
    reader.finish()?;
    Ok(branches)
}

/// Inspect every set resource through the same complete table validation used
/// by the emitter. Hash references resolve to Source tags, never stock twins.
pub fn source_references(
    source: &Payload,
    hashes: &BTreeMap<u64, u32>,
) -> Result<Vec<SourceReference>> {
    Ok(read(source, hashes)?
        .into_iter()
        .flat_map(|branch| branch.groups)
        .flat_map(|group| group.rows)
        .flat_map(|row| row.resources)
        .filter(|reference| reference.source != 0 && reference.source != u32::MAX)
        .collect())
}

struct Write<'a> {
    bytes: Vec<u8>,
    tags: &'a BTreeMap<u32, u32>,
    references: Vec<Reference>,
}

impl Write<'_> {
    fn array(&mut self, descriptor: usize, count: usize, stride: usize, class: u32) -> usize {
        self.bytes[descriptor..descriptor + 8].copy_from_slice(&(count as u64).to_le_bytes());
        self.bytes[descriptor + 8..descriptor + 16].fill(0);
        if count == 0 {
            return 0;
        }
        let header = (self.bytes.len() + 19) & !15;
        self.bytes.resize(header + 16 + count * stride, 0);
        self.bytes[header - 4..header].copy_from_slice(&0x80809FBDu32.to_le_bytes());
        self.bytes[header..header + 8].copy_from_slice(&(count as u64).to_le_bytes());
        self.bytes[header + 8..header + 12].copy_from_slice(&class.to_le_bytes());
        self.bytes[descriptor + 8..descriptor + 16]
            .copy_from_slice(&(header as i64 - descriptor as i64 - 8).to_le_bytes());
        header + 16
    }

    fn references(&mut self, row: &Row, at: usize) -> Result<()> {
        self.bytes[at..at + 8].copy_from_slice(&row.name);
        self.bytes[at + 24..at + 32].copy_from_slice(&row.flags);
        let first = self.array(at + 8, row.resources.len(), 4, 0x80800014);
        for (index, reference) in row.resources.iter().enumerate() {
            let source = reference.source;
            let offset = first + index * 4;
            let target = if source == 0 || source == u32::MAX {
                Some(source)
            } else {
                self.tags.get(&source).copied()
            };
            if let Some(target) = target {
                valid_tag(target)?;
            }
            self.bytes[offset..offset + 4]
                .copy_from_slice(&target.unwrap_or(u32::MAX).to_le_bytes());
            if source != 0 && source != u32::MAX {
                self.references.push(Reference {
                    offset,
                    source,
                    target,
                });
            }
        }
        Ok(())
    }

    fn groups(&mut self, branch: &Branch, descriptor: usize) -> Result<()> {
        self.bytes[descriptor + 16..descriptor + 24].copy_from_slice(&branch.flags);
        let first = self.array(descriptor, branch.groups.len(), 24, 0x80808BD5);
        for (index, group) in branch.groups.iter().enumerate() {
            let at = first + index * 24;
            self.bytes[at..at + 8].copy_from_slice(&group.kind);
            let rows = self.array(at + 8, group.rows.len(), 32, 0x80808BD7);
            for (index, row) in group.rows.iter().enumerate() {
                self.references(row, rows + index * 32)?;
            }
        }
        Ok(())
    }
}

/// Translate a whole table. Maps are supplied by package assembly, not inferred
/// from resource positions. Unresolved slots are returned for later relocation.
pub fn emit(
    source: &Payload,
    tags: &BTreeMap<u32, u32>,
    hashes: &BTreeMap<u64, u32>,
) -> Result<Table> {
    let branches = read(source, hashes)?;
    let mut writer = Write {
        bytes: source.0[..0xC8].to_vec(),
        tags,
        references: Vec::new(),
    };
    for (branch, descriptor) in branches.iter().zip([0x80, 0x98, 0xB0]) {
        writer.groups(branch, descriptor)?;
    }
    let size = writer.bytes.len() as u64;
    writer.bytes[..8].copy_from_slice(&size.to_le_bytes());
    Ok(Table {
        payload: Payload(writer.bytes),
        references: writer.references,
    })
}
