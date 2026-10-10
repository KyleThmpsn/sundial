//! The private copy of an ability entity's graph tree: the walk that finds which graphs
//! and impact tables lead to an edit, and the copies that name each other.
use super::*;

/// Copies `source` and every graph below it, up to [`SPAWN_DEPTH`] levels, whose values
/// `values` change, that `patches` patch or that `appends` grow, each graph once. A value names its graph by its
/// locator's graph tag, and one without a tag belongs to `source`. Each copy names the copies of
/// the graphs under it in place of the stock ones, so the stock graphs and every other graph
/// naming them stay as they are.
/// A graph the ability bank names is reached through the bank, and the copy of `source` then
/// takes a private copy of the bank that names the copies. The build gives that copy the rows it
/// adds to the stock bank afterwards (`ability::banks::sync_private_banks`).
/// Refuses a value whose graph `source` does not reach.
pub(in crate::item) fn append_private_graph_tree(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    patches: &BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>,
    appends: &BTreeMap<u32, Vec<WeaponRuntimeResourceAppend>>,
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let mut by_graph = BTreeMap::<u32, Vec<WeaponRuntimeValueOverride>>::new();
    for value in values {
        by_graph
            .entry(
                value
                    .locator
                    .graph_tag
                    .map(|tag| tag.get())
                    .unwrap_or(source.0),
            )
            .or_default()
            .push(value.clone());
    }
    let mut tree = GraphTree {
        manager,
        edited: by_graph
            .keys()
            .chain(patches.keys())
            .chain(appends.keys())
            .copied()
            .collect(),
        spawns: BTreeMap::new(),
        selves: BTreeMap::new(),
        entries: BTreeMap::new(),
        needed: BTreeSet::new(),
        copies: BTreeMap::new(),
        reached: BTreeSet::new(),
    };
    tree.walk(source.0)?;
    if let Some(unreached) = by_graph
        .keys()
        .chain(patches.keys())
        .chain(appends.keys())
        .find(|graph| **graph != source.0 && !tree.needed.contains(*graph))
    {
        return Err(invalid(format!(
            "A value names graph 0x{unreached:08X}, which {source} does not spawn"
        )));
    }
    tree.copy(source.0, (&by_graph, patches, appends), allocator, tags)
}

/// Every graph and impact table `source` reaches within [`SPAWN_DEPTH`] levels, itself included,
/// as [`append_private_graph_tree`] walks them.
pub(super) fn graph_closure(
    manager: &PackageManager,
    source: TagHash,
) -> AuthoringResult<BTreeSet<u32>> {
    let mut tree = GraphTree {
        manager,
        edited: BTreeSet::new(),
        spawns: BTreeMap::new(),
        selves: BTreeMap::new(),
        entries: BTreeMap::new(),
        needed: BTreeSet::new(),
        copies: BTreeMap::new(),
        reached: BTreeSet::new(),
    };
    tree.walk(source.0)?;
    Ok(tree.reached)
}

/// `values` of the ability `source`, split by the tree each edits: those for graphs a projectile
/// in `replacements` reaches, by replacement, go to the private copy that projectile is swapped in
/// as, and the rest to the copy of the ability's own tree. A value for a graph both reach goes to
/// both, and one neither reaches stays with the ability, whose copy refuses it.
/// An ability's own values, and the values of each swapped-in projectile's tree by replacement.
pub(in crate::item) type SplitValues = (
    Vec<WeaponRuntimeValueOverride>,
    BTreeMap<u32, Vec<WeaponRuntimeValueOverride>>,
);

