//! Typed controller connections, independent of the conversion of their owners.
//! The first endpoint consumes data from the second endpoint's selected channel.
use crate::tiger::payload::Payload;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Object {
    pub owner: u32,
    pub class: u32,
    pub offset: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub namespace: u32,
    pub object: Option<Object>,
    /// Local component ordinal, 0xFFFF for disconnected, or an opaque external
    /// selector. External selectors need their own explicitly supplied mapping.
    pub selector: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub consumer: Endpoint,
    pub provider: Endpoint,
    pub channel: u32,
    pub flags: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Graph {
    pub components: Vec<u32>,
    pub connections: Vec<Connection>,
    pub named_connections: Vec<Connection>,
}

/// Rows for the native 80809BC9 arrays at entity +20 and +30. These do not
/// replace owner conversion or the entity's interface registration tables.
#[derive(Debug, Serialize)]
pub struct NativeRows {
    pub connections: Vec<Vec<u8>>,
    pub named_connections: Vec<Vec<u8>>,
}

/// A provider interface's serialized dispatch contract. These method indices
/// describe one runtime and must not be copied to another runtime by ordinal.
#[derive(Debug, Serialize)]
pub struct Interface {
    pub definition_offset: usize,
    pub metadata_tag: u32,
    pub instance_class: u32,
    pub methods: Vec<Method>,
}

#[derive(Debug, Serialize)]
pub struct Method {
    pub implementation_class: u32,
    pub index: u32,
    pub arguments: [u64; 2],
}

impl Interface {
    /// The caller supplies the metadata referenced by the provider at +8.
    /// This applies to provider interfaces, not consumer parameter records.
    pub fn read(owner: &Payload, object: Object, metadata: &Payload, modern: bool) -> Result<Self> {
        ensure!(
            owner.u64(0)? == owner.0.len() as u64,
            "interface owner size differs"
        );
        ensure!(
            metadata.u64(0)? == metadata.0.len() as u64,
            "interface metadata size differs"
        );
        let at = usize::try_from(object.offset)?;
        let definition_offset = owner.pointer(at)?;
        owner.bytes::<16>(definition_offset)?;
        ensure!(
            owner.u32(at + 12)? == 0,
            "interface metadata padding differs"
        );
        ensure!(
            metadata.u32(8)? == object.class,
            "interface metadata class differs"
        );
        let metadata_tag = owner.u32(at + 8)?;
        ensure!(metadata_tag != u32::MAX, "interface metadata is null");
        let methods = metadata
            .array(16, 24, Some(if modern { 0x80809B25 } else { 0x80809C56 }))?
            .into_iter()
            .map(|row| {
                Ok(Method {
                    implementation_class: metadata.u32(row)?,
                    index: metadata.u32(row + 4)?,
                    arguments: [metadata.u64(row + 8)?, metadata.u64(row + 16)?],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            definition_offset,
            metadata_tag,
            instance_class: metadata.u32(12)?,
            methods,
        })
    }
}

impl Graph {
    pub fn read(entity: &Payload, modern: bool) -> Result<Self> {
        ensure!(
            entity.u64(0)? == entity.0.len() as u64,
            "entity file size differs"
        );
        let components = entity
            .array(
                if modern { 8 } else { 16 },
                12,
                Some(if modern { 0x80809ACD } else { 0x80809C04 }),
            )?
            .into_iter()
            .map(|at| entity.u32(at))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            components.len() < 0xFFFF,
            "component ordinal exceeds native range"
        );
        ensure!(
            components
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == components.len(),
            "ambiguous source component owner"
        );
        let read = |descriptor| -> Result<Vec<Connection>> {
            let stride = if modern { 56 } else { 72 };
            let class = if modern { 0x80809A8F } else { 0x80809BC9 };
            entity
                .array(descriptor, stride, Some(class))?
                .into_iter()
                .map(|row| {
                    let endpoint = |provider: bool| -> Result<Endpoint> {
                        let (name, reference, index) = match (modern, provider) {
                            (true, false) => (16, 0, 20),
                            (true, true) => (40, 24, 44),
                            (false, false) => (0, 8, 24),
                            (false, true) => (32, 40, 56),
                        };
                        if !modern {
                            ensure!(
                                entity.u32(row + name + 4)? == 0,
                                "native connection namespace padding differs"
                            );
                        }
                        let ordinal = if modern {
                            u64::from(entity.u32(row + index)?)
                        } else {
                            entity.u64(row + index)?
                        };
                        let object = Object {
                            owner: entity.u32(row + reference)?,
                            class: entity.u32(row + reference + 4)?,
                            offset: entity.u64(row + reference + 8)?,
                        };
                        let object = if ordinal == 0xFFFF {
                            ensure!(
                                object.owner == u32::MAX
                                    && object.class == u32::MAX
                                    && object.offset == 0,
                                "disconnected controller endpoint has a live reference"
                            );
                            None
                        } else {
                            if ordinal < 0xFFFF {
                                let component = usize::try_from(ordinal)?;
                                ensure!(
                                    components.get(component) == Some(&object.owner),
                                    "controller endpoint owner differs from component ordinal"
                                );
                            }
                            ensure!(
                                object.class != u32::MAX && object.owner != u32::MAX,
                                "connected controller endpoint is null"
                            );
                            Some(object)
                        };
                        Ok(Endpoint {
                            namespace: entity.u32(row + name)?,
                            object,
                            selector: ordinal,
                        })
                    };
                    Ok(Connection {
                        consumer: endpoint(false)?,
                        provider: endpoint(true)?,
                        channel: entity.u32(row + if modern { 48 } else { 64 })?,
                        flags: entity.u32(row + if modern { 52 } else { 68 })?,
                    })
                })
                .collect()
        };
        let connections = read(if modern { 0x18 } else { 0x20 })?;
        let named_connections = read(if modern { 0x28 } else { 0x30 })?;
        Ok(Self {
            components,
            connections,
            named_connections,
        })
    }

    pub fn objects(&self) -> impl Iterator<Item = Object> + '_ {
        self.connections
            .iter()
            .chain(&self.named_connections)
            .flat_map(|row| [row.consumer.object, row.provider.object])
            .flatten()
    }

    /// Check that every referenced component and embedded object was exported.
    pub fn validate_owners(&self, owners: &BTreeMap<u32, Payload>) -> Result<()> {
        for tag in &self.components {
            let owner = owners
                .get(tag)
                .with_context(|| format!("missing controller owner {tag:08X}"))?;
            ensure!(
                owner.u64(0)? == owner.0.len() as u64,
                "controller owner file size differs"
            );
        }
        for object in self.objects() {
            let owner = owners
                .get(&object.owner)
                .context("controller owner missing")?;
            ensure!(
                owner.u64(0)? == owner.0.len() as u64,
                "referenced owner file size differs"
            );
            let offset = usize::try_from(object.offset)?;
            owner.bytes::<16>(offset).with_context(|| {
                format!(
                    "controller object {:08X}:{:08X}+{:X}",
                    object.owner, object.class, object.offset
                )
            })?;
        }
        Ok(())
    }

    /// Every live object and provider channel must have an explicit mapping
    /// supplied by its component converter. No source address is copied as a
    /// fallback. External selectors also require an explicit map, since their
    /// resolution is not described by this entity's local component table.
    /// The target owner payloads are checked before returning rows.
    pub fn native_rows(
        &self,
        components: &[u32],
        owners: &BTreeMap<u32, Payload>,
        object_map: impl Fn(Object) -> Result<Object>,
        channel_map: impl Fn(Object, u32) -> Result<u32>,
        external_map: impl Fn(Object, u64) -> Result<u64>,
    ) -> Result<NativeRows> {
        ensure!(
            components.len() < 0xFFFF,
            "native component ordinal capacity"
        );
        let mut ordinals = BTreeMap::new();
        for (index, tag) in components.iter().enumerate() {
            ensure!(
                ordinals.insert(*tag, index).is_none(),
                "ambiguous target component owner"
            );
        }
        let lower = |rows: &[Connection]| -> Result<Vec<Vec<u8>>> {
            rows.iter()
                .map(|row| {
                    let mut bytes = vec![0; 72];
                    for (endpoint, name, at, index) in
                        [(&row.consumer, 0, 8, 24), (&row.provider, 32, 40, 56)]
                    {
                        bytes[name..name + 4].copy_from_slice(&endpoint.namespace.to_le_bytes());
                        let (object, ordinal) = if let Some(source) = endpoint.object {
                            let target = object_map(source).with_context(|| {
                                format!(
                                    "untranslated controller object {:08X}:{:08X}+{:X}",
                                    source.owner, source.class, source.offset
                                )
                            })?;
                            ensure!(
                                target.owner != u32::MAX && target.class != u32::MAX,
                                "controller mapping disconnected a live object"
                            );
                            let ordinal = if endpoint.selector > 0xFFFF {
                                let selector = external_map(source, endpoint.selector)
                                    .context("untranslated external controller selector")?;
                                ensure!(
                                    selector > 0xFFFF,
                                    "external controller selector became a local component"
                                );
                                selector
                            } else {
                                *ordinals
                                    .get(&target.owner)
                                    .context("mapped owner is not a target component")?
                                    as u64
                            };
                            let owner = owners
                                .get(&target.owner)
                                .context("mapped owner was not emitted")?;
                            ensure!(
                                owner.u64(0)? == owner.0.len() as u64,
                                "mapped owner file size differs"
                            );
                            owner.bytes::<16>(usize::try_from(target.offset)?)?;
                            (target, ordinal)
                        } else {
                            (
                                Object {
                                    owner: u32::MAX,
                                    class: u32::MAX,
                                    offset: 0,
                                },
                                0xFFFF,
                            )
                        };
                        bytes[at..at + 4].copy_from_slice(&object.owner.to_le_bytes());
                        bytes[at + 4..at + 8].copy_from_slice(&object.class.to_le_bytes());
                        bytes[at + 8..at + 16].copy_from_slice(&object.offset.to_le_bytes());
                        bytes[index..index + 8].copy_from_slice(&ordinal.to_le_bytes());
                    }
                    let channel = if let Some(provider) = row.provider.object {
                        channel_map(provider, row.channel)
                            .context("untranslated controller provider channel")?
                    } else {
                        row.channel
                    };
                    bytes[64..68].copy_from_slice(&channel.to_le_bytes());
                    bytes[68..72].copy_from_slice(&row.flags.to_le_bytes());
                    Ok(bytes)
                })
                .collect()
        };
        Ok(NativeRows {
            connections: lower(&self.connections)?,
            named_connections: lower(&self.named_connections)?,
        })
    }
}
