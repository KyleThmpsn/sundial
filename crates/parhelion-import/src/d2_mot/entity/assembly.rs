//! Nonmutating entity topology preflight. Unknown fields and converter gates
//! remain blockers. A structural plan does not establish runtime equivalence.
use super::{
    links::{Graph, NativeRows, Object},
    sequence::Native,
};
use crate::d2_mot::{
    native::effects::{
        controller::{
            Bank, Relocation,
            damage::Damage,
            movement::{Interfaces, Movement},
            response::Response,
        },
        resources::Table,
    },
    payload::Payload,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod validation;

#[cfg(test)]
mod tests;

#[derive(Clone, Serialize)]
pub struct Component {
    pub source: u32,
    pub target: u32,
}
/// A channel contract supplied by the provider converter. `methods` identifies
/// dispatch methods actually used, independently of the serialized channel.
#[derive(Clone, Serialize)]
pub struct Channel {
    pub provider: Object,
    pub source: u32,
    pub target: u32,
    pub methods: Vec<u32>,
}
#[derive(Clone, Serialize)]
pub struct External {
    pub object: Object,
    pub source: u64,
    pub target: u64,
}
#[derive(Clone, Serialize)]
pub struct Unsupported {
    pub object: Object,
    pub methods: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum ObligationKind {
    Component,
    Gate,
    Resource,
    Allocation,
    Interface,
    NamedPublication,
}
/// No arbitrary approval string or boolean can dismiss a converter obligation.
#[derive(Clone, Serialize)]
pub struct Pending {
    pub owner: u32,
    pub kind: ObligationKind,
    pub offset: Option<usize>,
    pub detail: serde_json::Value,
}
#[derive(Clone, Serialize)]
pub struct Dependency {
    pub owner: u32,
    pub offset: usize,
    pub source: u32,
    pub target: Option<u32>,
    pub class: u32,
}

#[derive(Default)]
pub struct Contracts {
    pub components: Vec<Component>,
    pub channels: Vec<Channel>,
    pub externals: Vec<External>,
    pub unsupported: Vec<Unsupported>,
    pub dependencies: Vec<Dependency>,
    /// Native target classes established by checked emitters or package metadata.
    pub resource_classes: BTreeMap<u32, u32>,
    pub pending: Vec<Pending>,
    pub unmapped_interfaces: Vec<Object>,
}

impl Contracts {
    /// Select component-owned contracts for one entity in a conversion group.
    /// entity_components is the union of the group's actual component arrays.
    /// Converted owners outside that union retain their resource dependencies and
    /// obligations in every view, including aliases of component targets. The
    /// caller must also retain the complete group ledger.
    pub fn for_graph(&self, graph: &Graph, entity_components: &BTreeSet<u32>) -> Self {
        let sources: BTreeSet<_> = graph.components.iter().copied().collect();
        let all_targets: BTreeSet<_> = self.components.iter().map(|row| row.target).collect();
        let resource_targets: BTreeSet<_> = self
            .components
            .iter()
            .filter(|row| !entity_components.contains(&row.source))
            .map(|row| row.target)
            .collect();
        let components: Vec<_> = self
            .components
            .iter()
            .filter(|row| sources.contains(&row.source))
            .cloned()
            .collect();
        let targets: BTreeSet<_> = components.iter().map(|row| row.target).collect();
        let includes_owner = |owner: u32| {
            resource_targets.contains(&owner)
                || !all_targets.contains(&owner)
                || targets.contains(&owner)
        };
        Self {
            components,
            channels: self
                .channels
                .iter()
                .filter(|row| sources.contains(&row.provider.owner))
                .cloned()
                .collect(),
            externals: self
                .externals
                .iter()
                .filter(|row| sources.contains(&row.object.owner))
                .cloned()
                .collect(),
            unsupported: self
                .unsupported
                .iter()
                .filter(|row| sources.contains(&row.object.owner))
                .cloned()
                .collect(),
            dependencies: self
                .dependencies
                .iter()
                .filter(|row| includes_owner(row.owner))
                .cloned()
                .collect(),
            resource_classes: self.resource_classes.clone(),
            pending: self
                .pending
                .iter()
                .filter(|row| match row.kind {
                    ObligationKind::Component => {
                        sources.contains(&row.owner) || !entity_components.contains(&row.owner)
                    }
                    _ => includes_owner(row.owner),
                })
                .cloned()
                .collect(),
            unmapped_interfaces: self
                .unmapped_interfaces
                .iter()
                .filter(|object| sources.contains(&object.owner))
                .copied()
                .collect(),
        }
    }

    pub fn damage(&mut self, converted: &Damage) {
        self.unsupported
            .extend(converted.omitted_methods.iter().map(|omitted| Unsupported {
                object: omitted.object,
                methods: omitted.methods.clone(),
            }));
    }

    /// Source siblings remain unresolved until an inspected target is supplied.
    pub fn movement(
        &mut self,
        converted: &Movement,
        interfaces: Option<&Interfaces>,
    ) -> Result<()> {
        let owner = converted.owner.u32(converted.owner.pointer(16)?)?;
        self.dependencies.extend(
            converted
                .siblings
                .iter()
                .map(|&(offset, source)| Dependency {
                    owner,
                    offset,
                    source,
                    target: None,
                    class: u32::MAX,
                }),
        );
        if let Some(interfaces) = interfaces {
            self.unmapped_interfaces
                .extend(interfaces.unmapped.iter().copied());
            for (object, value) in &interfaces.active_unmapped {
                self.pending.push(Pending {
                    owner,
                    kind: ObligationKind::Interface,
                    offset: None,
                    detail: serde_json::json!({
                        "source": object,
                        "field_offset": object.offset + 16,
                        "value": value,
                    }),
                });
            }
        }
        Ok(())
    }

    /// Gates are retained even when every resource tag already has a target.
    pub fn response(&mut self, converted: &Response) -> Result<()> {
        let owner = converted.owner.u32(converted.owner.pointer(16)?)?;
        for (index, gate) in converted.gates.iter().enumerate() {
            self.pending.push(Pending {
                owner,
                kind: ObligationKind::Gate,
                offset: Some(index),
                detail: serde_json::to_value(gate)?,
            });
        }
        for &name in &converted.named_conditions {
            self.pending.push(Pending {
                owner,
                kind: ObligationKind::NamedPublication,
                offset: None,
                detail: serde_json::json!({"name": name}),
            });
        }
        for reference in &converted.references {
            self.dependencies.push(Dependency {
                owner,
                offset: reference.offset,
                source: reference.source,
                target: reference.target,
                class: converted
                    .reference_classes
                    .get(&reference.offset)
                    .copied()
                    .unwrap_or(u32::MAX),
            });
        }
        Ok(())
    }

    pub fn resources(&mut self, owner: u32, table: &Table, classes: &BTreeMap<usize, u32>) {
        self.dependencies
            .extend(table.references.iter().map(|reference| Dependency {
                owner,
                offset: reference.offset,
                source: reference.source,
                target: reference.target,
                class: classes.get(&reference.offset).copied().unwrap_or(u32::MAX),
            }));
    }
}

#[derive(Serialize)]
pub struct Blocker {
    pub kind: String,
    pub detail: String,
}
#[derive(Serialize)]
pub struct Span {
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Serialize)]
pub struct ConnectionDisposition {
    pub named: bool,
    pub index: usize,
    pub scalar_polling: bool,
}
#[derive(Serialize)]
pub struct Report {
    /// Structural preflight only. No entity is emitted by this API.
    pub ready: bool,
    pub source_sha256: String,
    pub source_owner_sha256: BTreeMap<u32, String>,
    pub native_owner_sha256: BTreeMap<u32, String>,
    pub component_rows: Vec<[u8; 12]>,
    pub component_map: Vec<Component>,
    pub object_map: Vec<Relocation>,
    pub channel_map: Vec<Channel>,
    pub external_map: Vec<External>,
    pub unsupported: Vec<Unsupported>,
    pub dependencies: Vec<Dependency>,
    pub resource_classes: BTreeMap<u32, u32>,
    pub pending: Vec<Pending>,
    pub unmapped_interfaces: Vec<Object>,
    pub connections: Vec<ConnectionDisposition>,
    pub opaque: Vec<Span>,
    pub blockers: Vec<Blocker>,
    /// Topology rows may be available while opaque fields or gates still block
    /// complete assembly. Callers must not treat this option as readiness.
    pub rows: Option<NativeRows>,
}

pub(crate) fn digest(payload: &Payload) -> String {
    format!("{:x}", Sha256::digest(&payload.0))
}

fn blocker(blockers: &mut Vec<Blocker>, kind: &str, detail: impl Into<String>) {
    blockers.push(Blocker {
        kind: kind.into(),
        detail: detail.into(),
    });
}

/// Validate supplied converter contracts and account for every serialized edge.
/// Only the existing sequence adapter can discharge notification connections.
/// Component tail words and fields outside the known graph arrays are retained
/// as opaque blockers, including zeros, until their formats are established.
#[allow(clippy::too_many_arguments)]
pub fn preflight(
    source: &Payload,
    source_owners: &BTreeMap<u32, Payload>,
    native_owners: &BTreeMap<u32, Payload>,
    relocations: &[Relocation],
    contracts: &Contracts,
    sequences: &[&Native],
    banks: &[&Bank],
) -> Result<Report> {
    source.bytes::<56>(0)?;
    let original = Graph::read(source, true)?;
    original.validate_owners(source_owners)?;
    let mut graph = Graph::read(source, true)?;
    let mut audit = validation::Audit::new();
    for sequence in sequences {
        match sequence.link(&graph, banks) {
            Ok(linked) => graph = linked,
            Err(error) => {
                audit.topology_valid = false;
                blocker(
                    &mut audit.blockers,
                    "sequence_polling",
                    format!("{error:#}"),
                );
            }
        }
    }
    let connections = validation::connections(&original, &graph)?;
    let validation::Coverage {
        covered,
        component_rows,
    } = audit.coverage(source)?;
    let validation::Components {
        by_source: components,
        targets: target_components,
    } = audit.components(contracts, native_owners, &original);
    let objects = audit.objects(
        relocations,
        &components,
        &original,
        source_owners,
        native_owners,
    );
    let channels = audit.channels(contracts, &graph, &objects);
    let externals = audit.externals(contracts);
    audit.dependencies(contracts, native_owners, banks);
    let opaque = audit.opaque(source, &covered);
    let rows = match graph.native_rows(
        &target_components,
        native_owners,
        |object| {
            objects
                .get(&object)
                .copied()
                .context("missing object relocation")
        },
        |object, channel| {
            channels
                .get(&(object, channel))
                .copied()
                .context("missing channel contract")
        },
        |object, selector| {
            externals
                .get(&(object, selector))
                .copied()
                .context("missing external contract")
        },
    ) {
        Ok(rows) if audit.topology_valid => Some(rows),
        Ok(_) => None,
        Err(error) => {
            blocker(
                &mut audit.blockers,
                "connection_mapping",
                format!("{error:#}"),
            );
            None
        }
    };
    let ready = audit.blockers.is_empty() && rows.is_some();
    ensure!(
        connections.len() == original.connections.len() + original.named_connections.len(),
        "connection coverage differs"
    );
    Ok(Report {
        ready,
        source_sha256: digest(source),
        source_owner_sha256: source_owners
            .iter()
            .map(|(&tag, owner)| (tag, digest(owner)))
            .collect(),
        native_owner_sha256: native_owners
            .iter()
            .map(|(&tag, owner)| (tag, digest(owner)))
            .collect(),
        component_rows,
        component_map: contracts.components.clone(),
        object_map: objects
            .into_iter()
            .map(|(source, target)| Relocation { source, target })
            .collect(),
        channel_map: contracts.channels.clone(),
        external_map: contracts.externals.clone(),
        unsupported: contracts.unsupported.clone(),
        dependencies: contracts.dependencies.clone(),
        resource_classes: contracts.resource_classes.clone(),
        pending: contracts.pending.clone(),
        unmapped_interfaces: contracts.unmapped_interfaces.clone(),
        connections,
        opaque,
        blockers: audit.blockers,
        rows,
    })
}
