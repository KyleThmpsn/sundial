//! Native placement, dependency slots and source category mask preservation.
use super::*;

pub(super) struct Write<'a> {
    resources: &'a Resources,
    bytes: Vec<u8>,
    references: Vec<Reference>,
    gates: Vec<Gate>,
    named_conditions: BTreeSet<u32>,
    strings: Vec<(String, Vec<usize>)>,
    resolver: Option<&'a ResourceResolver>,
    external_classes: BTreeMap<usize, u32>,
    groups: Option<&'a GroupBindings>,
    group_aliases: Vec<GroupAliasUse>,
}

impl<'a> Write<'a> {
    pub fn new(resources: &'a Resources, resolver: Option<&'a ResourceResolver>) -> Self {
        Self {
            resources,
            bytes: vec![0; 40],
            references: Vec::new(),
            gates: Vec::new(),
            named_conditions: BTreeSet::new(),
            strings: Vec::new(),
            resolver,
            external_classes: BTreeMap::new(),
            groups: None,
            group_aliases: Vec::new(),
        }
    }

    fn prefix(&mut self, class: u32, alignment: usize) -> Result<usize> {
        let at = self
            .bytes
            .len()
            .checked_add(4 + alignment - 1)
            .context("selector output overflow")?
            & !(alignment - 1);
        self.bytes.resize(at, 0);
        put(&mut self.bytes, at - 4, &class.to_le_bytes())?;
        Ok(at)
    }

    fn reserve(&mut self, count: usize) -> Result<()> {
        let end = self
            .bytes
            .len()
            .checked_add(count)
            .context("selector output overflow")?;
        self.bytes.resize(end, 0);
        Ok(())
    }

    fn relative(&mut self, field: usize, at: usize) -> Result<()> {
        let delta =
            i64::try_from(at as i128 - field as i128).context("selector pointer overflow")?;
        put(&mut self.bytes, field, &delta.to_le_bytes())
    }

    fn array(&mut self, field: usize, class: u32, count: usize, stride: usize) -> Result<usize> {
        if count == 0 {
            return Ok(0);
        }
        let at = self.prefix(0x80809FBD, 16)?;
        self.bytes.extend_from_slice(&(count as u64).to_le_bytes());
        self.bytes
            .extend_from_slice(&u64::from(class).to_le_bytes());
        self.reserve(
            count
                .checked_mul(stride)
                .context("selector output array overflow")?,
        )?;
        put(&mut self.bytes, field, &(count as u64).to_le_bytes())?;
        self.relative(field + 8, at)?;
        Ok(at + 16)
    }

    fn string(&mut self, field: usize, text: Option<String>) {
        if let Some(text) = text {
            if let Some((_, fields)) = self.strings.iter_mut().find(|(value, _)| *value == text) {
                fields.push(field);
            } else {
                self.strings.push((text, vec![field]));
            }
        }
    }

    fn dictionary(&mut self, at: usize, source: u32) -> Result<()> {
        let target = self.resources.tags.get(&source).copied();
        if let Some(tag) = target {
            ensure!(
                (0x80800001..=0x81FFFFFF).contains(&tag) && tag != 0x811C9DC5,
                "invalid native selector dictionary tag"
            );
        }
        put(
            &mut self.bytes,
            at,
            &target.unwrap_or(u32::MAX).to_le_bytes(),
        )?;
        self.references.push(Reference {
            offset: at,
            source,
            target,
        });
        self.external_classes
            .insert(at, crate::d2_mot::native::categories::NATIVE_CLASS);
        Ok(())
    }

    fn masks(&mut self, mask: Mask) -> Result<usize> {
        let count = mask.words.len() / 14;
        ensure!(count == 2 || count == 4, "selector mask count differs");
        let at = self.prefix(if count == 2 { 0x808093F6 } else { 0x808093F5 }, 4)?;
        self.reserve(count * 40)?;
        for (word, bits) in mask.words.into_iter().enumerate() {
            for bit in 0..32 {
                if bits & (1u32 << bit) == 0 {
                    continue;
                }
                let index = (word % 14) * 32 + bit;
                let name = *self
                    .resources
                    .names
                    .get(index)
                    .context("selector category outside dictionary")?;
                if let Some(target) = self.resources.categories[index] {
                    let target = usize::from(target);
                    ensure!(target < 320, "native selector category outside mask");
                    self.bytes[at + (word / 14) * 40 + target / 8] |= 1 << (target % 8);
                } else {
                    self.gates.push(Gate {
                        offset: mask.source + word * 4,
                        index,
                        name,
                    });
                }
            }
        }
        self.bytes.extend_from_slice(&mask.tail);
        Ok(at)
    }

    fn selector(&mut self, flags: u64, rows: Vec<(u64, Node)>, at: usize) -> Result<()> {
        put(&mut self.bytes, at, &flags.to_le_bytes())?;
        let data = self.array(at + 8, 0x80809316, rows.len(), 16)?;
        for (i, (value, node)) in rows.into_iter().enumerate() {
            let row = data + i * 16;
            put(&mut self.bytes, row, &value.to_le_bytes())?;
            let target = self.node(node)?;
            self.relative(row + 8, target)?;
        }
        Ok(())
    }