pub(in crate::item) fn split_swapped_values(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    replacements: &[u32],
) -> AuthoringResult<SplitValues> {
    let mut swapped = BTreeMap::<u32, Vec<WeaponRuntimeValueOverride>>::new();
    if replacements.is_empty() || values.is_empty() {
        return Ok((values.to_vec(), swapped));
    }
    let own = graph_closure(manager, source)?;
    let closures = replacements
        .iter()
        .map(|&replacement| Ok((replacement, graph_closure(manager, TagHash(replacement))?)))
        .collect::<AuthoringResult<Vec<_>>>()?;
    let mut kept = Vec::new();
    for value in values {
        let graph = value.locator.graph_tag.map_or(source.0, |tag| tag.get());
        let mut placed = false;
        for (replacement, closure) in &closures {
            if closure.contains(&graph) {
                swapped.entry(*replacement).or_default().push(value.clone());
                placed = true;
            }
        }
        if own.contains(&graph) || !placed {
            kept.push(value.clone());
        }
    }
    Ok((kept, swapped))
}

/// The graphs below a source that lead to edited ones, and the copies made of them.
/// Each graph's own value overrides, resource patches and owner appends, by graph.
pub(super) type GraphEdits<'a> = (
    &'a BTreeMap<u32, Vec<WeaponRuntimeValueOverride>>,
    &'a BTreeMap<u32, Vec<WeaponRuntimeResourcePatch>>,
    &'a BTreeMap<u32, Vec<WeaponRuntimeResourceAppend>>,
);

pub(super) struct GraphTree<'a> {
    pub(super) manager: &'a PackageManager,
    pub(super) edited: BTreeSet<u32>,
    /// Where each walked graph names the graphs and impact tables below it.
    pub(super) spawns: BTreeMap<u32, Vec<sundial::package_authoring::ability_spawns::Spawn>>,
    /// Where each walked graph names itself, as a chain that spawns itself again does.
    pub(super) selves: BTreeMap<u32, Vec<sundial::package_authoring::ability_spawns::Spawn>>,
    /// The graphs and tables each walked impact table names, with the fields naming each.
    pub(super) entries: BTreeMap<u32, Vec<(u32, Vec<usize>)>>,
    /// Graphs and tables below the source that are edited or lead to an edited graph.
    pub(super) needed: BTreeSet<u32>,
    /// The copy of each graph and table, its tag taken before what is below it is copied, so a
    /// route back to one, such as a chain that spawns itself again, names the copy.
    pub(super) copies: BTreeMap<u32, TagHash>,
    /// Every graph and table the walk reached, the source included.
    pub(super) reached: BTreeSet<u32>,
}

