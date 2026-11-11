//! Source debris and particles scheduled by private native effect entities.
use super::super::{fragment, secondary::Bundle};
use super::*;
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        effect::{particle, source},
        native::seat,
        projectile::effect::light,
        resource::Pages,
        rig,
    },
    tiger::{
        allocation::Row,
        markers,
        sequence::{self, Kind, Node, Timing, spawn},
    },
};
use std::{collections::BTreeMap, path::Path};

fn dynamic_location(effect: &Value, part: &Value) -> Result<Option<String>> {
    let location = part["location"]
        .as_i64()
        .context("Source effect location")?;
    if location < 0 {
        return Ok(None);
    }
    let marker = effect["locations"][usize::try_from(location)?]["name"]
        .as_str()
        .context("Source effect location name")?;
    // The ReachMCC effect schema supplies these frames at runtime. In
    // particular, gravity and up are not sockets on the render model.
    Ok(matches!(marker, "gravity" | "up").then(|| {
        format!("Source effect location {marker} requires a runtime gravity-aligned frame")
    }))
}

/// Why an effect part of source group `group` stays pending: the native piece its translation
/// lacks. Objects (`bloc`) and cheap particle emitters (`cpem`) have their own adapters.
fn untranslated(group: &str) -> String {
    match group {
        "snd!" => "Sound part. Only a contact host plays an effect's sound parts, through a native once-sound event (80806B38)".into(),
        "ligh" | "gldf" => "Light part. Only a contact host plays an effect's light parts, through a native light event (80806F49)".into(),
        "decs" => "Decal part. A native decal event (808093C2) names a decal record (8080707E) whose two textures and two parameter programs (808071DC) have no established roles, and no converter authors one from a Reach decal system".into(),
        "sefc" => "Screen effect part. No native counterpart of a Reach area screen effect is established".into(),
        "jpt!" => "Damage effect part. Its area damage, camera shake and rumble need a native query or camera event".into(),
        "lens" => "Lens flare part. A native lens flare event (80806F40) names a flare record (80806F68) whose size and color come from its parameter program (808071DC), and no converter authors one from a source lens flare".into(),
        "cntl" => "Contrail part. Source contrails convert only as projectile flight attachments".into(),
        _ => format!("Part of group {group}. No converter translates this part kind"),
    }
}

fn repeat(scheduled: &mut Vec<Node>, effect: &Value, prefix: &str, loop_start: i64) -> Result<()> {
    if loop_start >= 0 {
        let index = usize::try_from(loop_start)?;
        let events = effect["events"].as_array().context("Source loop events")?;
        ensure!(
            events[index..].iter().any(|event| {
                ["delay_seconds", "duration_seconds"].iter().any(|field| {
                    event[field].as_array().is_some_and(|values| {
                        values.iter().all(|v| v.as_f64().is_some_and(|v| v > 0.))
                    })
                })
            }),
            "Source effect loop can repeat without advancing time"
        );
        let repeated = scheduled.split_off(index);
        scheduled.push(node(&format!("{prefix}/repeat"), Kind::Repeat(repeated)));
    }
    Ok(())
}

/// The frame a host's first locator gives its effects.
#[derive(Clone, Copy)]
enum Origin {
    /// The transform of the object creation that spawns the host, point kind 9.
    Creation,
    /// The contact an impact table creates the host for, point kind 13 over an owner transform.
    Contact,
    /// A damage marker of the vehicle that keeps the host.
    Marker(u32),
}

/// Name of the contact point in the flows of stock impact table entities.
const CONTACT_LOCATOR: u32 = 0x4a31_8ad6;

/// The once-sound event class. The stock host flow plays one, with selector `FF` at `+38`, its
/// cue at `+40`, an empty parameter array at `+48`, locator ordinal 0 at `+70`, no timing, and an
/// instance whose playback handle at `+40` stays `FFFFFFFF` until its first creation.
const SOUND_EVENT: u32 = 0x8080_6b38;

/// The light event class, kind 33. Its definition names a light record (`80807140`) at `+40`
/// and the locator ordinal at `+44`, holds stock parameters to `+78` and three embedded
/// expressions at `+78`, `+C0` and `+108`.
const LIGHT_EVENT: u32 = 0x8080_6f49;

/// The bus of private contact cues. The cue the stock contact host plays sends two of its three
/// layers to it and the third to `E2B7BC37`, and the hammer impact cue takes it too.
const CONTACT_BUS: u32 = 0x2ce9_3fb5;

/// The cue of the one once-sound event in the flow of stock host `80C0DC18`, `80BF59B7` in the
/// inspected stock packages.
fn host_cue(native: &mut Reader) -> Result<u32> {
    let bundle = Bundle::read(native, 0x80c0dc18)?;
    let mut cues = Vec::new();
    for payload in bundle.owners.values() {
        let d = payload.pointer(24)?;
        if payload.u32(d - 4)? != 0x808084e9 {
            continue;
        }
        for row in payload.array(d + 0x168, 24, Some(0x808093e6))? {
            let event = payload.pointer(row + 16)?;
            if payload.u32(event - 4)? == SOUND_EVENT {
                cues.push(payload.u32(event + 0x40)?);
            }
        }
    }
    let [cue] = cues[..] else {
        anyhow::bail!("The stock contact host flow has no single once-sound event")
    };
    Ok(cue)
}

/// A one-shot cue in `graph` for source sound `source`, its nodes named by `prefix` and its bank
/// in the linked namespace range `range` names, from the recordings in `recordings`. It takes the
/// bank, medium and envelope of the cue the stock contact host plays, and that cue's primary bus.
/// The cue's symbol and report follow.
pub(in crate::halo_reach) fn contact_cue(
    native: &mut Reader,
    graph: &mut Graph,
    source: &Value,
    prefix: &str,
    range: &str,
    recordings: &Path,
) -> Result<(String, Value)> {
    let template = host_cue(native)?;
    let report = crate::halo_reach::audio::once::detached(
        native,
        graph,
        source,
        template,
        Some(CONTACT_BUS),
        prefix,
        range,
        recordings,
    )?;
    Ok((format!("{prefix}-cue"), report))
}

