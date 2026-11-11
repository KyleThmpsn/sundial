//! The Rise of Iron firing presentation translated into the base's firing flows.
//!
//! A Rise of Iron weapon selects its firing presentation through its content resource (class
//! `8080248D`), shared by every weapon of a type: the `0x140`-byte variant row at resource `+1F0`
//! whose key at `+18` is the weapon's content key, or the default row at resource `+B0` when no
//! variant row carries it. The default rows name their slot entities directly at `+38`, `+48`,
//! `+58` and `+68`. The variant rows name a presentation parent at `+78` instead, whose `+10` is a
//! root of class `80803165` with entity slots at `+10`, `+20`, `+30` and `+40`. Each slot entity
//! runs an effect sequence
//! (`8080079A` instance, `80800610` resource) that plays the shot's sounds, muzzle particles and
//! lights. Shadowkeep's root `80803ADC` keeps the same slots at `+18`, `0x10` bytes apart, with two
//! more after them. Slot 0 carries the shot flow in both games and Shadowkeep's slots 2 and 3 carry
//! one sound event each, as the Rise of Iron slot 2 sequences do, so source slot `n` becomes native
//! slot `n`. That ordinal reading is inferred from those layouts.
//!
//! Each source sequence is written over the base's slot flow with the shared sequence writer, so
//! the flow keeps the base's envelope and allocation format and takes every source control, event
//! and locator. Its cues join the weapon's audio closure and its particle systems become private
//! systems of the firing effect. Event kinds the writer cannot translate yet, such as firing lights,
//! keep their timing as delays and are reported. The base's connection rows that fed the replaced
//! flow leave with it, and each source scalar input reads the slot bank's channel of its own name.
//! A name the base bank does not declare is inherited by a private copy of the bank when the
//! source bank inherits it from the parent channel of the same name in the default namespace, or
//! receives it there as an incoming value.
//! Any other undeclared name refuses the slot, because an unbound flow input faults the client.
use super::*;
use crate::d1_roi::{effect::Shared, sequence::owner::Owner};
use crate::tiger::weapon::presentation::{
    DELAY_ENVELOPES, Targets, connections, live_inputs, slot_targets, unset,
};
use anyhow::bail;
use std::collections::{BTreeMap, BTreeSet};

/// Event kinds the firing flows keep as timed delays beside the shared [`crate::d1_roi::effect::OMITTED`]
/// list.
const FIRING_OMITTED: [(u32, u32); 0] = [];

/// Rise of Iron content resource and presentation root classes.
const CONTENT: u32 = 0x8080_248d;
const ROOT: u32 = 0x8080_3165;
const SEQUENCE: u32 = 0x8080_079a;

/// The source slot entities and the sequences they run, of the presentation the weapon's content
/// key selects, by slot.
fn source_slots(source: &Source, export: &Value) -> Result<(Value, Targets)> {
    // The importer's one content row selection: the keyed row, or the default row at `+B0`
    // when no row carries the weapon's key.
    let content = super::super::content_row(source, export)?;
    let p = &content.owner;
    ensure!(
        p.u32(p.pointer(24)? - 4)? == CONTENT,
        "Source content resource class differs"
    );
    let row = content.row;
    // A row names its slot entities directly at `+38`, `0x10` bytes apart, as every inspected
    // default row does, or through a presentation parent at `+78`, as the keyed rows do.
    let direct = (0..4)
        .map(|slot| p.u32(row + 0x38 + slot * 0x10))
        .collect::<Result<Vec<_>>>()?;
    let parent = p.u32(row + 0x78)?;
    let mut described = json!({"content_key":format!("{:08X}",content.key),
        "row":if content.keyed { "keyed" } else { "default" }});
    let entities = if unset(parent) {
        ensure!(
            direct.iter().any(|&entity| !unset(entity)),
            "The source content row has no presentation"
        );
        described["form"] = json!("direct");
        direct
    } else {
        ensure!(
            direct.iter().all(|&entity| unset(entity)),
            "The source content row names slot entities both directly and through a parent"
        );
        let parent = source.read(parent)?;
        ensure!(
            parent.0.len() == 24 && parent.u64(0)? == 24,
            "Source presentation parent envelope differs"
        );
        let root_tag = parent.u32(0x10)?;
        ensure!(
            source.reference(root_tag)? == ROOT,
            "Source presentation root class differs"
        );
        let root = source.read(root_tag)?;
        described["form"] = json!("parent");
        described["root"] = json!(format!("{root_tag:08X}"));
        (0..4)
            .map(|slot| root.u32(0x10 + slot * 0x10))
            .collect::<Result<Vec<_>>>()?
    };
    let mut slots = BTreeMap::new();
    for (slot, &entity) in entities.iter().enumerate() {
        if unset(entity) {
            continue;
        }
        let e = source.read(entity)?;
        let mut sequences = Vec::new();
        for at in e.array(0x20, 12, Some(0x8080_0715))? {
            let owner = e.u32(at)?;
            if unset(owner) {
                continue;
            }
            let o = source.read(owner)?;
            if o.u32(o.pointer(16)? - 4)? == SEQUENCE {
                sequences.push(owner);
            }
        }
        match sequences.as_slice() {
            [] => {}
            [sequence] => {
                slots.insert(slot, (entity, *sequence));
            }
            _ => bail!("Source presentation slot {slot} runs several sequences"),
        }
    }
    ensure!(
        !slots.is_empty(),
        "The source presentation plays no sequence"
    );
    Ok((described, slots))
}

