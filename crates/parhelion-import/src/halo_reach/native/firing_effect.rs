//! The source barrel's firing effect in place of the donor's muzzle particles.
//!
//! A Shadowkeep weapon fires its presentation through the content row of its unnamed audio
//! component (`80803AC9`), whose parent (`8080744A`) names a root (`80803ADC`) of six entity
//! slots. Slot 0's effect flow (`808084D7` instance, `808084E9` definition) holds the firing
//! sounds beside the muzzle flash, smoke and casing particles and the firing lights. Its particle
//! and light events sit under parallel controls named `1st` and `3rd`, FNV-1 `33D5A42F` and
//! `2AD11966`, and bind marker locators of kind 3 and kind 2 respectively, both naming
//! `primary_trigger`.
//!
//! The rebuilt flow keeps every control and every event other than particle events, which become
//! delays of their own timing so serial and weighted controls keep their timeline and odds. The
//! kept events keep their own definitions. The source effect's particle systems join the root in
//! a group of their own: first-person systems under a `1st` control bound to kind-3 locators,
//! third-person systems under a `3rd` control bound to kind-2 locators, and systems of either
//! camera under both. A system binds the locators of the native marker its source location
//! becomes when the flow declares them for both views, as the stock `ejection` locators name the
//! ejection port the converted `primary_ejection` row carries. Any other system is placed relative
//! to the source muzzle marker, which the native `primary_trigger` row carries. A world-space
//! system binds the flow's `world_space` locator second, as stock casing and smoke events do. Kept
//! firing lights take private records in the source firing light's color, as [`light`] describes.
//! That the control names or locator kinds select the view is inferred from the stock layout. A
//! zooming weapon's first-person systems play under a gate on aim visibility inside the `1st`
//! control, so they stop where its body discards, as [`zoom`] describes.
mod light;
mod trace;
mod zoom;
use super::{marker, seat};
use crate::{
    halo_reach::{
        Scene,
        cache::{Cache, Tag},
        effect::particle,
        resource::Pages,
        rig,
    },
    presentation::Graph,
    tiger::{
        instance,
        payload::Payload,
        reader::Reader,
        sequence::{self, Kind, Node, Timing},
    },
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const PARTICLE: u32 = 0x8080_6cc6;
const DELAY: u32 = 0x8080_93cb;
const SERIAL: u32 = 0x8080_93d9;
const PARALLEL: u32 = 0x8080_93d7;
const RANGE: u32 = 0x8080_93cd;
const WEIGHTED: u32 = 0x8080_93d5;
/// Native flows whose events supply the delay and particle templates the damage effects use.
const ENVELOPES: [u32; 3] = [0x815b_901a, 0x80bc_308d, 0x80fc_7dfb];
/// MCC Reach U13 trigger behavior 6, Spew-Charge, whose shot is its secondary barrel.
const SPEW_CHARGE: i64 = 6;

fn hash(name: &str) -> u32 {
    name.bytes()
        .fold(0x811c9dc5, |h, b| h.wrapping_mul(0x01000193) ^ u32::from(b))
}

fn tag(text: &Value, what: &str) -> Result<u32> {
    Ok(u32::from_str_radix(
        text.as_str().with_context(|| format!("Native {what}"))?,
        16,
    )?)
}

fn pair(value: &Value, field: &str) -> Result<[f32; 2]> {
    let rows = value[field]
        .as_array()
        .filter(|rows| rows.len() == 2)
        .with_context(|| format!("Source effect {field}"))?;
    let mut out = [0.; 2];
    for (result, row) in out.iter_mut().zip(rows) {
        *result = row.as_f64().context("Source effect number")? as f32;
        ensure!(
            result.is_finite() && *result >= 0.,
            "Invalid source effect time"
        );
    }
    Ok(out)
}

/// The barrel the primary trigger fires as its shot.
fn barrel(behavior: &Value) -> Result<Option<usize>> {
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let Some(trigger) =
        behavior["nodes"][format!("{root:08X}")]["data"]["object"]["gameplay"]["triggers"]
            .as_array()
            .and_then(|triggers| triggers.first())
    else {
        return Ok(None);
    };
    let field = if trigger["behavior"].as_i64() == Some(SPEW_CHARGE) {
        "secondary_barrel"
    } else {
        "primary_barrel"
    };
    Ok(trigger[field].as_u64().map(usize::try_from).transpose()?)
}

/// The barrel's `role` effect, its source tag and its recorded layout.
fn effect<'a>(
    behavior: &'a Value,
    role: &str,
    barrel: usize,
) -> Result<Option<(&'a Value, &'a Value)>> {
    let root = behavior["root"]["datum"]
        .as_u64()
        .context("Behavior root")?;
    let edges = behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["owner"] == root && e["role"] == role && e["barrel"] == barrel)
        .collect::<Vec<_>>();
    ensure!(edges.len() <= 1, "Source barrel has several {role} effects");
    let Some(edge) = edges.first() else {
        return Ok(None);
    };
    // A barrel can name a sound in place of an effect, as an empty effect often does. It has no
    // events, so no particle systems.
    if edge["tag"]["group"] != "effe" {
        return Ok(Some((&edge["tag"], &NO_EVENTS)));
    }
    let datum = edge["tag"]["datum"]
        .as_u64()
        .context("Source effect identity")?;
    let node = &behavior["nodes"][format!("{datum:08X}")];
    ensure!(
        node["tag"]["group"] == "effe" && node["data"]["events"].is_array(),
        "Source {role} effect layout is absent"
    );
    Ok(Some((&edge["tag"], &node["data"])))
}