fn host_locators(
    payload: &mut Payload,
    allocation: &mut Payload,
    origin: Origin,
    owner: u32,
) -> Result<()> {
    let d = payload.pointer(24)?;
    for field in [0x178, 0x188, 0x200, 0x210, 0x220, 0x230] {
        ensure!(
            payload.u64(d + field)? == 0,
            "Damage effect host has additional dependencies"
        );
    }
    let locators = payload.array(d + 0x1c0, 16, Some(0x80808615))?;
    if let Origin::Marker(marker) = origin {
        let spans = payload.array(d + 0x1b0, 6, Some(0x80808617))?;
        let names = payload.array(d + 0x1d0, 4, Some(0x80808614))?;
        ensure!(
            locators.len() == 4
                && spans.len() == 4
                && names.len() == 4
                && payload.bytes::<16>(locators[0])?
                    == [
                        2, 0, 1, 0, 0x3c, 0, 0, 0, 0x86, 0x21, 0xa9, 0x3c, 0xff, 0, 0, 0
                    ]
                && payload.bytes::<6>(spans[0])? == [0, 1, 0, 1, 0x3c, 0],
            "Persistent damage effect marker locator differs"
        );
        let dependencies = payload.array(d + 0x2b8, 16, Some(0x80809789))?;
        ensure!(
            dependencies.len() == 1 && payload.u32(dependencies[0])? == owner,
            "Persistent damage effect dependency is not local"
        );
        put(&mut payload.0, locators[0] + 8, &marker.to_le_bytes())?;
        put(&mut payload.0, names[0], &marker.to_le_bytes())?;
    } else {
        ensure!(
            payload.u64(d + 0x2b8)? == 0
                && locators.len() == 1
                && payload.bytes::<16>(locators[0])?
                    == [
                        9, 0, 1, 0, 0x3f, 0, 0, 0, 0xc5, 0x9d, 0x1c, 0x81, 0xff, 0, 0, 0
                    ],
            "Damage effect origin locator differs"
        );
        if let Origin::Contact = origin {
            // Every stock flow an impact table names that draws particles puts them on a
            // kind-13 point with flags 3E and keeps one owner transform row at instance +170,
            // counted at definition +1E0 and allocated by child 82C28E51 of locator node
            // 846FDE3F. Flows without that point have no such row. A host with the point but no
            // row drew its effects at the world origin, and one on kind 9 at the firing weapon's
            // muzzle.
            let instance = payload.pointer(16)?;
            let spans = payload.array(d + 0x1b0, 6, Some(0x80808617))?;
            let names = payload.array(d + 0x1d0, 4, Some(0x80808614))?;
            let states = payload.array(instance + 0x190, 2, Some(0x80808618))?;
            ensure!(
                spans.len() == 1
                    && names.len() == 1
                    && states.len() == 1
                    && payload.bytes::<6>(spans[0])? == [0, 1, 0, 1, 0x3f, 0]
                    && payload.bytes::<2>(states[0])? == [0x3f, 0]
                    && payload.bytes::<16>(d + 0x1e0)?
                        == [0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
                    && payload.u64(instance + 0x170)? == 0,
                "Contact host locator differs"
            );
            payload.0[locators[0]] = 13;
            payload.0[locators[0] + 4] = 0x3e;
            payload.0[spans[0] + 4] = 0x3e;
            payload.0[states[0]] = 0x3e;
            put(&mut payload.0, names[0], &CONTACT_LOCATOR.to_le_bytes())?;
            let identity = [0f32, 0., 0., 1., 0., 0., 0., 1.]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            append(&mut payload.0, instance + 0x170, 0x80809f75, &identity, 32)?;
            put(&mut payload.0, d + 0x1e0, &1u32.to_le_bytes())?;
            let storage = [
                (0x82c28e51, 0x80809f75),
                (0x4c2e87a1, 0x80809f75),
                (0x6e385590, 0x80808618),
                (0xa6e3dd20, 0x80809bff),
            ]
            .map(|(name, class)| Row::named(name, class, 1, Vec::new()));
            Row::replace(
                &[Row::named(0x846fde3f, u32::MAX, 0, storage.into())],
                allocation,
                32,
                &[0x846fde3f],
            )?;
        }
    }
    Ok(())
}

struct Parts {
    children: Vec<Node>,
    creations: Vec<Creation>,
    pending: Vec<Value>,
    /// Cheap particle systems the event's particle burst also starts, their reports and their
    /// longest particle life.
    systems: Vec<String>,
    reports: Vec<Value>,
    life: f32,
    /// The once-sound and light events the event's sound and light parts play, and their reports.
    presented: Vec<Presented>,
}

/// A sound or light part's event in its effect event: the event's name, the cue or light record
/// the event names, and the part's report.
struct Presented {
    name: u32,
    class: u32,
    resource: String,
    report: Value,
}

/// What a contact host's sound and light parts read: the behavior closure whose sound nodes have
/// exported recordings, where those recordings are, and the name of the host's linked namespace
/// range.
#[derive(Clone, Copy)]
struct Audio<'a> {
    behavior: &'a Value,
    recordings: &'a Path,
    range: &'a str,
}

struct Emission {
    entity: String,
    marker: u32,
}

#[derive(Default)]
pub(super) struct Effects {
    emissions: BTreeMap<(usize, usize), Vec<Emission>>,
    slots: BTreeMap<usize, usize>,
    loops: BTreeMap<(usize, usize), String>,
    pub resources: Vec<(usize, String)>,
    pub report: Value,
}

impl Effects {
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn managed(
        &mut self,
        payload: &mut Payload,
        slot: usize,
        entity: Option<String>,
    ) -> Result<usize> {
        let at = sequence::object(payload, 0x80804c3c, 0x20)?;
        put(&mut payload.0, at + 0x10, &u32::MAX.to_le_bytes())?;
        put(
            &mut payload.0,
            at + 0x18,
            &i32::try_from(slot)?.to_le_bytes(),
        )?;
        if let Some(entity) = entity {
            self.resources.push((at + 0x10, entity));
        }
        Ok(at)
    }

    pub fn stop(&mut self, payload: &mut Payload) -> Result<Vec<usize>> {
        (0..self.slot_count())
            .map(|slot| self.managed(payload, slot, None))
            .collect()
    }

    pub fn responses(
        &mut self,
        payload: &mut Payload,
        section: usize,
        response: usize,
    ) -> Result<Vec<usize>> {
        let mut result = Vec::new();
        if let Some(&slot) = self.slots.get(&section) {
            // MCC's looping effect lasts until the next response in this section.
            let entity = self.loops.get(&(section, response)).cloned();
            result.push(self.managed(payload, slot, entity)?);
        }
        for emission in self
            .emissions
            .get(&(section, response))
            .into_iter()
            .flatten()
        {
            let at = sequence::object(payload, 0x80804c34, 0x40)?;
            put(&mut payload.0, at + 4, &emission.marker.to_le_bytes())?;
            put(&mut payload.0, at + 0x10, &u32::MAX.to_le_bytes())?;
            put(&mut payload.0, at + 0x18, &[1, 1, 0, 0])?;
            self.resources.push((at + 0x10, emission.entity.clone()));
            result.push(at);
        }
        Ok(result)
    }
}

fn vector<const N: usize>(value: &Value, field: &str) -> Result<[f32; N]> {
    let rows = value[field]
        .as_array()
        .filter(|rows| rows.len() == N)
        .with_context(|| format!("Source effect {field}"))?;
    let mut out = [0.; N];
    for (result, value) in out.iter_mut().zip(rows) {
        *result = value.as_f64().context("Source effect number")? as f32;
        ensure!(result.is_finite(), "Nonfinite source effect parameter");
    }
    Ok(out)
}