/// The names of a source slot sequence's inputs that its slot bank receives from outside under the
/// same name in the empty namespace, the only form a private native bank can add with the same
/// meaning. Source connection rows are 88 bytes at entity `+30`: consumer `+08`, input definition
/// `+10`, bank `+30` and channel `+50`. Source inputs are 48-byte rows at definition `+1A0` and
/// `+2A8`, named at `+28`. The source bank declares its channels at definition `+D8` and receives
/// them through `(namespace, name, ordinal)` rows, inherited at `+108` or incoming at `+118`. An
/// incoming declaration takes a getter's value from another component of the graph. The fusion
/// rifle banks `80AAA1B4` and `80AAA27C` receive `49FCE899` (`damage_type`) that way, from component
/// `80AAD0F9` (definition `80802202`), which publishes a channel of that name. Shadowkeep's firing
/// banks such as `8152945F` and `81579421` inherit the same name in the empty namespace, so both
/// forms carry the weapon's damage type into the slot.
fn source_inherited(source: &Source, entity: u32, sequence: u32) -> Result<BTreeSet<u32>> {
    let e = source.read(entity)?;
    let s = source.read(sequence)?;
    let definition = s.pointer(24)?;
    let mut names = BTreeMap::new();
    for field in [0x1A0, 0x2A8] {
        for row in s.array(definition + field, 48, Some(0x8080_0927))? {
            names.insert(u64::try_from(row)?, s.u32(row + 0x28)?);
        }
    }
    let mut inherited = BTreeSet::new();
    for row in e.array(0x30, 88, Some(0x8080_054e))? {
        if e.u32(row + 8)? != sequence {
            continue;
        }
        let Some(&name) = names.get(&e.u64(row + 16)?) else {
            continue;
        };
        let bank = source.read(e.u32(row + 48)?)?;
        let bank_definition = bank.pointer(24)?;
        if bank.u32(bank_definition - 4)? != 0x8080_08f6 {
            continue;
        }
        let channel = u32::try_from(e.u64(row + 80)?)?;
        let declarations = bank.array(bank_definition + 0xD8, 112, Some(0x8080_0883))?;
        let Some(&declaration) = declarations.get(usize::try_from(channel)?) else {
            continue;
        };
        if bank.u32(declaration)? != name {
            continue;
        }
        for field in [0x108, 0x118] {
            for at in bank.array(bank_definition + field, 12, None)? {
                if (bank.u32(at)?, bank.u32(at + 4)?, bank.u32(at + 8)?)
                    == (0x811c_9dc5, name, channel)
                {
                    inherited.insert(name);
                }
            }
        }
    }
    Ok(inherited)
}