/// The layout of a role that names a tag other than an effect.
static NO_EVENTS: Value = Value::Null;

fn count(effect: &Value, field: &str) -> usize {
    effect["events"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|event| event[field].as_array().into_iter().flatten())
        .filter(|row| !row["tag"].is_null())
        .count()
}

/// The misfire and empty effects' content. The native firing presentation has no slot checked
/// for either, so their visual parts stay pending.
fn secondary(behavior: &Value, barrel: usize) -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for role in ["misfire", "empty"] {
        let Some((source, effect)) = self::effect(behavior, role, barrel)? else {
            continue;
        };
        let systems = count(effect, "particles");
        let parts = count(effect, "parts");
        rows.push(if systems == 0 {
            json!({"role":role,"source":source,"particle_systems":0,"parts":parts,"status":"native",
                "reason":"The source effect has no particle systems. Its parts are sounds, which the audio translation owns."})
        } else {
            json!({"role":role,"source":source,"particle_systems":systems,"parts":parts,"status":"omitted",
                "reason":format!("The native firing presentation has no checked {role} slot")})
        });
    }
    Ok(rows)
}

/// The content row's slotted presentation, as the firing audio selects it.
fn presentation(native: &mut Reader, item: u32) -> Result<Value> {
    let rig = crate::tiger::rig::inspect_with_audio(native, item, false)?;
    let groups = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Native firing presentation is unavailable")?;
    let mut candidates = groups
        .iter()
        .filter(|group| {
            group["slots"]
                .as_array()
                .is_some_and(|slots| slots.iter().any(|slot| slot["slot"] == 0))
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        candidates.retain(|group| {
            group["sounds"].as_array().is_some_and(|sounds| {
                sounds
                    .iter()
                    .any(|sound| sound["event_id"] == group["content_key"])
            })
        });
    }
    ensure!(
        candidates.len() == 1,
        "Native firing presentation is absent or ambiguous"
    );
    let mut group = candidates[0].clone();
    if let Some(object) = group.as_object_mut() {
        object.remove("sounds");
        object.remove("sound_errors");
    }
    Ok(group)
}

/// The presentation group of the item's selected content row, as the firing loop takes it. A
/// continuous-fire donor such as Coldheart presents its shot in slot 4 and has no slot-0 entity
/// for [`presentation`] to find.
fn selected(native: &mut Reader, item: u32) -> Result<Value> {
    let rig = crate::tiger::rig::inspect_with_audio(native, item, false)?;
    let groups = rig["audio"]["unnamed_groups"]
        .as_array()
        .context("Native firing presentation is unavailable")?;
    let found = groups
        .iter()
        .filter(|group| group["content_key"] == rig["content_key"])
        .collect::<Vec<_>>();
    let [group] = found[..] else {
        bail!("The item's selected content row has no single presentation");
    };
    let mut group = group.clone();
    if let Some(object) = group.as_object_mut() {
        object.remove("sounds");
        object.remove("sound_errors");
    }
    Ok(group)
}

/// The flow owners of `entity` that start particle events, with their particle event counts.
fn particle_flows(native: &mut Reader, entity: u32) -> Result<Vec<(u32, usize)>> {
    let payload = (*native.tag(entity, Some(0x80809c0f))?).clone();
    let mut flows = Vec::new();
    for row in payload.array(16, 12, Some(0x80809c04))? {
        let owner = payload.u32(row)?;
        let p = native.tag(owner, Some(0x80809c36))?;
        let (Ok(instance), Ok(definition)) = (p.pointer(16), p.pointer(24)) else {
            continue;
        };
        if instance < 4
            || definition < 4
            || p.u32(instance - 4)? != 0x8080_84d7
            || p.u32(definition - 4)? != 0x8080_84e9
        {
            continue;
        }
        let mut particles = 0;
        for row in p.array(definition + 0x168, 24, Some(0x8080_93e6))? {
            let event = p.pointer(row + 16)?;
            if p.u32(event - 4)? == PARTICLE {
                particles += 1;
            }
        }
        if particles > 0 {
            flows.push((owner, particles));
        }
    }
    Ok(flows)
}

/// The flow's single-row marker locators, by the marker they name and point kind: 3 for the
/// first-person group and 2 for the third-person group of the stock firing flows. The first of
/// each pair is taken.
struct Binding {
    markers: BTreeMap<u32, [Option<u8>; 2]>,
    /// The kind-1 locator named `world_space`, the space binding of stock world-space events.
    world: Option<u8>,
}

impl Binding {
    fn read(p: &Payload, definition: usize) -> Result<Self> {
        let spans = p.array(definition + 0x1b0, 6, Some(0x8080_8617))?;
        let points = p.array(definition + 0x1c0, 16, Some(0x8080_8615))?;
        // Definition `+0x1D0` holds one name per locator.
        let names = p.array(definition + 0x1d0, 4, None)?;
        let mut markers = BTreeMap::<u32, [Option<u8>; 2]>::new();
        let mut world = None;
        for (ordinal, &span) in spans.iter().enumerate() {
            if p.u8(span + 3)? != 1 {
                continue;
            }
            let point = *points
                .get(usize::from(p.u8(span + 2)?))
                .context("Native locator point")?;
            let ordinal = u8::try_from(ordinal)?;
            let slot = match p.u8(point)? {
                3 => 0,
                2 => 1,
                1 => {
                    let name = names
                        .get(usize::from(ordinal))
                        .map(|&row| p.u32(row))
                        .transpose()?;
                    if world.is_none() && name == Some(hash("world_space")) {
                        world = Some(ordinal);
                    }
                    continue;
                }
                _ => continue,
            };
            let found = markers.entry(p.u32(point + 8)?).or_default();
            if found[slot].is_none() {
                found[slot] = Some(ordinal);
            }
        }
        Ok(Self { markers, world })
    }

    /// The first- and third-person locators of the first of `names` the flow declares for both.
    fn both(&self, names: &[u32]) -> Option<(u32, [u8; 2])> {
        names
            .iter()
            .find_map(|&name| match self.markers.get(&name)? {
                [Some(first), Some(third)] => Some((name, [*first, *third])),
                _ => None,
            })
    }
}

fn timing(p: &Payload, at: usize) -> Result<Timing> {
    Ok(Timing {
        delay: [p.f32(at + 0x1c)?, p.f32(at + 0x20)?],
        duration: [p.f32(at + 0x24)?, p.f32(at + 0x28)?],
        flags: p.u32(at + 0x18)?,
    })
}

fn delay(name: u32, mut timing: Timing) -> Node {
    timing.flags = 4;
    Node {
        name,
        timing,
        kind: Kind::Event(DELAY),
    }
}

/// The stock control tree, with every kept event under a fresh unique name for [`sequence::adopt`]
/// and every particle event replaced by a delay of its timing.
struct Tree<'a> {
    p: &'a Payload,
    controls: Vec<usize>,
    events: Vec<usize>,
    kept: Vec<(u32, usize)>,
    /// Each kept light event's definition and the view group of its nearest `1st` or `3rd`
    /// control, 0 or 1.
    lights: Vec<(usize, Option<usize>)>,
    removed: Vec<Value>,
}

