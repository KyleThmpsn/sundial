//! A cue the game starts once for a trigger hold and stops on release.
//!
//! Such a native firing sound names a stop event beside its play event. The play event starts
//! random containers that are continuous, loop without end and move on at a trigger rate, so one
//! start sounds a shot per interval, and the stop event breaks them so the shot in flight
//! finishes. Stock bank `80BBE59E` holds three such containers with a transition time of zero and
//! one additive curve from game parameter `6379397C` to the transition value, in seconds: 0.09 at
//! 10 and 0.065 at 15, the interval of that many shots a second.
//!
//! A source shot that its own game posts once per round sounds only the first round of a hold on
//! such a cue. Each element the source event plays is therefore placed under a container of the
//! same kind with the carrier's own cadence, and a stop event breaks those containers.
use super::route::{Objects, links, objects, word};
use super::*;

/// How often a held native cue sounds: the container's transition time with its random range,
/// and the game parameter curves that add to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cadence {
    time: [u8; 12],
    curves: Vec<Curve>,
}

#[derive(Clone, Debug, PartialEq)]
struct Curve {
    parameter: u32,
    accumulation: u8,
    scaling: u8,
    points: Vec<u8>,
}

/// The native transition value a curve drives, `PlayMechanismSpecialTransitionsValue`.
const TRANSITION: u8 = 7;
/// Continuous, global, and a play list that restarts with each play.
const CONTINUOUS: u8 = 0x1A;
const TRIGGER_RATE: u8 = 5;

/// A source event to wrap, with the cadence of the carrier's cue.
pub struct Held {
    pub event: u32,
    pub cadence: Cadence,
}

/// The identity the wrapped bank gives the stop event of source event `event`.
pub fn stop_event(event: u32) -> u32 {
    hash(&format!("held/{event:08x}/stop"))
}

struct Node<'a> {
    kind: u8,
    curves: Vec<(u8, Curve)>,
    rest: &'a [u8],
}

/// A version 113 node's curves and the bytes after its base parameters.
fn node(kind: u8, body: &[u8]) -> Result<Node<'_>> {
    // Identity, then the effect, bus and parent fields `links` reads.
    links(kind, body)?;
    let mut r = Read { bytes: body, at: 4 };
    r.u8()?;
    let effects = usize::from(r.u8()?);
    if effects > 0 {
        r.take(1 + 7 * effects)?;
    }
    r.take(1 + 4 + 4 + 1)?;
    let count = usize::from(r.u8()?);
    r.take(count * 5)?;
    let count = usize::from(r.u8()?);
    r.take(count * 9)?;
    let position = r.u8()?;
    if position & 9 == 9 {
        // A positioned node: its mode byte and attenuation. Mode 1 follows the emitter, and
        // any other carries an automation path this does not read.
        ensure!(
            r.u8()? & 3 == 1,
            "Held native container has automated positioning"
        );
        r.u32()?;
    }
    if r.u8()? & 8 != 0 {
        r.take(16)?;
    }
    r.take(6)?;
    for _ in 0..r.count()? {
        r.take(5)?;
        let states = usize::from(r.u16()?);
        r.take(states * 8)?;
    }
    let mut curves = Vec::new();
    for _ in 0..r.u16()? {
        let parameter = r.u32()?;
        let source = r.u8()?;
        let accumulation = r.u8()?;
        let target = r.u8()?;
        r.u32()?;
        let scaling = r.u8()?;
        let points = usize::from(r.u16()?);
        let points = r.take(points * 12)?.to_vec();
        if target == TRANSITION {
            ensure!(
                source == 0,
                "Held native cadence follows something other than a game parameter"
            );
            curves.push((
                target,
                Curve {
                    parameter,
                    accumulation,
                    scaling,
                    points,
                },
            ));
        }
    }
    Ok(Node {
        kind,
        curves,
        rest: &body[r.at..],
    })
}

fn children(r: &mut Read<'_>) -> Result<Vec<u32>> {
    (0..r.count()?).map(|_| r.u32()).collect()
}

