//! The source weapon's firing presentation written over the base's slot flows.
//!
//! A modern weapon fires its presentation through the content row of its unnamed audio component
//! (`80802CF4`), whose parent names a root of up to six slot entities, as Shadowkeep's does. Twin
//! weapons present in both versions occupy the same slots, so source slot `n` becomes native slot
//! `n`. Each modern slot entity runs one effect flow (`80808179`), which the sequence emitter
//! writes over the base slot's flow (`808084E9`) with the base's envelope and allocation format.
//!
//! The flow's resources convert once for the weapon: particle systems through the particle
//! converter, sound cues into private cues, banks and media, and feedback resources into native
//! feedback resources. Firing lights (kind 33) keep their timing as delays. The written flow names
//! every private resource by a placeholder and a patch, and its inputs read the slot bank's
//! channel of their own name through the slot entity's connection rows, as
//! `crate::tiger::weapon::presentation` describes.
//!
//! A slot keeps the base's flow, with the reason recorded, when any of its particle systems,
//! sounds or feedback resources does not convert, an event has no translation, the written flow
//! links endpoints other than its inputs or polls a bank, or an input names a channel the base
//! bank does not declare.
use super::{attachment, feedback};
use crate::d2_mot::entity::links::{Connection, Graph, Object};
use crate::d2_mot::{
    entity::sequence::{self, Bindings, Resource},
    particles,
    payload::Payload,
    reader::{Reader, write_json},
};
use crate::tiger::{
    projectile::Assets,
    weapon::presentation::{self as base, DELAY_ENVELOPES},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const ENTITY: u32 = 0x8080_9AD8;
const OWNER: u32 = 0x8080_9B06;
const FLOW: u32 = 0x8080_8179;
const PARTICLE_EVENT: u32 = 0x8080_67B9;
const SOUND_EVENT: u32 = 0x8080_6640;
const FEEDBACK_EVENT: u32 = 0x8080_2FFD;
const LIGHT_EVENT: u32 = 0x8080_6A52;
/// Stock flows that supply native envelopes where the base slot flow holds none: a feedback event
/// (`80803D8D`), and numeric outputs with scalar inputs.
const WITNESSES: [u32; 2] = [0x80B3_A75F, 0x80C2_399A];
/// The shipped particle system converted systems take their envelopes from, as effect
/// attachments do.
const PARTICLE_TEMPLATE: u32 = 0x80EF_30DB;
/// The stock flow whose raw-audio cue templates the converted cues.
const CUE_TEMPLATE_FLOW: u32 = 0x80F2_E50A;
/// Private symbols of the firing presentation.
const PREFIX: &str = "firing-";

pub struct Request<'a> {
    pub source_packages: &'a Path,
    pub native_packages: &'a Path,
    /// The source export, holding `rig.json` with its audio groups.
    pub source: &'a Path,
    /// The base's runtime rig, with its content key and components.
    pub native_rig: &'a Value,
    pub namespace: &'a str,
    pub work: &'a Path,
    pub graph: &'a Path,
}

struct Slot {
    slot: usize,
    entity: u32,
    /// The source slot entity, whose connection rows feed the flow.
    entity_payload: Payload,
    flow: u32,
    payload: Payload,
}

fn connections_to<'a>(graph: &'a Graph, consumer: &Object) -> Vec<&'a Connection> {
    graph
        .connections
        .iter()
        .chain(&graph.named_connections)
        .filter(|edge| edge.consumer.object == Some(*consumer))
        .collect()
}

