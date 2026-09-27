use super::*;
use crate::catalog::PresentationNodeChildren;

pub(in crate::catalog) fn scan_presentation_nodes(
    package: &mut ProgressionPackageData<'_>,
    objective_count: usize,
    flag_definitions: &mut [UnlockDefinition],
    value_definitions: &mut [UnlockDefinition],
) -> Result<Vec<PresentationNodeDef>, String> {
    let ProgressionPackageData {
        manager,
        root,
        globals,
        localized_tags,
        localized_cache,
    } = package;
    let definitions = manager
        .read_tag(TagHash(u32_at(
            root,
            8 + PRESENTATION_NODE_DEFINITION_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node definitions: {error}"))?;
    let strings = manager
        .read_tag(TagHash(u32_at(
            globals,
            16 + PRESENTATION_NODE_STRING_TABLE_SLOT * 16,
        )?))
        .map_err(|error| format!("Could not read presentation-node strings: {error}"))?;
    let (definition_count, definition_rows, definition_class) = array_at(&definitions, 8)?;
    let (string_count, string_rows, _) = array_at(&strings, 8)?;
    if definition_class != PRESENTATION_NODE_DEFINITION_ROW_CLASS {
        return Err(format!(
            "The installed presentation-node definition table has row class 0x{definition_class:08X}; expected 0x{PRESENTATION_NODE_DEFINITION_ROW_CLASS:08X}"
        ));
    }
    if definition_count != string_count {
        return Err(
            "The installed presentation-node definition and string tables do not match".into(),
        );
    }

    let mut nodes = Vec::with_capacity(definition_count);
    for index in 0..definition_count {
        let definition = definition_rows + index * PRESENTATION_NODE_DEFINITION_ROW_SIZE;
        let string = string_rows + index * PRESENTATION_NODE_STRING_ROW_SIZE;
        let hash = u32_at(&definitions, definition + PRESENTATION_NODE_HASH_OFFSET)?;
        if u32_at(&strings, string)? != hash {
            return Err(format!(
                "Presentation-node definition and string row {index} do not match"
            ));
        }
        let name = [0x08, 0x10]
            .into_iter()
            .filter_map(|offset| {
                resolve_string(
                    manager,
                    localized_tags,
                    localized_cache,
                    &strings,
                    string + offset,
                )
            })
            .find(|value| !value.trim().is_empty())
            .unwrap_or_default();
        let parents = definition_index_list(
            &definitions,
            definition + PRESENTATION_NODE_PARENTS_OFFSET,
            PRESENTATION_NODE_INDEX_ROW_CLASS,
            definition_count,
            "presentation-node parent",
        )?;
        let objective_index = usize::from(u16_at(
            &definitions,
            definition + PRESENTATION_NODE_OBJECTIVE_INDEX_OFFSET,
        )?);
        let objective_index = (objective_index != usize::from(u16::MAX)
            && objective_index < objective_count)
            .then_some(objective_index);
        let mut condition_references = ConditionReferences::default();
        for offset in PRESENTATION_NODE_CONDITION_OFFSETS {
            merge_condition_references(
                &mut condition_references,
                condition_references_at(&definitions, definition + offset)?,
            );
        }
        nodes.push(PresentationNodeDef {
            hash: u64::from(hash),
            name,
            parents,
            objective_index,
            condition_references,
        });
    }
    for node in &nodes {
        let context = ProgressionContextDef {
            direct_references: Vec::new(),
            hash: node.hash,
            kind: ProgressionContextKind::PresentationNode,
            name: node.name.clone(),
            type_name: String::new(),
            description: String::new(),
            paths: presentation_paths(&nodes, &node.parents),
            condition_programs: Vec::new(),
        };
        attach_condition_context(
            flag_definitions,
            value_definitions,
            &node.condition_references,
            &context,
        );
    }
    Ok(nodes)
}

pub(in crate::catalog) fn attach_presentation_node_objective_owners(
    objectives: &mut [ObjectiveDef],
    nodes: &[PresentationNodeDef],
) {
    for node in nodes {
        let Some(objective_index) = node.objective_index else {
            continue;
        };
        add_objective_owner(
            objectives,
            objective_index,
            ObjectiveOwnerDef {
                hash: node.hash,
                kind: ObjectiveOwnerKind::PresentationNode,
                name: node.name.clone(),
                type_name: "Presentation node".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: presentation_paths(nodes, &node.parents),
            },
        );
    }
}

pub(in crate::catalog) fn presentation_paths(
    nodes: &[PresentationNodeDef],
    parents: &[usize],
) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for &parent in parents {
        collect_presentation_paths(nodes, parent, &mut Vec::new(), &mut Vec::new(), &mut paths);
    }
    paths.retain(|path| !path.is_empty());
    let mut deduplicated = Vec::new();
    for path in paths {
        if !deduplicated.contains(&path) {
            deduplicated.push(path);
        }
    }
    deduplicated
}

pub(super) fn collect_presentation_paths(
    nodes: &[PresentationNodeDef],
    index: usize,
    visited: &mut Vec<usize>,
    names: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
) {
    if visited.contains(&index) {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    }
    let Some(node) = nodes.get(index) else {
        if !names.is_empty() {
            paths.push(names.clone());
        }
        return;
    };
    visited.push(index);
    let added_name = !node.name.trim().is_empty();
    if added_name {
        names.push(node.name.clone());
    }
    if node.parents.is_empty() {
        if !names.is_empty() {
            paths.push(names.clone());
        }
    } else {
        for &parent in &node.parents {
            collect_presentation_paths(nodes, parent, visited, names, paths);
        }
    }
    if added_name {
        names.pop();
    }
    visited.pop();
}

pub(in crate::catalog) fn definition_index_list(
    data: &[u8],
    descriptor: usize,
    expected_class: u32,
    definition_count: usize,
    label: &str,
) -> Result<Vec<usize>, String> {
    if u64_at(data, descriptor)? == 0 {
        return Ok(Vec::new());
    }
    let (count, rows, class) = array_at(data, descriptor)?;
    if class != expected_class {
        return Err(format!("Unexpected {label} row class 0x{class:08X}"));
    }
    if count > definition_count {
        return Err(format!("{label} list is larger than its definition table"));
    }
    let byte_count = count
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| format!("{label} list size overflowed"))?;
    let end = rows
        .checked_add(byte_count)
        .ok_or_else(|| format!("{label} list offset overflowed"))?;
    if end > data.len() {
        return Err(format!("{label} list extends beyond its package data"));
    }
    (0..count)
        .map(|index| {
            let definition_index = usize::from(u16_at(data, rows + index * size_of::<u16>())?);
            if definition_index >= definition_count {
                Err(format!("{label} index {definition_index} is out of range"))
            } else {
                Ok(definition_index)
            }
        })
        .collect()
}

/// Converts scanned nodes to their cached form, with parent indices resolved to hashes.
pub(in crate::catalog) fn presentation_node_models(
    nodes: &[PresentationNodeDef],
) -> Vec<PresentationNode> {
    nodes
        .iter()
        .map(|node| PresentationNode {
            hash: node.hash,
            name: node.name.clone(),
            parents: parent_node_hashes(nodes, &node.parents),
        })
        .collect()
}

/// Resolves parent node indices to hashes in package order, without repeats.
pub(in crate::catalog) fn parent_node_hashes(
    nodes: &[PresentationNodeDef],
    parents: &[usize],
) -> Vec<u64> {
    let mut hashes = Vec::with_capacity(parents.len());
    for node in parents.iter().filter_map(|&parent| nodes.get(parent)) {
        if !hashes.contains(&node.hash) {
            hashes.push(node.hash);
        }
    }
    hashes
}

/// Presentation-node lookups, built on first use and never serialized.
#[derive(Default)]
pub(in crate::catalog) struct PresentationIndex {
    nodes: HashMap<u64, usize>,
    collectibles: HashMap<u64, usize>,
    records: HashMap<u64, usize>,
    children: HashMap<u64, ChildPositions>,
}

/// Table positions of the definitions listing one node as a parent.
#[derive(Default)]
struct ChildPositions {
    nodes: Vec<usize>,
    collectibles: Vec<usize>,
    records: Vec<usize>,
}

impl PresentationIndex {
    fn build(catalog: &Catalog) -> Self {
        let mut index = Self::default();
        for (position, node) in catalog.presentation_nodes.iter().enumerate() {
            index.nodes.entry(node.hash).or_insert(position);
            for parent in &node.parents {
                index
                    .children
                    .entry(*parent)
                    .or_default()
                    .nodes
                    .push(position);
            }
        }
        for (position, collectible) in catalog.collectibles.iter().enumerate() {
            index
                .collectibles
                .entry(collectible.hash)
                .or_insert(position);
            for parent in &collectible.parent_nodes {
                index
                    .children
                    .entry(*parent)
                    .or_default()
                    .collectibles
                    .push(position);
            }
        }
        for (position, record) in catalog.records.iter().flatten().enumerate() {
            index.records.entry(record.hash).or_insert(position);
            for parent in &record.parent_nodes {
                index
                    .children
                    .entry(*parent)
                    .or_default()
                    .records
                    .push(position);
            }
        }
        for children in index.children.values_mut() {
            children.nodes.dedup();
            children.collectibles.dedup();
            children.records.dedup();
        }
        index
    }
}

impl Catalog {
    /// Every presentation node in table order.
    #[cfg(test)]
    pub(crate) fn presentation_nodes(&self) -> &[PresentationNode] {
        &self.presentation_nodes
    }

    pub(crate) fn presentation_node(&self, hash: u64) -> Option<&PresentationNode> {
        let position = *self.presentation_index().nodes.get(&hash)?;
        self.presentation_nodes.get(position)
    }

    /// The nodes, collectibles and records listing this node as a parent, in table order.
    pub(crate) fn presentation_node_children(&self, hash: u64) -> PresentationNodeChildren<'_> {
        let Some(children) = self.presentation_index().children.get(&hash) else {
            return PresentationNodeChildren::default();
        };
        let records = self.records.as_deref().unwrap_or_default();
        PresentationNodeChildren {
            nodes: children
                .nodes
                .iter()
                .filter_map(|&position| self.presentation_nodes.get(position))
                .collect(),
            collectibles: children
                .collectibles
                .iter()
                .filter_map(|&position| self.collectibles.get(position))
                .collect(),
            records: children
                .records
                .iter()
                .filter_map(|&position| records.get(position))
                .collect(),
        }
    }

    /// Root-first chain of first parents above `hash` (the node, collectible or record itself
    /// excluded). Empty when unknown.
    pub(crate) fn presentation_path(&self, hash: u64) -> Vec<&PresentationNode> {
        let index = self.presentation_index();
        let records = self.records.as_deref().unwrap_or_default();
        let first_parent = if let Some(node) = self.presentation_node(hash) {
            node.parents.first()
        } else if let Some(collectible) = index
            .collectibles
            .get(&hash)
            .and_then(|&position| self.collectibles.get(position))
        {
            collectible.parent_nodes.first()
        } else {
            index
                .records
                .get(&hash)
                .and_then(|&position| records.get(position))
                .and_then(|record| record.parent_nodes.first())
        };
        self.presentation_chain(hash, first_parent.copied())
    }

    /// Follows first parents up from `parent` and returns the chain root first. Stops at a
    /// repeated node or at `start`, so cyclic tables still end.
    pub(in crate::catalog) fn presentation_chain(
        &self,
        start: u64,
        parent: Option<u64>,
    ) -> Vec<&PresentationNode> {
        let mut chain = Vec::<&PresentationNode>::new();
        let mut next = parent;
        while let Some(hash) = next {
            if hash == start || chain.iter().any(|node| node.hash == hash) {
                break;
            }
            let Some(node) = self.presentation_node(hash) else {
                break;
            };
            chain.push(node);
            next = node.parents.first().copied();
        }
        chain.reverse();
        chain
    }

    fn presentation_index(&self) -> &PresentationIndex {
        self.presentation_index
            .get_or_init(|| PresentationIndex::build(self))
    }
}
