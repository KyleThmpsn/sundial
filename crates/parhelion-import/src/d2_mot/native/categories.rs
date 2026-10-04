//! Private category namespaces with immutable stock indices and explicit wire limits.
use crate::d2_mot::payload::Payload;
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

mod aliases;
pub use aliases::GroupAlias;

#[cfg(test)]
mod tests;

pub const MODERN_CLASS: u32 = 0x808097A2;
pub const NATIVE_CLASS: u32 = 0x808094B4;

struct Group {
    name: u32,
    bits: BTreeSet<usize>,
}

struct Partition {
    start: u16,
    count: u8,
}

struct Dictionary {
    names: Vec<u32>,
    groups: Vec<Group>,
    partitions: Vec<Partition>,
}

/// Appended names are valid for immutable definitions and local queries.
/// Their presence in this dictionary does not add native network capacity.
pub struct Namespace {
    pub payload: Payload,
    pub source_names: Vec<u32>,
    pub source_groups: BTreeMap<u32, BTreeSet<u32>>,
    pub groups: BTreeMap<u32, BTreeSet<u32>>,
    pub names: Vec<u32>,
    pub source_indices: Vec<Option<u16>>,
    pub added_static: Vec<u32>,
    /// Optional private group identities with unchanged stock memberships.
    pub group_aliases: BTreeMap<u32, GroupAlias>,
    wire_indices: BTreeSet<u16>,
}

struct Read<'a> {
    payload: &'a Payload,
    claimed: Vec<bool>,
    end: usize,
    marker: u32,
}

impl<'a> Read<'a> {
    fn claim(&mut self, at: usize, len: usize) -> Result<()> {
        let end = at.checked_add(len).context("category record overflow")?;
        let range = self
            .claimed
            .get_mut(at..end)
            .context("category record outside payload")?;
        ensure!(range.iter().all(|b| !b), "category records overlap");
        range.fill(true);
        self.end = self.end.max(end);
        Ok(())
    }

    fn array(
        &mut self,
        field: usize,
        class: u32,
        stride: usize,
        limit: usize,
    ) -> Result<Vec<usize>> {
        let count = usize::try_from(self.payload.u64(field)?)?;
        ensure!(
            count <= limit,
            "category array count exceeds format capacity"
        );
        if count == 0 {
            ensure!(
                self.payload.u64(field + 8)? == 0,
                "empty category array pointer differs"
            );
            return Ok(Vec::new());
        }
        let header = self.payload.pointer(field + 8)?;
        ensure!(
            header >= 4 && header % 16 == 0,
            "category array alignment differs"
        );
        ensure!(
            self.payload.u32(header - 4)? == self.marker
                && self.payload.u64(header)? == count as u64
                && self.payload.u64(header + 8)? == u64::from(class),
            "category array marker, count or class differs"
        );
        let rows = self.payload.array(field, stride, Some(class))?;
        self.claim(header - 4, 20 + count * stride)?;
        Ok(rows)
    }

    fn finish(self) -> Result<()> {
        ensure!(
            self.end == self.payload.0.len(),
            "category dictionary has trailing records"
        );
        ensure!(
            self.payload
                .0
                .iter()
                .zip(self.claimed)
                .all(|(&byte, claimed)| claimed || byte == 0),
            "category dictionary has unmodeled padding"
        );
        Ok(())
    }
}

impl Dictionary {
    fn named_groups(&self) -> BTreeMap<u32, BTreeSet<u32>> {
        self.groups
            .iter()
            .map(|group| {
                (
                    group.name,
                    group.bits.iter().map(|&bit| self.names[bit]).collect(),
                )
            })
            .collect()
    }