impl Tree<'_> {
    fn control(&mut self, index: usize, depth: usize, view: Option<usize>) -> Result<Node> {
        ensure!(depth < 64, "Native firing flow controls are too deep");
        let p = self.p;
        let at = *self
            .controls
            .get(index)
            .context("Native firing control is outside its flow")?;
        let class = p.u32(at - 4)?;
        let view = match p.u32(at + 0x10)? {
            name if name == hash("1st") => Some(0),
            name if name == hash("3rd") => Some(1),
            _ => view,
        };
        let selectors = p.array(at + 0x38, 4, Some(0x8080_93fb))?;
        let parent = u32::try_from(index)? << 16;
        let mut children = Vec::new();
        for &selector in &selectors {
            let child = usize::from(p.u16(selector + 2)?);
            if p.u16(selector)? == 0 {
                ensure!(
                    p.u32(self.controls.get(child).context("Native firing control")? + 0x14)?
                        == parent,
                    "Native firing control parent differs"
                );
                children.push(self.control(child, depth + 1, view)?);
                continue;
            }
            let event = *self
                .events
                .get(child)
                .context("Native firing event is outside its flow")?;
            ensure!(
                p.u32(event + 0x14)? == parent,
                "Native firing event parent differs"
            );
            let name = hash(&format!("reach/firing/native/{child}"));
            let class = p.u32(event - 4)?;
            if class == PARTICLE {
                let systems = p
                    .array(event + 0x40, 24, Some(0x8080_6cc8))?
                    .into_iter()
                    .map(|row| Ok(format!("{:08X}", p.u32(row + 16)?)))
                    .collect::<Result<Vec<_>>>()?;
                self.removed.push(json!({"event":child,"name":format!("{:08X}",p.u32(event+0x10)?),"systems":systems}));
                children.push(delay(name, timing(p, event)?));
            } else {
                self.kept.push((name, event));
                if class == light::EVENT {
                    self.lights.push((event, view));
                }
                children.push(Node {
                    name,
                    timing: timing(p, event)?,
                    kind: Kind::Event(class),
                });
            }
        }
        ensure!(
            !children.is_empty(),
            "Native firing control has no children"
        );
        let repeat = p.u8(at + 0x32)? == 1;
        let kind = match class {
            SERIAL if repeat => Kind::Repeat(children),
            SERIAL => Kind::Serial(children),
            PARALLEL if repeat => Kind::RepeatParallel(children),
            PARALLEL => Kind::Parallel(children),
            RANGE => Kind::Range {
                input: p.u32(at + 0x60)?,
                bounds: [p.f32(at + 0x58)?, p.f32(at + 0x5c)?],
                children,
            },
            WEIGHTED => {
                let weights = p.array(at + 0x58, 4, Some(0x8080_000f))?;
                ensure!(
                    weights.len() == children.len(),
                    "Native firing choice weights differ from its children"
                );
                Kind::Weighted(
                    weights
                        .iter()
                        .map(|&row| p.f32(row))
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .zip(children)
                        .collect(),
                )
            }
            other => bail!("Native firing control {other:08X} has no writer"),
        };
        Ok(Node {
            name: p.u32(at + 0x10)?,
            timing: timing(p, at)?,
            kind,
        })
    }
}

