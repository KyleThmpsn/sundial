//! Response records and checked absolute placement of their native arrays.
use super::read::Read;
use super::{Payload, Reference, put};
use anyhow::{Context, Result, bail, ensure};

pub(super) struct Write {
    pub owner: Payload,
    pub references: Vec<Reference>,
    pub gates: Vec<super::Gate>,
    pub reference_classes: std::collections::BTreeMap<usize, u32>,
    pub named_conditions: Vec<u32>,
    pub group_aliases: Vec<super::selectors::GroupAliasUse>,
}

impl Write {
    pub fn reserve(&mut self, size: usize) -> Result<usize> {
        let at = self.owner.0.len();
        self.owner
            .0
            .resize(at.checked_add(size).context("response output overflow")?, 0);
        Ok(at)
    }
    pub fn prefix(&mut self, class: u32, alignment: usize) -> Result<usize> {
        let at = self
            .owner
            .0
            .len()
            .checked_add(4 + alignment - 1)
            .context("response alignment overflow")?
            & !(alignment - 1);
        self.owner.0.resize(at, 0);
        put(&mut self.owner.0, at - 4, &class.to_le_bytes())?;
        Ok(at)
    }
    pub fn relative(&mut self, at: usize, target: usize) -> Result<()> {
        put(
            &mut self.owner.0,
            at,
            &(i64::try_from(target)? - i64::try_from(at)?).to_le_bytes(),
        )
    }
    pub fn copy(&mut self, read: &Read<'_>, from: usize, to: usize, size: usize) -> Result<()> {
        let bytes = read
            .source
            .0
            .get(from..from.checked_add(size).context("response copy overflow")?)
            .context("response copy extent")?;
        put(&mut self.owner.0, to, bytes)
    }
    pub fn array(
        &mut self,
        field: usize,
        class: u32,
        count: usize,
        stride: usize,
    ) -> Result<usize> {
        if count == 0 {
            return Ok(0);
        }
        let header = self.prefix(0x80809FBD, 16)?;
        self.reserve(
            16 + count
                .checked_mul(stride)
                .context("response array overflow")?,
        )?;
        put(&mut self.owner.0, header, &(count as u64).to_le_bytes())?;
        put(
            &mut self.owner.0,
            header + 8,
            &u64::from(class).to_le_bytes(),
        )?;
        put(&mut self.owner.0, field, &(count as u64).to_le_bytes())?;
        self.relative(field + 8, header)?;
        Ok(header + 16)
    }
    pub fn reference(&mut self, read: &mut Read<'_>, source: usize, target: usize) -> Result<()> {
        let tag = read.reference(source)?;
        let mapped = if matches!(tag, 0 | u32::MAX | 0x811C9DC5) {
            Some(tag)
        } else {
            let mapped = read.resources.tags.get(&tag).copied();
            if let Some(tag) = mapped {
                ensure!(
                    (0x80800001..=0x81FFFFFF).contains(&tag) && tag != 0x811C9DC5,
                    "invalid response resource binding"
                );
            }
            self.references.push(Reference {
                offset: target,
                source: tag,
                target: mapped,
            });
            mapped
        };
        put(
            &mut self.owner.0,
            target,
            &mapped.unwrap_or(u32::MAX).to_le_bytes(),
        )
    }
    pub fn record(&mut self, read: &mut Read<'_>, source: usize) -> Result<usize> {
        let marker = source.checked_sub(4).context("response record prefix")?;
        let class = read.source.u32(marker)?;
        if class == 0x808042CB {
            let resources = super::selectors::Resources {
                tags: read.resources.tags.clone(),
                names: read.resources.names.clone(),
                categories: read.resources.categories.clone(),
            };
            let empty = super::GroupBindings::default();
            let groups = read.groups.unwrap_or(&empty);
            let fragment = super::selectors::append_category(
                read.source,
                &mut self.owner,
                super::selectors::FragmentContract {
                    root_class: class,
                    field: source,
                    extent: 104,
                },
                &resources,
                groups,
            )?;
            read.adopt(&fragment)?;
            self.references.extend(fragment.references);
            self.reference_classes.extend(fragment.reference_classes);
            self.group_aliases.extend(fragment.group_aliases);
            self.named_conditions.extend(fragment.named_conditions);
            self.gates
                .extend(fragment.gates.into_iter().map(|g| super::Gate::Predicate {
                    offset: g.offset,
                    index: g.index,
                    name: g.name,
                }));
            return Ok(fragment.root);
        }
        let (native, size, length, alignment) = match class {
            0x80803FC6 => (0x80804B56, 12, 12, 4),
            0x80803FB5 => (0x80804B45, 32, 32, 4),
            0x80803FAF => (0x80804B3F, 8, 8, 4),
            0x80803F85 => (0x80804B17, 12, 12, 4),
            0x80803FD1 => (0x80804B61, 1, 1, 4),
            0x80803FD2 => (0x80804B62, 1, 1, 4),
            0x80803F75 => (0x80804B07, 1, 1, 4),
            0x808042CE => (0x80804D76, 1, 1, 4),
            0x80803FC7 => (0x80804B57, 48, 40, 8),
            0x80803F77 => (0x80804B09, 24, 16, 8),
            0x80803F78 => (0x80804B0A, 40, 40, 8),
            0x80803FB6 => (0x80804B46, 16, 16, 8),
            0x80803FC4 => (0x80804B54, 128, 112, 8),
            _ => bail!("unsupported response record class {class:08X}"),
        };
        ensure!(
            source % alignment == 0,
            "response source record alignment differs"
        );
        read.claim(marker, size + 4)?;
        let at = self.prefix(native, alignment)?;
        self.reserve(length)?;
        match class {
            0x80803FD1 | 0x80803FD2 | 0x80803F75 | 0x808042CE => {
                ensure!(read.source.u8(source)? <= 1, "response boolean differs");
                self.copy(read, source, at, 1)?;
            }
            0x80803FC7 => {
                ensure!(
                    read.source.u32(source + 20)? == 0xBF800000,
                    "active unsupported response scalar"
                );
                self.copy(read, source, at, 32)?;
                put(&mut self.owner.0, at + 20, &[0; 4])?;
                self.reference(read, source + 32, at + 32)?;
            }
            0x80803F78 => {
                let resources = super::selectors::Resources {
                    tags: read.resources.tags.clone(),
                    names: read.resources.names.clone(),
                    categories: read.resources.categories.clone(),
                };
                let empty = super::GroupBindings::default();
                let groups = read.groups.unwrap_or(&empty);
                let unbound = super::selectors::ResourceResolver {
                    hashes: read.resources.hashes.clone(),
                    source_classes: Default::default(),
                    native_classes: Default::default(),
                };
                let resolver = read.resolver.unwrap_or(&unbound);
                let fragment = super::selectors::append_value_selector(
                    read.source,
                    &mut self.owner,
                    super::selectors::FragmentContract {
                        root_class: class,
                        field: source,
                        extent: 40,
                    },
                    at,
                    &resources,
                    groups,
                    resolver,
                )?;
                read.adopt(&fragment)?;
                self.references.extend(fragment.references);
                self.reference_classes.extend(fragment.reference_classes);
                self.named_conditions.extend(fragment.named_conditions);
                self.group_aliases.extend(fragment.group_aliases);
                self.gates
                    .extend(fragment.gates.into_iter().map(|g| super::Gate::Predicate {
                        offset: g.offset,
                        index: g.index,
                        name: g.name,
                    }));
                self.gates.push(super::Gate::Selector { source });
                let rows = read.array(source + 24, 0x80803F7B, 8)?;
                let start = self.array(at + 24, 0x80804B0D, rows.len(), 8)?;
                for (index, row) in rows.into_iter().enumerate() {
                    let record = self.record(read, read.source.pointer(row)?)?;
                    self.relative(start + index * 8, record)?;
                }
            }
            0x80803F77 => {
                self.copy(read, source, at, 8)?;
                self.reference(read, source + 8, at + 8)?;
            }
            0x80803FC4 => {
                ensure!(
                    read.source.bytes::<72>(source + 24)? == [0; 72],
                    "active unsupported response category predicate"
                );
                self.copy(read, source, at, 8)?;
                self.reference(read, source + 8, at + 8)?;
                self.reference(read, source + 96, at + 88)?;
                self.copy(read, source + 112, at + 96, 16)?;
            }
            0x80803FB6 => {
                let rows = read.array(source, 0x80803FB8, 96)?;
                let start = self.array(at, 0x80804B48, rows.len(), 56)?;
                for (index, row) in rows.into_iter().enumerate() {
                    let to = start + index * 56;
                    for (sm, sn) in [(0, 0), (24, 16), (48, 32), (72, 48)] {
                        self.copy(read, row + sm, to + sn, 8)?;
                    }
                    for (sm, sn) in [(8, 8), (32, 24), (56, 40)] {
                        self.reference(read, row + sm, to + sn)?;
                    }
                    ensure!(
                        read.source.bytes::<16>(row + 80)? == [0; 16],
                        "active response row extension"
                    );
                }
            }
            _ => self.copy(read, source, at, length)?,
        }
        Ok(at)
    }
}