impl GraphTree<'_> {
    /// Walks every graph and impact table below `source`, each once from the shallowest level
    /// that names it, then finds those that lead to an edited graph. A table's graphs are at the
    /// table's level, the level below the graph naming it. Tables can name each other, as
    /// `80C70C91` and `80C70C92` do, so what leads to an edit is traced up from the edited graphs
    /// once every route is known. Judged while a route is still being walked, a table whose
    /// partner is on that route would lead nowhere, and the graphs above it would keep naming
    /// the stock chain.
    pub(super) fn walk(&mut self, source: u32) -> AuthoringResult<()> {
        use sundial::package_authoring::ability_spawns;
        let mut levels = BTreeMap::from([(source, 0)]);
        let mut above = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut walked = BTreeSet::new();
        // A table's entries share its level, so they go to the front and every node is walked
        // first from its shallowest level.
        let mut pending = std::collections::VecDeque::from([(source, 0)]);
        while let Some((node, depth)) = pending.pop_front() {
            if levels[&node] < depth || !walked.insert(node) {
                continue;
            }
            let table = ability_spawns::is_table(self.manager, node);
            if depth >= if table { SPAWN_DEPTH + 1 } else { SPAWN_DEPTH } {
                continue;
            }
            let (below, level) = if table {
                let entries = ability_spawns::table_entries(self.manager, node).map_err(invalid)?;
                let below = entries.iter().map(|(tag, _)| *tag).collect::<BTreeSet<_>>();
                self.entries.insert(node, entries);
                (below, depth)
            } else {
                let payload = read_tag(self.manager, TagHash(node), "spawned graph")?;
                let links = ability_spawns::links(self.manager, node, &payload).map_err(invalid)?;
                let mut places = links.spawns;
                places.extend(links.tables);
                self.selves.insert(node, links.selves);
                let below = places
                    .iter()
                    .map(|place| place.graph)
                    .collect::<BTreeSet<_>>();
                self.spawns.insert(node, places);
                (below, depth + 1)
            };
            for child in below {
                above.entry(child).or_default().insert(node);
                if levels.get(&child).is_none_or(|known| level < *known) {
                    levels.insert(child, level);
                    if level == depth {
                        pending.push_front((child, level));
                    } else {
                        pending.push_back((child, level));
                    }
                }
            }
        }
        self.reached = levels.keys().copied().collect();
        let mut leading = levels
            .keys()
            .copied()
            .filter(|node| {
                self.edited.contains(node) && !ability_spawns::is_table(self.manager, *node)
            })
            .collect::<Vec<_>>();
        while let Some(node) = leading.pop() {
            if self.needed.insert(node) {
                leading.extend(above.get(&node).into_iter().flatten().copied());
            }
        }
        Ok(())
    }

    /// Copies `table`, naming the copies of the needed graphs and tables in it at every field
    /// that names them. A table it names that names it back is copied too, so a chain such as
    /// `80C70C91` and `80C70C92`, which name each other, stays private throughout. Its tag is
    /// taken before the tables it names are copied, so a table that names itself, or the table
    /// above it, names the copies.
    pub(super) fn copy_table(
        &mut self,
        table: u32,
        edits: GraphEdits<'_>,
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<TagHash> {
        let mut payload = read_tag(self.manager, TagHash(table), "impact table")?;
        let ordinal = tags.len();
        let authored = allocator.assigned_tag(ordinal, "Private impact table", "impact table")?;
        tags.push(NewTagSpec {
            template_tag: TagHash(table),
            payload: Vec::new(),
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        self.copies.insert(table, authored);
        self.needed.insert(table);
        for (child, offsets) in self.entries.get(&table).cloned().unwrap_or_default() {
            let names_back = self
                .entries
                .get(&child)
                .is_some_and(|entries| entries.iter().any(|(tag, _)| *tag == table));
            if !(self.needed.contains(&child) || names_back || self.copies.contains_key(&child)) {
                continue;
            }
            let copy = self.copy(child, edits, allocator, tags)?;
            retarget_exact_tag_occurrences(
                &mut payload,
                TagHash(child),
                copy,
                &offsets,
                &format!("Impact table 0x{table:08X}"),
            )?;
        }
        tags[ordinal].payload = payload;
        Ok(authored)
    }

    /// Copies `graph` with its own values and patches, naming the copies of the needed graphs
    /// and impact tables below it, and of any graph above it that it names again.
    pub(super) fn copy(
        &mut self,
        graph: u32,
        (values, own_patches, appends): GraphEdits<'_>,
        allocator: AppendedTagAllocator,
        tags: &mut Vec<NewTagSpec>,
    ) -> AuthoringResult<TagHash> {
        if let Some(copy) = self.copies.get(&graph) {
            return Ok(*copy);
        }
        if self.entries.contains_key(&graph) {
            return self.copy_table(graph, (values, own_patches, appends), allocator, tags);
        }
        let ordinal = tags.len();
        let authored =
            allocator.assigned_tag(ordinal, "Private referenced graph", "runtime graph")?;
        tags.push(NewTagSpec {
            template_tag: TagHash(graph),
            payload: Vec::new(),
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        self.copies.insert(graph, authored);
        let mut patches = own_patches.get(&graph).cloned().unwrap_or_default();
        for spawn in self.spawns.get(&graph).cloned().unwrap_or_default() {
            // A place a swap patches names its replacement, not a copy of the stock graph.
            let swapped = patches.iter().any(|patch| {
                (patch.binding_hash, patch.resource_index, patch.offset)
                    == (spawn.binding_hash, spawn.resource_index, spawn.offset)
            });
            if swapped
                || !(self.needed.contains(&spawn.graph) || self.copies.contains_key(&spawn.graph))
            {
                continue;
            }
            let child = self.copy(spawn.graph, (values, own_patches, appends), allocator, tags)?;
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: spawn.binding_hash,
                resource_index: spawn.resource_index,
                offset: spawn.offset,
                bytes: child.0.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        // Where the stock graph names itself, the copy names itself.
        for place in self.selves.get(&graph).cloned().unwrap_or_default() {
            if patches.iter().any(|patch| {
                (patch.binding_hash, patch.resource_index, patch.offset)
                    == (place.binding_hash, place.resource_index, place.offset)
            }) {
                continue;
            }
            patches.push(WeaponRuntimeResourcePatch {
                binding_hash: place.binding_hash,
                resource_index: place.resource_index,
                offset: place.offset,
                bytes: authored.0.to_le_bytes().to_vec(),
                graph_values: Vec::new(),
                graph_removals: Vec::new(),
                graph_trajectories: None,
            });
        }
        let own = values.get(&graph).map_or(&[][..], Vec::as_slice);
        let grown = appends.get(&graph).map_or(&[][..], Vec::as_slice);
        tags[ordinal].payload = private_patched_graph(
            self.manager,
            TagHash(graph),
            own,
            &patches,
            grown,
            allocator,
            tags,
        )?;
        Ok(authored)
    }
}

/// Clone the graph with its edited values, raw patches and owner appends, and every component
/// owner they touch. All stock tags remain intact.
pub(super) fn append_private_patched_graph(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    patches: &[WeaponRuntimeResourcePatch],
    appends: &[WeaponRuntimeResourceAppend],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<TagHash> {
    let graph = private_patched_graph(manager, source, values, patches, appends, allocator, tags)?;
    let authored =
        allocator.assigned_tag(tags.len(), "Private referenced graph", "runtime graph")?;
    tags.push(NewTagSpec {
        template_tag: source,
        payload: graph,
        storage: crate::NewTagStorageMode::InheritTemplate,
    });
    Ok(authored)
}

/// The payload of a private copy of graph `source` with its edited values, raw patches and
/// owner appends, appending a copy of every component owner they touch.
pub(super) fn private_patched_graph(
    manager: &PackageManager,
    source: TagHash,
    values: &[WeaponRuntimeValueOverride],
    patches: &[WeaponRuntimeResourcePatch],
    appends: &[WeaponRuntimeResourceAppend],
    allocator: AppendedTagAllocator,
    tags: &mut Vec<NewTagSpec>,
) -> AuthoringResult<Vec<u8>> {
    if manager
        .get_entry(source)
        .is_none_or(|entry| entry.reference != WEAPON_ENTITY_CLASS)
    {
        return Err(invalid(format!(
            "Referenced runtime graph {source} is not a live weapon entity graph"
        )));
    }
    let mut graph = read_tag(manager, source, "referenced runtime graph")?;
    validate_weapon_entity(&graph).map_err(invalid)?;
    // Validate the source prerequisites now. Final assembly enrolls the required native
    // records along with every authored graph and action.
    sundial::package_authoring::sandbox_perk::entity::residency::inspect(manager, source.0)
        .map_err(invalid)?;
    let values = values
        .iter()
        .map(|value| {
            Ok(WeaponRuntimeValueOverride {
                locator: value.locator.for_graph(source.0.into()).map_err(invalid)?,
                value: value.value.clone(),
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    append_patched_runtime_resource_owners(
        manager, &mut graph, &values, patches, appends, allocator, tags,
    )?;
    validate_weapon_entity(&graph).map_err(invalid)?;
    Ok(graph)
}