/// Where an effect item draws: the locators of each view and its frames relative to them.
struct Placement {
    /// The native marker whose locators it binds, none for the muzzle's.
    marker: Option<u32>,
    locators: [u8; 2],
    frames: Vec<[f32; 16]>,
}

/// The placement of an effect item at its location, or why it is unavailable. A location with one
/// selected marker whose native name the flow declares locators for binds them at that marker. Any
/// other binds the muzzle's locators at its bind-pose offset from the muzzle.
fn placement(
    effect: &Value,
    item: &Value,
    scene: &Scene,
    (muzzle, anchor): (&str, ([f32; 3], [f32; 4])),
    (binding, at_muzzle): (&Binding, [u8; 2]),
) -> Result<std::result::Result<Placement, String>> {
    let location = item["location"]
        .as_i64()
        .context("Source effect location")?;
    let name = if location < 0 {
        ""
    } else {
        effect["locations"][usize::try_from(location)?]["name"]
            .as_str()
            .context("Source effect location name")?
    };
    let muzzle_frames = |frames| Placement {
        marker: None,
        locators: at_muzzle,
        frames,
    };
    if name.is_empty() || name == "primary_trigger" || name == muzzle {
        return Ok(Ok(muzzle_frames(vec![rig::IDENTITY])));
    }
    // The ReachMCC effect schema supplies these frames at runtime. They are not model sockets.
    if matches!(name, "gravity" | "up") {
        return Ok(Err(format!(
            "Source effect location {name} requires a runtime gravity-aligned frame"
        )));
    }
    let model = &scene.models[0].model;
    let mut poses = Vec::new();
    if name == "root" {
        poses.push(([0.; 3], [0., 0., 0., 1.]));
    }
    for marker in model.markers.iter().filter(|marker| marker.name == name) {
        if model.marker_selected(marker)? {
            poses.push(seat::marker_pose(model, marker)?);
        }
    }
    if poses.is_empty() {
        return Ok(Err(format!(
            "Source effect location {name} is absent from the model"
        )));
    }
    if name != "root"
        && poses.len() == 1
        && let Some((native, locators)) = binding.both(&marker::names(name, muzzle))
    {
        return Ok(Ok(Placement {
            marker: Some(native),
            locators,
            frames: vec![rig::IDENTITY],
        }));
    }
    let inverse = rig::inverse_rigid(&rig::transform(anchor.0, anchor.1));
    Ok(Ok(muzzle_frames(
        poses
            .into_iter()
            .map(|(position, orientation)| {
                rig::multiply(&inverse, &rig::transform(position, orientation))
            })
            .collect(),
    )))
}

fn chance(name: &str, skip: f32, child: Node) -> Result<Node> {
    ensure!(
        skip.is_finite() && (0.0..=1.0).contains(&skip),
        "Invalid source effect skip fraction"
    );
    if skip == 0. {
        return Ok(child);
    }
    Ok(Node {
        name: hash(name),
        timing: Timing::default(),
        kind: Kind::Weighted(vec![
            (1. - skip, child),
            (
                skip,
                delay(hash(&format!("{name}/skip")), Timing::default()),
            ),
        ]),
    })
}

/// The converted source systems, by view group, and what stays pending. The groups are first
/// person, third person, and first person only while not zoomed. Each event names its systems with
/// the locators each binds.
struct Converted {
    groups: [Vec<Node>; 3],
    events: Vec<FlowEvents>,
    reports: Vec<Value>,
    pending: Vec<Value>,
    sounds: usize,
    life: f32,
}

