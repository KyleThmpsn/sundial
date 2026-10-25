//! Compose selected native implementations without inheriting another projectile's graph.
use super::*;
use crate::tiger::entity::links::{self, Object};

pub(super) struct Assembly {
    pub entity: Payload,
    pub owners: BTreeMap<u32, Payload>,
    pub allocations: BTreeMap<u32, Payload>,
    pub source_trajectory: bool,
}

impl Assembly {
    pub fn prepare(
        reader: &mut Reader,
        tag: u32,
        behavior: &Value,
    ) -> Result<(Self, Option<shard::Shards>)> {
        let shards = shard::Shards::read(behavior)?;
        let assembly = Self::read(reader, tag)?;
        let assembly = if shards.is_some() || super::tracking::enabled(behavior)? {
            attached(reader, assembly, behavior)?
        } else {
            assembly
        };
        Ok((assembly, shards))
    }

    pub fn read(reader: &mut Reader, tag: u32) -> Result<Self> {
        let entity = (*reader.tag(tag, Some(0x80809c0f))?).clone();
        let owners = links::Graph::read(&entity, false)?
            .components
            .into_iter()
            .map(|tag| Ok((tag, (*reader.tag(tag, Some(0x80809c36))?).clone())))
            .collect::<Result<_>>()?;
        Ok(Self {
            entity,
            owners,
            allocations: BTreeMap::new(),
            source_trajectory: false,
        })
    }

    pub fn class(p: &Payload) -> Result<u32> {
        p.u32(p.pointer(24)? - 4)
    }

    pub fn one(&self, class: u32) -> Result<u32> {
        let tags = self
            .owners
            .iter()
            .filter_map(|(&tag, p)| (Self::class(p).ok() == Some(class)).then_some(tag))
            .collect::<Vec<_>>();
        ensure!(
            tags.len() == 1,
            "Projectile implementation {class:08X} is absent or ambiguous"
        );
        Ok(tags[0])
    }
}

/// Match the provider metadata and reciprocal implementation, rather than assuming offsets.
fn provider(source: Object, old: &Payload, tag: u32, new: &Payload) -> Result<Object> {
    let at = usize::try_from(source.offset)?;
    let definition = old.pointer(at)?;
    let class = old.u32(definition + 4)?;
    let metadata = old.bytes::<16>(at + 8)?;
    let matches = (0..new.0.len().saturating_sub(23))
        .step_by(8)
        .filter(|&at| {
            new.bytes::<16>(at + 8).ok() == Some(metadata)
                && new.pointer(at).ok().and_then(|d| new.u32(d + 4).ok()) == Some(class)
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Projectile provider implementation is absent or ambiguous"
    );
    Ok(Object {
        owner: tag,
        class: source.class,
        offset: matches[0] as u64,
    })
}

fn attached(reader: &mut Reader, presentation: Assembly, behavior: &Value) -> Result<Assembly> {
    let mechanical = Assembly::read(reader, 0x80ef28e0)?;
    let guidance = Assembly::read(reader, 0x80bf9d64)?;
    let tracking = super::tracking::enabled(behavior)?;
    let movement = mechanical.one(0x8080388f)?;
    let previous = guidance.one(0x8080388f)?;
    let mut selected = Vec::new();
    // The source render carrier contributes presentation only. In particular its
    // grenade health, cluster emitter and grenade response controller are omitted.
    for (&tag, p) in &presentation.owners {
        if matches!(
            Assembly::class(p)?,
            0x808072bd | 0x80808a0c | 0x80808507 | 0x808084e9
        ) {
            selected.push((tag, &presentation));
        }
    }
    for (&tag, p) in &mechanical.owners {
        if Assembly::class(p)? != 0x808084e9 {
            selected.push((tag, &mechanical));
        }
    }
    if tracking {
        for class in [0x8080377d, 0x80803769] {
            selected.push((guidance.one(class)?, &guidance));
        }
    }
    let mut owners = selected
        .iter()
        .map(|(tag, source)| (*tag, source.owners[tag].clone()))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        owners.len() == selected.len(),
        "Repeated assembled projectile component"
    );
    {
        use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
            Kind, Stored,
        };
        let p = owners.get_mut(&movement).unwrap();
        let instance = p.pointer(16)?;
        let speed =
            Stored::read(Kind::Speed, &p.0, movement, instance).map_err(anyhow::Error::msg)?;
        // This controller normally uses the instant-hit sentinel. A shard must
        // enter the moving path before the host's normal launch adaptation.
        speed.write(&mut p.0, 1.).map_err(anyhow::Error::msg)?;
    }
    let removed = if tracking {
        super::tracking::prepare(&mut owners, behavior)?
    } else {
        BTreeSet::new()
    };
    let mut graph = connections(&selected, &owners, &removed, previous, movement)?;
    let mut allocations = BTreeMap::new();
    if tracking {
        super::tracking::bind(
            reader,
            &mut owners,
            &mut graph,
            &mut allocations,
            behavior,
            &removed,
        )?;
    }
    let mut endpoints = owners.clone();
    for object in graph.objects() {
        if let std::collections::btree_map::Entry::Vacant(entry) = endpoints.entry(object.owner) {
            entry.insert((*reader.tag(object.owner, Some(0x80809c36))?).clone());
        }
    }
    let rows = graph.native_rows(
        &graph.components,
        &endpoints,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let components = selected
        .iter()
        .map(|(tag, source)| compiled::Component {
            owner: *tag,
            payload: &owners[tag],
            template: &source.owners[tag],
            entity: &source.entity,
        })
        .collect::<Vec<_>>();
    // Keep the selected presentation host's entity contract, including its
    // replication template. A movement donor can be a non-networked fragment.
    let entity = compiled::emit(&presentation.entity, &components, &rows)?;
    Ok(Assembly {
        entity,
        owners,
        allocations,
        source_trajectory: true,
    })
}

