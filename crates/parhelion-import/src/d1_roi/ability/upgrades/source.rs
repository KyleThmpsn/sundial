//! Original Rise of Iron presentation sequences on the retained Shadowkeep controller.
//!
//! Every source sequence occurrence in the Super graph becomes a private native sequence owner,
//! started through the same controller inputs that started it in the source. Particle systems,
//! cues and HUD resources come from the ability's shared effect resources.
use super::*;
use crate::{
    d1_roi::{
        connections,
        effect::{self, Shared},
    },
    tiger::reader::Reader,
};

pub(super) struct Sequence {
    pub id: u32,
    pub occurrence: usize,
    pub template: u32,
    pub allocation_template: u32,
    pub symbol: String,
    pub payload: Payload,
    pub allocation: Payload,
    pub patches: Vec<(usize, String)>,
}

/// The native channel bank with the source declarations it lacked, sealed, and its allocation.
/// Its `+0x44` allocation word is cleared for the private allocation.
pub(super) struct ChannelBank {
    pub owner: u32,
    pub payload: Payload,
    pub allocation_template: u32,
    pub allocation: Payload,
}

pub(super) struct Presentation {
    pub sequences: Vec<Sequence>,
    /// Native sequence components replaced by the source presentation.
    pub replaced: BTreeSet<u32>,
    /// Source-input connections from each translated sequence to the native channel bank.
    pub connections: Vec<links::Connection>,
    /// Present when the native channel bank needed the source's own declarations.
    pub channel_bank: Option<ChannelBank>,
    /// Retained-controller consumer definitions, relative to its definition root, with the
    /// translated provider the source connects to each one.
    pub controller_inputs: Vec<(usize, links::Endpoint)>,
    pub dependencies: Vec<dependency::Asset>,
    pub evidence: Value,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    source: &Source,
    native: &mut Reader,
    graph: u32,
    controller: u32,
    native_graph: &links::Graph,
    originals: &BTreeMap<u32, Payload>,
    shared: &Shared,
    symbol: &str,
) -> Result<Presentation> {
    let source_graph = connections::Graph::read(&source.read(graph)?)?;
    let positions = source_graph
        .components
        .iter()
        .enumerate()
        .filter(|(_, tag)| **tag == controller)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [occurrence] = positions.as_slice() else {
        anyhow::bail!("Source Radiance controller occurrence is missing or repeated");
    };
    let sequences = controller::Sequences::read(
        source,
        &source.read(controller)?,
        &source_graph,
        *occurrence,
    )?;

    // Native envelopes come from the presentation graph's own sequence owners.
    let mut native_sequences = Vec::new();
    for &tag in &native_graph.components {
        if effect::is_sequence(&originals[&tag])? {
            native_sequences.push(tag);
        }
    }
    let providers = native_graph
        .connections
        .iter()
        .filter_map(|row| row.provider.object)
        .filter(|object| object.class == 0x808092F8)
        .map(|object| object.owner)
        .collect::<BTreeSet<_>>();
    let template = native_sequences
        .iter()
        .copied()
        .filter(|tag| providers.contains(tag))
        .min_by_key(|tag| (originals[tag].0.len(), *tag))
        .context("Native presentation has no controller sequence provider")?;
    let input_envelope = native_sequences
        .iter()
        .copied()
        .find(|tag| effect::has_inputs(&originals[tag]))
        .context("Native presentation has no scalar input envelope")?;
    let envelopes = native_sequences
        .iter()
        .map(|tag| originals[tag].clone())
        .chain(shared.witnesses.iter().cloned())
        .collect::<Vec<_>>();
    let allocation_template = originals[&template].u32(0x44)?;
    let allocation = Payload(native.tag(allocation_template, Some(0x80809BBB))?.0.clone());

    let occurrences = effect::occurrences(source, &source_graph.components)?;
    ensure!(
        sequences
            .consumers()
            .iter()
            .all(|(_, provider)| occurrences.contains_key(provider)),
        "A Radiance sequence input names another component class"
    );

    // Every source scalar input reads a declaration of the Super graph's own channel bank. The
    // native bank already feeding the replaced sequences supplies the names both declare, and
    // the rest are added the way the source declares them. No input may stay unbound, since a
    // native kind-zero control condition follows its input's provider unchecked.
    let channel = native_graph
        .connections
        .iter()
        .filter_map(|row| row.provider.object)
        .find(|object| object.class == 0x808097C2)
        .context("Native presentation has no channel provider")?;
    let inputs = source_graph
        .connections
        .iter()
        .filter(|row| {
            row.consumer.class == 0x80800927
                && usize::try_from(row.consumer.selector)
                    .is_ok_and(|selector| occurrences.contains_key(&selector))
        })
        .collect::<Vec<_>>();
    let source_banks = inputs
        .iter()
        .map(|row| (row.provider.owner, row.provider.class))
        .collect::<BTreeSet<_>>();
    ensure!(
        source_banks.len() == 1 && source_banks.iter().all(|(_, class)| *class == 0x808005AB),
        "Radiance sequence inputs read another provider than one channel bank"
    );
    let (source_bank, _) = *source_banks.first().context("Source channel bank")?;
    let source_bank = source.read(source_bank)?;
    let source_schema = source_bank.pointer(24)?;
    let source_names = source_bank
        .array(source_schema + 0xD8, 112, Some(0x80800883))?
        .into_iter()
        .map(|row| source_bank.u32(row))
        .collect::<Result<Vec<_>>>()?;
    let source_inherited = source_bank
        .array(source_schema + 0x108, 12, None)?
        .into_iter()
        .map(|at| {
            Ok((
                usize::try_from(source_bank.u32(at + 8)?)?,
                (source_bank.u32(at)?, source_bank.u32(at + 4)?),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let read_names = |bank: &Payload| -> Result<Vec<u32>> {
        bank.array(bank.pointer(24)? + 0xD8, 112, Some(0x808097A1))?
            .into_iter()
            .map(|row| bank.u32(row))
            .collect()
    };
    let bank = &originals[&channel.owner];
    let native_names = read_names(bank)?;
    let mut added = Vec::new();
    for row in &inputs {
        let ordinal = usize::try_from(row.channel)?;
        let name = *source_names
            .get(ordinal)
            .context("Source sequence input reads no channel declaration")?;
        if native_names.contains(&name) || added.contains(&name) {
            continue;
        }
        // The native extension declares a name inherited from the parent channel of that name.
        ensure!(
            source_inherited.get(&ordinal) == Some(&(0x811C9DC5, name)),
            "Source channel {name:08X} has a producer the native bank lacks"
        );
        added.push(name);
    }
    let channel_bank = if added.is_empty() {
        None
    } else {
        let bank_allocation = bank.u32(0x44)?;
        let stock_allocation = native.tag(bank_allocation, Some(0x80809BBB))?;
        let (mut payload, extended_allocation) =
            crate::tiger::channel::inherited::extend(bank, &stock_allocation, &added)?;
        let names = read_names(&payload)?;
        let schema = payload.pointer(24)?;
        let lookups = payload
            .array(schema + 0x108, 12, Some(0x808097A0))?
            .into_iter()
            .map(|at| {
                Ok((
                    payload.u32(at + 8)?,
                    (payload.u32(at)?, payload.u32(at + 4)?),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        for &name in &added {
            let ordinal = names
                .iter()
                .position(|&v| v == name)
                .context("Added channel")?;
            ensure!(
                lookups.get(&u32::try_from(ordinal)?) == Some(&(0x811C9DC5, name)),
                "Added channel {name:08X} is declared unlike the source"
            );
        }
        let layout = crate::tiger::instance::LAYOUTS
            .iter()
            .find(|layout| layout.symbol == "object-channels")
            .context("Channel instance layout")?;
        crate::tiger::instance::seal(&mut payload, &mut Vec::new(), layout)?;
        put(&mut payload.0, 0x44, &u32::MAX.to_le_bytes())?;
        Some(ChannelBank {
            owner: channel.owner,
            payload,
            allocation_template: bank_allocation,
            allocation: extended_allocation,
        })
    };
    let channels = read_names(channel_bank.as_ref().map_or(bank, |bank| &bank.payload))?
        .into_iter()
        .enumerate()
        .map(|(ordinal, name)| Ok((name, u32::try_from(ordinal)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;

    let mut written = Vec::new();
    let mut dependencies = BTreeMap::<String, dependency::Asset>::new();
    let mut connections = Vec::new();
    let mut sequence_evidence = Vec::new();
    for (&index, &tag) in &occurrences {
        let (owner, bindings) = shared.bindings(tag)?;
        let id = 0x7E00_0000u32 + u32::try_from(index)?;
        let owner_symbol = format!("{symbol}-sequence-{index}");
        let mut output = owner.write(
            &originals[&template],
            &allocation,
            &originals[&input_envelope],
            &envelopes,
            &bindings,
        )?;
        effect::retarget(&mut output.owner, template, id)?;
        effect::merge(
            &mut dependencies,
            std::mem::take(&mut output.prepared.dependencies),
        )?;
        let definitions = owner.definitions()?;
        ensure!(
            definitions.len() == output.inputs.len(),
            "Translated sequence inputs differ from the source"
        );
        let mut bound = BTreeSet::new();
        let mut connected = Vec::new();
        for row in inputs
            .iter()
            .filter(|row| row.consumer.selector == u64::try_from(index).unwrap_or(u64::MAX))
        {
            let at = definitions
                .iter()
                .position(|&definition| definition as u64 == row.consumer.offset)
                .context("Source sequence connection names an unknown input")?;
            ensure!(
                bound.insert(at),
                "A source sequence input is connected twice"
            );
            let (name, definition) = output.inputs[at];
            let ordinal = *channels
                .get(&source_names[usize::try_from(row.channel)?])
                .context("Source channel is undeclared in the native bank")?;
            connections.push(links::Connection {
                consumer: effect::endpoint(id, 0x80809789, definition)?,
                provider: effect::endpoint(
                    channel.owner,
                    channel.class,
                    usize::try_from(channel.offset)?,
                )?,
                channel: ordinal,
                flags: 0,
            });
            connected.push(json!({"name":name,"channel":ordinal,"source_channel":row.channel}));
        }
        if let Some(at) = (0..output.inputs.len()).find(|at| !bound.contains(at)) {
            anyhow::bail!(
                "Source sequence {tag:08X} input {:08X} has no channel connection",
                output.inputs[at].0
            );
        }
        sequence_evidence.push(json!({"occurrence":index,"source":tag,"owner":id,
            "translation":owner.evidence(),"connected_inputs":connected}));
        written.push(Sequence {
            id,
            occurrence: index,
            template,
            allocation_template,
            symbol: owner_symbol,
            payload: output.owner,
            allocation: output.allocation,
            patches: output.patches,
        });
    }

    let mut controller_inputs = Vec::new();
    for (consumer, provider) in sequences.consumers() {
        let sequence = written
            .iter()
            .find(|sequence| sequence.occurrence == provider)
            .context("Radiance sequence provider was not translated")?;
        let (_, definition) = effect::pair(&sequence.payload)?;
        controller_inputs.push((
            consumer,
            effect::endpoint(sequence.id, 0x808092F8, definition + 0x90)?,
        ));
    }
    let replaced = native_sequences.into_iter().collect::<BTreeSet<_>>();
    source.unchanged()?;
    Ok(Presentation {
        sequences: written,
        replaced,
        connections,
        controller_inputs,
        dependencies: dependencies.into_values().collect(),
        evidence: json!({"template":template,"input_envelope":input_envelope,
            "channel_provider":{"owner":channel.owner,"class":channel.class,"offset":channel.offset},
            "added_channels":added,
            "sequences":sequence_evidence,
            "limits":["Animation and the remaining source event kinds keep their timing as native delays. Sound does too when its cue closure is refused.",
                "Channels the native bank lacks are inherited from the parent, as in the source. Whether the Shadowkeep player supplies them is unverified.",
                "Natural playback, rendering and gameplay remain unverified."]}),
        channel_bank,
    })
}