#[allow(clippy::too_many_arguments)]
fn convert(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    effect: &Value,
    muzzle: (&str, ([f32; 3], [f32; 4])),
    binding: (&Binding, [u8; 2]),
    zooms: bool,
) -> Result<Converted> {
    let mut result = Converted {
        groups: [Vec::new(), Vec::new(), Vec::new()],
        events: Vec::new(),
        reports: Vec::new(),
        pending: Vec::new(),
        sounds: 0,
        life: 0.,
    };
    if effect["loop_start"].as_i64() != Some(-1) {
        result.pending.push(json!({"reasons":["A looping firing effect needs a retiring native host"],"loop_start":effect["loop_start"]}));
        return Ok(result);
    }
    for (event_index, event) in effect["events"]
        .as_array()
        .context("Source effect events")?
        .iter()
        .enumerate()
    {
        admit_event_parts(event_index, event, &mut result)?;
        let mut systems: [Vec<(String, Vec<u8>)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (system_index, system) in event["particles"]
            .as_array()
            .context("Source particle systems")?
            .iter()
            .enumerate()
        {
            if system["tag"].is_null() {
                continue;
            }
            let camera = system["camera"].as_i64().context("Source camera mode")?;
            // Mode 3 plays in both first and third person, as mode 0 plays independent of it.
            let views: &[usize] = match camera {
                0 | 3 => &[0, 1],
                1 => &[0],
                2 => &[1],
                _ => {
                    result.pending.push(
                        json!({"event":event_index,"system":system_index,"source":system["tag"],
                        "reasons":[format!("Source camera mode {camera}")]}),
                    );
                    continue;
                }
            };
            // Reach keeps a first-person system while zoomed only with Render When Zoomed, but the
            // converted body it is emitted from discards at full aim, so every one stops there.
            let render_when_zoomed = system["flags"]
                .as_u64()
                .context("Source particle system flags")?
                & zoom::RENDER_WHEN_ZOOMED
                != 0;
            let placement = match placement(effect, system, scene, muzzle, binding)? {
                Ok(placement) => placement,
                Err(reason) => {
                    result
                        .pending
                        .push(json!({"event":event_index,"system":system_index,
                        "source":system["tag"],"reasons":[reason]}));
                    continue;
                }
            };
            // The view groups place the camera mode, so the converter receives camera 0.
            let mut adapted = system.clone();
            adapted["camera"] = json!(0);
            let model = particle::model::is_model(cache, &adapted)?;
            for (emitter_index, emitter) in system["emitters"]
                .as_array()
                .context("Source particle emitters")?
                .iter()
                .enumerate()
            {
                let reasons = if model {
                    particle::model::pending(cache, &adapted, emitter)?
                } else {
                    particle::pending(cache, &adapted, emitter)?
                };
                if !reasons.is_empty() {
                    result
                        .pending
                        .push(json!({"event":event_index,"system":system_index,
                        "emitter":emitter_index,"source":system["tag"],"reasons":reasons}));
                    continue;
                }
                for (marker_index, frame) in placement.frames.iter().enumerate() {
                    let name = format!(
                        "reach-firing-{event_index}-{system_index}-{emitter_index}-{marker_index}"
                    );
                    let metres = scene.metres();
                    let world = binding.0.world;
                    // A world-space system stores its particles in the world binding's frame, so
                    // they stay where they were emitted while the weapon moves.
                    let (symbol, mut report, life, framed) = if model {
                        let mut parsed = particle::model::read(
                            cache, pages, native, &adapted, emitter, metres, frame,
                        )?;
                        let framed = world.is_some() && parsed.bind_world()?;
                        let (symbol, report) =
                            particle::model::emit(cache, pages, native, graph, &parsed, &name)?;
                        (symbol, report, parsed.life, framed)
                    } else {
                        let mut parsed =
                            particle::read(cache, native, &adapted, emitter, metres, frame)?;
                        let framed = world.is_some() && parsed.bind_world()?;
                        let (symbol, report) =
                            particle::emit(cache, pages, native, graph, &parsed, &name)?;
                        (symbol, report, parsed.life, framed)
                    };
                    result.life = result.life.max(life);
                    for &view in views {
                        let mut locators = vec![placement.locators[view]];
                        if let Some(world) = world.filter(|_| framed) {
                            locators.push(world);
                        }
                        let group = if view == 0 && zooms { 2 } else { view };
                        systems[group].push((symbol.clone(), locators));
                    }
                    report["event"] = json!(event["name"]);
                    report["camera"] = json!(camera);
                    report["conditions"]["camera"] = json!(camera);
                    report["render_when_zoomed"] = json!(render_when_zoomed);
                    report["hidden_while_zoomed"] = json!(zooms && views.contains(&0));
                    report["coordinates"] = system["coordinates"].clone();
                    report["location_frame"] = json!(marker_index);
                    report["native_marker"] =
                        json!(placement.marker.map(|name| format!("{name:08X}")));
                    report["locators"] = json!(placement.locators);
                    result.reports.push(report);
                }
            }
        }
        append_particle_events(event_index, event, systems, &mut result)?;
    }
    Ok(result)
}

/// The source firing effect in the donor's firing presentation: its particle systems in the
/// rebuilt slot-0 flow, and its beams in the hue of the donor's native trace, as [`trace`]
/// describes. Either part translates without the other. A muzzle part that fails leaves the
/// donor's muzzle particles and records why.
pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    behavior: &Value,
    scene: &Scene,
    item: u32,
    root: &Path,
) -> Result<Value> {
    let mut receipt = muzzle(cache, pages, native, behavior, scene, item, root)
        .unwrap_or_else(|error| json!({"status":"native","reason":format!("{error:#}")}));
    let Some(barrel) = barrel(behavior)? else {
        return Ok(receipt);
    };
    let Some((_, effect)) = self::effect(behavior, "fire", barrel)? else {
        return Ok(receipt);
    };
    let group = if receipt["native_presentation"].is_object() {
        receipt["native_presentation"].clone()
    } else {
        match selected(native, item) {
            Ok(group) => group,
            Err(error) => {
                receipt["trace"] = json!({"status":"native","reason":format!("{error:#}")});
                return Ok(receipt);
            }
        }
    };
    let rebuilt = receipt["replacements"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| tag(&row["component_owner"], "rebuilt firing owner"))
        .collect::<Result<Vec<_>>>()?;
    let rows = |key: &str| receipt[key].as_array().cloned().unwrap_or_default();
    let (mut owners, mut allocations) = (rows("owners"), rows("allocations"));
    let mut replacements = rows("replacements");
    let mut assets = Graph::new(
        root.to_path_buf(),
        receipt["assets"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let trace = match trace::build(
        cache,
        native,
        &mut assets,
        root,
        (&mut owners, &mut allocations),
        &group,
        effect,
        &rebuilt,
    ) {
        Ok(Some(trace)) => trace,
        Ok(None) => return Ok(receipt),
        Err(error) => {
            receipt["trace"] = json!({"status":"native","reason":format!("{error:#}")});
            return Ok(receipt);
        }
    };
    receipt["trace"] = trace.report;
    if trace.replacements.is_empty() {
        return Ok(receipt);
    }
    replacements.extend(trace.replacements);
    receipt["replacements"] = json!(replacements);
    receipt["owners"] = json!(owners);
    receipt["allocations"] = json!(allocations);
    receipt["assets"] = json!({"installable":true,"nodes":assets.nodes});
    receipt["native_presentation"] = group;
    if receipt["status"] == "native" {
        receipt["muzzle"] = json!({"status":"native","reason":receipt["reason"]});
        receipt["reason"] = json!(
            "The source beam's hue draws on the donor's native trace, and the donor's muzzle particles stay"
        );
    }
    // The trace carries the source beam's hue alone, so the effect is at most partial.
    receipt["status"] = json!("partial");
    Ok(receipt)
}

/// Rebuilds the donor's slot-0 firing flow around the source systems and records the private
/// owner and allocation the build appends in place of the stock ones.
fn muzzle(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    behavior: &Value,
    scene: &Scene,
    item: u32,
    root: &Path,
) -> Result<Value> {
    let Some(barrel) = barrel(behavior)? else {
        return Ok(
            json!({"status":"source_only","reason":"The source weapon has no primary trigger"}),
        );
    };
    let Some((source, effect)) = self::effect(behavior, "fire", barrel)? else {
        return Ok(json!({"status":"source_only","barrel":barrel,
            "reason":"The primary barrel has no firing effect"}));
    };
    let secondary = secondary(behavior, barrel)?;
    let muzzle = marker::muzzle(&scene.object.gameplay);
    let model = &scene.models[0].model;
    let muzzle = if model.markers.iter().any(|marker| marker.name == muzzle) {
        muzzle
    } else {
        "primary_trigger".to_owned()
    };
    let anchor = seat::marker(model, &muzzle).context("Source muzzle marker")?;
    let group = presentation(native, item)?;
    let entity = tag(
        &group["slots"]
            .as_array()
            .context("Native presentation slots")?
            .iter()
            .find(|slot| slot["slot"] == 0)
            .context("Native presentation slot 0")?["entity"],
        "firing entity",
    )?;
    let flows = particle_flows(native, entity)?;
    ensure!(
        flows.len() == 1,
        "The native firing entity has no single particle flow"
    );
    let (owner, _) = flows[0];
    let mut retained = Vec::new();
    for slot in group["slots"].as_array().into_iter().flatten() {
        let other = tag(&slot["entity"], "presentation entity")?;
        if other == entity {
            continue;
        }
        for (flow, particles) in particle_flows(native, other)? {
            retained.push(json!({"slot":slot["slot"],"entity":format!("{other:08X}"),
                "flow":format!("{flow:08X}"),"particle_events":particles}));
        }
    }
    let stock = (*native.tag(owner, Some(0x80809c36))?).clone();
    let definition = stock.pointer(24)?;
    let binding = Binding::read(&stock, definition)?;
    let Some((_, [first, third])) = binding.both(&[hash("primary_trigger")]) else {
        bail!(
            "The native firing flow declares no first- and third-person primary_trigger locators"
        );
    };
    let mut assets = Graph::new(root.to_path_buf(), Vec::new());
    let zooms = scene.object.tag.group == "weap"
        && crate::halo_reach::hud::scope::zooms(&scene.object.gameplay);
    let converted = convert(
        cache,
        pages,
        native,
        &mut assets,
        scene,
        effect,
        (&muzzle, anchor),
        (&binding, [first, third]),
        zooms,
    )?;
    let mut receipt = json!({"source":source,"barrel":barrel,"muzzle":muzzle,
        "native_presentation":group,"secondary_effects":secondary,"retained_native_particles":retained,
        "source_sound_parts":converted.sounds,"particles":converted.reports,"pending":converted.pending,
        "maximum_particle_life":converted.life,"gameplay_verified":false});
    if converted.events.is_empty() {
        receipt["status"] = json!("native");
        receipt["reason"] =
            json!("No source particle system converted, so the donor's muzzle particles stay");
        return Ok(receipt);
    }
    let instance_tag = stock.u32(stock.pointer(16)?)?;
    ensure!(
        instance_tag == owner,
        "Native firing flow names another owner"
    );
    let mut tree = Tree {
        p: &stock,
        controls: stock
            .array(definition + 0x158, 24, Some(0x8080_93e6))?
            .into_iter()
            .map(|row| stock.pointer(row + 16))
            .collect::<Result<_>>()?,
        events: stock
            .array(definition + 0x168, 24, Some(0x8080_93e6))?
            .into_iter()
            .map(|row| stock.pointer(row + 16))
            .collect::<Result<_>>()?,
        kept: Vec::new(),
        lights: Vec::new(),
        removed: Vec::new(),
    };
    ensure!(
        tree.controls
            .first()
            .is_some_and(|&root| stock.u32(root + 0x14).ok() == Some(0xffff00ff)),
        "Native firing flow root differs"
    );
    let mut flow = tree.control(0, 0, None)?;
    let Tree {
        kept,
        lights,
        removed,
        ..
    } = tree;
    let mut payload = stock.clone();
    let allocation_tag = payload.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
    let [mut first_person, third_person, unzoomed] = converted.groups;
    let hidden_while_zoomed = converted
        .reports
        .iter()
        .filter(|report| report["hidden_while_zoomed"] == true)
        .count();
    let gate = if unzoomed.is_empty() {
        None
    } else {
        let input = zoom::input(
            &mut payload,
            &mut allocation,
            crate::tiger::channel::inherited::AIM_VISIBILITY,
        )?;
        first_person.push(zoom::gate(input.ordinal, unzoomed));
        Some(input)
    };
    let mut views = Vec::new();
    for (label, nodes) in [("1st", first_person), ("3rd", third_person)] {
        if !nodes.is_empty() {
            views.push(Node {
                name: hash(label),
                timing: Timing::default(),
                kind: Kind::Parallel(nodes),
            });
        }
    }
    let Kind::Parallel(children) = &mut flow.kind else {
        bail!("Native firing flow root is not a single parallel control");
    };
    children.push(Node {
        name: hash("reach/firing"),
        timing: Timing::default(),
        kind: Kind::Parallel(views),
    });
    let mut envelopes = vec![stock.clone()];
    for envelope in ENVELOPES {
        envelopes.push((*native.tag(envelope, Some(0x80809c36))?).clone());
    }
    let written = sequence::write(&mut payload, &mut allocation, &flow, &envelopes)?;
    for (name, old) in &kept {
        sequence::adopt(
            &mut payload,
            *written.events.get(name).context("Kept firing event")?,
            *old,
        )?;
    }
    let mut patches = Vec::new();
    for (name, bound) in &converted.events {
        for (offset, symbol) in sequence::particle::write_bound(
            &mut payload,
            *written.events.get(name).context("Source firing event")?,
            bound,
        )? {
            patches.push(json!({"offset":offset,"symbol":symbol}));
        }
    }
    let firing_lights = light::recolor_all(
        native,
        &mut assets,
        cache,
        scene,
        (&mut payload, &mut patches),
        &lights,
    )?;
    let end = payload.0.len();
    let span = instance::seal_span(&mut payload, &mut patches, end)?;
    let symbol = format!("reach-firing-flow-{owner:08X}");
    let allocation_symbol = format!("{symbol}-allocation");
    let mut owners = Vec::new();
    let mut allocations = Vec::new();
    crate::graph::add(root, &mut owners, &symbol, owner, &payload.0, None, patches)?;
    crate::graph::add(
        root,
        &mut allocations,
        &allocation_symbol,
        allocation_tag,
        &allocation.0,
        None,
        Vec::new(),
    )?;
    let mut flow_row = json!({"slot":0,"entity":format!("{entity:08X}"),
        "component_owner":format!("{owner:08X}"),"owner":symbol,"allocation":allocation_symbol});
    let mut replacements = Vec::new();
    let zoom_report = match &gate {
        None => json!({"status":if zooms {"translated"} else {"absent"},"source_zooms":zooms,
        "hidden_while_zoomed":0,
        "reason":if zooms {
            "No source system draws in first person"
        } else {
            "The source weapon does not zoom"
        }}),
        Some(input) => {
            let slot = (*native.tag(entity, Some(0x80809c0f))?).clone();
            let bank = zoom::bank(native, &slot)?;
            let (before, after) = zoom::connections(&slot, owner, input, &bank)?;
            flow_row["connections"] =
                json!({"before":hex::encode(before),"after":hex::encode(after)});
            let bank_symbol = format!("reach-firing-bank-{:08X}", bank.tag);
            if let Some((private, tag, bank_allocation)) = &bank.private {
                let bank_allocation_symbol = format!("{bank_symbol}-allocation");
                crate::graph::add(
                    root,
                    &mut owners,
                    &bank_symbol,
                    bank.tag,
                    &private.0,
                    None,
                    Vec::new(),
                )?;
                crate::graph::add(
                    root,
                    &mut allocations,
                    &bank_allocation_symbol,
                    *tag,
                    &bank_allocation.0,
                    None,
                    Vec::new(),
                )?;
                replacements.push(json!({"slot":0,"entity":format!("{entity:08X}"),
                    "component_owner":format!("{:08X}",bank.tag),"owner":bank_symbol,
                    "allocation":bank_allocation_symbol}));
            }
            json!({"status":"translated","source_zooms":true,"hidden_while_zoomed":hidden_while_zoomed,
                "channel":format!("{:08X}",crate::tiger::channel::inherited::AIM_VISIBILITY),
                "input":input.ordinal,"bank":format!("{:08X}",bank.tag),"bank_declaration":bank.channel,
                "private_bank":bank.private.is_some(),"shown_below":0.99,
                "reason":"First-person systems play only below 0.99 aim visibility, where the body discards and the scope draws, including those Reach keeps while zoomed"})
        }
    };
    replacements.insert(0, flow_row);
    let complete = converted.pending.is_empty()
        && retained.is_empty()
        && secondary.iter().all(|row| row["status"] == "native");
    receipt["status"] = json!(if complete { "translated" } else { "partial" });
    receipt["reason"] = json!(if complete {
        "The source firing particles replace the donor's muzzle particles in the native firing flow"
    } else {
        "The source firing particles replace the donor's muzzle particles. Some source parts stay pending"
    });
    let markers = binding
        .markers
        .iter()
        .map(|(name, pair)| (format!("{name:08X}"), json!(pair)))
        .collect::<serde_json::Map<String, Value>>();
    receipt["native_flow"] = json!({"entity":format!("{entity:08X}"),"owner":format!("{owner:08X}"),
        "allocation":format!("{allocation_tag:08X}"),"removed_particle_events":removed,
        "kept_events":kept.len(),"locators":{"first_person":first,"third_person":third,
            "world_space":binding.world,"markers":markers},
        "sealed_span":span});
    receipt["limits"] = json!([
        "The rebuilt flow keeps the donor's firing sounds, firing lights and other non-particle events, and replaces each donor particle event with a delay of its timing. Kept firing lights take private records in the source light's color where firing_lights says so, at the donor's reach and brightness scale.",
        "First-person systems play under a 1st control on kind-3 locators and third-person systems under a 3rd control on kind-2 locators. That these select the view is inferred from the stock flow layout.",
        "A system at a marker the flow declares locators for, such as the ejection port, binds them and follows that marker. Other systems are placed relative to the source muzzle in the bind pose and keep that offset while the muzzle bone animates.",
        "World-space systems bind the flow's world_space locator second, as stock casing and smoke events do, so their particles stay where they were emitted. Local-space systems use the emitter frame, the binding stock muzzle flashes use.",
        "Source event skip fractions become weighted choices. Native random streams differ from Reach.",
        "A zooming weapon's first-person systems play under a range gate below 0.99 on aim visibility, which the slot entity's channel bank inherits. Reach hides those without Render When Zoomed at any zoom level, so they also play through the earlier part of aiming, where the weapon still draws. Reach keeps those with the flag, its casings and plasma glows, while zoomed, and here they stop at full aim with the body they are emitted from."
    ]);
    receipt["zoom"] = zoom_report;
    receipt["firing_lights"] = json!(firing_lights);
    receipt["assets"] = json!({"installable":true,"nodes":assets.nodes});
    receipt["owners"] = json!(owners);
    receipt["allocations"] = json!(allocations);
    receipt["replacements"] = json!(replacements);
    Ok(receipt)
}

type FlowEvents = (u32, Vec<(String, Vec<u8>)>);

fn admit_event_parts(event_index: usize, event: &Value, result: &mut Converted) -> Result<()> {
    for (part_index, part) in event["parts"]
        .as_array()
        .context("Source effect parts")?
        .iter()
        .enumerate()
    {
        if part["tag"].is_null() {
            continue;
        }
        let source: Tag = serde_json::from_value(part["tag"].clone())?;
        if source.group == "snd!" {
            result.sounds += 1;
            continue;
        }
        result.pending.push(
            json!({"event":event_index,"part":part_index,"source":source,
            "reasons":["Separate effect part translation"]}),
        );
    }
    Ok(())
}

type ParticleEvents = [Vec<(String, Vec<u8>)>; 3];

fn append_particle_events(
    event_index: usize,
    event: &Value,
    systems: ParticleEvents,
    result: &mut Converted,
) -> Result<()> {
    let timing = Timing {
        delay: pair(event, "delay_seconds")?,
        duration: pair(event, "duration_seconds")?,
        flags: 0,
    };
    let skip = event["skip_fraction"]
        .as_f64()
        .context("Source event probability")? as f32;
    for (group, list) in systems.into_iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let label = format!("reach/firing/event/{event_index}/{group}");
        let name = hash(&label);
        result.groups[group].push(chance(
            &format!("{label}/choice"),
            skip,
            Node {
                name,
                timing,
                kind: Kind::Event(PARTICLE),
            },
        )?);
        result.events.push((name, list));
    }
    Ok(())
}
