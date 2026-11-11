//! Compose selected native implementations without inheriting another projectile's graph.
use super::*;
use crate::tiger::entity::links::{self, Object};

pub(super) struct Assembly {
    pub entity: Payload,
    pub owners: BTreeMap<u32, Payload>,
    pub allocations: BTreeMap<u32, Payload>,
    pub source_trajectory: bool,
    /// The launch mode change and how kept consumers of an omitted channel bank read the
    /// assembled bank.
    pub report: Value,
}

/// A kept consumer that read an omitted channel bank by ordinal, waiting for the assembled bank to
/// declare the same name.
struct Rename {
    named: bool,
    index: usize,
    name: u32,
    source: usize,
    bank: u32,
    old_bank: u32,
}

/// A bank's declaration names at definition `+D8`, in ordinal order.
fn declared(bank: &Payload) -> Result<Vec<u32>> {
    let definition = bank.pointer(24)?;
    bank.array(definition + 0xd8, 112, Some(0x808097a1))?
        .into_iter()
        .map(|row| bank.u32(row))
        .collect()
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

    /// Adds a stock component that another entity hosts without connections, such as the
    /// once-sound flow a vehicle's firing sound plays from. The entity keeps its connections and
    /// component ordinals, and the added component keeps its host's registration.
    pub fn host(&mut self, reader: &mut Reader, owner: u32, host: u32) -> Result<()> {
        let hosted = (*reader.tag(host, Some(0x80809c0f))?).clone();
        let links = links::Graph::read(&hosted, false)?;
        ensure!(
            links.components.contains(&owner)
                && links.connections.is_empty()
                && links.named_connections.is_empty(),
            "Hosted projectile component needs an unconnected host"
        );
        ensure!(
            !self.owners.contains_key(&owner),
            "Hosted projectile component already exists"
        );
        let payload = (*reader.tag(owner, Some(0x80809c36))?).clone();
        let graph = links::Graph::read(&self.entity, false)?;
        let mut endpoints = self.owners.clone();
        for object in graph.objects() {
            if let std::collections::btree_map::Entry::Vacant(entry) = endpoints.entry(object.owner)
            {
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
        self.owners.insert(owner, payload);
        let entity = {
            let mut components = graph
                .components
                .iter()
                .map(|tag| compiled::Component {
                    owner: *tag,
                    payload: &self.owners[tag],
                    template: &self.owners[tag],
                    entity: &self.entity,
                })
                .collect::<Vec<_>>();
            components.push(compiled::Component {
                owner,
                payload: &self.owners[&owner],
                template: &self.owners[&owner],
                entity: &hosted,
            });
            compiled::emit(&self.entity, &components, &rows)?
        };
        self.entity = entity;
        Ok(())
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
            report: Value::Null,
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
    let guidance = Assembly::read(reader, GUIDANCE)?;
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
    let launch;
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
        // Flag bit 20 of definition `+0x48` selects the instant trace. All 66 stock movements at
        // the instant-hit speed set it, as do 11 near-instant ones at 200 to 9000, while the other
        // 383 moving movements clear it and hold `FFFFFFFF` at `+0x6C`, where traces keep a small
        // count. With the bit kept, a shard hits at once and only its visual flies.
        let definition = p.pointer(24)?;
        let flags = p.u32(definition + 0x48)?;
        let trace = p.u32(definition + 0x6c)?;
        put(
            &mut p.0,
            definition + 0x48,
            &(flags & !0x0010_0000).to_le_bytes(),
        )?;
        put(&mut p.0, definition + 0x6c, &u32::MAX.to_le_bytes())?;
        // The low bytes of `+0x78` hold trace settings (`0300`, `0404`) on instant movements and
        // are zero on most moving ones. The Symmetry seeker's moving edit clears them the same way.
        let settings = p.u32(definition + 0x78)?;
        put(
            &mut p.0,
            definition + 0x78,
            &(settings & 0xffff_0000).to_le_bytes(),
        )?;
        launch = json!({"flags_before":format!("{flags:08X}"),
            "flags_after":format!("{:08X}", flags & !0x0010_0000),"trace_count_before":trace,
            "settings_before":format!("{settings:08X}"),
            "basis":"Stock movement survey: instant trace bit 20 of definition +0x48 and +0x6C FFFFFFFF on moving movements"});
    }
    let removed = if tracking {
        super::tracking::prepare(&mut owners, behavior)?
    } else {
        BTreeSet::new()
    };
    let (mut graph, renames) = connections(&selected, &owners, &removed, previous, movement)?;
    let mut allocations = BTreeMap::new();
    let channels = declare(
        reader,
        &selected,
        &mut owners,
        &mut graph,
        &mut allocations,
        &renames,
    )?;
    if !tracking && let Some(rename) = renames.first() {
        // Guidance seals the bank after its own inputs. Without guidance the grown arrays still
        // need a definition view past them, or the client's instance copy leaves them out.
        let p = owners
            .get_mut(&rename.bank)
            .context("Assembled channel bank")?;
        let end = p.0.len();
        crate::tiger::instance::seal_span(p, &mut Vec::new(), end)?;
    }
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
        report: json!({"launch":launch,"channels":channels}),
    })
}