    pub(super) fn node(&mut self, node: Node) -> Result<usize> {
        match node {
            Node::External { debug, source } => {
                let target = self
                    .resolver
                    .context("external selector requires resource class evidence")?
                    .target(source, self.resources)?;
                let at = self.prefix(0x8080930F, 8)?;
                self.reserve(16)?;
                self.string(at, debug);
                put(&mut self.bytes, at + 8, &target.to_le_bytes())?;
                self.references.push(Reference {
                    offset: at + 8,
                    source,
                    target: Some(target),
                });
                self.external_classes.insert(at + 8, NATIVE_CLASS);
                Ok(at)
            }
            Node::Raw { class, data } => {
                if class == 0x80804D75 {
                    self.named_conditions.insert(u32::from_le_bytes(
                        data.as_slice()
                            .try_into()
                            .context("named selector field width")?,
                    ));
                }
                let at = self.prefix(class, 4)?;
                self.bytes.extend_from_slice(&data);
                Ok(at)
            }
            Node::Selector { flags, rows } => {
                let at = self.prefix(0x80809310, 8)?;
                self.reserve(24)?;
                self.selector(flags, rows, at)?;
                Ok(at)
            }
            Node::Factions(values) => {
                let at = self.prefix(0x80804D78, 8)?;
                self.reserve(16)?;
                let data = self.array(at, 0x80806829, values.len(), 1)?;
                if !values.is_empty() {
                    put(&mut self.bytes, data, &values)?;
                }
                Ok(at)
            }
            Node::Category {
                rows,
                debug,
                dictionary,
                kind,
                masks,
            } => {
                let at = self.prefix(0x80804D73, 8)?;
                self.reserve(96)?;
                for (i, items) in rows.into_iter().enumerate() {
                    let data = self.array(at + i * 16, 0x808094B3, items.len(), 24)?;
                    for (n, row) in items.into_iter().enumerate() {
                        let field = data + n * 24;
                        let mut name = row.name;
                        if let Some(index) = self
                            .resources
                            .names
                            .iter()
                            .position(|name| *name == row.name)
                        {
                            if self.resources.categories[index].is_none() {
                                self.gates.push(Gate {
                                    offset: row.source,
                                    index,
                                    name: row.name,
                                });
                            }
                        } else {
                            name = self
                                .groups
                                .context("selector group correspondence missing")?
                                .validate(row.name, row.source, self.resources, &mut self.gates)?;
                        }
                        if name != row.name {
                            self.group_aliases.push(GroupAliasUse {
                                source: row.name,
                                native: name,
                                source_offset: row.source,
                                native_offset: field,
                            });
                        }
                        put(&mut self.bytes, field, &name.to_le_bytes())?;
                        self.string(field + 8, row.debug);
                        self.dictionary(field + 16, row.dictionary)?;
                    }
                }
                self.string(at + 64, debug);
                self.dictionary(at + 72, dictionary)?;
                put(&mut self.bytes, at + 80, &kind.to_le_bytes())?;
                if let Some(mask) = masks {
                    let target = self.masks(mask)?;
                    self.relative(at + 88, target)?;
                }
                Ok(at)
            }
        }
    }

    fn flush_strings(&mut self) -> Result<()> {
        for (text, fields) in std::mem::take(&mut self.strings) {
            let at = self.prefix(0x80800065, 8)?;
            self.bytes
                .extend_from_slice(&((text.len() + 1) as u64).to_le_bytes());
            self.bytes.extend_from_slice(text.as_bytes());
            self.bytes.push(0);
            for field in fields {
                self.relative(field, at + 8)?;
            }
        }
        Ok(())
    }

    pub(super) fn with_groups(mut self, groups: &'a GroupBindings) -> Self {
        self.groups = Some(groups);
        self
    }

    pub(super) fn fragment(
        mut self,
        root: Node,
        destination: &Payload,
    ) -> Result<(Selector, usize)> {
        self.bytes = destination.0.clone();
        let at = self.node(root)?;
        self.flush_strings()?;
        Ok((
            Selector {
                payload: Payload(self.bytes),
                references: self.references,
                gates: self.gates,
                named_conditions: self.named_conditions.into_iter().collect(),
                external_classes: self.external_classes,
                group_aliases: self.group_aliases,
            },
            at,
        ))
    }

    pub(super) fn inline(
        mut self,
        root: Node,
        destination: &Payload,
        at: usize,
    ) -> Result<Selector> {
        let Node::Selector { flags, rows } = root else {
            anyhow::bail!("inline value selector class differs");
        };
        self.bytes = destination.0.clone();
        self.selector(flags, rows, at)?;
        self.flush_strings()?;
        Ok(Selector {
            payload: Payload(self.bytes),
            references: self.references,
            gates: self.gates,
            named_conditions: self.named_conditions.into_iter().collect(),
            external_classes: self.external_classes,
            group_aliases: self.group_aliases,
        })
    }

    pub fn finish(mut self, root: Node) -> Result<Selector> {
        let Node::Selector { flags, rows } = root else {
            anyhow::bail!("selector root class differs");
        };
        self.selector(flags, rows, 8)?;
        self.flush_strings()?;
        let count = self.bytes.len() as u64;
        put(&mut self.bytes, 0, &count.to_le_bytes())?;
        Ok(Selector {
            payload: Payload(self.bytes),
            references: self.references,
            external_classes: self.external_classes,
            gates: self.gates,
            named_conditions: self.named_conditions.into_iter().collect(),
            group_aliases: self.group_aliases,
        })
    }
}