/// The modern flow's bank notification input, its one pull dependency, has no native field.
/// Native sequence conditions poll the bank their inputs read instead, so the input can go when
/// it and every scalar input name the source slot's own channel bank, the proof
/// [`sequence::Native::link`] makes for effect attachments. The base slot entity's connection
/// rows then bind each input to the base bank's channel of the same name.
fn discharge(entity: &Payload, written: &sequence::Native) -> Result<()> {
    let graph = Graph::read(entity, true)?;
    for dependency in &written.pull_dependencies {
        let notifications = connections_to(&graph, dependency);
        let [notification] = notifications.as_slice() else {
            bail!("The source firing flow's bank notification is missing or ambiguous");
        };
        ensure!(
            notification.channel == 0 && notification.flags == 0,
            "The source firing flow's bank notification mode needs translation"
        );
        let inputs = written
            .objects
            .iter()
            .filter(|mapping| {
                mapping.source.owner == dependency.owner && mapping.source.class == 0x8080_9591
            })
            .collect::<Vec<_>>();
        let Some(provider) = notification.provider.object else {
            ensure!(
                inputs.is_empty()
                    && notification.provider.selector == 0xFFFF
                    && notification.provider.namespace == 0x811C_9DC5,
                "A disconnected firing flow notification has inputs or a named provider"
            );
            continue;
        };
        ensure!(
            provider.class == 0x8080_95B3,
            "The source firing flow's notification provider is not a channel bank"
        );
        for input in inputs {
            let edges = connections_to(&graph, &input.source);
            let [edge] = edges.as_slice() else {
                bail!("A source firing flow input has no single provider");
            };
            let scalar = edge
                .provider
                .object
                .context("A source firing flow input is disconnected")?;
            ensure!(
                scalar.owner == provider.owner
                    && scalar.class == 0x8080_95CF
                    && edge.provider.selector == notification.provider.selector
                    && edge.provider.namespace == notification.provider.namespace
                    && edge.flags == 0,
                "A source firing flow input does not poll the slot's channel bank"
            );
        }
    }
    Ok(())
}

fn hex(value: &Value, what: &str) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().with_context(|| format!("Source {what}"))?,
        16,
    )?)
}

/// The slot entities of the source content row's presentation and the flow each runs.
fn source_slots(r: &mut Reader, rig: &Value) -> Result<Vec<Slot>> {
    let groups = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Source firing presentation groups")?
        .iter()
        .filter(|group| group["slots"].as_array().is_some_and(|s| !s.is_empty()))
        .collect::<Vec<_>>();
    let [group] = groups.as_slice() else {
        bail!(
            "The source content row selects {} firing presentations with slot entities, not one",
            groups.len()
        );
    };
    let mut slots = Vec::new();
    for row in group["slots"]
        .as_array()
        .context("Source presentation slots")?
    {
        let slot = usize::try_from(row["slot"].as_u64().context("Source presentation slot")?)?;
        let entity = hex(&row["entity"], "presentation slot entity")?;
        let e = r.tag(entity, Some(ENTITY))?;
        let mut flows = Vec::new();
        for at in e.array(8, 12, None)? {
            let owner = e.u32(at)?;
            if base::unset(owner) {
                continue;
            }
            let p = r.tag(owner, Some(OWNER))?;
            if p.u32(p.pointer(24)? - 4)? == FLOW {
                flows.push((owner, (*p).clone()));
            }
        }
        let [(flow, payload)] = flows.as_slice() else {
            bail!(
                "Source presentation slot {slot} runs {} flows, not one",
                flows.len()
            );
        };
        slots.push(Slot {
            slot,
            entity,
            entity_payload: (*e).clone(),
            flow: *flow,
            payload: payload.clone(),
        });
    }
    Ok(slots)
}

/// A flow's events, each its class and offset.
fn events(p: &Payload) -> Result<Vec<(u32, usize)>> {
    let definition = p.pointer(24)?;
    p.array(definition + 0x1D8, 24, Some(0x8080_91F1))?
        .into_iter()
        .map(|row| {
            let at = p.pointer(row + 16)?;
            Ok((p.u32(at - 4)?, at))
        })
        .collect()
}

/// The particle systems, sound cues and feedback resources every slot flow names.
#[derive(Default)]
struct Sources {
    systems: BTreeSet<u32>,
    sounds: BTreeSet<u32>,
    feedback: BTreeSet<u32>,
}

fn sources(r: &Reader, slots: &[Slot]) -> Result<Sources> {
    let mut found = Sources::default();
    for slot in slots {
        attachment::event_resources(r, &slot.payload, &mut found.systems, &mut found.sounds)?;
        for (class, at) in events(&slot.payload)? {
            if class != FEEDBACK_EVENT {
                continue;
            }
            found.feedback.insert(r.ref64(&slot.payload, at + 0x30)?);
            let fallback = slot.payload.u32(at + 0x318)?;
            if fallback != u32::MAX {
                found.feedback.insert(fallback);
            }
        }
    }
    Ok(found)
}

