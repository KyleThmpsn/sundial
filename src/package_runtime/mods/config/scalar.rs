//! Bounded native CUI traversal proving that every configured scalar has one supported consumer.
use super::{Binding, HEADER_SIZE, Kind};
use crate::package_payload::{bytes_at, i64_at, relative_offset, u16_at, u32_at, u64_at};
use std::collections::BTreeMap;

const ARRAY_MARKER: u32 = 0x80809fbd;
const ARRAY_LIMIT: usize = 1 << 20;
const VISIT_BUDGET: usize = 4 << 20;

pub(super) fn validate(bytes: &[u8], sites: &[&Binding]) -> Result<(), String> {
    Validator::new(bytes, sites).validate()
}

#[derive(Clone, Copy)]
struct Pool {
    field: usize,
    class: u32,
    stride: usize,
}

const POOLS: &[Pool] = &[
    Pool {
        field: 136,
        class: 0x8080468d,
        stride: 1,
    },
    Pool {
        field: 152,
        class: 0x80804696,
        stride: 4,
    },
    Pool {
        field: 168,
        class: 0x80804695,
        stride: 8,
    },
    Pool {
        field: 184,
        class: 0x80804694,
        stride: 4,
    },
    Pool {
        field: 200,
        class: 0x80804694,
        stride: 4,
    },
    Pool {
        field: 216,
        class: 0x80804693,
        stride: 16,
    },
    Pool {
        field: 232,
        class: 0x80804692,
        stride: 4,
    },
    Pool {
        field: 264,
        class: 0x80804690,
        stride: 4,
    },
    Pool {
        field: 280,
        class: 0x8080468b,
        stride: 2,
    },
    Pool {
        field: 312,
        class: 0x8080468f,
        stride: 4,
    },
    Pool {
        field: 328,
        class: 0x80804688,
        stride: 16,
    },
];
struct Validator<'a> {
    bytes: &'a [u8],
    sites: &'a [&'a Binding],
    references: Vec<usize>,
    nodes: Vec<Option<u16>>,
    budget: usize,
}

