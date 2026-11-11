//! The retained native presentation graph. Its controller and Super bank become private copies,
//! the source presentation's sequences replace its own, and every reference into the controller
//! or bank moves to their private layouts before the graph and its replication are emitted again.
use super::*;

/// One private asset before allocation: its symbol, template, payload and explicit references.
pub(super) type PendingAsset = (String, u32, Payload, Vec<(usize, String)>);

/// The private symbols of the graph and of its own components.
pub(super) struct Names {
    pub(super) root: String,
    pub(super) controller: String,
    pub(super) bank: String,
    pub(super) channels: String,
}

impl Names {
    pub(super) fn new(symbol: &str) -> Self {
        Self {
            root: symbol.to_owned(),
            controller: format!("{symbol}-controller"),
            bank: format!("{symbol}-bank"),
            channels: format!("{symbol}-channels"),
        }
    }
}

/// The native Super presentation graph, its components and its one controller.
pub(super) struct NativeGraph {
    pub(super) graph: Payload,
    pub(super) connections: links::Graph,
    pub(super) controller_tag: u32,
    pub(super) bank_tag: u32,
    pub(super) originals: BTreeMap<u32, Payload>,
}

impl NativeGraph {
    pub(super) fn read(manager: &PackageManager, tag: u32) -> Result<Self> {
        let graph = read(manager, tag, 0x80809C0F)?;
        let connections = links::Graph::read(&graph, false)?;
        let bank_tag = ability_modifier::entity_bank(&graph.0)
            .map_err(anyhow::Error::msg)?
            .context("Native presentation has no ability bank")?;
        ensure!(
            ability_modifier::bank_slot(bank_tag)
                == Some(sundial::package_authoring::ability_bank::AbilityTarget::Super),
            "Self-revive requires a native Super presentation graph"
        );
        let mut originals = BTreeMap::new();
        let mut controllers = Vec::new();
        for &tag in &connections.components {
            let owner = read(manager, tag, 0x80809C36)?;
            let class = owner.u32(owner.pointer(16)? - 4)?;
            if runtime::native_type_inherits(class, 0x80803BB7) {
                controllers.push(tag);
            }
            originals.insert(tag, owner);
        }
        let [controller_tag] = controllers.as_slice() else {
            anyhow::bail!("Native presentation needs one controller");
        };
        Ok(Self {
            controller_tag: *controller_tag,
            graph,
            connections,
            bank_tag,
            originals,
        })
    }
}

/// Original rows into the replaced sequences, and the controller's own sequence inputs, are
/// superseded by the source presentation's connections. Returns the extended channel bank's owner,
/// which must be a retained native component.
pub(super) fn retain(
    connections: &mut links::Graph,
    presentation: &source::Presentation,
    (controller_tag, bank_tag): (u32, u32),
) -> Result<Option<u32>> {
    let replaced = &presentation.replaced;
    ensure!(
        !replaced.contains(&controller_tag) && !replaced.contains(&bank_tag),
        "Native presentation sequences include a controller or bank"
    );
    let channel_tag = presentation
        .channel_bank
        .as_ref()
        .map(|channels| channels.owner);
    ensure!(
        channel_tag.is_none_or(|tag| {
            ![controller_tag, bank_tag].contains(&tag) && !replaced.contains(&tag)
        }),
        "The extended channel bank is not a retained native component"
    );
    let superseded = |row: &links::Connection| {
        [row.consumer.object, row.provider.object]
            .into_iter()
            .flatten()
            .any(|object| replaced.contains(&object.owner))
            || row
                .consumer
                .object
                .is_some_and(|object| object.owner == controller_tag && object.class == 0x808092F4)
    };
    connections.connections.retain(|row| !superseded(row));
    connections.named_connections.retain(|row| !superseded(row));
    connections.components.retain(|tag| !replaced.contains(tag));
    Ok(channel_tag)
}

/// Moves a reference to the retained controller or Super bank to its private layout.
pub(super) struct Remap<'a> {
    tags: (u32, u32),
    offsets: &'a controller::native::Offsets,
    old_runtime: usize,
    old_definition: usize,
    old_classes: (u32, u32),
    pub(super) old_bank_definition: usize,
    bank_definition: usize,
}

