//! Discharge the modern bank notification edge using native scalar polling.
use super::Native;
use crate::d2_mot::{
    entity::links::{Connection, Graph, Object},
    native::effects::controller::Bank,
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

impl Native {
    /// Native sequence conditions read linked bank values when evaluated. The
    /// modern notification input can be removed only when all sequence scalar
    /// inputs resolve to the same completed bank, with valid native channels.
    /// A validated sequence may have no numeric consumers. Its named writes
    /// resolve through the host independently of the notification edge.
    /// Other connections are retained for the entity's ordinary typed linker.
    pub fn link(&self, graph: &Graph, banks: &[&Bank]) -> Result<Graph> {
        let mut removed = BTreeSet::<Object>::new();
        for dependency in &self.pull_dependencies {
            let notifications = graph
                .connections
                .iter()
                .chain(&graph.named_connections)
                .filter(|edge| edge.consumer.object == Some(*dependency))
                .collect::<Vec<_>>();
            ensure!(
                notifications.len() == 1,
                "sequence bank notification is missing or ambiguous"
            );
            let notification = notifications[0];
            ensure!(
                notification.channel == 0 && notification.flags == 0,
                "sequence bank notification mode requires translation"
            );
            ensure!(
                removed.insert(*dependency),
                "duplicate sequence notification dependency"
            );
            let inputs = self
                .objects
                .iter()
                .filter(|mapping| {
                    mapping.source.owner == dependency.owner && mapping.source.class == 0x80809591
                })
                .collect::<Vec<_>>();
            let Some(provider) = notification.provider.object else {
                // Graph::read has already checked the null owner, class and
                // offset. There is no source notifier to reproduce, and no
                // scalar receiver may depend on one in this admitted form.
                ensure!(
                    inputs.is_empty()
                        && notification.provider.selector == 0xFFFF
                        && notification.provider.namespace == 0x811C9DC5,
                    "disconnected sequence notification has inputs or a named provider"
                );
                continue;
            };
            ensure!(
                provider.class == 0x808095B3,
                "sequence notification provider is not a channel bank"
            );
            let matches = banks
                .iter()
                .filter(|bank| {
                    bank.objects.iter().any(|mapping| {
                        mapping.source == provider && mapping.target.class == 0x80807C75
                    })
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "sequence notification bank is not uniquely translated"
            );
            let bank = matches[0];
            for input in inputs {
                ensure!(
                    input.target.class == 0x80809789,
                    "sequence scalar target class differs"
                );
                let edges = graph
                    .connections
                    .iter()
                    .chain(&graph.named_connections)
                    .filter(|edge| edge.consumer.object == Some(input.source))
                    .collect::<Vec<_>>();
                ensure!(
                    edges.len() == 1,
                    "sequence scalar provider is missing or ambiguous"
                );
                let edge = edges[0];
                let scalar = edge
                    .provider
                    .object
                    .context("sequence scalar provider is disconnected")?;
                ensure!(
                    scalar.owner == provider.owner
                        && scalar.class == 0x808095CF
                        && edge.provider.selector == notification.provider.selector
                        && edge.provider.namespace == notification.provider.namespace
                        && edge.flags == 0
                        && (edge.channel as usize) < bank.channels,
                    "sequence scalar does not poll the notification bank"
                );
                ensure!(
                    bank.objects
                        .iter()
                        .any(|mapping| mapping.source == scalar
                            && mapping.target.class == 0x808097C2),
                    "sequence scalar bank interface is not translated"
                );
            }
        }
        let retain = |edge: &&Connection| {
            edge.consumer
                .object
                .is_none_or(|object| !removed.contains(&object))
        };
        Ok(Graph {
            components: graph.components.clone(),
            connections: graph.connections.iter().filter(retain).cloned().collect(),
            named_connections: graph
                .named_connections
                .iter()
                .filter(retain)
                .cloned()
                .collect(),
        })
    }
}