impl<'a> Validator<'a> {
    fn new(bytes: &'a [u8], sites: &'a [&'a Binding]) -> Self {
        Self {
            bytes,
            sites,
            references: vec![0; sites.len()],
            nodes: vec![None; sites.len()],
            budget: VISIT_BUDGET,
        }
    }
    fn overlap(&self, at: usize, len: usize) -> Option<usize> {
        self.sites
            .iter()
            .position(|s| s.offset < at.saturating_add(len) && at < s.offset + s.kind.width())
    }
    fn structure(&self, at: usize, len: usize) -> Result<(), String> {
        if at.checked_add(len).is_none_or(|end| end > self.bytes.len())
            || self.overlap(at, len).is_some()
        {
            return Err("Scalar overlaps structural data".into());
        }
        Ok(())
    }
    fn pointer(&self, field: usize) -> Result<usize, String> {
        let delta = i64_at(self.bytes, field)?;
        if delta == 0 {
            return Err("Null scalar pointer".into());
        }
        let at = relative_offset(field, 0, delta)?;
        if at >= self.bytes.len() {
            return Err("Scalar pointer exceeds its entry".into());
        }
        Ok(at)
    }
    fn array(
        &mut self,
        field: usize,
        class: u32,
        stride: usize,
        structural: bool,
    ) -> Result<Vec<usize>, String> {
        self.structure(field, 16)?;
        let count = usize::try_from(u64_at(self.bytes, field)?)
            .map_err(|_| "CUI array count is too large")?;
        if count == 0 {
            return Ok(Vec::new());
        }
        if count > ARRAY_LIMIT || count > self.budget {
            return Err("CUI traversal limit exceeded".into());
        }
        self.budget -= count;
        let header = self.pointer(field + 8)?;
        self.structure(
            header.checked_sub(4).ok_or("CUI array marker is missing")?,
            20,
        )?;
        if u32_at(self.bytes, header - 4)? != ARRAY_MARKER
            || u64_at(self.bytes, header)? != count as u64
            || u32_at(self.bytes, header + 8)? != class
        {
            return Err("Invalid CUI array metadata".into());
        }
        let rows = header + 16;
        let len = count
            .checked_mul(stride)
            .ok_or("CUI array span overflowed")?;
        if structural {
            self.structure(rows, len)?;
        } else if rows
            .checked_add(len)
            .is_none_or(|end| end > self.bytes.len())
        {
            return Err("CUI pool exceeds its entry".into());
        }
        Ok((0..count).map(|i| rows + i * stride).collect())
    }
    fn validate(mut self) -> Result<(), String> {
        self.structure(0, HEADER_SIZE)?;
        if u64_at(self.bytes, 0)? != self.bytes.len() as u64 {
            return Err("CUI resource size disagrees".into());
        }
        let mut pools = Vec::new();
        for pool in POOLS {
            pools.push(self.array(pool.field, pool.class, pool.stride, false)?);
        }
        self.properties(&pools)?;
        self.comparisons()?;
        self.types()?;
        if self.references.iter().any(|&count| count != 1) {
            return Err("Configured scalar has no unique consumer".into());
        }
        Ok(())
    }

    fn properties(&mut self, pools: &[Vec<usize>]) -> Result<(), String> {
        for object in self.array(72, 0x8080462a, 64, true)? {
            for component in self.array(object + 32, 0x808046d4, 24, true)? {
                let rows = self.array(component + 8, 0x80804858, 24, true)?;
                for position in 0..rows.len() {
                    self.property(pools, &rows, position, component)?;
                }
            }
        }
        Ok(())
    }

    fn property(
        &mut self,
        pools: &[Vec<usize>],
        rows: &[usize],
        position: usize,
        component: usize,
    ) -> Result<(), String> {
        let row = rows[position];
        let value = self.pointer(row + 16)?;
        let matches = pools
            .iter()
            .enumerate()
            .filter(|(index, rows)| {
                rows.first().is_some_and(|&first| {
                    value >= first
                        && value - first < rows.len() * POOLS[*index].stride
                        && (value - first).is_multiple_of(POOLS[*index].stride)
                })
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("CUI property has no unique typed pool".into());
        }
        let pool = matches[0].0;
        let Pool { field, stride, .. } = POOLS[pool];
        let Some(index) = self.overlap(value, stride) else {
            return Ok(());
        };
        let site = self.sites[index];
        let node = u16::try_from(u64_at(self.bytes, component)?)
            .map_err(|_| "CUI node exceeds 16 bits")?;
        let selector = u64_at(self.bytes, row + 8)?;
        let path = self.pointer(row)?;
        if site.profile != "cui-property-v1"
            || site.property_offset != Some(row)
            || site.pool_offset != Some(field)
            || site.offset != value
            || site.kind.width() != stride
            || !self.array(path, 0x80804949, 4, true)?.is_empty()
        {
            return Err("CUI property binding disagrees".into());
        }
        for &later in &rows[position + 1..] {
            if u64_at(self.bytes, later + 8)? == selector {
                let path = self.pointer(later)?;
                if self.array(path, 0x80804949, 4, true)?.is_empty() {
                    return Err("CUI property is shadowed".into());
                }
            }
        }
        let valid = if site.kind == Kind::Bool {
            field == 136 && selector == 0x41a && bytes_at::<1>(self.bytes, value)?[0] <= 1
        } else {
            matches!(field, 184 | 200)
                && (0x206..=0x209).contains(&selector)
                && f32::from_le_bytes(bytes_at(self.bytes, value)?).is_finite()
        };
        if !valid {
            return Err("Unsupported CUI property scalar".into());
        }
        self.references[index] += 1;
        self.nodes[index] = Some(node);
        Ok(())
    }

    fn comparisons(&mut self) -> Result<(), String> {
        for row in self.array(88, 0x808046d8, 56, true)? {
            if i64_at(self.bytes, row + 48)? == 0 {
                continue;
            }
            let transform = self.pointer(row + 48)?;
            self.structure(
                transform.checked_sub(4).ok_or("Missing comparison class")?,
                8,
            )?;
            let class = u32_at(self.bytes, transform - 4)?;
            let kind = bytes_at::<1>(self.bytes, transform)?[0];
            if class == 0x808046e0 && kind == 0 {
                continue;
            }
            let code = bytes_at::<1>(self.bytes, transform + 1)?[0];
            let scalar = bytes_at::<4>(self.bytes, transform + 4)?;
            if code > 5
                || !matches!(
                    (class, kind),
                    (0x808046dc, 1) | (0x808046db, 2) | (0x808046da, 3)
                )
            {
                return Err("Unsupported CUI comparison".into());
            }
            if let Some(index) = self.overlap(transform + 4, 4) {
                let site = self.sites[index];
                if site.profile != "cui-comparison-v1"
                    || site.binding_offset != Some(row)
                    || site.offset != transform + 4
                    || !((site.kind == Kind::Int && kind == 1)
                        || (site.kind == Kind::Float
                            && kind == 2
                            && f32::from_le_bytes(scalar).is_finite()))
                {
                    return Err("CUI comparison binding disagrees".into());
                }
                self.references[index] += 1;
            }
        }
        Ok(())
    }

    fn types(&mut self) -> Result<(), String> {
        let types = self.array(56, 0x80804622, 16, true)?;
        let mut by_node = BTreeMap::<u16, Vec<u64>>::new();
        for row in types {
            by_node
                .entry(u16_at(self.bytes, row + 10)?)
                .or_default()
                .push(u64_at(self.bytes, row)?);
        }
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(node) = node else {
                continue;
            };
            let types = by_node
                .get(node)
                .ok_or("CUI property node has no native type")?;
            if types.len() != 1 {
                return Err("CUI property node has no unique native type".into());
            }
            let class = types[0];
            if !(if self.sites[index].kind == Kind::Bool {
                class == 0x0ce30d0b
            } else {
                matches!(
                    class,
                    0xae7e6d99 | 0x0ce30d0b | 0x7d1bd221 | 0x725ad1d887a7707b
                )
            }) {
                return Err("Unsupported CUI property node type".into());
            }
        }
        Ok(())
    }
}