    fn read(payload: &Payload, modern: bool) -> Result<Self> {
        ensure!(
            payload.u64(0)? == payload.0.len() as u64,
            "category dictionary size differs"
        );
        let capacity = if modern { 448 } else { 320 };
        let width = capacity / 8;
        let mut read = Read {
            payload,
            claimed: vec![false; payload.0.len()],
            end: 0,
            marker: if modern { 0x80809FB8 } else { 0x80809FBD },
        };
        read.claim(0, 56)?;
        let names = read
            .array(8, 0x80800070, 4, capacity)?
            .into_iter()
            .map(|at| payload.u32(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !names.is_empty()
                && names.iter().copied().collect::<BTreeSet<_>>().len() == names.len(),
            "empty or duplicate category names"
        );
        let group_class = if modern { 0x808097AC } else { 0x808094BE };
        let mut group_names = BTreeSet::new();
        let mut groups = Vec::new();
        for at in read.array(24, group_class, width + 4, 4096)? {
            let name = payload.u32(at)?;
            ensure!(group_names.insert(name), "duplicate category group");
            let mut bits = BTreeSet::new();
            for bit in 0..capacity {
                if payload.u8(at + 4 + bit / 8)? & (1 << (bit % 8)) != 0 {
                    ensure!(bit < names.len(), "category group has an unnamed bit");
                    bits.insert(bit);
                }
            }
            groups.push(Group { name, bits });
        }
        let partition_class = if modern { 0x808097AA } else { 0x808094BC };
        let mut wire = BTreeSet::new();
        let mut partitions = Vec::new();
        for at in read.array(40, partition_class, 4, 256)? {
            let start = payload.u16(at)?;
            let count = payload.u8(at + 2)?;
            ensure!(
                count > 0 && payload.u8(at + 3)? == 0,
                "category partition count or padding differs"
            );
            let end = usize::from(start) + usize::from(count);
            ensure!(end <= names.len(), "category partition exceeds name table");
            for bit in usize::from(start)..end {
                ensure!(wire.insert(bit), "category partitions overlap");
            }
            partitions.push(Partition { start, count });
        }
        read.finish()?;
        Ok(Self {
            names,
            groups,
            partitions,
        })
    }

    fn emit(&self) -> Result<Payload> {
        ensure!(
            self.names.len() <= 320,
            "native category dictionary capacity exceeded"
        );
        let mut rows = vec![Vec::new(), Vec::new(), Vec::new()];
        for name in &self.names {
            rows[0].extend_from_slice(&name.to_le_bytes());
        }
        for group in &self.groups {
            rows[1].extend_from_slice(&group.name.to_le_bytes());
            let mut mask = [0u8; 40];
            for &bit in &group.bits {
                ensure!(
                    bit < self.names.len(),
                    "native category group exceeds names"
                );
                mask[bit / 8] |= 1 << (bit % 8);
            }
            rows[1].extend_from_slice(&mask);
        }
        for part in &self.partitions {
            rows[2].extend_from_slice(&part.start.to_le_bytes());
            rows[2].extend_from_slice(&[part.count, 0]);
        }
        let mut bytes = vec![0u8; 56];
        for ((field, class, stride), raw) in [
            (8usize, 0x80800070u32, 4usize),
            (24, 0x808094BE, 44),
            (40, 0x808094BC, 4),
        ]
        .into_iter()
        .zip(rows)
        {
            if raw.is_empty() {
                continue;
            }
            let count = (raw.len() / stride) as u64;
            let header = (bytes.len() + 4 + 15) & !15;
            bytes.resize(header, 0);
            bytes[header - 4..header].copy_from_slice(&0x80809FBDu32.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&u64::from(class).to_le_bytes());
            bytes.extend_from_slice(&raw);
            bytes[field..field + 8].copy_from_slice(&count.to_le_bytes());
            bytes[field + 8..field + 16]
                .copy_from_slice(&i64::try_from(header - field - 8)?.to_le_bytes());
        }
        let size = bytes.len() as u64;
        bytes[..8].copy_from_slice(&size.to_le_bytes());
        Ok(Payload(bytes))
    }
}

/// Extend a private copy of a native dictionary for requested immutable names.
/// Existing names, group memberships and network partitions keep their indices.
/// The caller must allocate and bind the returned asset in its private package.
pub fn emit_static(
    source: &Payload,
    stock: &Payload,
    requested: &BTreeSet<u32>,
) -> Result<Namespace> {
    let source = Dictionary::read(source, true)?;
    let mut target = Dictionary::read(stock, false)?;
    ensure!(
        target.emit()?.0 == stock.0,
        "native category dictionary placement differs"
    );
    let source_names = source.names.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        requested.is_subset(&source_names),
        "requested category absent from source dictionary"
    );
    let existing = target.names.iter().copied().collect::<BTreeSet<_>>();
    let added_static = source
        .names
        .iter()
        .copied()
        .filter(|name| requested.contains(name) && !existing.contains(name))
        .collect::<Vec<_>>();
    ensure!(
        target.names.len() + added_static.len() <= 320,
        "private category dictionary exceeds native capacity"
    );
    target.names.extend(&added_static);
    let indices = target
        .names
        .iter()
        .enumerate()
        .map(|(i, &name)| (name, i))
        .collect::<BTreeMap<_, _>>();
    let added = added_static.iter().copied().collect::<BTreeSet<_>>();
    for group in &source.groups {
        let bits = group
            .bits
            .iter()
            .filter_map(|&bit| {
                let name = source.names[bit];
                added.contains(&name).then(|| indices[&name])
            })
            .collect::<BTreeSet<_>>();
        if bits.is_empty() {
            continue;
        }
        if let Some(existing) = target.groups.iter_mut().find(|g| g.name == group.name) {
            existing.bits.extend(bits);
        } else {
            target.groups.push(Group {
                name: group.name,
                bits,
            });
        }
    }
    let source_indices = source
        .names
        .iter()
        .map(|name| indices.get(name).map(|&index| index as u16))
        .collect();
    let wire_indices = target
        .partitions
        .iter()
        .flat_map(|p| p.start..p.start + u16::from(p.count))
        .collect();
    let payload = target.emit()?;
    let source_groups = source.named_groups();
    let groups = target.named_groups();
    Ok(Namespace {
        source_groups,
        groups,
        payload,
        source_names: source.names,
        names: target.names,
        source_indices,
        added_static,
        group_aliases: BTreeMap::new(),
        wire_indices,
    })
}

impl Namespace {
    fn mask(&self, source: &[u8], dynamic: bool) -> Result<[u8; 40]> {
        ensure!(source.len() == 56, "source category mask width differs");
        let mut result = [0u8; 40];
        for bit in 0..448 {
            if source[bit / 8] & (1 << (bit % 8)) == 0 {
                continue;
            }
            let target = self
                .source_indices
                .get(bit)
                .copied()
                .flatten()
                .with_context(|| format!("unmapped category mask bit {bit}"))?;
            ensure!(
                usize::from(target) < self.names.len() && target < 320,
                "category correspondence exceeds native names"
            );
            ensure!(
                !dynamic || self.wire_indices.contains(&target),
                "category bit {bit} is unavailable on the native wire"
            );
            result[usize::from(target) / 8] |= 1 << (target % 8);
        }
        Ok(result)
    }

    pub fn definition_mask(&self, source: &[u8]) -> Result<[u8; 40]> {
        self.mask(source, false)
    }

    pub fn dynamic_mask(&self, source: &[u8]) -> Result<[u8; 40]> {
        self.mask(source, true)
    }
}