/// Translate the source firing presentation onto the base's own presentation. Its nodes join
/// `assembly.nodes`, which the caller moves into the firing effect, and its cues join the
/// weapon's audio closure.
pub(super) fn prepare(assembly: &mut Assembly<'_>, export: &Value) -> Result<Value> {
    let (source_presentation, slots) = source_slots(assembly.source, export)?;
    let rig = assembly.evidence[0]["native_runtime"].clone();
    let (presentation, targets) =
        slot_targets(&mut assembly.native, &rig, &slots.keys().copied().collect())?;
    let groups = slots
        .values()
        .map(|&(_, tag)| BTreeSet::from([tag]))
        .collect::<Vec<_>>();
    let omitted = crate::d1_roi::effect::OMITTED
        .into_iter()
        .chain(FIRING_OMITTED)
        .collect::<BTreeSet<_>>();
    let mut shared = Shared::prepare_omitting(
        assembly.source,
        &mut assembly.native,
        &groups,
        "d1-firing",
        &omitted,
    )?;
    // A shot whose sound or particle systems do not convert would play less than the base flow it
    // replaces: a sequence with a refused cue keeps its sound events only as delays, and one that
    // plays a refused particle system keeps none of its particle events. The base's presentation
    // stays, with the converter's reason.
    for (lost, refusals, what) in [
        ("sequences_without_sound", "audio_refusals", "shot sound"),
        (
            "sequences_without_particles",
            "particle_refusals",
            "shot particles",
        ),
    ] {
        if shared.evidence[lost]
            .as_array()
            .is_some_and(|tags| !tags.is_empty())
        {
            let refusal = &shared.evidence[refusals][0];
            let system = refusal["system"]
                .as_u64()
                .map(|system| format!("system {system:08X}: "))
                .unwrap_or_default();
            bail!(
                "The source {what} does not convert: {system}{}",
                refusal["error"].as_str().unwrap_or("no reason recorded")
            );
        }
    }
    // The firing sequences create no graphs and query nothing. A shot's feedback event names a
    // feedback resource, which takes its own private closure, as the ability path's do.
    let (graphs, profiles, feedback) = shared.named_resources();
    ensure!(
        graphs.is_empty() && profiles.is_empty(),
        "The source firing presentation creates graphs or queries targets"
    );
    ensure!(
        shared.actions().is_empty(),
        "The source firing presentation plays animation"
    );
    let mut named = crate::d1_roi::effect::Resources::default();
    let mut feedback_assets = Vec::new();
    for tag in feedback {
        let closure = crate::d1_roi::native::damage_profile::feedback::translate(
            assembly.source,
            tag,
            "d1-firing",
        )?;
        named.feedback.insert(
            tag,
            closure
                .first()
                .context("Translated firing feedback closure is empty")?
                .symbol
                .clone(),
        );
        feedback_assets.extend(closure);
    }
    shared.set_resources(named);
    let start = assembly.nodes.len();
    let mut published = BTreeSet::new();
    for asset in feedback_assets {
        if !published.insert(asset.symbol.clone()) {
            continue;
        }
        let template = assembly
            .native
            .classes(asset.class)
            .into_iter()
            .min()
            .context("Native firing feedback template is absent")?;
        let patches = asset
            .patches
            .iter()
            .map(|patch| (patch.offset, patch.symbol.clone()))
            .collect::<Vec<_>>();
        assembly.node(&asset.symbol, template, &asset.payload.0, None, &patches)?;
    }
    for node in std::mem::take(&mut shared.nodes) {
        assembly.node(
            &node.symbol,
            node.template,
            &node.payload,
            node.reference.as_deref(),
            &node.patches,
        )?;
    }
    let assets = assembly.nodes.drain(start..).collect::<Vec<_>>();
    let mut owners = Vec::new();
    let mut allocations = Vec::new();
    let mut replacements = Vec::new();
    let mut report = Vec::new();
    for (&slot, &(source_entity, sequence_tag)) in &slots {
        let (entity, flow) = targets[&slot];
        let (sequence, bindings): (&Owner, _) = shared.bindings(sequence_tag)?;
        let template = (*assembly.native.tag(flow, Some(0x8080_9c36))?).clone();
        let allocation_tag = template.u32(0x44)?;
        let allocation = (*assembly.native.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
        // The base flow's own events supply every class it holds. Stock firing flows hold no
        // delay, which an event kept as timed delay needs, so the stock flows that supply the Reach
        // firing route's delay templates follow it.
        let mut envelopes = vec![template.clone()];
        envelopes.extend(shared.witnesses.iter().cloned());
        for tag in DELAY_ENVELOPES {
            envelopes.push((*assembly.native.tag(tag, Some(0x8080_9c36))?).clone());
        }
        // A base flow without scalar inputs, as on some scout and sniper bases, takes its input
        // row pair from the first envelope that holds one, as the effect component route does.
        // A sequence without inputs reads none of it.
        let input_envelope = envelopes
            .iter()
            .find(|envelope| crate::d1_roi::effect::has_inputs(envelope))
            .unwrap_or(&template);
        let written = sequence.write(
            &template,
            &allocation,
            input_envelope,
            &envelopes,
            &bindings,
        )?;
        ensure!(
            written.prepared.dependencies.is_empty(),
            "Source firing sequence {sequence_tag:08X} names further private resources"
        );
        ensure!(
            written.owner.u32(0x44)? == allocation_tag,
            "Translated firing flow names another allocation"
        );
        let owner_symbol = format!("d1-firing-flow-{slot}");
        let allocation_symbol = format!("{owner_symbol}-allocation");
        let start = assembly.nodes.len();
        assembly.node(
            &owner_symbol,
            flow,
            &written.owner.0,
            None,
            &written.patches,
        )?;
        assembly.node(
            &allocation_symbol,
            allocation_tag,
            &written.allocation.0,
            None,
            &[],
        )?;
        let mut added = assembly.nodes.drain(start..);
        owners.push(added.next().context("Translated firing flow node")?);
        allocations.push(added.next().context("Translated firing allocation node")?);
        drop(added);
        let mut row = json!({"slot":slot,"entity":format!("{entity:08X}"),
            "component_owner":format!("{flow:08X}"),"owner":owner_symbol,
            "allocation":allocation_symbol});
        let names = written
            .inputs
            .iter()
            .map(|&(name, _)| name)
            .collect::<Vec<_>>();
        let inputs = live_inputs(&written.owner, &names)?;
        let inherit = source_inherited(assembly.source, source_entity, sequence_tag)?;
        let (rows, bank) = connections(&mut assembly.native, entity, flow, &inputs, &inherit)?;
        if let Some((before, after)) = rows {
            row["connections"] = json!({"before":hex::encode(before),"after":hex::encode(after)});
        }
        // The flow row comes first, so its rows still name the base bank when the bank row
        // retargets that bank to its private copy, as on the Reach firing route.
        replacements.push(row);
        let mut private_bank = Value::Null;
        if let Some(bank) = bank {
            let bank_symbol = format!("d1-firing-bank-{slot}");
            let bank_allocation_symbol = format!("{bank_symbol}-allocation");
            let start = assembly.nodes.len();
            assembly.node(&bank_symbol, bank.tag, &bank.payload.0, None, &[])?;
            assembly.node(
                &bank_allocation_symbol,
                bank.allocation_tag,
                &bank.allocation.0,
                None,
                &[],
            )?;
            let mut added = assembly.nodes.drain(start..);
            owners.push(added.next().context("Private firing bank node")?);
            allocations.push(
                added
                    .next()
                    .context("Private firing bank allocation node")?,
            );
            drop(added);
            replacements.push(json!({"slot":slot,"entity":format!("{entity:08X}"),
                "component_owner":format!("{:08X}",bank.tag),"owner":bank_symbol,
                "allocation":bank_allocation_symbol}));
            private_bank = json!({"base":format!("{:08X}",bank.tag),
                "inherited":bank.inherited.iter().map(|name| format!("{name:08X}")).collect::<Vec<_>>()});
        }
        report.push(
            json!({"slot":slot,"source_entity":format!("{source_entity:08X}"),
            "source_sequence":format!("{sequence_tag:08X}"),
            "native_entity":format!("{entity:08X}"),"native_flow":format!("{flow:08X}"),
            "private_bank":private_bank,"sequence":sequence.evidence()}),
        );
    }
    // Only cues whose sound the shared closure kept are bound by the written flows. The report
    // lists those this presentation added, so the weapon can drop them with its effect.
    let mut added = Vec::new();
    if let Some((_, roots)) = &shared.audio {
        for cue in shared.cues() {
            if roots.contains(&crate::d1_roi::audio::cue_symbol(cue))
                && assembly.projectile_cues.insert(cue)
            {
                added.push(cue);
            }
        }
    }
    Ok(json!({"status":"translated","source":source_presentation,
        "slots":report,"closure":shared.evidence,"cues":added,
        "effect":{"native_presentation":presentation,"owners":owners,"allocations":allocations,
            "replacements":replacements,"assets":{"installable":true,"nodes":assets}},
        "limits":["Source slot ordinals become native slot ordinals, inferred from both presentation layouts.",
            "Event kinds without a translator, such as firing lights, keep their timing as delays.",
            "The flows' scheduling, first- and third-person drawing and sound in play are gameplay checks."],
        "gameplay_verified":false}))
}
