//! Allocate and link every component occurrence in an attachment graph.
use super::*;
use crate::d2_mot::entity::{shared, spatial};

pub(super) struct Plan {
    pub source: u32,
    pub target: u32,
    graph: Graph,
    components: Vec<Component>,
    metadata: BTreeMap<u32, Payload>,
}

pub(super) struct Bindings<'a> {
    pub resources: &'a BTreeMap<u32, BTreeMap<usize, sequence::Resource>>,
    pub sequences: &'a BTreeMap<u32, sequence::Bindings>,
    pub selectors: &'a sequence::Selectors,
    /// These payloads have already passed the creation-parameter ownership check.
    pub specialized: &'a BTreeMap<u32, Payload>,
    pub actions: &'a BTreeMap<u32, u32>,
}

fn template(class: u32) -> Result<(u32, u32)> {
    Ok(match class {
        0x80803F64 => (0x80C70C0E, 0x80B3A568),
        0x808032AA => (0x80BFDC7A, 0x80B59A93),
        0x80808EDD => (0x80C709E1, 0x80B58D0E),
        0x80808757 => (0x815282DF, 0x815282E1),
        0x80809597 => (0x815282E0, 0x815282E1),
        0x80808159 => (0x80C70B84, 0x80BC57DD),
        0x80808179 => (0x80F2E50A, 0x80BBB972),
        0x80802AB8 => (0x80C709E8, 0x80B8228A),
        0x80803FFC => (0x8161E6F2, 0x80BB74DC),
        0x8080B11F => (0x80FEF700, 0x80BB74DC),
        0x808032E5 => (0x80FEF1D2, 0x80BB73C9),
        0x8080433E => (0x80FEF780, 0x80BB75EE),
        0x8080313F => (0x80FEF6FD, 0x80BB74DC),
        0x80802939 => (0x80EF62A3, 0x80BAB778),
        _ => anyhow::bail!("attachment component {class:08X} has no native assembly"),
    })
}

impl Plan {
    /// Reserve every graph before its owners are emitted. Recursive references
    /// therefore resolve without borrowing another graph's mutable component.
    pub fn reserve(
        source: &mut Reader,
        native: &mut Reader,
        closure: &closure::Closure,
        assets: &mut Assets,
    ) -> Result<BTreeMap<u32, Self>> {
        let mut targets = BTreeMap::new();
        for &tag in closure.graphs.keys() {
            targets.insert(
                tag,
                assets.reserve(native, format!("attachment-graph-{tag:08X}"))?,
            );
        }
        let mut plans = BTreeMap::new();
        for (&tag, payload) in &closure.graphs {
            let graph = Graph::read(payload, true)?;
            graph.validate_owners(&closure.owners)?;
            let mut components = Vec::new();
            let mut metadata_table = BTreeMap::new();
            for &owner in &graph.components {
                let original = &closure.owners[&owner];
                let class = original.u32(original.pointer(24)? - 4)?;
                let (template_tag, parent) = template(class)?;
                let envelope = native.tag(template_tag, None)?;
                metadata(source, &[original], &mut metadata_table)?;
                metadata(native, &[&envelope], &mut metadata_table)?;
                components.push(Component {
                    source: owner,
                    class,
                    template_tag,
                    template: (*envelope).clone(),
                    entity: (*native.tag(parent, Some(0x80809C0F))?).clone(),
                    owner: assets
                        .reserve(native, format!("attachment-{tag:08X}-owner-{owner:08X}"))?,
                    allocation: assets.reserve(
                        native,
                        format!("attachment-{tag:08X}-allocation-{owner:08X}"),
                    )?,
                    payload: None,
                });
            }
            plans.insert(
                tag,
                Self {
                    source: tag,
                    target: targets[&tag],
                    graph,
                    components,
                    metadata: metadata_table,
                },
            );
        }
        Ok(plans)
    }