/// The converted particle systems: their graph nodes, beneath `firing-particles`, and each source
/// system's private symbol.
fn particle_systems(
    request: &Request<'_>,
    systems: &BTreeSet<u32>,
) -> Result<(Vec<Value>, BTreeMap<u32, String>, Value)> {
    if systems.is_empty() {
        return Ok((Vec::new(), BTreeMap::new(), Value::Null));
    }
    let render_inputs = request.work.join("render-inputs");
    for (packages, modern, era) in [
        (request.source_packages, true, "modern"),
        (request.native_packages, false, "native"),
    ] {
        let folder = render_inputs.join(format!("tfx-{era}"));
        fs::create_dir_all(&folder)?;
        let mut reader = Reader::new(packages, &folder, modern)?;
        write_json(
            &folder.join("context.json"),
            &crate::tiger::shader::globals::read(&mut reader, modern)?,
        )?;
        reader.finish()?;
    }
    let decompiler = super::automatic::decompiler(&mut |_| {})?;
    let directory = request.graph.join("firing-particles");
    let section = particles::native::convert(&particles::native::Request {
        source_packages: request.source_packages,
        native_packages: request.native_packages,
        render_inputs: &render_inputs,
        decompiler: &decompiler,
        graph: &directory,
        work: &request.work.join("particle-conversion"),
        systems: &systems.iter().copied().collect::<Vec<_>>(),
        inputs: &BTreeMap::new(),
        template: PARTICLE_TEMPLATE,
    })?;
    let mut nodes = section["nodes"].as_array().cloned().unwrap_or_default();
    for node in &mut nodes {
        for field in ["symbol", "reference"] {
            if let Some(symbol) = node[field].as_str() {
                node[field] = json!(format!("{PREFIX}{symbol}"));
            }
        }
        let file = node["file"].as_str().context("Firing particle file")?;
        node["file"] = json!(format!("firing-particles/{file}"));
        for patch in node["patches"].as_array_mut().into_iter().flatten() {
            let symbol = patch["symbol"].as_str().context("Firing particle patch")?;
            patch["symbol"] = json!(format!("{PREFIX}{symbol}"));
        }
    }
    let mut symbols = BTreeMap::new();
    for (tag, symbol) in section["systems"].as_object().into_iter().flatten() {
        let symbol = symbol.as_str().context("Firing particle symbol")?;
        symbols.insert(u32::from_str_radix(tag, 16)?, format!("{PREFIX}{symbol}"));
    }
    let report = json!({"coverage":section["coverage"],"refused":section["refused"]});
    Ok((nodes, symbols, report))
}

/// The first raw-audio cue of the stock flow converted cues are laid out on.
fn cue_template(native: &mut Reader) -> Result<u32> {
    let flow = native.tag(CUE_TEMPLATE_FLOW, None)?;
    for at in (0..flow.0.len().saturating_sub(3)).step_by(4) {
        let tag = flow.u32(at)?;
        if native.reference(tag).ok() == Some(0x8080_9802) {
            return Ok(tag);
        }
    }
    bail!("The native cue template flow holds no cue")
}

