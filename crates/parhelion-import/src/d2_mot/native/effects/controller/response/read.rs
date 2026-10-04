//! Source response extents and exhaustive resource reference enumeration.
use super::{Payload, Resources};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Read<'a> {
    pub source: &'a Payload,
    pub resources: &'a Resources,
    pub groups: Option<&'a super::GroupBindings>,
    pub resolver: Option<&'a super::selectors::ResourceResolver>,
    spans: BTreeMap<usize, usize>,
    references: BTreeSet<usize>,
}

impl<'a> Read<'a> {
    pub fn new(source: &'a Payload, resources: &'a Resources) -> Self {
        Self {
            source,
            resources,
            groups: None,
            resolver: None,
            spans: BTreeMap::new(),
            references: BTreeSet::new(),
        }
    }

    pub fn claim(&mut self, start: usize, size: usize) -> Result<()> {
        let end = start
            .checked_add(size)
            .context("response extent overflow")?;
        ensure!(
            end <= self.source.0.len(),
            "response extent outside payload"
        );
        ensure!(
            self.spans
                .range(..=start)
                .next_back()
                .is_none_or(|(_, end)| *end <= start)
                && self
                    .spans
                    .range(start..)
                    .next()
                    .is_none_or(|(at, _)| *at >= end),
            "response records or arrays overlap at {start:X}"
        );
        self.spans.insert(start, end);
        Ok(())
    }

    pub fn array(&mut self, field: usize, class: u32, stride: usize) -> Result<Vec<usize>> {
        let rows = self.source.array(field, stride, Some(class))?;
        ensure!(rows.len() <= 65536, "response array capacity differs");
        if rows.is_empty() {
            ensure!(
                self.source.u64(field + 8)? == 0,
                "empty response array has a pointer"
            );
        } else {
            let header = self.source.pointer(field + 8)?;
            let marker = header.checked_sub(4).context("response array marker")?;
            ensure!(
                header % 16 == 0
                    && self.source.u32(marker)? == 0x80809FB8
                    && self.source.u64(header + 8)? == u64::from(class),
                "response array envelope differs"
            );
            self.claim(marker, 20 + rows.len() * stride)?;
        }
        Ok(rows)
    }

    /// Unset references are still included in the modern enumeration.
    pub fn reference(&mut self, at: usize) -> Result<u32> {
        ensure!(
            self.references.insert(at),
            "response reference decoded twice"
        );
        let tag = self.source.u32(at)?;
        let form = self.source.u32(at + 4)?;
        let hash = self.source.u64(at + 8)?;
        if tag == u32::MAX && form == 0 && hash != 0 {
            self.resources
                .hashes
                .get(&hash)
                .copied()
                .context("response resource hash missing")
        } else {
            ensure!(
                form <= 2 && hash == 0,
                "unsupported response reference form"
            );
            Ok(tag)
        }
    }

    pub fn adopt(&mut self, fragment: &super::selectors::Fragment) -> Result<()> {
        for &(start, end) in &fragment.source_spans {
            self.claim(start, end - start)?;
        }
        for &field in &fragment.source_references {
            self.reference(field)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        let rows = self.array(0x60, 0x8080906E, 8)?;
        let mut enumerated = BTreeSet::new();
        for row in rows {
            ensure!(
                enumerated.insert(self.source.pointer(row)?),
                "duplicate response reference enumeration"
            );
        }
        ensure!(
            enumerated == self.references,
            "response reference enumeration differs"
        );
        let mut end = 0;
        for (&start, &following) in &self.spans {
            ensure!(
                self.source.0[end..start].iter().all(|v| *v == 0),
                "unparsed response data at {end:X}"
            );
            end = following;
        }
        ensure!(
            self.source.0[end..].iter().all(|v| *v == 0),
            "unparsed response tail at {end:X}"
        );
        Ok(())
    }
}