/// The guided grenade whose steering (`8080377D`) and target acquisition (`80803769`) a guided
/// source flies with.
const GUIDANCE: u32 = 0x80bf_9d64;

/// The effect-only carrier `carrier` without its `omitted` components and with the guided
/// grenade's steering and target acquisition, so that a guided source without a render model
/// steers. Both read the carrier's movement through the provider the grenade's movement offers,
/// matched by its metadata, and the steering's turn rate binds to the movement's speed and
/// distance through the carrier's channel bank, as on a modeled guided carrier. The carrier keeps
/// its own movement, flow, contact responses and launch.
pub(super) fn steered(
    reader: &mut Reader,
    carrier: &Assembly,
    omitted: &BTreeSet<u32>,
    behavior: &Value,
) -> Result<Assembly> {
    let original = links::Graph::read(&carrier.entity, false)?;
    ensure!(
        !original
            .connections
            .iter()
            .chain(&original.named_connections)
            .any(|row| {
                row.provider
                    .object
                    .is_some_and(|object| omitted.contains(&object.owner))
                    && row
                        .consumer
                        .object
                        .is_some_and(|object| !omitted.contains(&object.owner))
            }),
        "An omitted carrier component feeds another component"
    );
    let guidance = Assembly::read(reader, GUIDANCE)?;
    let movement = carrier.one(0x8080388f)?;
    let previous = guidance.one(0x8080388f)?;
    let mut selected = carrier
        .owners
        .keys()
        .filter(|tag| !omitted.contains(tag))
        .map(|&tag| (tag, carrier))
        .collect::<Vec<_>>();
    for class in [0x8080377d, 0x80803769] {
        selected.push((guidance.one(class)?, &guidance));
    }
    let mut owners = selected
        .iter()
        .map(|(tag, source)| (*tag, source.owners[tag].clone()))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        owners.len() == selected.len(),
        "Repeated steered projectile component"
    );
    let removed = super::tracking::prepare(&mut owners, behavior)?;
    let (mut graph, renames) = connections(&selected, &owners, &removed, previous, movement)?;
    let mut allocations = BTreeMap::new();
    let channels = declare(
        reader,
        &selected,
        &mut owners,
        &mut graph,
        &mut allocations,
        &renames,
    )?;
    super::tracking::bind(
        reader,
        &mut owners,
        &mut graph,
        &mut allocations,
        behavior,
        &removed,
    )?;
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
    let entity = compiled::emit(&carrier.entity, &components, &rows)?;
    let hex = |tags: Vec<u32>| {
        tags.into_iter()
            .map(|tag| format!("{tag:08X}"))
            .collect::<Vec<_>>()
    };
    let grafted = selected[selected.len() - 2..]
        .iter()
        .map(|(tag, _)| *tag)
        .collect();
    let report = json!({"guidance":format!("{GUIDANCE:08X}"),"components":hex(grafted),
        "omitted":hex(omitted.iter().copied().collect()),"channels":channels});
    Ok(Assembly {
        entity,
        owners,
        allocations,
        source_trajectory: carrier.source_trajectory,
        report,
    })
}