/// Private cues, banks and media for the source sounds. A bank that does not translate costs only
/// its own cues. Returns each converted cue's reserved identity and a report per bank. A failed
/// conversion leaves no node behind.
fn audio(
    request: &Request<'_>,
    (source, native): (&mut Reader, &mut Reader),
    assets: &mut Assets,
    sounds: &BTreeSet<u32>,
) -> Result<(BTreeMap<u32, u32>, Value)> {
    let mut groups = BTreeMap::<u32, BTreeSet<u32>>::new();
    let mut reports = Vec::new();
    for &cue in sounds {
        let bank = source
            .tag(cue, Some(0x8080_9738))
            .and_then(|payload| Ok(crate::d2_mot::audio::cue::Cue::read(&payload)?.bank));
        match bank {
            Ok(bank) => {
                groups.entry(bank).or_default().insert(cue);
            }
            Err(error) => reports.push(json!({"cue":format!("{cue:08X}"),"status":"refused",
                "reason":format!("{error:#}")})),
        }
    }
    if groups.is_empty() {
        return Ok((BTreeMap::new(), json!(reports)));
    }
    let template = cue_template(native)?;
    // Each bank's cues convert alone first, into assets that are thrown away, which tells a
    // bank that does not translate from one that does. Banks share media and settings, and a
    // conversion instantiates its banks in one Wwise namespace, so the cues that pass then
    // convert together, as one effect's audio does.
    let mut passing = BTreeSet::new();
    for (bank, group) in &groups {
        let directory = request.work.join("audio-probe").join(format!("{bank:08X}"));
        fs::create_dir_all(&directory)?;
        let probe = attachment::Request {
            source_tag: *bank,
            source_packages: request.source_packages,
            native_packages: request.native_packages,
            directory: &directory,
            namespace: request.namespace,
        };
        match attachment::audio::convert(
            source,
            native,
            &mut Assets::default(),
            group,
            template,
            &probe,
        ) {
            Ok(_) => {
                passing.extend(group.iter().copied());
                reports.push(
                    json!({"bank":format!("{bank:08X}"),"status":"translated","cues":group.len()}),
                );
            }
            Err(error) => {
                crate::cancellation::check()?;
                reports.push(
                    json!({"bank":format!("{bank:08X}"),"status":"refused","cues":group.len(),
                    "reason":format!("{error:#}")}),
                );
            }
        }
    }
    if passing.is_empty() {
        return Ok((BTreeMap::new(), json!(reports)));
    }
    let directory = request.work.join("audio");
    fs::create_dir_all(&directory)?;
    let together = attachment::Request {
        source_tag: *groups.keys().next().context("Firing audio banks")?,
        source_packages: request.source_packages,
        native_packages: request.native_packages,
        directory: &directory,
        namespace: request.namespace,
    };
    let checkpoint = assets.clone();
    match attachment::audio::convert(source, native, assets, &passing, template, &together) {
        Ok((cues, report)) => Ok((cues, json!({"banks":reports,"converted":report}))),
        Err(error) => {
            crate::cancellation::check()?;
            *assets = checkpoint;
            Ok((
                BTreeMap::new(),
                json!({"banks":reports,"status":"refused","reason":format!("{error:#}")}),
            ))
        }
    }
}

/// Private feedback resources, each source resource's reserved identity, and refused roots.
fn feedback_resources(
    source: &mut Reader,
    native: &Reader,
    assets: &mut Assets,
    roots: &BTreeSet<u32>,
) -> Result<(BTreeMap<u32, u32>, BTreeMap<String, String>)> {
    let mut ids = BTreeMap::new();
    let mut refused = BTreeMap::new();
    for &root in roots {
        let resources = match feedback::convert(source, root) {
            Ok(resources) => resources,
            Err(error) => {
                crate::cancellation::check()?;
                refused.insert(format!("{root:08X}"), format!("{error:#}"));
                continue;
            }
        };
        let fresh = resources
            .into_iter()
            .filter(|resource| !ids.contains_key(&resource.source))
            .collect::<Vec<_>>();
        for resource in &fresh {
            let id = assets.reserve(native, format!("{PREFIX}feedback-{:08X}", resource.source))?;
            ids.insert(resource.source, id);
        }
        for resource in fresh {
            let mut payload = resource.payload;
            for &(offset, member) in &resource.members {
                let id = ids
                    .get(&member)
                    .context("Feedback member was not translated")?;
                payload.0[offset..offset + 4].copy_from_slice(&id.to_le_bytes());
            }
            let template = native
                .classes(resource.class)
                .into_iter()
                .min()
                .context("Native feedback template is absent")?;
            assets.push(ids[&resource.source], template, payload, None)?;
        }
    }
    Ok((ids, refused))
}

/// A compiled identity of the sequence's own label or a particle event's label.
fn identity(bindings: &mut Bindings, namespace: &str, owner: u32, label: u32) -> Result<()> {
    if bindings.identities.contains_key(&label) {
        return Ok(());
    }
    let text = format!("{namespace}/firing/{owner:08X}/{label:08X}");
    let value = text.bytes().fold(0x811C_9DC5u32, |h, b| {
        h.wrapping_mul(16_777_619) ^ u32::from(b)
    });
    ensure!(
        value != u32::MAX
            && !bindings
                .identities
                .values()
                .any(|&previous| previous == value),
        "Firing compiled identity is reserved or collides"
    );
    bindings.identities.insert(label, value);
    Ok(())
}

/// The converted resources every slot binds.
struct Converted {
    assets: Assets,
    systems: BTreeMap<u32, String>,
    cues: BTreeMap<u32, u32>,
    feedback: BTreeMap<u32, u32>,
    envelopes: Vec<Payload>,
}

