//! The source weapon's firing sound on the carrier's native firing cue.
//!
//! The weapon's presentation component `808040D4` names effect entities in 0x30-byte slots from
//! definition `+C0`, and slot 0 is the firing effect. Its effect component `80809F51` holds node
//! records `80807ECA`: a name at `+04`, flags at `+0C` and a wide reference at `+2C`. A node
//! whose reference resolves to a Wwise event resource `8080B128` is a sound node. The resource
//! stores the event identity at `+08` and its media tags in the array at `+20`. It names no
//! bank: the bank is the raw entry immediately before it, which must define the event.
//!
//! A source one shot sounds one round. An automatic source weapon also holds a loop that a
//! second event of the same bank breaks on release, with no surround media in the weapons read,
//! so only one shots are candidates. A firing effect holds one shots for the shooter's own view and
//! for other listeners, with small mechanical layers beside them. In every surveyed weapon the
//! shooter's shot is the one whose event carries media of more than two channels: the same
//! four-channel set sits beside its mono and stereo layers, and the events other weapons share
//! and the mechanical layers have none. The primary firing sound is the largest such shot. Two
//! of equal size that play the same media are one sound under two events, and the first in
//! effect order is taken. Which node the source itself plays for a shot is decided by effect
//! graph conditions this does not read.
//!
//! The carrier's firing entity can hold a second once node that every content row of its
//! presentation shares, a layer of the weapon family. The carrier's own cue is the one the other
//! rows do not sound.
//!
//! Most carriers start their cue for every round. A carrier whose cue names a stop event starts
//! it once for a trigger hold and repeats inside its bank, so there the source shot repeats the
//! same way (`tiger::audio::bank::held`).
//!
//! Source banks are Wwise version 150, as Monument of Triumph's are, so they lower through the
//! same route and the graph's `audio` section has the same shape.
use super::*;
use crate::tiger::audio::{self as wwise, modern::Start};
use std::collections::{BTreeMap, BTreeSet};
use tiger_pkg::TagHash;

const PRESENTATION: u32 = 0x808040D4;
const EFFECT: u32 = 0x80809F51;
const NODE: u32 = 0x80807ECA;
const EVENT: u32 = 0x8080B128;
const ENTITY: u32 = 0x8080BAAD;
const OWNER: u32 = 0x8080BADB;

struct Node {
    name: u32,
    event: u32,
    identity: u32,
    bank: u32,
    media: Vec<u32>,
    start: Start,
    /// Where the node sits in its effect, for effect order.
    order: (usize, usize),
}

impl Node {
    fn report(&self) -> Value {
        json!({"node":format!("{:08X}",self.name),"event":format!("{:08X}",self.event),
            "event_id":format!("{:08X}",self.identity),"bank":format!("{:08X}",self.bank),
            "media":self.media.len(),"start":format!("{:?}",self.start)})
    }

    /// The media identities the event plays, and the largest channel count among them.
    fn layout(&self, source: &mut Reader) -> Result<(Vec<u32>, u16)> {
        let mut identities = Vec::with_capacity(self.media.len());
        let mut channels = 0;
        for tag in &self.media {
            let entry = source
                .manager
                .get_entry(TagHash(*tag))
                .context("Source sound media is missing")?;
            ensure!(
                entry.file_type == 26,
                "Source sound media is not a raw audio asset"
            );
            identities.push(entry.reference);
            channels = channels.max(wwise::transcode::channels(&source.tag(*tag, None)?.0)?);
        }
        identities.sort_unstable();
        Ok((identities, channels))
    }
}

/// An entity's component owners with their definition classes.
fn components(source: &mut Reader, entity: u32) -> Result<Vec<(std::sync::Arc<Payload>, u32)>> {
    let payload = source.tag(entity, Some(ENTITY))?;
    payload
        .array(8, 12, Some(0x8080BAA2))?
        .into_iter()
        .map(|at| {
            let owner = source.tag(payload.u32(at)?, Some(OWNER))?;
            let class = owner.u32(owner.pointer(24)? - 4)?;
            Ok((owner, class))
        })
        .collect()
}