    pub fn emit(
        mut self,
        source: &mut Reader,
        native: &mut Reader,
        closure: &closure::Closure,
        assets: &mut Assets,
        bindings: &Bindings<'_>,
        stable_inputs: impl Fn(&Graph, &[Object]) -> Result<()>,
    ) -> Result<Value> {
        let scalar_input = native.tag(0x80F2E50A, None)?;
        let mut banks = BTreeMap::new();
        for component in &self.components {
            if component.class == 0x80809597 {
                let allocation = native.tag(component.template.u32(0x44)?, None)?;
                let bank = controller::bank(
                    &closure.owners[&component.source],
                    &component.template,
                    &allocation,
                    component.owner,
                    component.allocation,
                )
                .with_context(|| format!("attachment bank {:08X}", component.source))?;
                // Bank::gates describes the obligations of its enclosing graph.
                // The endpoint walk and compiled allocation assembly below
                // discharge those structural obligations. Runtime activation
                // remains a separate gameplay acceptance check.
                banks.insert(component.source, bank);
            }
        }
        let mut objects = BTreeMap::new();
        let mut unbound = BTreeSet::new();
        let mut stable = Vec::new();
        let mut graph = self.graph;
        for component in &mut self.components {
            let original = bindings
                .specialized
                .get(&component.source)
                .unwrap_or(&closure.owners[&component.source]);
            let allocation_tag = component.template.u32(0x44)?;
            let allocation = native.tag(allocation_tag, None)?;
            let empty = BTreeMap::new();
            let resources = bindings.resources.get(&component.source).unwrap_or(&empty);
            let (payload, allocation_payload, mappings) = match component.class {
                0x80809597 => {
                    let bank = &banks[&component.source];
                    (
                        bank.owner.clone(),
                        bank.allocation.clone(),
                        bank.objects
                            .iter()
                            .map(|r| controller::Relocation {
                                source: r.source,
                                target: r.target,
                            })
                            .collect(),
                    )
                }
                0x80803F64 | 0x808032AA | 0x80808159 => {
                    let mut owner = retag(&component.template, component.owner)?;
                    put(&mut owner, 0x44, &component.allocation.to_le_bytes())?;
                    let mappings = match component.class {
                        0x80803F64 => context::interfaces(original, &owner, &self.metadata)?,
                        0x808032AA => shared::globals(original, &owner, &self.metadata)?,
                        _ => {
                            let source_allocation = source.tag(original.u32(0x84)?, None)?;
                            context::effect::interfaces(
                                original,
                                &owner,
                                &source_allocation,
                                &allocation,
                                &self.metadata,
                            )?
                        }
                    };
                    (owner, (*allocation).clone(), mappings)
                }
                0x80808EDD => {
                    let result = controller::network::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        component.owner,
                        component.allocation,
                    )?;
                    (result.owner, result.allocation, result.objects)
                }
                0x80808757 => {
                    let mut result = controller::parameters::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        component.owner,
                        component.allocation,
                    )?;
                    if bindings.specialized.contains_key(&component.source) {
                        result.freeze_creation()?;
                    }
                    (result.owner, result.allocation, result.objects)
                }
                0x80802AB8 => {
                    let result = transform::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        &graph,
                        component.owner,
                        component.allocation,
                    )?;
                    (result.owner, result.allocation, result.objects)
                }
                0x80802939 => {
                    let names = spatial_names(source, closure, original)?;
                    let result = spatial::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        &graph,
                        &names,
                        component.owner,
                        component.allocation,
                    )?;
                    (result.owner, result.allocation, result.objects)
                }
                0x80803FFC => {
                    let result = controller::health::emit(
                        original,
                        controller::health::Templates {
                            owner: &component.template,
                            allocation: &allocation,
                            scalar_input: &scalar_input,
                            metadata: &self.metadata,
                        },
                        &controller::health::Bindings {
                            resources: resources.clone(),
                            categories: bindings.selectors.resources.clone(),
                        },
                        component.owner,
                        component.allocation,
                    )
                    .with_context(|| format!("health owner {:08X}", component.source))?;
                    unbound.extend(result.unbound);
                    (result.owner, result.allocation, result.objects)
                }
                0x8080B11F => {
                    let result = controller::timed::emit(
                        original,
                        controller::timed::Templates {
                            owner: &component.template,
                            allocation: &allocation,
                            scalar_input: &scalar_input,
                            metadata: &self.metadata,
                        },
                        &controller::timed::Bindings {
                            resources: resources.clone(),
                            categories: bindings.selectors.resources.clone(),
                            groups: bindings.selectors.groups.clone(),
                            resolver: bindings.selectors.resolver.clone(),
                        },
                        component.owner,
                        component.allocation,
                    )
                    .with_context(|| format!("timed owner {:08X}", component.source))?;
                    named(&result.named_conditions, bindings.selectors)?;
                    unbound.extend(result.unbound);
                    stable.extend(result.stable_inputs);
                    (result.owner, result.allocation, result.objects)
                }
                0x808032E5 => {
                    let result = controller::modifier::emit(
                        original,
                        &component.template,
                        &allocation,
                        &scalar_input,
                        &self.metadata,
                        component.owner,
                        component.allocation,
                    )?;
                    unbound.extend(result.unbound);
                    (result.owner, result.allocation, result.objects)
                }
                0x8080433E => {
                    let result = controller::damage_events::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        component.owner,
                        component.allocation,
                    )?;
                    unbound.extend(result.unbound);
                    (result.owner, result.allocation, result.objects)
                }
                0x8080313F => {
                    let field = original.pointer(24)? + 0x50;
                    let action = source.ref64(original, field)?;
                    let action = *bindings
                        .actions
                        .get(&action)
                        .context("hosted attachment action is unbound")?;
                    let result = controller::perk::emit(
                        original,
                        &component.template,
                        &allocation,
                        &self.metadata,
                        action,
                        component.owner,
                        component.allocation,
                    )?;
                    unbound.extend(result.unbound);
                    (result.owner, result.allocation, result.objects)
                }
                0x80808179 => {
                    let mut sequence_bindings = bindings
                        .sequences
                        .get(&component.source)
                        .context("attachment sequence has no completed resource bindings")?
                        .clone();
                    sequence_bindings.metadata.extend(self.metadata.clone());
                    sequence_bindings.selectors = Some(bindings.selectors.clone());
                    let result = sequence::emit(
                        original,
                        &component.template,
                        &allocation,
                        &sequence_bindings,
                        component.owner,
                        component.allocation,
                    )
                    .with_context(|| format!("attachment sequence {:08X}", component.source))?;
                    graph = result
                        .link(&graph, &banks.values().collect::<Vec<_>>())
                        .with_context(|| {
                            format!("attachment sequence {:08X} links", component.source)
                        })?;
                    (result.owner, result.allocation, result.objects)
                }
                _ => unreachable!(),
            };
            for mapping in mappings {
                if let Some(previous) = objects.insert(mapping.source, mapping.target) {
                    ensure!(
                        previous == mapping.target,
                        "attachment endpoint has conflicting mappings"
                    );
                }
            }
            assets.push(
                component.allocation,
                allocation_tag,
                allocation_payload,
                None,
            )?;
            assets.push(
                component.owner,
                component.template_tag,
                payload.clone(),
                (native.reference(component.template_tag)? == allocation_tag)
                    .then_some(component.allocation),
            )?;
            component.payload = Some(payload);
        }
        stable_inputs(&graph, &stable)
            .context("attachment duration refresh inputs are not stable")?;
        reject_live_unbound(&mut graph, &unbound)?;
        let owners = self
            .components
            .iter()
            .map(|c| {
                Ok((
                    c.owner,
                    c.payload
                        .clone()
                        .context("attachment component was not emitted")?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let component_tags = self.components.iter().map(|c| c.owner).collect::<Vec<_>>();
        let rows = graph.native_rows(
            &component_tags,
            &owners,
            |object| {
                objects
                    .get(&object)
                    .copied()
                    .with_context(|| format!("untranslated attachment endpoint {object:?}"))
            },
            |_, channel| Ok(channel),
            |_, _| anyhow::bail!("attachment external selector requires translation"),
        )?;
        let components = self
            .components
            .iter()
            .map(|c| compiled::Component {
                owner: c.owner,
                payload: &owners[&c.owner],
                template: &c.template,
                entity: &c.entity,
            })
            .collect::<Vec<_>>();
        let envelope = native.tag(0x80BC57DD, Some(0x80809C0F))?;
        let mut assembled = compiled::emit(&envelope, &components, &rows)?;
        // Replication belongs to this graph's new owners, including graphs that
        // share source components. A stock registration cannot describe them.
        if self.components.iter().any(|c| c.class == 0x80808EDD) {
            let reference = self
                .components
                .iter()
                .map(|c| c.entity.u32(0x88))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .find(|tag| !matches!(*tag, 0 | u32::MAX))
                .context("native attachment replication envelope absent")?;
            native.tag(reference, Some(0x80809BB6))?;
            let replication = assets.reserve(
                native,
                format!("attachment-{:08X}-replication", self.source),
            )?;
            assets.push(
                replication,
                reference,
                compiled::replication::emit(self.target, &components)?,
                None,
            )?;
            put(&mut assembled, 0x88, &replication.to_le_bytes())?;
        }
        Graph::read(&assembled, false)?.validate_owners(&owners)?;
        assets.push(self.target, 0x80BC57DD, assembled, None)?;
        Ok(
            json!({"source":self.source,"target":self.target,"components":self.components.len(),
            "connections":rows.connections.len(),"named_connections":rows.named_connections.len(),
            "gameplay_verified":false}),
        )
    }
}

/// The two unsupported spatial outputs must have no possible dynamic consumer.
/// Conservatively inspect all bytes in the closed dependency set, including
/// particle programs. A matching opaque word refuses translation rather than
/// assuming that an unrecognized record cannot request the getter by name.
fn spatial_names(
    source: &mut Reader,
    closure: &closure::Closure,
    owner: &Payload,
) -> Result<BTreeSet<u32>> {
    let d = owner.pointer(24)?;
    let mut names = BTreeSet::new();
    for offset in [0x128, 0x178] {
        let name = owner.u32(d + offset + 32)?;
        let used = closure
            .owners
            .values()
            .filter(|p| p.0 != owner.0)
            .chain(closure.graphs.values())
            .chain(closure.actions.values())
            .chain(closure.resources.values().map(|r| &r.payload))
            .any(|p| p.0.windows(4).any(|bytes| bytes == name.to_le_bytes()));
        if used {
            names.insert(name);
        }
        for &tag in &closure.particles {
            let p = source.tag(tag, None)?;
            if p.0.windows(4).any(|bytes| bytes == name.to_le_bytes()) {
                names.insert(name);
            }
        }
    }
    Ok(names)
}

fn named(names: &[u32], selectors: &sequence::Selectors) -> Result<()> {
    for name in names {
        ensure!(
            selectors.accepts_name(name),
            "attachment selector input {name:08X} has no qualified context lookup"
        );
    }
    Ok(())
}

fn reject_live_unbound(graph: &mut Graph, unbound: &BTreeSet<Object>) -> Result<()> {
    for edges in [&mut graph.connections, &mut graph.named_connections] {
        for edge in edges.iter() {
            ensure!(
                edge.provider
                    .object
                    .is_none_or(|object| !unbound.contains(&object)),
                "attachment uses a modern-only provider"
            );
            if edge
                .consumer
                .object
                .is_some_and(|object| unbound.contains(&object))
            {
                ensure!(
                    edge.provider.object.is_none() && edge.flags == 0 && edge.channel == 0,
                    "attachment connects a modern-only input"
                );
            }
        }
        edges.retain(|edge| {
            edge.consumer
                .object
                .is_none_or(|object| !unbound.contains(&object))
        });
    }
    Ok(())
}