fn product(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn angles([yaw, pitch]: [f32; 2]) -> Result<[f32; 4]> {
    // Source forward is (cos(yaw)*cos(pitch), sin(yaw)*cos(pitch), sin(pitch)).
    rig::quaternion(product(
        [0., 0., (yaw * 0.5).sin(), (yaw * 0.5).cos()],
        [0., -(pitch * 0.5).sin(), 0., (pitch * 0.5).cos()],
    ))
}

fn node(name: &str, kind: Kind) -> Node {
    Node {
        name: hash(name),
        timing: Timing::default(),
        kind,
    }
}

fn delay(name: &str, duration: [f32; 2]) -> Node {
    Node {
        name: hash(name),
        timing: Timing {
            duration,
            flags: 4,
            ..Timing::default()
        },
        kind: Kind::Event(0x808093cb),
    }
}

fn chance(name: &str, skip: f32, child: Node) -> Result<Node> {
    ensure!(
        skip.is_finite() && (0.0..=1.0).contains(&skip),
        "Invalid source effect skip fraction"
    );
    if skip == 0. {
        return Ok(child);
    }
    Ok(node(
        name,
        Kind::Weighted(vec![
            (1. - skip, child),
            (skip, delay(&format!("{name}/skip"), [0.; 2])),
        ]),
    ))
}

struct Creation {
    name: u32,
    entity: String,
    parameters: spawn::Parameters,
}

struct Particles {
    name: u32,
    systems: Vec<String>,
}

/// Why a system a budget leaves out stays pending.
const BUDGET: &str = "System budget. Another native particle system would keep more of the client's system slots alive at the host's creation rate than its share allows";

/// The particle system slots one weapon's repeatedly created contact hosts may hold. The client's
/// particle system pool has 512 slots, and starting a system with none free faults the client. In
/// a captured exhaustion, stock world and event content held 353 and projectile contrails 52, and
/// 105 contact systems ran the pool out. This share is a calibration from that capture.
const CONTACT_SLOTS: f32 = 48.;

/// How long a host's systems hold their slots, as a multiple of the host's own life. In the same
/// capture a Needler firing at most 12 contacts a second held 105 slots with 7 systems a contact,
/// at least 1.25 seconds a contact, while its host lived 0.72 seconds. That ratio, rounded up.
const RESIDENCY: f32 = 2.;

/// The systems one host keeps under a slot budget. A host created `rate` times a second lives its
/// effects' longest `timeline` plus its longest particle life, so about
/// `rate * RESIDENCY * (timeline + life)` hosts hold slots at once, one per system. A survey pass
/// collects every candidate system's particle life, and the build pass admits the shortest lived
/// ones while that estimate stays within [`CONTACT_SLOTS`].
struct Admission {
    rate: f32,
    timeline: f32,
    survey: bool,
    /// The lives the survey met, then the lives the build pass may admit.
    lives: Vec<f32>,
    candidates: usize,
    systems: usize,
    life: f32,
    dropped: usize,
}

impl Admission {
    fn live(&self, systems: usize, life: f32) -> f32 {
        self.rate * RESIDENCY * (self.timeline + life) * systems as f32
    }

    /// Whether the build pass admits a system whose particles live `life` seconds. The survey
    /// pass records the life and admits nothing.
    fn admit(&mut self, life: f32) -> bool {
        if self.survey {
            self.lives.push(life);
            return false;
        }
        let Some(at) = self
            .lives
            .iter()
            .position(|&admitted| admitted.to_bits() == life.to_bits())
        else {
            self.dropped += 1;
            return false;
        };
        self.lives.swap_remove(at);
        self.systems += 1;
        self.life = self.life.max(life);
        true
    }

    /// Ends the survey, keeping the shortest lives the budget holds.
    fn settle(&mut self) {
        let mut lives = std::mem::take(&mut self.lives);
        lives.sort_by(f32::total_cmp);
        self.candidates = lives.len();
        let mut longest = 0f32;
        for life in lives {
            if self.live(self.lives.len() + 1, longest.max(life)) > CONTACT_SLOTS {
                break;
            }
            longest = longest.max(life);
            self.lives.push(life);
        }
        self.survey = false;
    }

    fn report(&self) -> Value {
        json!({"contacts_per_second":self.rate,"slots":CONTACT_SLOTS,"residency":RESIDENCY,
            "host_seconds":self.timeline + self.life,"candidates":self.candidates,
            "systems":self.systems,"dropped":self.dropped,
            "live_systems":self.live(self.systems, self.life)})
    }
}

/// The longest time a finite host plays source effect `effect` before its particle lifetime
/// allowance: each event's longest delay and duration in turn.
fn timeline(effect: &Value) -> Result<f32> {
    let mut seconds = 0f32;
    for event in effect["events"]
        .as_array()
        .context("Source effect events")?
    {
        for field in ["delay_seconds", "duration_seconds"] {
            // Random endpoints may descend.
            let [a, b] = vector::<2>(event, field)?;
            seconds += a.max(b).max(0.);
        }
    }
    Ok(seconds)
}

/// What decides an emitted particle system's content. A full system converts its source emitter
/// into its native definition bytes, which carry the placement frame. A cheap system reads its
/// source with the event's mean duration at the placement frame.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SystemKey {
    Full {
        source: u32,
        emitter: u64,
        definition: Vec<u8>,
    },
    Cheap {
        source: u32,
        duration: u32,
        frame: [u32; 16],
    },
}

struct Compiler<'a> {
    cache: &'a Cache,
    pages: &'a mut Pages,
    native: &'a mut Reader,
    graph: &'a mut Graph,
    scene: &'a Scene,
    fragments: BTreeMap<(u32, [u32; 4]), (String, Value)>,
    /// Emitted particle systems by what decides their content: the source system and emitter, and
    /// the inputs their conversion reads besides the vehicle's fixed scale. Effects at several
    /// damage sections, events or markers often place the same source emitter alike, and one
    /// read-only system serves them all, as stock effects share their systems.
    systems: BTreeMap<SystemKey, (String, Value)>,
    /// Source locations the host's own frame supplies besides `""` and `replace`.
    anchored: &'static [&'static str],
    admission: Option<Admission>,
    /// A contact host's route for sound and light parts. Other hosts leave them pending.
    audio: Option<Audio<'a>>,
}