/// The resource fields of one source event.
fn bind_event(
    source: &Reader,
    native: &Reader,
    converted: &mut Converted,
    (flow, class, at): (&Payload, u32, usize),
    bindings: &mut Bindings,
) -> Result<()> {
    match class {
        PARTICLE_EVENT => {
            for row in flow.array(at + 0x28, 24, Some(0x8080_67BB))? {
                for field in sequence::presentation::fields(flow, row)? {
                    let system = flow.u32(field)?;
                    let symbol = converted.systems.get(&system).with_context(|| {
                        format!("The source shot particles do not convert: system {system:08X}")
                    })?;
                    let tag = converted.assets.external(native, symbol)?;
                    bindings.resources.insert(
                        field,
                        Resource {
                            source_class: 0x8080_6920,
                            native_class: 0x8080_6E28,
                            tag,
                        },
                    );
                }
            }
        }
        SOUND_EVENT => {
            let cue = source.ref64(flow, at + 0x50)?;
            let tag = *converted.cues.get(&cue).with_context(|| {
                format!("The source shot sound does not convert: cue {cue:08X}")
            })?;
            bindings.resources.insert(
                at + 0x50,
                Resource {
                    source_class: 0x8080_9738,
                    native_class: 0x8080_9802,
                    tag,
                },
            );
        }
        FEEDBACK_EVENT => {
            for (field, resource) in [
                (at + 0x30, source.ref64(flow, at + 0x30)?),
                (at + 0x318, flow.u32(at + 0x318)?),
            ] {
                if resource == u32::MAX && field == at + 0x318 {
                    continue;
                }
                let tag = *converted.feedback.get(&resource).with_context(|| {
                    format!("The source shot feedback does not convert: resource {resource:08X}")
                })?;
                bindings.resources.insert(
                    field,
                    Resource {
                        source_class: 0x8080_31DE,
                        native_class: 0x8080_3EB0,
                        tag,
                    },
                );
            }
        }
        LIGHT_EVENT => {
            bindings.omitted_events.insert(at);
        }
        _ => {}
    }
    Ok(())
}

fn bindings(
    source: &Reader,
    native: &Reader,
    converted: &mut Converted,
    slot: &Slot,
    namespace: &str,
) -> Result<Bindings> {
    let flow = &slot.payload;
    let mut bindings = Bindings {
        event_templates: converted.envelopes.clone(),
        ..Bindings::default()
    };
    let definition = flow.pointer(24)?;
    let owner = flow.u32(flow.pointer(16)?)?;
    identity(
        &mut bindings,
        namespace,
        owner,
        flow.u32(definition + 0x30C)?,
    )?;
    for (class, at) in events(flow)? {
        bind_event(source, native, converted, (flow, class, at), &mut bindings)?;
        if class == PARTICLE_EVENT {
            identity(&mut bindings, namespace, owner, flow.u32(at + 0x108)?)?;
        }
    }
    Ok(bindings)
}

/// The source flow's scalar input names, main inputs then auxiliary, the order the emitter
/// writes them.
fn input_names(flow: &Payload) -> Result<Vec<u32>> {
    let definition = flow.pointer(24)?;
    let mut names = Vec::new();
    for field in [0x208, 0x380] {
        for row in flow.array(definition + field, 40, Some(0x8080_9591))? {
            names.push(flow.u32(row + 32)?);
        }
    }
    Ok(names)
}

/// One written slot: its owner and allocation nodes and its replacement rows.
struct Written {
    owners: Vec<Value>,
    allocations: Vec<Value>,
    replacements: Vec<Value>,
    report: Value,
}