impl<'a> Remap<'a> {
    pub(super) fn new(
        original_controller: &Payload,
        bank_template: &Payload,
        offsets: &'a controller::native::Offsets,
        bank_definition: usize,
        tags: (u32, u32),
    ) -> Result<Self> {
        let old_runtime = original_controller.pointer(16)?;
        let old_definition = original_controller.pointer(24)?;
        Ok(Self {
            tags,
            offsets,
            old_runtime,
            old_definition,
            old_classes: (
                original_controller.u32(old_runtime - 4)?,
                original_controller.u32(old_definition - 4)?,
            ),
            old_bank_definition: usize::try_from(
                bank_template.u64(bank_template.pointer(16)? + 8)?,
            )?,
            bank_definition,
        })
    }

    fn object(&self, mut object: links::Object) -> Result<links::Object> {
        let (controller_tag, bank_tag) = self.tags;
        if object.owner == controller_tag {
            if object.class == self.old_classes.0 {
                ensure!(
                    object.offset == self.old_runtime as u64,
                    "Native concrete controller reference is not its root"
                );
                object.class = 0x80804176;
            }
            if object.class == self.old_classes.1 {
                ensure!(
                    object.offset == self.old_definition as u64,
                    "Native concrete definition reference is not its root"
                );
                object.class = 0x80804177;
            }
            object.offset = u64::try_from(self.offsets.retained(usize::try_from(object.offset)?)?)?;
        } else if object.owner == bank_tag {
            let delta = usize::try_from(object.offset)?
                .checked_sub(self.old_bank_definition)
                .context("Bank endpoint precedes its definition")?;
            ensure!(
                matches!(delta, 0x48 | 0x60),
                "Native graph references a donor-specific bank interface"
            );
            object.offset = u64::try_from(self.bank_definition + delta)?;
        }
        Ok(object)
    }

    /// Every connection endpoint into the controller or bank.
    pub(super) fn connections(&self, connections: &mut links::Graph) -> Result<()> {
        for connection in connections
            .connections
            .iter_mut()
            .chain(&mut connections.named_connections)
        {
            for endpoint in [&mut connection.consumer, &mut connection.provider] {
                if let Some(object) = endpoint.object {
                    endpoint.object = Some(self.object(object)?);
                }
            }
        }
        Ok(())
    }
}

/// Activation creates the entity named at definition +7C0, as source +708 does, and upgrades
/// replace it through the bank's activation-entity rows. All nine controller inputs follow the
/// source's own mode-selected sequence providers. The common triple and both specialized triples
/// exist in the prepared controller. The source presentation's own connections join last.
pub(super) fn controller_inputs(
    (payload, patches): (&Payload, &mut Vec<(usize, String)>),
    connections: &mut links::Graph,
    presentation: &source::Presentation,
    controller_tag: u32,
    activation: Option<&String>,
) -> Result<()> {
    let runtime = payload.pointer(16)?;
    let definition = usize::try_from(payload.u64(runtime + 8)?)?;
    if let Some(base) = activation {
        ensure!(
            payload.u32(definition + 0x7C0)? == u32::MAX,
            "Retained controller already names an activation entity"
        );
        patches.push((definition + 0x7C0, base.clone()));
    }
    for (consumer, provider) in &presentation.controller_inputs {
        let offset = definition + consumer;
        ensure!(
            payload.u32(offset)? == controller_tag && payload.u32(offset + 4)? == 0x808092F3,
            "Retained controller sequence input is absent"
        );
        connections.connections.push(links::Connection {
            consumer: links::Endpoint {
                namespace: 0x811C9DC5,
                object: Some(links::Object {
                    owner: controller_tag,
                    class: 0x808092F4,
                    offset: u64::try_from(offset)?,
                }),
                selector: 0,
            },
            provider: provider.clone(),
            channel: 0,
            flags: 0,
        });
    }
    connections
        .connections
        .extend(presentation.connections.iter().cloned());
    Ok(())
}

/// The retained native owners, each component-local reference to the controller or bank moved,
/// and the extended channel bank in place of its native copy.
pub(super) fn owners(
    manager: &PackageManager,
    originals: &BTreeMap<u32, Payload>,
    presentation: &source::Presentation,
    remap: &Remap<'_>,
    (controller_tag, bank_tag): (u32, u32),
) -> Result<BTreeMap<u32, Payload>> {
    let replaced = &presentation.replaced;
    let mut owners = originals.clone();
    owners.retain(|tag, _| !replaced.contains(tag));
    // Component-local references can address another component directly. Graph connection rows
    // are not the only places an embedded controller address can appear.
    for (&tag, owner) in &mut owners {
        if tag != controller_tag && tag != bank_tag {
            local_references(manager, (tag, owner), remap, replaced)?;
        }
    }
    // The extension replaces the retained copy whole, so the copy must have needed no remap.
    if let Some(channels) = &presentation.channel_bank {
        ensure!(
            owners[&channels.owner].0 == originals[&channels.owner].0,
            "The extended channel bank refers to the controller or ability bank"
        );
        owners.insert(channels.owner, channels.payload.clone());
    }
    Ok(owners)
}