impl Compiler<'_> {
    fn admit(&mut self, life: f32) -> bool {
        self.admission
            .as_mut()
            .is_none_or(|admission| admission.admit(life))
    }

    fn fragment(&mut self, tag: &Tag, frame: [f32; 4]) -> Result<String> {
        let key = (tag.datum, frame.map(f32::to_bits));
        if let Some((symbol, _)) = self.fragments.get(&key) {
            return Ok(symbol.clone());
        }
        let (symbol, report) = fragment::build(
            self.cache,
            self.pages,
            self.native,
            self.graph,
            tag,
            self.scene.scale,
            frame,
        )?;
        self.fragments.insert(key, (symbol.clone(), report));
        Ok(symbol)
    }

    fn poses(
        &self,
        effect: &Value,
        part: &Value,
        anchor: ([f32; 3], [f32; 4]),
    ) -> Result<Vec<([f32; 3], [f32; 4])>> {
        let location = part["location"]
            .as_i64()
            .context("Source object location")?;
        let marker = if location < 0 {
            ""
        } else {
            effect["locations"][usize::try_from(location)?]["name"]
                .as_str()
                .context("Source effect marker")?
        };
        let model = &self.scene.models[0].model;
        let mut poses = Vec::new();
        // Reach's special "replace" location uses the marker supplied by the
        // effect caller, including the model's damage-transition marker.
        if matches!(marker, "" | "replace") || self.anchored.contains(&marker) {
            poses.push(anchor);
        } else {
            for source in model.markers.iter().filter(|source| source.name == marker) {
                if model.marker_selected(source)? {
                    poses.push(seat::marker_pose(model, source)?);
                }
            }
        }
        // A part placed at a marker the selected model doesn't carry, such as the large truck
        // cab's fx_hood_damage, plays at the response's own anchor.
        if poses.is_empty() {
            poses.push(anchor);
        }
        let offset = if part["relative_offset_source"].is_null() {
            [0.; 3]
        } else {
            vector::<3>(part, "relative_offset_source")?.map(|value| value * self.scene.metres())
        };
        let inverse = rig::inverse_rigid(&rig::transform(anchor.0, anchor.1));
        let conjugate = [-anchor.1[0], -anchor.1[1], -anchor.1[2], anchor.1[3]];
        poses
            .into_iter()
            .map(|(position, orientation)| {
                let world = rig::point(&rig::transform(position, orientation), offset, true);
                Ok((
                    rig::point(&inverse, world, true),
                    rig::quaternion(product(conjugate, orientation))?,
                ))
            })
            .collect()
    }

    /// The native systems of cheap particle emitter part `part_index`, one per marker, or the
    /// reasons it stays pending.
    fn cheap(
        &mut self,
        (effect, event): (&Value, &Value),
        source: &Tag,
        (prefix, event_index, part_index): (&str, usize, usize),
        anchor: ([f32; 3], [f32; 4]),
        parts: &mut Parts,
    ) -> Result<()> {
        let part = &event["parts"][part_index];
        let mut reasons = particle::cheap::pending(self.cache, source)?;
        reasons.extend(dynamic_location(effect, part)?);
        if part["environment"] != 0
            || part["violence"] != 0
            || part["camera"] != 0
            || part["secondary_location"] != -1
            || part["flags"] != 0
            || part["scale_a"] != 0
            || part["scale_b"] != 0
        {
            reasons.push("Conditional, flagged or scaled cheap particle part".into());
        }
        if !reasons.is_empty() {
            parts
                .pending
                .push(json!({"event":event_index,"part":part_index,
                "source":source,"reasons":reasons}));
            return Ok(());
        }
        let [shortest, longest] = vector::<2>(event, "duration_seconds")?;
        let orientation = angles(vector(part, "relative_orientation_radians")?)?;
        for (marker_index, (position, rotation)) in
            self.poses(effect, part, anchor)?.into_iter().enumerate()
        {
            let frame = rig::transform(position, rig::quaternion(product(rotation, orientation))?);
            let duration = 0.5 * (shortest + longest);
            let cheap = particle::cheap::read(
                self.cache,
                self.pages,
                self.native,
                source,
                duration,
                self.scene.metres(),
                &frame,
            )?;
            if !self.admit(cheap.life) {
                parts
                    .pending
                    .push(json!({"event":event_index,"part":part_index,
                    "marker":marker_index,"source":source,"reasons":[BUDGET]}));
                continue;
            }
            let key = SystemKey::Cheap {
                source: source.datum,
                duration: duration.to_bits(),
                frame: frame.map(f32::to_bits),
            };
            let (symbol, report) = match self.systems.get(&key) {
                Some(shared) => shared.clone(),
                None => {
                    let name =
                        format!("{prefix}-event-{event_index}-cheap-{part_index}-{marker_index}");
                    let emitted = particle::cheap::emit(
                        self.cache,
                        self.pages,
                        self.native,
                        self.graph,
                        &cheap,
                        &name,
                    )?;
                    self.systems.insert(key, emitted.clone());
                    emitted
                }
            };
            parts.life = parts.life.max(cheap.life);
            parts.systems.push(symbol);
            parts.reports.push(report);
        }
        Ok(())
    }

    /// The once-sound or light event of sound, light or cheap light part `part`, its node named
    /// under `name` and its graph nodes under `symbol`. A sound takes a [`contact_cue`] from its
    /// exported recordings. A light takes a private copy of the finish light in the source color,
    /// with a cheap light's radius, for the source light's lifetime or else its effect event's
    /// mean duration, at least one frame at 30 hertz. Every part plays at the host's contact point.
    /// A part that depends on its environment, violence mode or camera, or that sits at a source
    /// marker the host lacks, is refused with its reason.
    fn present(
        &mut self,
        audio: Audio<'_>,
        (effect, event, part): (&Value, &Value, &Value),
        source: &Tag,
        (name, symbol): (&str, &str),
    ) -> Result<Presented> {
        ensure!(
            part["environment"] == 0
                && part["violence"] == 0
                && part["camera"] == 0
                && part["secondary_location"] == -1,
            "Conditional or beam part"
        );
        if let Some(reason) = dynamic_location(effect, part)? {
            anyhow::bail!(reason);
        }
        let location = part["location"].as_i64().context("Source part location")?;
        let marker = if location < 0 {
            ""
        } else {
            effect["locations"][usize::try_from(location)?]["name"]
                .as_str()
                .context("Source part location name")?
        };
        ensure!(
            matches!(marker, "" | "replace") || self.anchored.contains(&marker),
            "The part sits at source marker {marker}, which the contact host lacks"
        );
        let mut limits = vec!["The part plays at the contact point."];
        if !part["relative_offset_source"].is_null()
            && vector::<3>(part, "relative_offset_source")? != [0.; 3]
        {
            limits.push("Its source offset from the contact is not carried.");
        }
        if source.group == "snd!" {
            let node = &audio.behavior["nodes"][format!("{:08X}", source.datum)];
            ensure!(
                node["tag"]["group"] == "snd!",
                "The sound is outside the behavior closure, so its recordings were not exported"
            );
            ensure!(
                audio.recordings.is_dir(),
                "Source audio is not configured for this import, so no recording was exported"
            );
            let (cue, report) = contact_cue(
                self.native,
                self.graph,
                node,
                symbol,
                &format!("{}/{name}", audio.range),
                audio.recordings,
            )?;
            return Ok(Presented {
                name: hash(&format!("{name}/sound")),
                class: SOUND_EVENT,
                resource: cue,
                report: json!({"source":source,"status":"translated","cue":report,"limits":limits}),
            });
        }
        let lit = light::source(self.cache, source, self.scene.metres())?;
        let [short, long] = vector::<2>(event, "duration_seconds")?;
        let seconds = if lit.seconds > 0. {
            lit.seconds
        } else {
            0.5 * (short + long)
        }
        .max(1. / 30.);
        let (placed, report) = light::template(self.native, lit.color)?
            .reach(lit.radius)?
            .add_named(self.native, self.graph, seconds, symbol)?;
        limits.push("The light keeps the finish light's brightness. The source intensity curve is not carried.");
        Ok(Presented {
            name: hash(&format!("{name}/light")),
            class: LIGHT_EVENT,
            resource: placed.symbol,
            report: json!({"source":source,"status":"translated","linear_color":lit.color,
                "seconds":seconds,"radius_metres":lit.radius,"native":report,"limits":limits}),
        })
    }

    fn parts(
        &mut self,
        (effect, event): (&Value, &Value),
        (prefix, event_name): (&str, &str),
        event_index: usize,
        anchor: ([f32; 3], [f32; 4]),
    ) -> Result<Parts> {
        let mut result = Parts {
            children: Vec::new(),
            creations: Vec::new(),
            pending: Vec::new(),
            systems: Vec::new(),
            reports: Vec::new(),
            life: 0.,
            presented: Vec::new(),
        };
        for (part_index, part) in event["parts"]
            .as_array()
            .context("Source event parts")?
            .iter()
            .enumerate()
        {
            if part["tag"].is_null() {
                continue;
            }
            let source: Tag = serde_json::from_value(part["tag"].clone())?;
            if matches!(source.group.as_str(), "snd!" | "ligh" | "gldf")
                && let Some(audio) = self.audio
            {
                // The budget survey places no graph node.
                if self
                    .admission
                    .as_ref()
                    .is_some_and(|admission| admission.survey)
                {
                    continue;
                }
                let start = self.graph.nodes.len();
                match self.present(
                    audio,
                    (effect, event, part),
                    &source,
                    (
                        &format!("{event_name}/part/{part_index}"),
                        &format!("{prefix}-event-{event_index}-part-{part_index}"),
                    ),
                ) {
                    Ok(presented) => {
                        result.children.push(Node {
                            name: presented.name,
                            timing: if presented.class == LIGHT_EVENT {
                                Timing {
                                    duration: [presented.report["seconds"].as_f64().unwrap_or(0.)
                                        as f32; 2],
                                    ..Timing::default()
                                }
                            } else {
                                Timing::default()
                            },
                            kind: Kind::Event(presented.class),
                        });
                        result.presented.push(presented);
                    }
                    Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
                    Err(error) => {
                        self.graph.nodes.truncate(start);
                        result
                            .pending
                            .push(json!({"event":event_index,"part":part_index,
                            "source":source,"reasons":[format!("{error:#}")]}));
                    }
                }
                continue;
            }
            if source.group == "cpem" {
                self.cheap(
                    (effect, event),
                    &source,
                    (prefix, event_index, part_index),
                    anchor,
                    &mut result,
                )
                .with_context(|| format!("Cheap particle emitter {}", source.path))?;
                continue;
            }
            let Parts {
                children,
                creations,
                pending,
                ..
            } = &mut result;
            if source.group != "bloc" {
                pending.push(
                    json!({"event":event_index,"part":part_index,"source":source,
                    "reasons":[untranslated(&source.group)]}),
                );
                continue;
            }
            if let Some(reason) = dynamic_location(effect, part)? {
                pending.push(json!({"event":event_index,"part":part_index,
                    "source":source,"reasons":[reason]}));
                continue;
            }
            ensure!(
                part["environment"] == 0
                    && part["violence"] == 0
                    && part["camera"] == 0
                    && part["secondary_location"] == -1
                    && part["scale_a"] == 0
                    && part["scale_b"] == 0,
                "Detached source object has an unsupported conditional or scaling policy"
            );
            let flags = part["flags"].as_u64().context("Detached source flags")?;
            ensure!(
                flags & !0x40 == 0,
                "Detached source object flags need translation"
            );
            // ReachMCC marks Velocity Orientation unused. Its object creation
            // branch reads the separate relative orientation for the visible frame.
            let frame = angles(vector(part, "relative_orientation_radians")?)?;
            let entity = self
                .fragment(&source, frame)
                .with_context(|| format!("Detached object {}", source.path))?;
            let speed =
                vector::<2>(part, "velocity_source")?.map(|value| value * self.scene.metres());
            let angular = vector::<2>(part, "angular_velocity_radians")?;
            let frame = rig::transform([0.; 3], frame);
            // MCC's object branch at 00120A39 retains the placement scale of one
            // initialized by 0049CC24. The part's radius modifier is not object scale.
            let scale = [1.; 2];
            // A cone of a half turn or more launches in every direction, which the native
            // cone's widest bound already does.
            let cone = (part["velocity_cone_radians"]
                .as_f64()
                .context("Source debris cone")? as f32)
                .min(std::f32::consts::PI);
            for (marker_index, (position, rotation)) in
                self.poses(effect, part, anchor)?.into_iter().enumerate()
            {
                let name = format!("{event_name}/part/{part_index}/marker/{marker_index}");
                let event = node(&name, Kind::Event(0x80808881));
                creations.push(Creation {
                    name: event.name,
                    entity: entity.clone(),
                    parameters: spawn::Parameters {
                        locator: 0,
                        position,
                        rotation,
                        scale,
                        speed,
                        cone,
                        angular_min: rig::point(&frame, [angular[0], 0., 0.], false),
                        angular_max: rig::point(&frame, [angular[1], 0., 0.], false),
                    },
                });
                children.push(event);
            }
        }
        Ok(result)
    }

    /// The scheduled events of source effect `tag`, with node names under `prefix`, or the
    /// effect's report when it has nothing a host can play.
    #[allow(clippy::cognitive_complexity)]
    fn compile(
        &mut self,
        tag: &Tag,
        prefix: &str,
        anchor: ([f32; 3], [f32; 4]),
        skip: f32,
        parent_marker: Option<u32>,
    ) -> Result<std::result::Result<Compiled, Value>> {
        ensure!(
            tag.group == "effe",
            "Detached-piece source is not an effect"
        );
        let effect = source::read(self.cache, tag.address()?)?;
        let loop_start = effect["loop_start"]
            .as_i64()
            .context("Source effect loop entry")?;
        if loop_start >= 0 && parent_marker.is_none() {
            return Ok(Err(
                json!({"source":tag,"emitted_objects":0,"emitted_particle_systems":0,
                "pending":[{"status":"Looping damage effects require health-state retirement and repeat activation","effect":effect}]}),
            ));
        }
        let mut scheduled = Vec::new();
        let mut creations = Vec::new();
        let mut particle_events = Vec::new();
        let mut particle_reports = Vec::new();
        let mut maximum_life = 0f32;
        let mut pending = Vec::new();
        let mut presented = Vec::new();
        for (event_index, event) in effect["events"]
            .as_array()
            .context("Source effect events")?
            .iter()
            .enumerate()
        {
            let event_name = format!("{prefix}/event/{event_index}");
            let Parts {
                mut children,
                creations: objects,
                pending: omitted,
                mut systems,
                reports,
                life,
                presented: played,
            } = self.parts((&effect, event), (prefix, &event_name), event_index, anchor)?;
            creations.extend(objects);
            pending.extend(omitted);
            presented.extend(played);
            particle_reports.extend(reports);
            maximum_life = maximum_life.max(life);
            for (system_index, system) in event["particles"]
                .as_array()
                .context("Source particle systems")?
                .iter()
                .enumerate()
            {
                if system["tag"].is_null() {
                    continue;
                }
                if let Some(reason) = dynamic_location(&effect, system)? {
                    pending.push(json!({"event":event_index,"system":system_index,
                        "source":system["tag"],"reasons":[reason]}));
                    continue;
                }
                for (emitter_index, emitter) in system["emitters"]
                    .as_array()
                    .context("Source particle emitters")?
                    .iter()
                    .enumerate()
                {
                    let reasons = particle::pending(self.cache, system, emitter)?;
                    if !reasons.is_empty() {
                        pending.push(json!({"event":event_index,"system":system_index,"emitter":emitter_index,
                            "source":system["tag"],"reasons":reasons}));
                        continue;
                    }
                    for (marker_index, (position, rotation)) in
                        self.poses(&effect, system, anchor)?.into_iter().enumerate()
                    {
                        let frame = rig::transform(position, rotation);
                        let parsed = particle::read(
                            self.cache,
                            self.native,
                            system,
                            emitter,
                            self.scene.metres(),
                            &frame,
                        )?;
                        if !self.admit(parsed.life) {
                            pending.push(json!({"event":event_index,"system":system_index,
                                "emitter":emitter_index,"marker":marker_index,
                                "source":system["tag"],"reasons":[BUDGET]}));
                            continue;
                        }
                        maximum_life = maximum_life.max(parsed.life);
                        let key = SystemKey::Full {
                            source: parsed.source.datum,
                            emitter: emitter["offset"]
                                .as_u64()
                                .context("Source emitter address")?,
                            definition: parsed.definition.0.clone(),
                        };
                        let (symbol, report) = match self.systems.get(&key) {
                            Some(shared) => shared.clone(),
                            None => {
                                let name = format!(
                                    "{prefix}-event-{event_index}-particle-{system_index}-{emitter_index}-{marker_index}"
                                );
                                let emitted = particle::emit(
                                    self.cache,
                                    self.pages,
                                    self.native,
                                    self.graph,
                                    &parsed,
                                    &name,
                                )?;
                                self.systems.insert(key, emitted.clone());
                                emitted
                            }
                        };
                        systems.push(symbol);
                        particle_reports.push(report);
                    }
                }
            }
            if !systems.is_empty() {
                let name = hash(&format!("{event_name}/particles"));
                children.push(Node {
                    name,
                    timing: Timing {
                        duration: vector(event, "duration_seconds")?,
                        flags: 4,
                        ..Timing::default()
                    },
                    kind: Kind::Event(0x80806cc6),
                });
                particle_events.push(Particles { name, systems });
            }
            // Empty events still advance the source event clock.
            children.push(delay(
                &format!("{event_name}/duration"),
                vector(event, "duration_seconds")?,
            ));
            let actions = chance(
                &format!("{event_name}/choice"),
                event["skip_fraction"]
                    .as_f64()
                    .context("Source event probability")? as f32,
                node(&format!("{event_name}/parallel"), Kind::Parallel(children)),
            )?;
            scheduled.push(node(
                &format!("{event_name}/cycle"),
                Kind::Serial(vec![
                    delay(
                        &format!("{event_name}/delay"),
                        vector(event, "delay_seconds")?,
                    ),
                    actions,
                ]),
            ));
        }
        if creations.is_empty() && particle_events.is_empty() && presented.is_empty() {
            return Ok(Err(
                json!({"source":tag,"pending":pending,"emitted_objects":0,"emitted_particle_systems":0}),
            ));
        }
        repeat(&mut scheduled, &effect, prefix, loop_start)?;
        let content = chance(
            &format!("{prefix}/response-choice"),
            skip,
            node(&format!("{prefix}/source"), Kind::Serial(scheduled)),
        )?;
        let parts = presented
            .iter()
            .map(|part| part.report.clone())
            .collect::<Vec<_>>();
        let report = json!({"source":tag,"emitted_objects":creations.len(),"pending":pending,
            "emitted_particle_systems":particle_reports.len(),"particles":particle_reports,"maximum_particle_life":maximum_life,
            "sound_and_light_parts":parts,
            "response_skip_fraction":skip,"loop_start":loop_start,"managed":parent_marker.is_some(),"gameplay_verified":false,
            "limits":["Effect-local marker offsets use the source bind pose relative to the live damage marker. Source spin magnitudes use the native local forward axis. Cone bounds use the native polar distribution. Source first-person suppression and exact random streams remain separate acceptance boundaries."]});
        Ok(Ok(Compiled {
            content,
            creations,
            particles: particle_events,
            presented,
            life: maximum_life,
            report,
        }))
    }

    /// A private host entity, its graph nodes named by `prefix`, that plays `effects` together at
    /// `origin`, and beside them a once-sound event for each of `sounds`, an event name and a cue
    /// symbol. A finite host retires once the longest particle life has passed after its effects
    /// end. A managed host, the one a damage marker anchors, plays a single effect until its health
    /// slot changes.
    fn write(
        &mut self,
        prefix: &str,
        effects: Vec<Compiled>,
        sounds: &[(&str, String)],
        origin: Origin,
    ) -> Result<String> {
        let managed = matches!(origin, Origin::Marker(_));
        ensure!(
            !effects.is_empty() && (!managed || (effects.len() == 1 && sounds.is_empty())),
            "Effect host content differs"
        );
        let maximum_life = effects.iter().map(|e| e.life).fold(0f32, f32::max);
        let mut creations = Vec::new();
        let mut particle_events = Vec::new();
        let mut contents = Vec::new();
        // The host's own once-sounds, then each part's once-sound or light.
        let mut presented = sounds
            .iter()
            .map(|(name, cue)| {
                (
                    hash(&format!("{prefix}/sound/{name}")),
                    SOUND_EVENT,
                    cue.clone(),
                )
            })
            .collect::<Vec<_>>();
        for effect in effects {
            creations.extend(effect.creations);
            particle_events.extend(effect.particles);
            presented.extend(
                effect
                    .presented
                    .into_iter()
                    .map(|part| (part.name, part.class, part.resource)),
            );
            contents.push(effect.content);
        }
        contents.extend(
            sounds
                .iter()
                .map(|(name, _)| node(&format!("{prefix}/sound/{name}"), Kind::Event(SOUND_EVENT))),
        );
        let content = if contents.len() == 1 {
            contents.remove(0)
        } else {
            node(&format!("{prefix}/effects"), Kind::Parallel(contents))
        };
        let root = if managed {
            node(&format!("{prefix}/root"), Kind::Serial(vec![content]))
        } else {
            node(
                &format!("{prefix}/root"),
                Kind::Serial(vec![
                    content,
                    delay(&format!("{prefix}/particle-lifetime"), [maximum_life; 2]),
                    node(&format!("{prefix}/retire"), Kind::Event(0x80804ab0)),
                ]),
            )
        };
        let mut bundle = Bundle::read(self.native, if managed { 0x80c0d93e } else { 0x80c0dc18 })?;
        ensure!(
            bundle.entity().u16(0x96)? == 17,
            "Damage effect host kind differs"
        );
        let owners = bundle
            .owners
            .iter()
            .filter_map(|(&tag, payload)| {
                let d = payload.pointer(24).ok()?;
                (payload.u32(d - 4).ok()? == 0x808084e9).then_some(tag)
            })
            .collect::<Vec<_>>();
        ensure!(
            owners.len() == 1,
            "Damage effect host sequence is absent or ambiguous"
        );
        let owner = owners[0];
        let mut envelopes = vec![bundle.owners[&owner].clone()];
        for tag in [0x815b901a, 0x80bc308d, 0x80fc7dfb] {
            envelopes.push((*self.native.tag(tag, Some(0x80809c36))?).clone());
        }
        // A light event's instance and body come from the stock flow whose light event names the
        // finish light record its private copies are made from.
        let lit = presented.iter().any(|&(_, class, _)| class == LIGHT_EVENT);
        let light_flow = if lit {
            let flow = (*self.native.tag(light::EVENT_FLOW, Some(0x80809c36))?).clone();
            envelopes.push(flow.clone());
            Some(flow)
        } else {
            None
        };
        let payload = bundle.owners.get_mut(&owner).unwrap();
        host_locators(
            payload,
            &mut bundle.allocations.get_mut(&owner).unwrap().1,
            origin,
            owner,
        )?;
        let written = sequence::write(
            payload,
            &mut bundle.allocations.get_mut(&owner).unwrap().1,
            &root,
            &envelopes,
        )?;
        for creation in &creations {
            let field = spawn::write(
                payload,
                written.events[&creation.name],
                &creation.parameters,
            )
            .with_context(|| format!("Object creation in {prefix}"))?;
            bundle
                .resources
                .entry(owner)
                .or_default()
                .insert(field, creation.entity.clone());
        }
        for particles in &particle_events {
            for (field, symbol) in sequence::particle::write(
                payload,
                written.events[&particles.name],
                &particles.systems,
                0,
            )? {
                bundle
                    .resources
                    .entry(owner)
                    .or_default()
                    .insert(field, symbol);
            }
        }
        presentation(
            payload,
            &written,
            &presented,
            light_flow.as_ref(),
            bundle.resources.entry(owner).or_default(),
        )?;
        // Kind 38 uses +42 to delete its selected object. With selector FF at
        // +38, 0058E590 selects this flow's host. Zero flags would do nothing.
        if !managed {
            let retire = written.events[&hash(&format!("{prefix}/retire"))];
            payload.0[retire + 0x42] = 1;
        }
        bundle.seal(owner)?;
        bundle.emit(self.graph, prefix)
    }

    fn effect(
        &mut self,
        tag: &Tag,
        prefix: &str,
        anchor: ([f32; 3], [f32; 4]),
        skip: f32,
        parent_marker: Option<u32>,
    ) -> Result<(Option<String>, Value)> {
        match self.compile(tag, prefix, anchor, skip, parent_marker)? {
            Err(report) => Ok((None, report)),
            Ok(compiled) => {
                let report = compiled.report.clone();
                let origin = parent_marker.map_or(Origin::Creation, Origin::Marker);
                let entity = self.write(prefix, vec![compiled], &[], origin)?;
                Ok((Some(entity), report))
            }
        }
    }
}