fn write_slot(
    request: &Request<'_>,
    (source, native): (&Reader, &mut Reader),
    converted: &mut Converted,
    slot: &Slot,
    (entity, flow): (u32, u32),
) -> Result<Written> {
    let bindings = bindings(source, native, converted, slot, request.namespace)?;
    let template = (*native.tag(flow, Some(0x8080_9C36))?).clone();
    let allocation_tag = template.u32(0x44)?;
    let allocation = (*native.tag(allocation_tag, Some(0x8080_9BBB))?).clone();
    let written = sequence::emit(
        &slot.payload,
        &template,
        &allocation,
        &bindings,
        flow,
        allocation_tag,
    )?;
    discharge(&slot.entity_payload, &written)?;
    ensure!(
        written
            .objects
            .iter()
            .all(|object| matches!(object.source.class, 0x8080_9590 | 0x8080_9591)),
        "The source firing flow links endpoints other than its inputs"
    );
    ensure!(
        written.owner.u32(0x44)? == allocation_tag,
        "Translated firing flow names another allocation"
    );
    // The emitter seals the flow, so its definition is present twice. Placeholders cover every
    // occurrence of a private identity, in both copies.
    let mut owner = written.owner;
    let patches = converted
        .assets
        .placeholders(&mut owner)?
        .into_iter()
        .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
        .collect::<Vec<_>>();
    let inputs = base::live_inputs(&owner, &input_names(&slot.payload)?)?;
    let (rows, bank) = base::connections(native, entity, flow, &inputs, &BTreeSet::new())?;
    ensure!(bank.is_none(), "The base slot bank needs a private copy");
    let symbol = format!("{PREFIX}flow-{}", slot.slot);
    let allocation_symbol = format!("{symbol}-allocation");
    let mut owners = Vec::new();
    let mut allocations = Vec::new();
    crate::graph::add(
        request.graph,
        &mut owners,
        &symbol,
        flow,
        &owner.0,
        None,
        patches,
    )?;
    crate::graph::add(
        request.graph,
        &mut allocations,
        &allocation_symbol,
        allocation_tag,
        &written.allocation.0,
        None,
        Vec::new(),
    )?;
    let mut row = json!({"slot":slot.slot,"entity":format!("{entity:08X}"),
        "component_owner":format!("{flow:08X}"),"owner":symbol,"allocation":allocation_symbol});
    if let Some((before, after)) = rows {
        row["connections"] = json!({"before":hex::encode(before),"after":hex::encode(after)});
    }
    Ok(Written {
        owners,
        allocations,
        replacements: vec![row],
        report: json!({"slot":slot.slot,"status":"translated","source_entity":format!("{:08X}",slot.entity),
            "source_flow":format!("{:08X}",slot.flow),"native_entity":format!("{entity:08X}"),
            "native_flow":format!("{flow:08X}"),"inputs":inputs.len()}),
    })
}

/// The stock flows that supply native event envelopes the base flow lacks.
fn envelopes(native: &mut Reader, targets: &base::Targets) -> Result<Vec<Payload>> {
    let mut tags = targets.values().map(|&(_, flow)| flow).collect::<Vec<_>>();
    tags.extend(DELAY_ENVELOPES);
    tags.extend(WITNESSES);
    tags.into_iter()
        .map(|tag| Ok((*native.tag(tag, Some(0x8080_9C36))?).clone()))
        .collect()
}

/// Convert the source presentation, returning the graph's `firing_effect`, or `None` when no
/// slot translates, and the report.
fn convert(request: &Request<'_>) -> Result<(Option<Value>, Value)> {
    let rig: Value = serde_json::from_slice(&fs::read(request.source.join("rig.json"))?)?;
    let mut source =
        Reader::discovery(request.source_packages, &request.work.join("source"), true)?;
    let mut native =
        Reader::discovery(request.native_packages, &request.work.join("native"), false)?;
    let slots = source_slots(&mut source, &rig)?;
    let wanted = slots.iter().map(|slot| slot.slot).collect::<BTreeSet<_>>();
    let (presentation, targets) = base::slot_targets(&mut native, request.native_rig, &wanted)?;
    let found = sources(&source, &slots)?;
    let (particle_nodes, systems, particle_report) = particle_systems(request, &found.systems)?;
    let mut assets = Assets::default();
    let (cues, audio_report) = audio(
        request,
        (&mut source, &mut native),
        &mut assets,
        &found.sounds,
    )?;
    if !assets.names.is_empty() {
        assets.rename("attachment-", PREFIX)?;
    }
    let (feedback_ids, feedback_refused) =
        feedback_resources(&mut source, &native, &mut assets, &found.feedback)?;
    let envelopes = envelopes(&mut native, &targets)?;
    let mut converted = Converted {
        assets,
        systems,
        cues,
        feedback: feedback_ids,
        envelopes,
    };
    let (mut owners, mut allocations, mut replacements, mut slot_reports) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for slot in &slots {
        let target = targets[&slot.slot];
        match write_slot(
            request,
            (&source, &mut native),
            &mut converted,
            slot,
            target,
        ) {
            Ok(written) => {
                owners.extend(written.owners);
                allocations.extend(written.allocations);
                replacements.extend(written.replacements);
                slot_reports.push(written.report);
            }
            Err(error) => {
                crate::cancellation::check()?;
                slot_reports.push(json!({"slot":slot.slot,"status":"native",
                    "source_entity":format!("{:08X}",slot.entity),"source_flow":format!("{:08X}",slot.flow),
                    "reason":format!("{error:#}")}));
            }
        }
    }
    let report = json!({"slots":slot_reports,"particles":particle_report,"audio":audio_report,
        "feedback_refused":feedback_refused,
        "limits":["Source slot ordinals become native slot ordinals, established on twin weapons present in both versions.",
            "Firing lights keep their timing as delays.",
            "The flows' scheduling, first- and third-person drawing, sound, rumble and camera motion in play are gameplay checks."],
        "gameplay_verified":false});
    if replacements.is_empty() {
        fs::remove_dir_all(request.graph.join("firing-particles")).ok();
        return Ok((None, report));
    }
    let mut nodes = particle_nodes;
    nodes.extend(converted.assets.write_nodes(request.graph, "firing")?);
    let effect = json!({"native_presentation":presentation,"owners":owners,"allocations":allocations,
        "replacements":replacements,"assets":{"installable":true,"nodes":nodes}});
    Ok((Some(effect), report))
}