fn local_references(
    manager: &PackageManager,
    (tag, owner): (u32, &mut Payload),
    remap: &Remap<'_>,
    replaced: &BTreeSet<u32>,
) -> Result<()> {
    let (controller_tag, bank_tag) = remap.tags;
    for (at, target) in
        declared_payload_object_references(manager, tag, &owner.0).map_err(anyhow::Error::msg)?
    {
        ensure!(
            !replaced.contains(&target),
            "A retained component refers to a replaced presentation sequence"
        );
        if target != controller_tag && target != bank_tag {
            continue;
        }
        let object = remap.object(links::Object {
            owner: target,
            class: owner.u32(at + 4)?,
            offset: owner.u64(at + 8)?,
        })?;
        put(&mut owner.0, at + 4, &object.class.to_le_bytes())?;
        put(&mut owner.0, at + 8, &object.offset.to_le_bytes())?;
    }
    Ok(())
}

/// The source presentation's sequences join the graph under temporary identities that no native
/// owner holds.
pub(super) fn add_sequences(
    manager: &PackageManager,
    sequences: &BTreeMap<u32, &source::Sequence>,
    owners: &mut BTreeMap<u32, Payload>,
    connections: &mut links::Graph,
) -> Result<()> {
    for (&id, sequence) in sequences {
        ensure!(
            !owners.contains_key(&id) && manager.get_entry(TagHash(id)).is_none(),
            "Temporary sequence identity collides with a native owner"
        );
        owners.insert(id, sequence.payload.clone());
        connections.components.push(id);
    }
    Ok(())
}

/// The rewired graph and the owners it emits.
pub(super) struct Retained<'a> {
    pub(super) graph: &'a Payload,
    pub(super) connections: &'a links::Graph,
    pub(super) originals: &'a BTreeMap<u32, Payload>,
    pub(super) owners: &'a BTreeMap<u32, Payload>,
    pub(super) sequences: &'a BTreeMap<u32, &'a source::Sequence>,
    /// The controller's and the Super bank's tags.
    pub(super) tags: (u32, u32),
    pub(super) channel_tag: Option<u32>,
}

impl Retained<'_> {
    /// The native owner a component copies: a sequence's template, or the component itself.
    fn template_of(&self, tag: u32) -> u32 {
        self.sequences
            .get(&tag)
            .map_or(tag, |sequence| sequence.template)
    }
}

/// The provider registrations the controller and Super bank publish, at their private offsets.
pub(super) struct Providers {
    controller: Vec<Provider>,
    bank: Vec<Provider>,
}

impl Providers {
    pub(super) fn read(
        manager: &PackageManager,
        context: &Retained<'_>,
        controller: &controller::native::Prepared,
        bank: &controller::PreparedBank,
        (bank_template, old_bank_definition): (&Payload, usize),
    ) -> Result<Self> {
        let (controller_tag, bank_tag) = context.tags;
        let mut controller_providers = providers(
            manager,
            context.graph,
            controller_tag,
            &context.originals[&controller_tag],
            &controller.payload,
            |offset| controller.offsets.provider(offset),
        )?;
        let binding = controller_providers
            .iter_mut()
            .filter(|provider| provider.offset == controller.provider)
            .collect::<Vec<_>>();
        ensure!(
            binding.len() == 1,
            "Native slot-binding provider is missing or ambiguous"
        );
        for provider in binding {
            provider.metadata = controller.metadata.clone();
        }
        let bank_providers = providers(
            manager,
            context.graph,
            bank_tag,
            bank_template,
            &bank.payload,
            |offset| {
                if offset == bank_template.pointer(16)? {
                    return Ok(Some(bank.runtime));
                }
                let delta = offset
                    .checked_sub(old_bank_definition)
                    .context("Native bank provider precedes its definition")?;
                ensure!(
                    delta.checked_add(24).is_some_and(|end| end <= 0x88),
                    "Native bank provider leaves its common definition"
                );
                Ok(Some(bank.definition + delta))
            },
        )?;
        Ok(Self {
            controller: controller_providers,
            bank: bank_providers,
        })
    }
}