/// Names each sound's cue or light's record in the event [`Compiler::write`] wrote for it in host
/// flow `payload`, by recording the event's resource field in `resources`. A once-sound event's
/// written body already matches the stock event's apart from its cue, and plays at locator 0, the
/// host's origin. A light event takes the body of `light_flow`'s light event that names the stock
/// finish light record, at locator 0, as [`sequence::light::write`] describes.
fn presentation(
    payload: &mut Payload,
    written: &sequence::Written,
    presented: &[(u32, u32, String)],
    light_flow: Option<&Payload>,
    resources: &mut BTreeMap<usize, String>,
) -> Result<()> {
    for (name, class, resource) in presented {
        let event = written.events[name];
        if *class == LIGHT_EVENT {
            let flow = light_flow.context("Host light event has no stock light flow")?;
            let field = sequence::light::write(payload, event, flow, light::TEMPLATE, 0)?;
            resources.insert(field, resource.clone());
            continue;
        }
        let instance = usize::try_from(payload.u64(event + 8)?)?;
        ensure!(
            payload.u32(event + 0x38)? == 0xff
                && payload.u64(event + 0x48)? == 0
                && payload.u8(event + 0x70)? == 0
                && payload.u32(instance - 4)? == 0x80806b37,
            "Host sound event differs"
        );
        put(&mut payload.0, event + 0x40, &u32::MAX.to_le_bytes())?;
        put(&mut payload.0, instance + 0x40, &u32::MAX.to_le_bytes())?;
        resources.insert(event + 0x40, resource.clone());
    }
    Ok(())
}