/// One sound node's event, its bank and how the bank starts it.
fn node(source: &mut Reader, name: u32, event: u32, order: (usize, usize)) -> Result<Node> {
    let resource = source.tag(event, Some(EVENT))?;
    let identity = resource.u32(8)?;
    let media = resource
        .array(0x20, 4, None)?
        .into_iter()
        .map(|at| resource.u32(at))
        .collect::<Result<Vec<_>>>()?;
    let bank = event
        .checked_sub(1)
        .context("Source sound event has no preceding entry")?;
    let entry = source
        .manager
        .get_entry(TagHash(bank))
        .context("Source sound event has no preceding entry")?;
    ensure!(
        entry.file_type == 26 && entry.file_subtype == 6,
        "The entry before source sound event {event:08X} is not a sound bank"
    );
    let start = wwise::modern::start(&source.tag(bank, None)?.0, identity)
        .with_context(|| format!("Source sound bank {bank:08X} for event {identity:08X}"))?;
    Ok(Node {
        name,
        event,
        identity,
        bank,
        media,
        start,
        order,
    })
}

/// The sound nodes of the weapon's firing effect, the entity in slot 0 of its presentation.
fn firing_nodes(source: &mut Reader, weapon: u32) -> Result<Vec<Node>> {
    let mut presentations = components(source, weapon)?
        .into_iter()
        .filter(|(_, class)| *class == PRESENTATION);
    let (Some((presentation, _)), None) = (presentations.next(), presentations.next()) else {
        anyhow::bail!("Source weapon has no single presentation component");
    };
    let firing = source
        .ref64(&presentation, presentation.pointer(24)? + 0xC0)
        .ok()
        .filter(|tag| source.reference(*tag).ok() == Some(ENTITY))
        .context("Source weapon presentation has no firing effect in slot 0")?;
    let mut nodes = BTreeMap::new();
    for (index, (effect, class)) in components(source, firing)?.into_iter().enumerate() {
        if class != EFFECT {
            continue;
        }
        for at in (0..effect.0.len().saturating_sub(0x3C)).step_by(4) {
            if effect.u32(at)? != NODE {
                continue;
            }
            let Ok(event) = source.ref64(&effect, at + 0x2C) else {
                continue;
            };
            if source.reference(event).ok() != Some(EVENT) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(slot) = nodes.entry(event) {
                slot.insert(node(source, effect.u32(at + 4)?, event, (index, at))?);
            }
        }
    }
    let mut nodes = nodes.into_values().collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.order);
    Ok(nodes)
}

/// The shooter's own shot among the firing effect's nodes: the largest one shot whose media
/// include more than two channels.
fn primary<'a>(source: &mut Reader, nodes: &'a [Node]) -> Result<&'a Node> {
    let mut shots = Vec::new();
    for node in nodes {
        if node.start == Start::OneShot && !node.media.is_empty() {
            let (identities, channels) = node.layout(source)?;
            if channels > 2 {
                shots.push((node, identities));
            }
        }
    }
    // Stable, so shots of one size keep effect order.
    shots.sort_by_key(|(node, _)| std::cmp::Reverse(node.media.len()));
    let Some((first, identities)) = shots.first() else {
        anyhow::bail!("Source firing effect has no one-shot sound with surround media");
    };
    ensure!(
        shots
            .iter()
            .take_while(|(node, _)| node.media.len() == first.media.len())
            .all(|(_, other)| other == identities),
        "Source firing effect has two one-shot sounds of equal size and different media"
    );
    Ok(*first)
}