/// Retire the graph audio's hash-fitted firing sounds for every translated slot. Fitted and
/// preserved pairs name the base flow a translated flow replaces, so a fitted sound would be
/// written into the wrong flow. Unmatched source cues of a translated slot are played by its
/// translated flow. Returns the count of each list removed.
fn retire_fitted(document: &mut Value, replaced: &[Value], translated: &[Value]) -> Value {
    let mut removed = serde_json::Map::new();
    // A graph without audio keeps none. Indexing would insert the keys it reads.
    let Some(audio) = document.get_mut("audio").filter(|audio| audio.is_object()) else {
        return Value::Object(removed);
    };
    for list in ["firing_events", "preserved_firing_events"] {
        if let Some(events) = audio.get_mut(list).and_then(Value::as_array_mut) {
            let before = events.len();
            events.retain(|event| {
                !event["native_sounds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|sound| replaced.contains(&sound["component_owner"]))
            });
            removed.insert(list.into(), json!(before - events.len()));
        }
    }
    if let Some(events) = audio
        .get_mut("unmatched_firing_events")
        .and_then(Value::as_array_mut)
    {
        let before = events.len();
        events.retain(|sound| !translated.contains(&sound["component_owner"]));
        removed.insert(
            "unmatched_firing_events".into(),
            json!(before - events.len()),
        );
    }
    Value::Object(removed)
}

/// Translate the source firing presentation and record it in the graph as `firing_effect`, with
/// its report at `firing_translation.presentation`. Hash-fitted firing sounds for a replaced flow
/// leave the graph's audio, since the translated flow plays the source's own. A refusal keeps the
/// base's presentation and records why, so the import still succeeds.
pub fn apply(request: &Request<'_>) -> Result<Value> {
    let (effect, mut report) = match convert(request) {
        Ok(result) => result,
        Err(error) => {
            crate::cancellation::check()?;
            if crate::cancellation::is_cancelled(&error) {
                return Err(error);
            }
            (
                None,
                json!({"reason":format!("{error:#}"),"gameplay_verified":false}),
            )
        }
    };
    let manifest = request.graph.join("asset-graph.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&manifest)?)?;
    report["status"] = json!(if effect.is_some() {
        "translated"
    } else {
        "native"
    });
    if let Some(effect) = effect {
        let replaced = effect["replacements"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| row["component_owner"].clone())
            .collect::<Vec<_>>();
        let translated = report["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|slot| slot["status"] == "translated")
            .map(|slot| slot["source_flow"].clone())
            .collect::<Vec<_>>();
        report["fitted_firing_sounds_removed"] =
            json!(retire_fitted(&mut document, &replaced, &translated));
        document["firing_effect"] = effect;
    }
    document["firing_translation"]["presentation"] = report.clone();
    write_json(&manifest, &document)?;
    Ok(report)
}