/// One source effect's scheduled events and the objects and particle systems they create, before
/// a host plays them.
struct Compiled {
    content: Node,
    creations: Vec<Creation>,
    particles: Vec<Particles>,
    presented: Vec<Presented>,
    life: f32,
    report: Value,
}

/// A private host entity, its graph nodes named by `prefix`, that plays source `effects` together
/// at the contact point an impact table creates it for, on their own event timing, and retires
/// once their particles end. Each effect is a tag and the prefix of its event names. The `normal`,
/// `reflection`, `forward` and `backward` locations, the frames a collision supplies, are that
/// point's frame. That the native contact frame stands for them is inferred, and `forward` and
/// `backward` share it although the source opposes them. Each
/// effect's report follows, with the error of one that fails to convert, and no host when no
/// effect has a playable part. Graph nodes a failed effect added before its error stay unnamed by
/// the host. A host created `rate` times a second keeps the particle systems an [`Admission`]
/// admits and reports the rest as pending, and the admission report follows.
///
/// A written host also plays each of `sounds`, a source sound behavior node and an event name,
/// once when it is created. Each takes a [`contact_cue`] made from the recordings in `recordings`,
/// in the namespace range named by `range` and its event name. A sound that does not convert
/// reports its error and leaves the graph unchanged. The effects' own sound parts play the same
/// way within their events, from their nodes in the behavior closure `behavior`, and their light
/// and cheap light parts as native light events, as [`Compiler::present`] describes.
#[allow(clippy::too_many_arguments)]
pub(in crate::halo_reach) fn host(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    prefix: &str,
    effects: &[(&Tag, &str)],
    (sounds, behavior, recordings, range): (&[(&Value, &str)], &Value, &Path, &str),
    rate: Option<f32>,
) -> Result<Hosted> {
    let mut longest = 0f32;
    for &(tag, _) in effects {
        // An effect that does not read reports its error when it compiles.
        if let Ok(effect) = tag.address().and_then(|at| source::read(cache, at)) {
            longest = longest.max(timeline(&effect)?);
        }
    }
    let mut compiler = Compiler {
        cache,
        pages,
        native,
        graph,
        scene,
        fragments: BTreeMap::new(),
        systems: BTreeMap::new(),
        anchored: &["normal", "reflection", "forward", "backward"],
        audio: Some(Audio {
            behavior,
            recordings,
            range,
        }),
        admission: rate.map(|rate| Admission {
            rate,
            timeline: longest,
            survey: true,
            lives: Vec::new(),
            candidates: 0,
            systems: 0,
            life: 0.,
            dropped: 0,
        }),
    };
    if compiler.admission.is_some() {
        // The survey emits no particle system. Its object fragments stay cached for the build.
        for &(tag, name) in effects {
            if let Err(error) = compiler.compile(tag, name, ([0.; 3], [0., 0., 0., 1.]), 0., None)
                && crate::cancellation::is_cancelled(&error)
            {
                return Err(error);
            }
        }
        if let Some(admission) = &mut compiler.admission {
            admission.settle();
        }
    }
    let mut compiled = Vec::new();
    let mut reports = Vec::new();
    for &(tag, name) in effects {
        match compiler.compile(tag, name, ([0.; 3], [0., 0., 0., 1.]), 0., None) {
            Ok(Ok(effect)) => {
                reports.push(effect.report.clone());
                compiled.push(effect);
            }
            Ok(Err(report)) => reports.push(report),
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => reports.push(json!({"source":tag,"emitted_objects":0,
                "emitted_particle_systems":0,"error":format!("{error:#}")})),
        }
    }
    let budget = compiler
        .admission
        .as_ref()
        .map_or(Value::Null, Admission::report);
    if compiled.is_empty() {
        return Ok(Hosted {
            entity: None,
            effects: reports,
            sounds: Vec::new(),
            budget,
        });
    }
    let (cues, played) = host_sounds(
        compiler.native,
        compiler.graph,
        prefix,
        sounds,
        (recordings, range),
    )?;
    Ok(Hosted {
        entity: Some(compiler.write(prefix, compiled, &cues, Origin::Contact)?),
        effects: reports,
        sounds: played,
        budget,
    })
}