fn connections(
    selected: &[(u32, &Assembly)],
    owners: &BTreeMap<u32, Payload>,
    removed: &BTreeSet<Object>,
    previous: u32,
    movement: u32,
) -> Result<(links::Graph, Vec<Rename>)> {
    let mut graph = links::Graph {
        components: selected.iter().map(|(tag, _)| *tag).collect(),
        connections: Vec::new(),
        named_connections: Vec::new(),
    };
    let mut renames = Vec::new();
    for (index, &(tag, source)) in selected.iter().enumerate() {
        let original = links::Graph::read(&source.entity, false)?;
        for (rows, named) in [
            (original.connections, false),
            (original.named_connections, true),
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
                        // A bank's provider names its value by ordinal, and the assembled bank
                        // orders other names. The render carrier's flow reads damage type
                        // `49FCE899`, attachment `FCE565B9` and perk keys there, so the row takes
                        // the assembled ordinal of the same name or waits for its declaration.
                        if class == 0x80809790 && object.class == 0x808097c2 {
                            let name = *declared(old)?
                                .get(usize::try_from(row.channel)?)
                                .context("Omitted bank channel exceeds its declarations")?;
                            match declared(&owners[&candidates[0]])?
                                .iter()
                                .position(|&declared| declared == name)
                            {
                                Some(ordinal) => row.channel = u32::try_from(ordinal)?,
                                None => {
                                    renames.push(Rename {
                                        named,
                                        index: if named {
                                            graph.named_connections.len()
                                        } else {
                                            graph.connections.len()
                                        },
                                        name,
                                        source: index,
                                        bank: candidates[0],
                                        old_bank: object.owner,
                                    });
                                    row.channel = u32::MAX;
                                }
                            }
                        }
                    }
                }
                if named {
                    graph.named_connections.push(row);
                } else {
                    graph.connections.push(row);
                }
            }
        }
    }
    Ok((graph, renames))
}

