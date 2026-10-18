//! Checked source trees, shared strings and exhaustive parsed extents.
use super::*;

pub(super) struct Read<'a> {
    p: &'a Payload,
    spans: BTreeMap<usize, usize>,
    strings: BTreeMap<usize, String>,
    nodes: BTreeSet<usize>,
    resolver: Option<&'a ResourceResolver>,
    references: BTreeSet<usize>,
}

impl<'a> Read<'a> {
    pub fn new(p: &'a Payload, resolver: Option<&'a ResourceResolver>) -> Self {
        Self {
            p,
            spans: BTreeMap::new(),
            strings: BTreeMap::new(),
            nodes: BTreeSet::new(),
            resolver,
            references: BTreeSet::new(),
        }
    }

    pub fn claim(&mut self, at: usize, count: usize) -> Result<()> {
        let end = at.checked_add(count).context("selector extent overflow")?;
        ensure!(end <= self.p.0.len(), "selector extent outside payload");
        ensure!(
            self.spans
                .range(..=at)
                .next_back()
                .is_none_or(|(_, end)| *end <= at)
                && self
                    .spans
                    .range(at..)
                    .next()
                    .is_none_or(|(start, _)| *start >= end),
            "selector records or arrays overlap at {at:X}"
        );
        self.spans.insert(at, end);
        Ok(())
    }

    fn array(&mut self, field: usize, class: u32, stride: usize) -> Result<Vec<usize>> {
        let rows = self.p.array(field, stride, Some(class))?;
        ensure!(rows.len() <= 65536, "selector array capacity differs");
        if rows.is_empty() {
            ensure!(
                self.p.u64(field + 8)? == 0,
                "empty selector array has a pointer"
            );
        } else {
            let header = self.p.pointer(field + 8)?;
            let marker = header.checked_sub(4).context("selector array marker")?;
            ensure!(
                header % 16 == 0
                    && self.p.u32(marker)? == 0x80809FB8
                    && self.p.u64(header + 8)? == u64::from(class),
                "selector array envelope differs"
            );
            self.claim(marker, 20 + rows.len() * stride)?;
        }
        Ok(rows)
    }

    fn string(&mut self, field: usize) -> Result<Option<String>> {
        if self.p.u64(field)? == 0 {
            return Ok(None);
        }
        let at = self.p.pointer(field)?;
        if let Some(text) = self.strings.get(&at) {
            return Ok(Some(text.clone()));
        }
        // A serialized string pool can hold several strings under one prefix.
        // A field must point to a complete string boundary, not an interior byte.
        let limit = at.checked_sub(12).context("selector string prefix")?;
        let mut candidate = None;
        for prefix in (40..=limit).step_by(4) {
            if self.p.u32(prefix)? != 0x80800065 {
                continue;
            }
            let count = usize::try_from(self.p.u64(prefix + 4)?)?;
            let start = prefix + 12;
            let end = start
                .checked_add(count)
                .context("selector string overflow")?;
            if count != 0 && start <= at && at < end && end <= self.p.0.len() {
                ensure!(
                    candidate.is_none(),
                    "selector string pool envelopes overlap"
                );
                candidate = Some((prefix, start, end));
            }
        }
        let (prefix, start, end) = candidate.context("selector string pool prefix differs")?;
        self.claim(prefix, end - prefix)?;
        let mut cursor = start;
        while cursor < end {
            let count = self.p.0[cursor..end]
                .iter()
                .position(|byte| *byte == 0)
                .context("selector string terminator differs")?;
            ensure!(count != 0, "empty selector pooled string");
            let text = std::str::from_utf8(&self.p.0[cursor..cursor + count])
                .context("selector string UTF-8")?
                .to_owned();
            self.strings.insert(cursor, text);
            cursor += count + 1;
        }
        Ok(Some(
            self.strings
                .get(&at)
                .context("selector pointer is not a string boundary")?
                .clone(),
        ))
    }