/// The contact cue of each of host `prefix`'s `sounds` that converts, by event name, and each
/// sound's cue report or error.
fn host_sounds<'a>(
    native: &mut Reader,
    graph: &mut Graph,
    prefix: &str,
    sounds: &[(&Value, &'a str)],
    (recordings, range): (&Path, &str),
) -> Result<DamageCues<'a>> {
    let mut cues = Vec::new();
    let mut reports = Vec::new();
    for &(source, name) in sounds {
        match contact_cue(
            native,
            graph,
            source,
            &format!("{prefix}-sound-{name}"),
            &format!("{range}/{name}"),
            recordings,
        ) {
            Ok((cue, report)) => {
                cues.push((name, cue));
                reports.push(report);
            }
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => {
                reports.push(json!({"source":source["tag"],"error":format!("{error:#}")}));
            }
        }
    }
    Ok((cues, reports))
}

/// What [`host`] wrote: the host entity's symbol, if any, each effect's report, each sound's cue
/// report or error in the order of its sounds when a host was written, and the particle system
/// budget's report.
pub(in crate::halo_reach) struct Hosted {
    pub entity: Option<String>,
    pub effects: Vec<Value>,
    pub sounds: Vec<Value>,
    pub budget: Value,
}

/// The permutations a response leaves: the intact selection with that response's own
/// region transitions applied. A destroyed state can carry markers the intact model lacks,
/// and a hidden region carries none.
fn response_selection(scene: &Scene, response: &Value) -> Result<BTreeMap<String, String>> {
    let mut selection = scene.models[0].model.selected.clone();
    let Some(damage) = &scene.damage else {
        return Ok(selection);
    };
    for transition in response["regions"]
        .as_array()
        .context("Source region transitions")?
    {
        let name = transition["region"]
            .as_str()
            .context("Source transition region")?;
        // A compiled response can keep a region the model no longer has, with no runtime
        // index. The threshold writer skips it as well.
        if transition["runtime_index"] == -1 {
            continue;
        }
        let state = usize::try_from(
            transition["state"]
                .as_i64()
                .context("Source damage state ordinal")?,
        )?;
        let region = damage
            .regions
            .iter()
            .find(|region| region.name == name)
            .context("Source transition region is absent from the damage geometry")?;
        let permutation = region
            .states
            .get(state)
            .context("Source damage state ordinal")?;
        selection.insert(name.to_owned(), permutation.clone());
    }
    Ok(selection)
}

pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    assembly: &mut Assembly,
    scene: &Scene,
    bones: &Value,
) -> Result<Effects> {
    let mut compiler = Compiler {
        cache,
        pages,
        native,
        graph,
        scene,
        fragments: BTreeMap::new(),
        systems: BTreeMap::new(),
        anchored: &[],
        admission: None,
        audio: None,
    };
    let sections = scene.object.gameplay["damage"]["sections"]
        .as_array()
        .context("Source damage sections")?;
    let first = usize::try_from(
        bones["source_bone_first"]
            .as_u64()
            .context("Damage effect source rig")?,
    )?;
    let mut result = Effects::default();
    let mut markers = Vec::new();
    let mut report = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        if section["responses"]
            .as_array()
            .context("Source damage responses")?
            .iter()
            .any(|response| !response["looping_effect"].is_null())
        {
            result.slots.insert(section_index, result.slots.len());
        }
        for (response_index, response) in section["responses"]
            .as_array()
            .context("Source damage responses")?
            .iter()
            .enumerate()
        {
            if response["flags"]
                .as_u64()
                .context("Source response flags")?
                & 0x10000
                != 0
            {
                continue;
            }
            for (field, marker_field) in [
                ("generic_effect", Some("generic_marker")),
                ("specific_effect", Some("specific_marker")),
                ("looping_effect", None),
            ] {
                if response[field].is_null() {
                    continue;
                }
                let tag: Tag = serde_json::from_value(response[field].clone())?;
                if tag.group != "effe" {
                    report.push(json!({"section":section_index,"response":response_index,"field":field,"source":tag,"status":"Non-effect damage reference"}));
                    continue;
                }
                let (source_marker, placements) =
                    response_placements(scene, response, marker_field, first)?;
                if placements.is_empty() {
                    report.push(json!({"section":section_index,"response":response_index,"field":field,"source":tag,"marker":source_marker,"status":"Source marker is absent"}));
                    continue;
                }
                let count = placements.len();
                for (placement, (anchor, bone)) in placements.into_iter().enumerate() {
                    let prefix = if count == 1 {
                        format!("damage-{section_index}-{response_index}-{field}")
                    } else {
                        format!("damage-{section_index}-{response_index}-{field}-{placement}")
                    };
                    let marker = hash(&format!("source/{prefix}"));
                    let (entity, effect) = compiler.effect(
                        &tag,
                        &prefix,
                        anchor,
                        response["skip_fraction"]
                            .as_f64()
                            .context("Source damage probability")? as f32,
                        marker_field.is_none().then_some(marker),
                    )?;
                    if let Some(entity) = entity {
                        let (position, orientation) = seat::frame(bones, bone, anchor)?;
                        markers.push(markers::Marker {
                            name: marker,
                            binding: [1, u32::try_from(bone)?],
                            position: [position[0], position[1], position[2], 1.],
                            orientation,
                            row: 0,
                        });
                        if marker_field.is_none() {
                            result.loops.insert((section_index, response_index), entity);
                        } else {
                            result
                                .emissions
                                .entry((section_index, response_index))
                                .or_default()
                                .push(Emission { entity, marker });
                        }
                    }
                    report.push(json!({"section":section_index,"response":response_index,"field":field,"placement":placement,"effect":effect}));
                }
            }
        }
    }
    if !markers.is_empty() {
        let owner = assembly.owner(markers::NATIVE_COMPONENT)?;
        let payload = markers::append_skeletal(
            &assembly.payloads[&owner],
            &markers,
            bones["bones"]
                .as_array()
                .context("Damage effect target rig")?
                .len(),
        )?;
        assembly.payloads.insert(owner, payload);
    }
    result.report = json!({"effects":report,"managed_slots":result.slots,"fragments":compiler.fragments.into_values().map(|(_, report)| report).collect::<Vec<_>>(),
        "gameplay_verified":false});
    Ok(result)
}

type DamageCues<'a> = (Vec<(&'a str, String)>, Vec<Value>);

type DamagePlacement = (([f32; 3], [f32; 4]), usize);

fn response_placements<'a>(
    scene: &Scene,
    response: &'a Value,
    marker_field: Option<&str>,
    first: usize,
) -> Result<(&'a str, Vec<DamagePlacement>)> {
    let source_marker = match marker_field {
        Some(field) => response[field].as_str().context("Source damage marker")?,
        None => "",
    };
    let model = &scene.models[0].model;
    // A source effect plays at every marker sharing its name. A name with
    // no marker under the response's selection places nothing.
    let placements = if source_marker.is_empty() {
        vec![(([0.; 3], [0., 0., 0., 1.]), first)]
    } else {
        let selection = response_selection(scene, response)?;
        seat::markers_in(model, source_marker, &selection)?
            .into_iter()
            .map(|marker| {
                Ok((
                    seat::marker_pose(model, marker)?,
                    marker.bone.map_or(first, |bone| first + bone),
                ))
            })
            .collect::<Result<Vec<_>>>()?
    };
    Ok((source_marker, placements))
}