/// The emitted graph and replication record, with the patches that name their private owners.
pub(super) struct Emitted {
    graph: Payload,
    graph_patches: Vec<(usize, String)>,
    replication: Payload,
    replication_template: u32,
    replication_patches: Vec<(usize, String)>,
}

/// Emits the graph with every retained, private and sequence component, then its replication
/// record with the allocations they take.
pub(super) fn emit(
    manager: &PackageManager,
    context: &Retained<'_>,
    providers: &Providers,
    controller: &controller::native::Prepared,
    (bank_allocation, channel_allocation): (u32, Option<u32>),
    names: &Names,
    native_graph: u32,
) -> Result<Emitted> {
    let (controller_tag, bank_tag) = context.tags;
    let authored = [
        (controller_tag, &providers.controller),
        (bank_tag, &providers.bank),
    ]
    .into_iter()
    .map(|(tag, rows)| {
        (
            tag,
            rows.iter()
                .map(|row| compiled::authored::Provider {
                    key: row.key,
                    offset: row.offset,
                    metadata: &row.metadata,
                    condition: row.condition,
                })
                .collect(),
        )
    })
    .collect::<BTreeMap<_, _>>();
    let rows = context.connections.native_rows(
        &context.connections.components,
        context.owners,
        Ok,
        |_, channel| Ok(channel),
        |_, selector| Ok(selector),
    )?;
    let components = context
        .connections
        .components
        .iter()
        .map(|&tag| compiled::Component {
            owner: tag,
            payload: &context.owners[&tag],
            template: &context.originals[&context.template_of(tag)],
            entity: context.graph,
        })
        .collect::<Vec<_>>();
    let graph = compiled::emit_authored(context.graph, &components, &rows, &authored)?;
    let graph_patches = graph_patches(&graph, context, controller, names)?;
    let replication_template = context.graph.u32(0x88)?;
    read(manager, replication_template, 0x80809BB6)?;
    let mut allocations = BTreeMap::from([
        (
            controller_tag,
            context.originals[&controller_tag].u32(0x44)?,
        ),
        (bank_tag, bank_allocation),
    ]);
    if let (Some(owner), Some(template)) = (context.channel_tag, channel_allocation) {
        allocations.insert(owner, template);
    }
    let replication =
        compiled::replication::emit_authored(native_graph, &components, &allocations)?;
    let replication_patches = replication_patches(&replication, context, names)?;
    Ok(Emitted {
        graph,
        graph_patches,
        replication,
        replication_template,
        replication_patches,
    })
}

/// The emitted graph's references to its replication record, its sequences and the controller's
/// private slot-binding interface, which exactly one registration names.
fn graph_patches(
    graph: &Payload,
    context: &Retained<'_>,
    controller: &controller::native::Prepared,
    names: &Names,
) -> Result<Vec<(usize, String)>> {
    let mut patches = vec![(0x88, format!("{}-replication", names.root))];
    for at in (0..graph.0.len().saturating_sub(3)).step_by(4) {
        if let Some(sequence) = context.sequences.get(&graph.u32(at)?) {
            patches.push((at, sequence.symbol.clone()));
        }
    }
    let mut binding_count = 0;
    for row in graph.array(88, 40, Some(0x80809C22))? {
        if graph.u32(row + 16)? == context.tags.0
            && graph.u64(row + 24)? == controller.provider as u64
        {
            ensure!(
                graph.u32(row + 32)? == controller.metadata_template,
                "Authored slot-binding metadata identity differs"
            );
            patches.push((row + 32, format!("{}-interface", names.controller)));
            binding_count += 1;
        }
    }
    ensure!(
        binding_count == 1,
        "Authored slot-binding registration is missing or ambiguous"
    );
    Ok(patches)
}

/// The replication record's references to the graph and to each private allocation.
fn replication_patches(
    replication: &Payload,
    context: &Retained<'_>,
    names: &Names,
) -> Result<Vec<(usize, String)>> {
    let (controller_tag, bank_tag) = context.tags;
    let mut patches = vec![(8, names.root.clone())];
    for (&tag, row) in
        context
            .connections
            .components
            .iter()
            .zip(replication.array(0x30, 32, Some(0x80809BB8))?)
    {
        if tag == controller_tag {
            patches.push((row + 4, format!("{}-allocation", names.controller)));
        }
        if tag == bank_tag {
            patches.push((row + 4, format!("{}-allocation", names.bank)));
        }
        if Some(tag) == context.channel_tag {
            patches.push((row + 4, format!("{}-allocation", names.channels)));
        }
        if let Some(sequence) = context.sequences.get(&tag) {
            patches.push((row + 4, format!("{}-allocation", sequence.symbol)));
        }
    }
    Ok(patches)
}

