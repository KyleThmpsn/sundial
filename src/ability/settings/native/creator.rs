use super::*;

#[derive(Clone, Copy)]
struct Child {
    at: usize,
    class: u32,
    source: u32,
}

impl Reader<'_> {
    fn children(&mut self, definition: usize, source: usize) -> Result<Vec<Option<Child>>, String> {
        let definitions = self.array(definition, 0x8080_93E6)?;
        let sources = self.array(source, 0x8080_93E5)?;
        if definitions.len() != sources.len() {
            return Err("Creator arrays disagree".into());
        }
        definitions
            .into_iter()
            .zip(sources)
            .map(|(definition, source)| {
                if self.pair(definition, 0x8080_93E6, 0x8080_93E5)? != source {
                    return Err("Creator row selects another source".into());
                }
                let dr = i64_at(self.data, definition + 0x10)?;
                let sr = i64_at(self.data, source + 0x20)?;
                if dr == 0 && sr == 0 {
                    return Ok(None);
                }
                if dr == 0 || sr == 0 {
                    return Err("Creator child pair disagrees about presence".into());
                }
                let at = relative_offset(definition, 0x10, dr)?;
                let state = relative_offset(source, 0x20, sr)?;
                if at < 4 || state < 4 {
                    return Err("Creator child has no type marker".into());
                }
                let class = u32_at(self.data, at - 4)?;
                let source_class = u32_at(self.data, state - 4)?;
                if self.pair(at, class, source_class)? != state {
                    return Err("Creator child selects another state".into());
                }
                Ok(Some(Child {
                    at,
                    class,
                    source: source_class,
                }))
            })
            .collect()
    }

    /// The graph a creating node names, when it is one: a kind-15 node, whose creation path
    /// (`40C0A0`) loads the graph from +0x74.
    fn created(&self, node: &Child) -> Result<Option<u32>, String> {
        if node.class != 0x8080_8881
            || node.source != 0x8080_8880
            || bytes_at::<1>(self.data, node.at + 0x30)?[0] != 15
        {
            return Ok(None);
        }
        let graph = u32_at(self.data, node.at + 0x74)?;
        Ok((graph != 0 && graph != u32::MAX).then_some(graph))
    }

    /// What each creating group and creating node of this root makes, by where its settings sit:
    /// a group, the graphs of the kind-15 nodes it selects, and a node, the graph it names.
    pub(super) fn creations(&mut self) -> Result<Vec<(usize, Vec<u32>)>, String> {
        let root = self.root.owner_offset as usize;
        let source = self.pair(root, 0x8080_84E9, 0x8080_84D7)?;
        let groups = self.children(root + 0x158, source + 0xA0)?;
        let nodes = self.children(root + 0x168, source + 0xB0)?;
        let mut found = Vec::new();
        for group in groups.iter().flatten() {
            if !matches!(
                (group.class, group.source),
                (0x8080_93D7, 0x8080_93D6) | (0x8080_93D9, 0x8080_93D8)
            ) {
                continue;
            }
            let mut graphs = Vec::new();
            for selector in self.array(group.at + 0x38, 0x8080_93FB)? {
                let word = u32_at(self.data, selector)?;
                if word & 0xFF == 1
                    && let Some(Some(node)) = nodes.get((word >> 16) as usize)
                    && let Some(graph) = self.created(node)?
                    && !graphs.contains(&graph)
                {
                    graphs.push(graph);
                }
            }
            found.push((group.at, graphs));
        }
        for node in nodes.iter().flatten() {
            if let Some(graph) = self.created(node)? {
                found.push((node.at, vec![graph]));
            }
        }
        Ok(found)
    }

    pub(super) fn creator(&mut self) -> Result<(), String> {
        let root = self.root.owner_offset as usize;
        let source = self.pair(root, 0x8080_84E9, 0x8080_84D7)?;
        let groups = self.children(root + 0x158, source + 0xA0)?;
        let nodes = self.children(root + 0x168, source + 0xB0)?;
        for (index, group) in groups.iter().enumerate() {
            let Some(group) = group else {
                continue;
            };
            if !matches!(
                (group.class, group.source),
                (0x8080_93D7, 0x8080_93D6) | (0x8080_93D9, 0x8080_93D8)
            ) {
                continue;
            }
            let at = group.at;
            if bytes_at::<1>(self.data, at + 0x30)?[0] > 1
                || bytes_at::<1>(self.data, at + 0x32)?[0] != 0
            {
                continue;
            }
            let mut creates = false;
            for selector in self.array(at + 0x38, 0x8080_93FB)? {
                let word = u32_at(self.data, selector)?;
                let selected = (word >> 16) as usize;
                if word & 0xFF00 != 0 {
                    return Err("Creator child selector has unsupported flags".into());
                }
                let child = match word & 0xFF {
                    0 => groups.get(selected),
                    1 => nodes.get(selected),
                    _ => return Err("Creator child selector has an unknown domain".into()),
                }
                .ok_or("Creator child selector leaves its array")?;
                if let Some(child) = child {
                    creates |= word & 0xFF == 1
                        && child.class == 0x8080_8881
                        && child.source == 0x8080_8880
                        && bytes_at::<1>(self.data, child.at + 0x30)?[0] == 15;
                }
            }
            if !creates || !self.seconds(at + 0x1C)? || !self.seconds(at + 0x24)? {
                continue;
            }
            let route = [index as u32];
            self.put(
                Property::MinimumActivationDelay,
                group.class,
                at,
                0x1C,
                &route,
                Codec::Float,
            )?;
            // Keep a dormant maximum editable. Its UI gate follows the draft's minimum delay.
            self.put(
                Property::MaximumActivationDelay,
                group.class,
                at,
                0x20,
                &route,
                Codec::Float,
            )?;
            if u32_at(self.data, at + 0x18)? & 0x18 == 0 {
                self.put(
                    Property::MinimumCycleDuration,
                    group.class,
                    at,
                    0x24,
                    &route,
                    Codec::Float,
                )?;
                self.put(
                    Property::MaximumCycleDuration,
                    group.class,
                    at,
                    0x28,
                    &route,
                    Codec::Float,
                )?;
            }
            self.put(
                Property::RepeatCount,
                group.class,
                at,
                0x31,
                &route,
                Codec::UnsignedByte,
            )?;
        }
        for (index, node) in nodes.into_iter().enumerate() {
            let Some(node) = node else {
                continue;
            };
            if node.class != 0x8080_8881
                || node.source != 0x8080_8880
                || bytes_at::<1>(self.data, node.at + 0x30)?[0] != 15
                || u32_at(self.data, node.at + 0x18)? & 0x18 != 0
                || !self.seconds(node.at + 0x24)?
                || self.float(node.at + 0x28)? <= 0.0001
            {
                continue;
            }
            self.put(
                Property::MinimumPartDuration,
                node.class,
                node.at,
                0x24,
                &[index as u32],
                Codec::Float,
            )?;
            self.put(
                Property::MaximumPartDuration,
                node.class,
                node.at,
                0x28,
                &[index as u32],
                Codec::Float,
            )?;
        }
        Ok(())
    }
}
