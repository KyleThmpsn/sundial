//! Source component occurrences and 88-byte connection rows, with explicit native endpoint mappings.
use crate::tiger::{entity::links, payload::Payload};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Endpoint {
    pub namespace: u64,
    pub owner: u32,
    pub class: u64,
    pub offset: u64,
    /// The local occurrence, disconnected FFFF, or an explicitly qualified external selector.
    pub selector: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Connection {
    pub consumer: Endpoint,
    pub provider: Endpoint,
    pub channel: u64,
}

#[derive(Debug, Serialize)]
pub struct Graph {
    pub components: Vec<u32>,
    pub connections: Vec<Connection>,
    pub named_connections: Vec<Connection>,
}

impl Graph {
    pub fn read(entity: &Payload) -> Result<Self> {
        ensure!(
            entity.u64(0)? == entity.0.len() as u64,
            "Source graph file size differs"
        );
        let components = entity
            .array(0x20, 12, Some(0x80800715))?
            .into_iter()
            .map(|row| entity.u32(row))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !components.is_empty() && components.len() < 0xFFFF,
            "Source graph has no components or exceeds the ordinal capacity"
        );
        let read = |field| -> Result<Vec<Connection>> {
            entity
                .array(field, 88, Some(0x8080054E))?
                .into_iter()
                .map(|row| {
                    let endpoint = |base: usize| -> Result<Endpoint> {
                        ensure!(
                            entity.u32(row + base + 12)? == 0,
                            "Source endpoint owner padding differs"
                        );
                        let result = Endpoint {
                            namespace: entity.u64(row + base)?,
                            owner: entity.u32(row + base + 8)?,
                            offset: entity.u64(row + base + 16)?,
                            class: entity.u64(row + base + 24)?,
                            selector: entity.u64(row + base + 32)?,
                        };
                        if result.selector < 0xFFFF {
                            ensure!(
                                components.get(result.selector as usize) == Some(&result.owner),
                                "Source endpoint owner differs from its component occurrence"
                            );
                            ensure!(
                                result.owner != u32::MAX
                                    && !matches!(result.class, 0xFFFFFFFF | u64::MAX),
                                "Source local endpoint is null"
                            );
                        } else if result.selector == 0xFFFF {
                            ensure!(
                                result.owner == u32::MAX
                                    && matches!(result.class, 0xFFFFFFFF | u64::MAX)
                                    && result.offset == 0,
                                "Disconnected source endpoint retains an object"
                            );
                        }
                        Ok(result)
                    };
                    Ok(Connection {
                        consumer: endpoint(0)?,
                        provider: endpoint(40)?,
                        channel: entity.u64(row + 80)?,
                    })
                })
                .collect()
        };
        let connections = read(0x30)?;
        let named_connections = read(0x40)?;
        Ok(Self {
            components,
            connections,
            named_connections,
        })
    }

    pub fn validate_owners(&self, owners: &BTreeMap<u32, Payload>) -> Result<()> {
        for tag in &self.components {
            let owner = owners
                .get(tag)
                .with_context(|| format!("Missing source owner {tag:08X}"))?;
            ensure!(
                owner.u64(0)? == owner.0.len() as u64,
                "Source owner file size differs"
            );
        }
        for row in self.connections.iter().chain(&self.named_connections) {
            for endpoint in [row.consumer, row.provider] {
                if endpoint.selector >= 0xFFFF {
                    continue;
                }
                owners
                    .get(&endpoint.owner)
                    .context("Source endpoint owner is absent")?
                    .bytes::<16>(usize::try_from(endpoint.offset)?)?;
            }
        }
        Ok(())
    }

    /// Each source occurrence receives a distinct private owner, in source order. Every endpoint
    /// and channel needs a component converter's mapping. No shared source tag is an instance key.
    pub fn native_rows(
        &self,
        occurrences: &[u32],
        owners: &BTreeMap<u32, Payload>,
        map_endpoint: impl Fn(Endpoint) -> Result<links::Endpoint>,
        map_channel: impl Fn(Endpoint, u64) -> Result<u32>,
    ) -> Result<links::NativeRows> {
        ensure!(
            occurrences.len() == self.components.len()
                && occurrences.iter().copied().collect::<BTreeSet<_>>().len() == occurrences.len()
                && occurrences.iter().all(|owner| *owner != u32::MAX),
            "Native owners omit or collapse source component occurrences"
        );
        for id in occurrences {
            let owner = owners
                .get(id)
                .context("Native component occurrence was not emitted")?;
            ensure!(
                owner.u64(0)? == owner.0.len() as u64 && owner.u32(owner.pointer(16)?)? == *id,
                "Native component occurrence identity differs"
            );
        }
        let lower = |rows: &[Connection]| -> Result<Vec<Vec<u8>>> {
            rows.iter()
                .map(|row| {
                    let mut bytes = vec![0; 72];
                    for (source, name, reference, selector) in
                        [(row.consumer, 0, 8, 24), (row.provider, 32, 40, 56)]
                    {
                        let target = map_endpoint(source).with_context(|| {
                            format!(
                                "Untranslated source endpoint {:08X}+{:X}, occurrence {:X}",
                                source.owner, source.offset, source.selector
                            )
                        })?;
                        if source.selector < 0xFFFF {
                            ensure!(
                                target.selector == source.selector
                                    && target.object.is_some_and(|object| occurrences
                                        .get(source.selector as usize)
                                        == Some(&object.owner)),
                                "Native endpoint changed its source occurrence"
                            );
                        } else if source.selector == 0xFFFF {
                            ensure!(
                                target.selector == 0xFFFF && target.object.is_none(),
                                "Disconnected source endpoint became live"
                            );
                        } else {
                            ensure!(
                                target.selector > 0xFFFF && target.object.is_some(),
                                "External source endpoint lost its explicitly mapped selector"
                            );
                        }
                        let object = target.object.unwrap_or(links::Object {
                            owner: u32::MAX,
                            class: u32::MAX,
                            offset: 0,
                        });
                        if let Some(object) = target.object {
                            ensure!(
                                object.owner != u32::MAX && object.class != u32::MAX,
                                "Mapped source endpoint is null"
                            );
                            let owner = owners
                                .get(&object.owner)
                                .context("Mapped endpoint owner was not emitted")?;
                            ensure!(
                                owner.u64(0)? == owner.0.len() as u64,
                                "Mapped endpoint owner size differs"
                            );
                            owner.bytes::<16>(usize::try_from(object.offset)?)?;
                        }
                        bytes[name..name + 4].copy_from_slice(&target.namespace.to_le_bytes());
                        bytes[reference..reference + 4]
                            .copy_from_slice(&object.owner.to_le_bytes());
                        bytes[reference + 4..reference + 8]
                            .copy_from_slice(&object.class.to_le_bytes());
                        bytes[reference + 8..reference + 16]
                            .copy_from_slice(&object.offset.to_le_bytes());
                        bytes[selector..selector + 8]
                            .copy_from_slice(&target.selector.to_le_bytes());
                    }
                    let channel = map_channel(row.provider, row.channel)
                        .context("Untranslated source provider channel")?;
                    bytes[64..68].copy_from_slice(&channel.to_le_bytes());
                    // D1 has one 64-bit channel field. It has no native +44 flags word to copy.
                    Ok(bytes)
                })
                .collect()
        };
        Ok(links::NativeRows {
            connections: lower(&self.connections)?,
            named_connections: lower(&self.named_connections)?,
        })
    }
}
