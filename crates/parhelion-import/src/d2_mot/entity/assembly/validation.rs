//! Validation phases for source topology and supplied native conversion contracts.
use super::*;

pub(super) struct Coverage {
    pub covered: Vec<bool>,
    pub component_rows: Vec<[u8; 12]>,
}

pub(super) struct Components {
    pub by_source: BTreeMap<u32, u32>,
    pub targets: Vec<u32>,
}

pub(super) struct Audit {
    pub blockers: Vec<Blocker>,
    pub topology_valid: bool,
}

impl Audit {
    pub(super) fn new() -> Self {
        Self {
            blockers: Vec::new(),
            topology_valid: true,
        }
    }

    pub(super) fn coverage(&mut self, source: &Payload) -> Result<Coverage> {
        let mut covered = vec![false; source.0.len()];
        let mut occupied = vec![false; source.0.len()];
        covered[..56].fill(true);
        occupied[..56].fill(true);
        let mut component_rows = Vec::new();
        for (descriptor, stride, class) in [
            (8, 12, 0x80809ACD),
            (24, 56, 0x80809A8F),
            (40, 56, 0x80809A8F),
        ] {
            let rows = source.array(descriptor, stride, Some(class))?;
            if rows.is_empty() {
                if source.u64(descriptor + 8)? != 0 {
                    blocker(
                        &mut self.blockers,
                        "empty_array_pointer",
                        format!("descriptor {descriptor:X}"),
                    );
                }
                continue;
            }
            let header = source.pointer(descriptor + 8)?;
            let end = rows.last().context("nonempty entity array")? + stride;
            if occupied[header..end].iter().any(|&used| used) {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "entity_array_overlap",
                    format!("descriptor {descriptor:X}, span {header:X}..{end:X}"),
                );
            }
            occupied[header..end].fill(true);
            if source.u32(header + 12)? != 0 {
                blocker(
                    &mut self.blockers,
                    "array_header_padding",
                    format!("header {header:X}"),
                );
            }
            covered[header..header + 16].fill(true);
            for at in rows {
                if descriptor == 8 {
                    component_rows.push(source.bytes::<12>(at)?);
                    covered[at..at + 4].fill(true);
                } else {
                    covered[at..at + stride].fill(true);
                }
            }
        }
        Ok(Coverage {
            covered,
            component_rows,
        })
    }

    pub(super) fn components(
        &mut self,
        contracts: &Contracts,
        native_owners: &BTreeMap<u32, Payload>,
        original: &Graph,
    ) -> Components {
        let mut components = BTreeMap::new();
        let mut targets = BTreeSet::new();
        for component in &contracts.components {
            if components
                .insert(component.source, component.target)
                .is_some()
                || !targets.insert(component.target)
                || !original.components.contains(&component.source)
            {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "ambiguous_component",
                    format!("{:08X}", component.source),
                );
            }
            match native_owners.get(&component.target) {
                Some(owner) if owner.u64(0).ok() == Some(owner.0.len() as u64) => {}
                _ => {
                    self.topology_valid = false;
                    blocker(
                        &mut self.blockers,
                        "incomplete_component",
                        format!("{:08X}", component.target),
                    );
                }
            }
        }
        let target_components = original
            .components
            .iter()
            .filter_map(|tag| {
                let target = components.get(tag).copied();
                if target.is_none() {
                    self.topology_valid = false;
                    blocker(
                        &mut self.blockers,
                        "missing_component",
                        format!("{tag:08X}"),
                    );
                }
                target
            })
            .collect::<Vec<_>>();
        Components {
            by_source: components,
            targets: target_components,
        }
    }

    pub(super) fn objects(
        &mut self,
        relocations: &[Relocation],
        components: &BTreeMap<u32, u32>,
        original: &Graph,
        source_owners: &BTreeMap<u32, Payload>,
        native_owners: &BTreeMap<u32, Payload>,
    ) -> BTreeMap<Object, Object> {
        let mut objects = BTreeMap::new();
        let mut target_objects = BTreeMap::new();
        for relocation in relocations {
            if let Some(previous) = objects.get(&relocation.source) {
                if *previous == relocation.target {
                    continue;
                }
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "ambiguous_object",
                    format!(
                        "{:?}: {:?} versus {:?}",
                        relocation.source, previous, relocation.target
                    ),
                );
                continue;
            }
            objects.insert(relocation.source, relocation.target);
            if let Some(previous) = target_objects.insert(relocation.target, relocation.source) {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "unsupported_object_collapse",
                    format!(
                        "{previous:?} and {:?} -> {:?}",
                        relocation.source, relocation.target
                    ),
                );
            }
            if !original.components.contains(&relocation.source.owner) {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "unsupported_external_owner",
                    format!("{:?}", relocation.source),
                );
            }
            let valid = components.get(&relocation.source.owner) == Some(&relocation.target.owner)
                && relocation.source.class != u32::MAX
                && relocation.target.class != u32::MAX
                && source_owners
                    .get(&relocation.source.owner)
                    .is_some_and(|owner| {
                        usize::try_from(relocation.source.offset)
                            .ok()
                            .is_some_and(|at| owner.bytes::<16>(at).is_ok())
                    })
                && native_owners
                    .get(&relocation.target.owner)
                    .is_some_and(|owner| {
                        usize::try_from(relocation.target.offset)
                            .ok()
                            .is_some_and(|at| owner.bytes::<16>(at).is_ok())
                    });
            if !valid {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "invalid_object",
                    format!("{:?} -> {:?}", relocation.source, relocation.target),
                );
            }
        }
        objects
    }

    pub(super) fn channels(
        &mut self,
        contracts: &Contracts,
        graph: &Graph,
        objects: &BTreeMap<Object, Object>,
    ) -> BTreeMap<(Object, u32), u32> {
        let mut channels = BTreeMap::new();
        for channel in &contracts.channels {
            if channels
                .insert((channel.provider, channel.source), channel.target)
                .is_some()
            {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "ambiguous_channel",
                    format!("{:?}:{}", channel.provider, channel.source),
                );
            }
        }
        for edge in graph.connections.iter().chain(&graph.named_connections) {
            for object in [edge.consumer.object, edge.provider.object]
                .into_iter()
                .flatten()
            {
                if contracts.unmapped_interfaces.contains(&object) {
                    self.topology_valid = false;
                    blocker(
                        &mut self.blockers,
                        "unmapped_interface",
                        format!("{object:?}"),
                    );
                }
                if !objects.contains_key(&object) {
                    self.topology_valid = false;
                    blocker(&mut self.blockers, "missing_object", format!("{object:?}"));
                }
            }
            if edge.flags != 0 {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "connection_flags",
                    format!("{:08X}", edge.flags),
                );
            }
            if let Some(provider) = edge.provider.object {
                if let Some(channel) = contracts
                    .channels
                    .iter()
                    .find(|c| c.provider == provider && c.source == edge.channel)
                {
                    for unsupported in contracts
                        .unsupported
                        .iter()
                        .filter(|u| u.object == provider)
                    {
                        if channel.methods.is_empty()
                            || channel
                                .methods
                                .iter()
                                .any(|m| unsupported.methods.contains(m))
                        {
                            self.topology_valid = false;
                            blocker(
                                &mut self.blockers,
                                "unsupported_method",
                                format!("{provider:?}:{}", edge.channel),
                            );
                        }
                    }
                }
            }
        }
        channels
    }

    pub(super) fn externals(&mut self, contracts: &Contracts) -> BTreeMap<(Object, u64), u64> {
        let mut externals = BTreeMap::new();
        for external in &contracts.externals {
            if external.source <= 0xFFFF
                || external.target <= 0xFFFF
                || externals
                    .insert((external.object, external.source), external.target)
                    .is_some()
            {
                self.topology_valid = false;
                blocker(
                    &mut self.blockers,
                    "external_selector",
                    format!("{:?}", external.object),
                );
            }
        }
        externals
    }

    pub(super) fn dependencies(
        &mut self,
        contracts: &Contracts,
        native_owners: &BTreeMap<u32, Payload>,
        banks: &[&Bank],
    ) {
        for dependency in &contracts.dependencies {
            let valid = dependency.class != u32::MAX
                && dependency.target.is_some_and(|target| {
                    native_owners
                        .get(&dependency.owner)
                        .and_then(|owner| owner.u32(dependency.offset).ok())
                        == Some(target)
                        && contracts.resource_classes.get(&target) == Some(&dependency.class)
                        && native_owners.contains_key(&target)
                });
            if !valid {
                blocker(
                    &mut self.blockers,
                    "resource_dependency",
                    format!(
                        "{:08X}+{:X} from {:08X}",
                        dependency.owner, dependency.offset, dependency.source
                    ),
                );
            }
        }
        for pending in &contracts.pending {
            blocker(
                &mut self.blockers,
                "converter_obligation",
                format!(
                    "{:08X} {:?} {:?}: {}",
                    pending.owner, pending.kind, pending.offset, pending.detail
                ),
            );
        }
        // Existing bank gates cannot be dismissed by a supplied approval. Sequence
        // polling discharges an edge, not construction or other bank obligations.
        for bank in banks {
            for gate in &bank.gates {
                blocker(&mut self.blockers, "bank_gate", gate.clone());
            }
        }
    }

    pub(super) fn opaque(&mut self, source: &Payload, covered: &[bool]) -> Vec<Span> {
        let mut opaque = Vec::new();
        let mut at = 0;
        while at < covered.len() {
            if covered[at] {
                at += 1;
                continue;
            }
            let start = at;
            while at < covered.len() && !covered[at] {
                at += 1;
            }
            opaque.push(Span {
                offset: start,
                bytes: at - start,
                sha256: format!("{:x}", Sha256::digest(&source.0[start..at])),
            });
            blocker(
                &mut self.blockers,
                "unvalidated_entity_bytes",
                format!("{start:X}..{at:X}"),
            );
        }
        opaque
    }
}

pub(super) fn connections(original: &Graph, graph: &Graph) -> Result<Vec<ConnectionDisposition>> {
    let mut connections = Vec::new();
    for (named, source, retained) in [
        (false, &original.connections, &graph.connections),
        (true, &original.named_connections, &graph.named_connections),
    ] {
        let mut next = 0;
        for (index, edge) in source.iter().enumerate() {
            let retained_here = retained.get(next) == Some(edge);
            if retained_here {
                next += 1;
            }
            connections.push(ConnectionDisposition {
                named,
                index,
                scalar_polling: !retained_here,
            });
        }
        ensure!(
            next == retained.len(),
            "sequence adapter changed connection order or added an edge"
        );
    }
    Ok(connections)
}