    fn dictionary(&mut self, field: usize) -> Result<u32> {
        ensure!(
            self.references.insert(field),
            "selector dictionary decoded twice"
        );
        let tag = self.p.u32(field)?;
        ensure!(
            (0x80800001..=0x81FFFFFF).contains(&tag)
                && tag != 0x811C9DC5
                && self.p.u32(field + 4)? == 1
                && self.p.u64(field + 8)? == 0,
            "unsupported selector dictionary reference"
        );
        Ok(tag)
    }

    pub(super) fn masks(&mut self, field: usize) -> Result<Option<Mask>> {
        if self.p.u64(field)? == 0 {
            return Ok(None);
        }
        let at = self.p.pointer(field)?;
        let prefix = at.checked_sub(4).context("selector mask prefix")?;
        ensure!(at % 4 == 0, "selector mask alignment differs");
        let count = match self.p.u32(prefix)? {
            0x8080976E => 2,
            0x8080976D => 4,
            class => anyhow::bail!("selector mask class {class:08X} requires conversion"),
        };
        self.claim(prefix, 4 + count * 56 + 4)?;
        let words = (0..count * 14)
            .map(|i| self.p.u32(at + i * 4))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(Mask {
            source: at,
            words,
            tail: self.p.bytes(at + count * 56)?,
        }))
    }

    pub fn selector(&mut self, at: usize, depth: usize) -> Result<Node> {
        ensure!(depth < 128, "selector recursion limit exceeded");
        let flags = self.p.u64(at)?;
        ensure!(flags == 0x100 || flags == 0x101, "selector flags differ");
        let mut rows = Vec::new();
        for row in self.array(at + 8, 0x808091B7, 16)? {
            let value = self.p.u64(row)?;
            let target = self.p.pointer(row + 8)?;
            rows.push((value, self.node(target, depth + 1)?));
        }
        Ok(Node::Selector {
            flags: flags & 1,
            rows,
        })
    }

    pub(super) fn node(&mut self, at: usize, depth: usize) -> Result<Node> {
        ensure!(
            depth < 128 && self.nodes.len() < 65536 && self.nodes.insert(at),
            "selector recursion or node alias differs"
        );
        let prefix = at.checked_sub(4).context("selector predicate prefix")?;
        let class = self.p.u32(prefix)?;
        let raw = match class {
            0x808042C8 => Some((0x80804D71, 8)),
            0x808042C9 => Some((0x80804D72, 4)),
            0x808042CD => Some((0x80804D75, 4)),
            0x808042CE => Some((0x80804D76, 1)),
            0x808042D3 => Some((0x80804D7B, 1)),
            0x808042D4 => Some((0x80804D7C, 1)),
            0x808042D8 => Some((0x80804D80, 1)),
            _ => None,
        };
        if let Some((class, size)) = raw {
            ensure!(at.is_multiple_of(4), "selector predicate alignment differs");
            self.claim(prefix, 4 + size)?;
            return Ok(Node::Raw {
                class,
                data: self.p.0[at..at + size].to_vec(),
            });
        }
        ensure!(at.is_multiple_of(8), "selector predicate alignment differs");
        if class == 0x808091B0 {
            self.claim(prefix, 28)?;
            let resolver = self
                .resolver
                .context("external selector requires resource class evidence")?;
            let source = resolver.source(self.p, at + 8)?;
            ensure!(
                self.references.insert(at + 8),
                "selector reference decoded twice"
            );
            return Ok(Node::External {
                debug: self.string(at)?,
                source,
            });
        }
        if class == 0x808091B1 {
            self.claim(prefix, 28)?;
            return self.selector(at, depth);
        }
        if class == 0x808042D0 {
            self.claim(prefix, 20)?;
            let mut values = Vec::new();
            for row in self.array(at, 0x80809446, 4)? {
                values.push(
                    u8::try_from(self.p.u32(row)?)
                        .context("selector faction value exceeds native byte")?,
                );
            }
            return Ok(Node::Factions(values));
        }
        if class == 0x808042D5 {
            self.claim(prefix, 148)?;
            let comparison = self.p.u8(at + 0x88)?;
            ensure!(
                self.p.u64(at)? == 0
                    && comparison <= 5
                    && self.p.0[at + 0x89..at + 0x90].iter().all(|&byte| byte == 0),
                "numeric selector comparison mode differs"
            );
            return Ok(Node::Numeric {
                programs: [self.numeric(at + 8)?, self.numeric(at + 0x48)?],
                comparison,
            });
        }
        ensure!(
            class == 0x808042CB,
            "selector predicate {class:08X} requires conversion"
        );
        self.claim(prefix, 108)?;
        self.category(at, at + 88)
    }

    /// Event filters separate their source lists from the compiled property by
    /// an optional flags word. Both regions belong to the containing action.
    pub(super) fn labels(&mut self, at: usize, property: usize) -> Result<Node> {
        self.claim(at, 88)?;
        self.claim(property, 16)?;
        self.category(at, property)
    }

    fn category(&mut self, at: usize, property: usize) -> Result<Node> {
        let mut rows: [Vec<Row>; 4] = std::array::from_fn(|_| Vec::new());
        for (i, output) in rows.iter_mut().enumerate() {
            for row in self.array(at + i * 16, 0x80809787, 32)? {
                ensure!(
                    self.p.u32(row + 4)? == 0,
                    "selector category row padding differs"
                );
                output.push(Row {
                    source: row,
                    name: self.p.u32(row)?,
                    debug: self.string(row + 8)?,
                    dictionary: self.dictionary(row + 16)?,
                });
            }
        }
        Ok(Node::Category {
            rows,
            debug: self.string(at + 64)?,
            dictionary: self.dictionary(at + 72)?,
            kind: self.p.u64(property)?,
            masks: self.masks(property + 8)?,
        })
    }

    fn numeric(&mut self, at: usize) -> Result<Numeric> {
        let bindings = self
            .array(at, 0x8080920F, 8)?
            .into_iter()
            .map(|row| {
                let kind = self.p.u32(row)?;
                let name = self.p.u32(row + 4)?;
                ensure!(
                    kind <= 1 && ![0, u32::MAX, 0x811C9DC5].contains(&name),
                    "numeric selector binding kind or name differs"
                );
                Ok([kind, name])
            })
            .collect::<Result<Vec<_>>>()?;
        // Both context evaluators initialize eight Float4 input slots on the
        // stack. The serialized input count must fit that actual workspace.
        ensure!(
            bindings.len() <= 8,
            "numeric selector exceeds the native input workspace"
        );
        ensure!(
            self.p.u64(at + 48)? == bindings.len() as u64 && self.p.u64(at + 56)? == 1,
            "numeric selector dimensions differ"
        );
        let code = self
            .array(at + 16, 0x80800009, 1)?
            .into_iter()
            .map(|row| self.p.u8(row))
            .collect::<Result<Vec<_>>>()?;
        let mut constants = Vec::new();
        for row in self.array(at + 32, 0x80800090, 16)? {
            constants.extend_from_slice(&self.p.bytes::<16>(row)?);
        }
        // Validate the complete program before retaining either half.
        super::super::super::procedural::lower_program(
            &code,
            constants.len() / 16,
            bindings.len(),
        )?;
        Ok(Numeric {
            bindings,
            code,
            constants,
        })
    }

    pub(super) fn ownership(self) -> (Vec<(usize, usize)>, Vec<usize>) {
        (
            self.spans.into_iter().collect(),
            self.references.into_iter().collect(),
        )
    }

    pub fn finish(self) -> Result<()> {
        let mut end = 0;
        for (at, following) in self.spans {
            ensure!(
                self.p.0[end..at].iter().all(|v| *v == 0),
                "unparsed selector data at {end:X}"
            );
            end = following;
        }
        ensure!(
            self.p.0[end..].iter().all(|v| *v == 0),
            "unparsed selector tail at {end:X}"
        );
        Ok(())
    }
}