/// Declares on the assembled bank each name an omitted bank fed to a kept consumer. The input that
/// fed the omitted bank keeps its provider, mapped to the assembled owner of its class by provider
/// metadata. A name whose provider the assembly lacks, such as the shared `80803F6E` owner's,
/// takes an inherited declaration, which holds its default until a host publishes the name.
fn declare(
    reader: &mut Reader,
    selected: &[(u32, &Assembly)],
    owners: &mut BTreeMap<u32, Payload>,
    graph: &mut links::Graph,
    allocations: &mut BTreeMap<u32, Payload>,
    renames: &[Rename],
) -> Result<Value> {
    use crate::tiger::channel::{inherited, inputs};
    let Some(first) = renames.first() else {
        return Ok(Value::Null);
    };
    ensure!(
        renames
            .iter()
            .all(|rename| (rename.bank, rename.old_bank, rename.source)
                == (first.bank, first.old_bank, first.source)),
        "Omitted channel banks are ambiguous"
    );
    let bank = first.bank;
    let source = selected[first.source].1;
    let old = source
        .owners
        .get(&first.old_bank)
        .context("Omitted channel bank")?;
    let old_names = declared(old)?;
    let definition = old.pointer(24)?;
    let old_inputs = old.array(definition + 0xc8, 32, Some(0x808097a8))?;
    let old_rows = old.array(definition + 0xd8, 112, Some(0x808097a1))?;
    let original = links::Graph::read(&source.entity, false)?;
    let names = renames
        .iter()
        .map(|rename| rename.name)
        .collect::<BTreeSet<_>>();
    let mut fed = Vec::new();
    let mut defaults = Vec::new();
    for &name in &names {
        let ordinal = old_names
            .iter()
            .position(|&declared| declared == name)
            .context("Omitted bank name")?;
        // The omitted bank's input of this ordinal, and the one row that feeds it.
        let feeding = old_inputs.get(ordinal).and_then(|&input| {
            let consumer = Object {
                owner: first.old_bank,
                class: 0x808097a8,
                offset: input as u64,
            };
            let rows = original
                .connections
                .iter()
                .chain(&original.named_connections)
                .filter(|row| row.consumer.object == Some(consumer))
                .collect::<Vec<_>>();
            match rows.as_slice() {
                [row] => row.provider.object,
                _ => None,
            }
        });
        let mapped = feeding.and_then(|object| {
            if owners.contains_key(&object.owner) {
                return Some(object);
            }
            let payload = source.owners.get(&object.owner)?;
            let class = Assembly::class(payload).ok()?;
            let candidates = owners
                .iter()
                .filter(|(_, p)| Assembly::class(p).ok() == Some(class))
                .map(|(&tag, _)| tag)
                .collect::<Vec<_>>();
            let [tag] = candidates.as_slice() else {
                return None;
            };
            provider(object, payload, *tag, &owners[tag]).ok()
        });
        // The incoming lookup keys an input by its provider's property, which is the name here.
        let property = mapped
            .map(|object| owners[&object.owner].u32(usize::try_from(object.offset)? + 24))
            .transpose()?;
        match (mapped, property) {
            (Some(object), Some(property)) if property == name => fed.push((
                inputs::Input {
                    name,
                    namespace: 0x811c9dc5,
                    property,
                    value_type: old.u16(old_rows[ordinal] + 74)?,
                },
                object,
            )),
            _ => defaults.push(name),
        }
    }
    let allocation_tag = owners[&bank].u32(0x44)?;
    let mut allocation = match allocations.get(&allocation_tag) {
        Some(grown) => grown.clone(),
        None => (*reader.tag(allocation_tag, Some(0x80809bbb))?).clone(),
    };
    let mut payload = owners[&bank].clone();
    let ordinal = |graph: &links::Graph, owner: u32| -> Result<u64> {
        Ok(graph
            .components
            .iter()
            .position(|&tag| tag == owner)
            .context("Assembled channel component")? as u64)
    };
    if !fed.is_empty() {
        let declarations = fed
            .iter()
            .map(|(input, _)| inputs::Input {
                name: input.name,
                namespace: input.namespace,
                property: input.property,
                value_type: input.value_type,
            })
            .collect::<Vec<_>>();
        let extension = inputs::extend(&payload, &allocation, &declarations)?;
        for row in graph
            .connections
            .iter_mut()
            .chain(&mut graph.named_connections)
        {
            if row
                .provider
                .object
                .is_some_and(|v| v.owner == bank && v.class == 0x808097c2)
            {
                row.channel = extension.channel(row.channel)?;
            }
            if let Some(object) = row
                .consumer
                .object
                .as_mut()
                .filter(|v| v.owner == bank && v.class == 0x808097a8)
            {
                object.offset = *extension
                    .relocated
                    .get(&usize::try_from(object.offset)?)
                    .context("Assembled bank consumer outside its inputs")?
                    as u64;
            }
        }
        for ((_, object), &declaration) in fed.iter().zip(&extension.declarations) {
            let consumer = links::Endpoint {
                namespace: 0x811c9dc5,
                selector: ordinal(graph, bank)?,
                object: Some(Object {
                    owner: bank,
                    class: 0x808097a8,
                    offset: declaration as u64,
                }),
            };
            let feeder = links::Endpoint {
                namespace: 0x811c9dc5,
                selector: ordinal(graph, object.owner)?,
                object: Some(*object),
            };
            graph.connections.push(links::Connection {
                consumer,
                provider: feeder,
                channel: 0,
                flags: 0,
            });
        }
        payload = extension.payload;
        allocation = extension.allocation;
    }
    if !defaults.is_empty() {
        (payload, allocation) = inherited::extend(&payload, &allocation, &defaults)?;
    }
    let declared_now = declared(&payload)?;
    for rename in renames {
        let rows = if rename.named {
            &mut graph.named_connections
        } else {
            &mut graph.connections
        };
        rows[rename.index].channel = u32::try_from(
            declared_now
                .iter()
                .position(|&declared| declared == rename.name)
                .context("Assembled bank lacks a declared name")?,
        )?;
    }
    owners.insert(bank, payload);
    allocations.insert(allocation_tag, allocation);
    Ok(
        json!({"bank":format!("{bank:08X}"),"omitted":format!("{:08X}", first.old_bank),
        "fed":fed.iter().map(|(input, object)| json!({"name":format!("{:08X}", input.name),
            "provider":format!("{:08X}", object.owner),"offset":object.offset})).collect::<Vec<_>>(),
        "defaulted":defaults.iter().map(|name| format!("{name:08X}")).collect::<Vec<_>>(),
        "rows":renames.len()}),
    )
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