/// The private symbol of every component and of the graph itself.
pub(super) fn symbols(
    context: &Retained<'_>,
    names: &Names,
    native_graph: u32,
) -> BTreeMap<u32, String> {
    let (controller_tag, bank_tag) = context.tags;
    context
        .connections
        .components
        .iter()
        .map(|&tag| {
            (
                tag,
                if tag == controller_tag {
                    names.controller.clone()
                } else if tag == bank_tag {
                    names.bank.clone()
                } else if let Some(sequence) = context.sequences.get(&tag) {
                    sequence.symbol.clone()
                } else {
                    format!("{}-{tag:08X}", names.root)
                },
            )
        })
        .chain([(native_graph, names.root.clone())])
        .collect()
}

/// Every owner as a pending asset with the references its own preparation recorded. A sequence
/// also gives its allocation.
pub(super) fn owner_assets(
    context: &Retained<'_>,
    symbols: &BTreeMap<u32, String>,
    controller: &controller::native::Prepared,
    bank: &controller::PreparedBank,
    names: &Names,
) -> Result<Vec<PendingAsset>> {
    let (controller_tag, bank_tag) = context.tags;
    let mut pending = Vec::new();
    for (&tag, payload) in context.owners {
        let sequence = context.sequences.get(&tag);
        let mut patches = if tag == controller_tag {
            controller.patches.clone()
        } else if tag == bank_tag {
            bank.patches.clone()
        } else {
            sequence.map_or_else(Vec::new, |sequence| sequence.patches.clone())
        };
        if tag == bank_tag {
            patches.push((0x44, format!("{}-allocation", names.bank)));
        }
        if Some(tag) == context.channel_tag {
            patches.push((0x44, format!("{}-allocation", names.channels)));
        }
        if let Some(sequence) = sequence {
            pending.push(sequence_allocation(sequence, (tag, payload), &mut patches)?);
        }
        pending.push((
            symbols[&tag].clone(),
            context.template_of(tag),
            payload.clone(),
            patches,
        ));
    }
    Ok(pending)
}

/// A sequence owner's allocation. Sealing copies the record stream, so every owner word needs
/// its private tag in `patches`.
fn sequence_allocation(
    sequence: &source::Sequence,
    (tag, payload): (u32, &Payload),
    patches: &mut Vec<(usize, String)>,
) -> Result<PendingAsset> {
    for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
        if payload.u32(at)? == tag {
            patches.push((at, sequence.symbol.clone()));
        }
    }
    patches.push((0x44, format!("{}-allocation", sequence.symbol)));
    Ok((
        format!("{}-allocation", sequence.symbol),
        sequence.allocation_template,
        sequence.allocation.clone(),
        Vec::new(),
    ))
}

/// The emitted graph, the controller's private interface, the replication record and the
/// allocations of the controller, bank and extended channel bank.
pub(super) fn graph_assets(
    emitted: Emitted,
    controller: &controller::native::Prepared,
    bank: &controller::PreparedBank,
    (context, bank_allocation, channels): (&Retained<'_>, u32, Option<&source::ChannelBank>),
    names: &Names,
    native_graph: u32,
) -> Result<Vec<PendingAsset>> {
    let controller_allocation = context.originals[&context.tags.0].u32(0x44)?;
    let mut pending = vec![
        (
            names.root.clone(),
            native_graph,
            emitted.graph,
            emitted.graph_patches,
        ),
        (
            format!("{}-interface", names.controller),
            controller.metadata_template,
            controller.metadata.clone(),
            Vec::new(),
        ),
        (
            format!("{}-replication", names.root),
            emitted.replication_template,
            emitted.replication,
            emitted.replication_patches,
        ),
        (
            format!("{}-allocation", names.controller),
            controller_allocation,
            controller.allocation.clone(),
            Vec::new(),
        ),
        (
            format!("{}-allocation", names.bank),
            bank_allocation,
            bank.allocation.clone(),
            Vec::new(),
        ),
    ];
    if let Some(channels) = channels {
        pending.push((
            format!("{}-allocation", names.channels),
            channels.allocation_template,
            channels.allocation.clone(),
            Vec::new(),
        ));
    }
    Ok(pending)
}