/// The held containers under `target`, through switch and layer containers, each with its
/// cadence.
fn loops(
    all: &Objects<'_>,
    target: u32,
    found: &mut BTreeMap<u32, Cadence>,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 16, "Native audio hierarchy is too deep");
    let Some((kind, body)) = all.get(&target) else {
        return Ok(());
    };
    if !matches!(kind, 5 | 6 | 9) {
        return Ok(());
    }
    let node = node(*kind, body)?;
    let mut r = Read {
        bytes: node.rest,
        at: 0,
    };
    match node.kind {
        5 => {
            let count = r.u16()?;
            r.take(4)?;
            let time = <[u8; 12]>::try_from(r.take(12)?)?;
            r.take(2)?;
            let transition = r.u8()?;
            r.take(2)?;
            let bits = r.u8()?;
            if count == 0 && transition == TRIGGER_RATE && bits & 8 != 0 {
                found.insert(
                    target,
                    Cadence {
                        time,
                        curves: node.curves.into_iter().map(|(_, curve)| curve).collect(),
                    },
                );
            }
        }
        6 => {
            r.take(1 + 4 + 4 + 1)?;
            for child in children(&mut r)? {
                loops(all, child, found, depth + 1)?;
            }
        }
        _ => {
            for child in children(&mut r)? {
                loops(all, child, found, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// The held containers the actions of `event` reach whose kind satisfies `wanted`.
fn reached(
    all: &Objects<'_>,
    event: u32,
    wanted: impl Fn(u16) -> bool,
) -> Result<BTreeMap<u32, Cadence>> {
    let (kind, body) = all
        .get(&event)
        .with_context(|| format!("Native bank has no event {event:08X}"))?;
    ensure!(
        *kind == 4,
        "Native audio object {event:08X} is not an event"
    );
    let mut found = BTreeMap::new();
    for index in 0..word(body, 4)? as usize {
        let (kind, action) = all
            .get(&word(body, 8 + 4 * index)?)
            .context("Native event action is outside its bank")?;
        ensure!(*kind == 3, "Native event names an object that is no action");
        let mut r = Read {
            bytes: action,
            at: 4,
        };
        if wanted(r.u16()?) {
            loops(all, r.u32()?, &mut found, 0)?;
        }
    }
    Ok(found)
}

/// The cadence of a native cue that `play` starts and `stop` ends. Every held container the
/// play event starts must share it, and the stop event must break each of them.
pub fn cadence(bank: &[u8], play: u32, stop: u32) -> Result<Cadence> {
    let all = objects(bank)?;
    let started = reached(&all, play, |action| action >> 8 == 4)?;
    let broken = reached(&all, stop, |action| action >> 8 == 0x1C)?;
    let first = started
        .values()
        .next()
        .context("Native cue has a stop event and starts no trigger-rate container")?;
    ensure!(
        started.values().all(|cadence| cadence == first),
        "Native cue's held containers differ in cadence"
    );
    ensure!(
        started.keys().eq(broken.keys()),
        "Native stop event does not break the containers its play event starts"
    );
    let time = f32::from_le_bytes(first.time[..4].try_into()?);
    ensure!(
        time.is_finite() && time >= 0. && (time > 0. || !first.curves.is_empty()),
        "Native held cue has no trigger interval"
    );
    Ok(first.clone())
}

/// One wrapped element: the container that repeats it and the action that breaks it.
pub(super) struct Wrap {
    pub container: u32,
    pub breaker: u32,
    /// The element's own parent, which becomes the container's, once the element is lowered.
    pub parent: Option<u32>,
    /// Whether a mixer of this bank listed the element and now lists the container.
    pub adopted: bool,
}

pub(super) struct Plan {
    pub cadence: Cadence,
    pub stop: u32,
    /// The actions of the held event.
    pub actions: BTreeSet<u32>,
    /// By played element.
    pub wraps: BTreeMap<u32, Wrap>,
}

impl Plan {
    /// Reads the play actions of `held.event` from a source bank's framed objects, each its type,
    /// identity and body after the identity.
    pub(super) fn read(
        held: &Held,
        entries: &[(u8, u32, Read<'_>)],
        taken: &mut BTreeSet<u32>,
    ) -> Result<Self> {
        let body = |identity: u32, kind: u8| {
            entries
                .iter()
                .find(|(found, id, _)| *id == identity && *found == kind)
                .map(|(_, _, r)| Read {
                    bytes: r.bytes,
                    at: r.at,
                })
        };
        let mut event = body(held.event, 4).context("Held source event is absent from its bank")?;
        let mut claim = |name: String| -> Result<u32> {
            let identity = hash(&name);
            ensure!(
                identity != 0 && taken.insert(identity),
                "Held audio object identity collision"
            );
            Ok(identity)
        };
        let stop = claim(format!("held/{:08x}/stop", held.event))?;
        let mut actions = BTreeSet::new();
        let mut wraps = BTreeMap::new();
        for _ in 0..event.vcount()? {
            let action = event.u32()?;
            let mut r = body(action, 3).context("Held source event action is absent")?;
            ensure!(
                r.u16()? >> 8 == 4 && actions.insert(action),
                "Held source event does more than play"
            );
            let target = r.u32()?;
            ensure!(
                entries
                    .iter()
                    .any(|(kind, id, _)| *id == target && matches!(kind, 2 | 5 | 6 | 9)),
                "Held source event plays an element outside its bank"
            );
            if let std::collections::btree_map::Entry::Vacant(slot) = wraps.entry(target) {
                slot.insert(Wrap {
                    container: claim(format!("held/{target:08x}"))?,
                    breaker: claim(format!("held/{target:08x}/break"))?,
                    parent: None,
                    adopted: false,
                });
            }
        }
        ensure!(!wraps.is_empty(), "Held source event plays nothing");
        Ok(Self {
            cadence: held.cadence.clone(),
            stop,
            actions,
            wraps,
        })
    }

    /// The container that repeats `element`, written as the native ones are: no properties of
    /// its own, so the element inherits what it did, an endless continuous random list of the
    /// one element and the carrier's trigger interval.
    pub(super) fn container(&self, element: u32) -> Result<Write> {
        let wrap = &self.wraps[&element];
        let mut w = Write::default();
        w.reference(wrap.container, Kind::Object);
        w.bytes.extend([0; 3]);
        w.u32(0);
        w.reference(
            wrap.parent
                .context("Held element was not lowered as a node")?,
            Kind::Object,
        );
        // No priority override, properties or ranges, inherited positioning, no auxiliary
        // sends, default voice settings and no states.
        w.bytes.extend([0, 0, 0, 0xC0, 0, 0, 1, 0, 0, 0, 0]);
        w.u32(0);
        w.u16(u16::try_from(self.cadence.curves.len())?);
        for (index, curve) in self.cadence.curves.iter().enumerate() {
            // A native game parameter, which stays in the game's namespace.
            w.u32(curve.parameter);
            w.bytes.extend([0, curve.accumulation, TRANSITION]);
            w.u32(u32::try_from(index)? + 1);
            w.u8(curve.scaling);
            w.u16(u16::try_from(curve.points.len() / 12)?);
            w.bytes.extend(&curve.points);
        }
        w.bytes.extend([0; 6]);
        w.bytes.extend(self.cadence.time);
        w.u16(0);
        w.bytes.extend([TRIGGER_RATE, 0, 0, CONTINUOUS]);
        w.u32(1);
        w.reference(element, Kind::Object);
        w.u16(1);
        w.reference(element, Kind::Object);
        w.u32(50_000);
        Ok(w)
    }

    /// The break action of each container and the stop event that runs them, as typed objects.
    pub(super) fn stop(&self, bank: &BTreeSet<u32>) -> Result<Vec<(u8, Write)>> {
        let mut objects = Vec::new();
        let mut event = Write::default();
        event.reference(self.stop, Kind::Object);
        event.u32(u32::try_from(self.wraps.len())?);
        for (element, wrap) in &self.wraps {
            let parent = wrap
                .parent
                .context("Held element was not lowered as a node")?;
            ensure!(
                wrap.adopted || !bank.contains(&parent),
                "Held element {element:08X} plays under a container, which a repeating \
                 container cannot join"
            );
            let mut action = Write::default();
            action.reference(wrap.breaker, Kind::Object);
            // Break on the element of the posting game object, with no properties.
            action.u16(0x1C03);
            action.reference(wrap.container, Kind::Object);
            action.bytes.extend([0; 3]);
            objects.push((3, action));
            event.reference(wrap.breaker, Kind::Object);
        }
        objects.push((4, event));
        Ok(objects)
    }
}