/// The event identities that sound once in slot 0 of the carrier presentation's other content
/// rows.
fn family_events(native: &mut Reader, rig: &Value, group: &Value) -> Result<BTreeSet<String>> {
    let owner_tag = hash(group, "owner")?;
    let owner = native.tag(owner_tag, Some(0x80809C36))?;
    let resource = owner.pointer(24)?;
    let selected = &rig["audio"]["content_key"];
    let components = rig["components"]
        .as_array()
        .context("Native rig components")?
        .iter()
        .filter(|component| component["owner"] == group["owner"])
        .cloned()
        .collect::<Vec<_>>();
    let mut events = BTreeSet::new();
    for row in owner.array(resource + 0x240, 0x1C0, Some(0x80803ACF))? {
        let content = owner.u32(row + 0x10)?;
        if json!(format!("{content:08X}")) == *selected {
            continue;
        }
        let other = wwise::inspect(native, &components, content, false)?;
        for sound in other["unnamed_groups"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|group| group["sounds"].as_array().into_iter().flatten())
        {
            if sound["root_slot"] == 0
                && sound["node_kind"] == "once"
                && let Some(event) = sound["event_id"].as_str()
            {
                events.insert(event.to_owned());
            }
        }
    }
    Ok(events)
}

/// The carrier's primary firing cue: the single once node with media in slot 0 of its native
/// presentation. A node whose event carries the presentation's content key is preferred, since
/// other once nodes of the firing entity are mechanical or tail layers. Failing that, a node the
/// presentation's other content rows also sound is a layer the family shares, and the one node
/// they do not sound is the carrier's own.
fn native_cue<'a>(native: &mut Reader, rig: &'a Value) -> Result<(&'a Value, &'a Value)> {
    let mut candidates = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Native firing presentation unavailable")?
        .iter()
        .flat_map(|group| {
            group["sounds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |sound| (group, sound))
        })
        .filter(|(_, sound)| {
            sound["root_slot"] == 0
                && sound["component_class"] == "808084E9"
                && sound["node_kind"] == "once"
                && sound["media"].as_array().is_some_and(|m| !m.is_empty())
        })
        .collect::<Vec<_>>();
    if candidates
        .iter()
        .any(|(group, sound)| sound["event_id"] == group["content_key"])
    {
        candidates.retain(|(group, sound)| sound["event_id"] == group["content_key"]);
    }
    if candidates.len() > 1 {
        let mut own = Vec::new();
        for (group, sound) in &candidates {
            let shared = family_events(native, rig, group)?;
            if sound["event_id"]
                .as_str()
                .is_some_and(|event| !shared.contains(event))
            {
                own.push((*group, *sound));
            }
        }
        if own.len() == 1 {
            candidates = own;
        }
    }
    ensure!(
        candidates.len() == 1,
        "Native firing sound is absent or ambiguous, {} candidates: {}",
        candidates.len(),
        candidates
            .iter()
            .map(|(_, sound)| format!(
                "event {} with {} media",
                sound["event_id"].as_str().unwrap_or("unknown"),
                sound["media"].as_array().map_or(0, Vec::len)
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(candidates[0])
}

/// The graph's `audio` section: the source primary firing sound paired with the carrier's
/// firing cue, with the lowered bank and decoded media beside the graph.
fn convert(
    (source, packages): (&mut Reader, &Path),
    (native, native_packages): (&mut Reader, &Path),
    weapon: u32,
    item_tag: u32,
    graph: &Path,
) -> Result<Value> {
    let nodes = firing_nodes(source, weapon)?;
    let primary = primary(source, &nodes)?;
    let media_ids = primary
        .media
        .iter()
        .map(|tag| {
            let entry = source
                .manager
                .get_entry(TagHash(*tag))
                .context("Source sound media is missing")?;
            ensure!(
                entry.file_type == 26,
                "Source sound media is not a raw audio asset"
            );
            Ok(format!("{:08X}", entry.reference))
        })
        .collect::<Result<Vec<_>>>()?;
    let rig = crate::tiger::rig::inspect_with_audio(native, item_tag, false)?;
    let (presentation, cue) = native_cue(native, &rig)?;
    let cue_bank = native.tag(hash(cue, "bank")?, None)?;
    // A cue with a stop event starts once for a trigger hold. The source shot then repeats at
    // the cue's own cadence until a stop event the lowered bank adds.
    let held = match cue["secondary_event_id"]
        .as_str()
        .map(|stop| u32::from_str_radix(stop, 16))
        .transpose()?
        .filter(|stop| !matches!(*stop, 0 | u32::MAX | 0x811C_9DC5))
    {
        Some(stop) => Some(wwise::bank::held::Held {
            event: primary.identity,
            cadence: wwise::bank::held::cadence(&cue_bank.0, hash(cue, "event_id")?, stop)
                .context("Native held firing cue")?,
        }),
        None => None,
    };
    let stop = held.as_ref().map_or(0x811C_9DC5, |held| {
        wwise::bank::held::stop_event(held.event)
    });
    let sound = json!({"tag":format!("{:08X}",primary.event),
        "event_id":format!("{:08X}",primary.identity),
        "secondary_event_id":format!("{stop:08X}"),
        "bank":format!("{:08X}",primary.bank),
        "media":primary.media.iter().map(|tag| format!("{tag:08X}")).collect::<Vec<_>>(),
        "media_ids":media_ids});
    // The source's buses belong to its own game and Shadowkeep's Init defines none of them, so
    // the sound outputs through the bus the carrier's own shot plays on.
    let cue_media = cue["media_ids"]
        .as_array()
        .context("Native firing cue media")?
        .iter()
        .map(|id| {
            Ok(u32::from_str_radix(
                id.as_str().context("Native media identity")?,
                16,
            )?)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let output_bus = wwise::bank::route::media_bus(&cue_bank.0, &cue_media)
        .context("Native firing output bus")?;
    let mut report = json!({"authoring_schema":3,"runtime_entity":rig["runtime_entity"],
        "matched_events":[],"native_output_bus":format!("{output_bus:08X}"),
        "held":held.is_some(),
        "firing_events":[{"sounds":[sound],"native_sounds":[cue],
            "native_presentation":presentation,"mapping":"marathon_primary_fire"}],
        "source_nodes":nodes.iter().map(Node::report).collect::<Vec<_>>(),
        "primary":primary.report(),
        "limits":["The shooter's largest source one shot replaces the carrier's own firing cue. \
            A once node the carrier's weapon family shares keeps playing. Every source bus \
            becomes the carrier shot's output bus, so source bus volumes, effects, ducking and \
            sends to source auxiliary buses are lost, as are modulators, per-sound obstruction \
            and diffraction. On a carrier cue that starts once for a trigger hold, the shot \
            repeats at the carrier's own trigger interval until release, an interval the \
            carrier's game parameter sets and the source weapon's own rate may not match. The \
            source's own held loops, the other one shots of the firing effect, \
            foley, layers chosen by source game state, third-person playback and loudness are \
            not translated or verified."]});
    wwise::convert_media(
        (source, packages),
        (native, native_packages),
        graph,
        &mut report,
        wwise::bank::Dialect {
            output_bus: Some(output_bus),
            ..wwise::bank::Dialect::MARATHON
        },
        held.as_ref().map(|held| (primary.bank, held)),
    )?;
    ensure!(
        report.get("conversion_errors").is_none(),
        "Firing sound conversion failed: {}",
        report["conversion_errors"]
    );
    Ok(report)
}

/// Adds the weapon's firing sound to its graph report. A weapon whose firing sound cannot be
/// read or converted keeps the carrier's sound, and the report records why.
pub(super) fn author(
    source: (&mut Reader, &Path),
    native: (&mut Reader, &Path),
    report: &mut Value,
    item_tag: u32,
    graph: &Path,
) -> Result<()> {
    let weapon = hash(
        report["source_assembly"]
            .get(0)
            .context("Marathon assembly has no weapon entity")?,
        "entity",
    )?;
    match convert(source, native, weapon, item_tag, graph) {
        Ok(audio) => {
            report["audio"] = audio;
            report["source_audio"] = json!({"status":"firing","gameplay_verified":false});
        }
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(error) => {
            // Files a failed conversion left behind are not named by the graph.
            let _ = fs::remove_dir_all(graph.join("audio"));
            report["source_audio"] = json!({"status":"carrier","reason":format!("{error:#}"),
                "gameplay_verified":false});
        }
    }
    Ok(())
}