fn connections(
    selected: &[(u32, &Assembly)],
    owners: &BTreeMap<u32, Payload>,
    removed: &BTreeSet<Object>,
    previous: u32,
    movement: u32,
) -> Result<links::Graph> {
    let mut graph = links::Graph {
        components: selected.iter().map(|(tag, _)| *tag).collect(),
        connections: Vec::new(),
        named_connections: Vec::new(),
    };
    for &(tag, source) in selected {
        let original = links::Graph::read(&source.entity, false)?;
        for (rows, output) in [
            (original.connections, &mut graph.connections),
            (original.named_connections, &mut graph.named_connections),
        ] {
            for mut row in rows {
                let Some(consumer) = row.consumer.object else {
                    continue;
                };
                if consumer.owner != tag || removed.contains(&consumer) {
                    continue;
                }
                // A flow's controller inputs keep their connections: its nodes read them
                // beside the autonomous flight branch, and an unconnected input faults the
                // client when a node first evaluates it.
                if let Some(object) = row.provider.object {
                    if object.owner == previous {
                        row.provider.object = Some(provider(
                            object,
                            source
                                .owners
                                .get(&previous)
                                .context("Projectile provider source")?,
                            movement,
                            &owners[&movement],
                        )?);
                    } else if row.provider.selector < 0xffff && !owners.contains_key(&object.owner)
                    {
                        // An omitted provider of the consumer's own source, such as the render
                        // carrier's channel interpolation owner that the flow's controller
                        // inputs read, maps to the assembled owner of the same class by its
                        // provider metadata, as the guidance movement maps to the mechanical.
                        let old = source
                            .owners
                            .get(&object.owner)
                            .context("Projectile provider source")?;
                        let class = Assembly::class(old)?;
                        let candidates = owners
                            .iter()
                            .filter(|(_, p)| Assembly::class(p).ok() == Some(class))
                            .map(|(tag, _)| *tag)
                            .collect::<Vec<_>>();
                        ensure!(
                            candidates.len() == 1,
                            "Selected projectile component {:08X} ({:08X} at +{:X}) requires an omitted provider {:08X} ({:08X} at +{:X}, selector {:X})",
                            consumer.owner,
                            consumer.class,
                            consumer.offset,
                            object.owner,
                            object.class,
                            object.offset,
                            row.provider.selector
                        );
                        row.provider.object = Some(provider(
                            object,
                            old,
                            candidates[0],
                            &owners[&candidates[0]],
                        )?);
                    }
                }
                output.push(row);
            }
        }
    }
    Ok(graph)
}

pub(super) fn without_inputs(
    entity: &Payload,
    owners: &BTreeMap<u32, Payload>,
    removed: &BTreeSet<Object>,
) -> Result<Payload> {
    let graph = links::Graph::read(entity, false)?;
    // Remaining references retain their native addresses and ordinals. External
    // objects are unchanged, so reuse their already validated serialized rows.
    let keep = |rows: Vec<usize>| -> Result<Vec<Vec<u8>>> {
        let mut kept = Vec::new();
        for at in rows {
            let object = Object {
                owner: entity.u32(at + 8)?,
                class: entity.u32(at + 12)?,
                offset: entity.u64(at + 16)?,
            };
            if !removed.contains(&object) {
                kept.push(entity.bytes::<72>(at)?.to_vec());
            }
        }
        Ok(kept)
    };
    let rows = links::NativeRows {
        connections: keep(entity.array(32, 72, Some(0x80809bc9))?)?,
        named_connections: keep(entity.array(48, 72, Some(0x80809bc9))?)?,
    };
    let components = graph
        .components
        .iter()
        .map(|tag| compiled::Component {
            owner: *tag,
            payload: &owners[tag],
            template: &owners[tag],
            entity,
        })
        .collect::<Vec<_>>();
    compiled::emit(entity, &components, &rows)
}
