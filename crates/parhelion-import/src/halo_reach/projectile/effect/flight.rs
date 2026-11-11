//! Every attached source flight element, and the whole flight presentation of an effect-only
//! projectile in its carrier's own effect flow.
//!
//! A source projectile attaches effects (`effe`), whose events hold parts and particle systems,
//! and other tags directly. A modeled carrier's flight host draws the contrail parts and, beside
//! the bolt's contrails, the particle systems at the projectile origin. The rest is listed as
//! pending. An effect-only carrier has no model or marker owner. Its flow
//! whose locators name `root` (kind 0) and `world_space` (kind 1) binds the stock bolt's streak to
//! `root`, the projectile itself, and hosts the converted source instead: each contrail profile
//! through the billboard contrail and each particle emitter through the Reach particle conversion,
//! at the projectile origin, each light volume as a native beam of profiles, and the first source
//! light or cheap light as a recolored native light. The flow's controls are rebuilt with one
//! event per converted system, held until the projectile retires and bound to `root`, and the
//! flow's own light event, adopted, names the private light. Root
//! completion no longer retires the projectile. A source with no flight element leaves the flow
//! out. A source whose elements all stay pending keeps the carrier's own flight effects, held for
//! the flight, so the projectile stays visible, and says so.
use super::*;
use crate::tiger::sequence;
use std::collections::BTreeSet;

/// FNV-1 of `root` and `world_space`.
const ROOT: u32 = 0x7bd0_5f71;
const WORLD_SPACE: u32 = 0x9199_59c0;
const FLOW: u32 = 0x8080_84e9;
const PARTICLES: u32 = 0x8080_6cc6;
const LIGHT: u32 = 0x8080_6f49;

/// One attached source flight element, named by its place in the attachments.
enum Element {
    /// A contrail part and its origin in the projectile's frame, or why it has none.
    Contrail {
        source: Value,
        tag: Tag,
        origin: std::result::Result<[f32; 3], String>,
    },
    /// A particle system at the projectile origin.
    Particles {
        source: Value,
        system: Value,
    },
    /// A light volume system at the projectile origin.
    Volume {
        source: Value,
        tag: Tag,
    },
    /// A light (`ligh`) or cheap light (`gldf`).
    Light {
        source: Value,
        tag: Tag,
    },
    Other {
        source: Value,
        reason: String,
    },
}

/// A converted flight system and whether its event binds a world frame after the emitter.
pub(super) struct Flight {
    pub symbol: String,
    pub world: bool,
}

/// Why a light volume system or particle system away from the projectile origin stays pending.
const AT_MARKER: &str = "The element sits at a source marker the carrier lacks";

fn reason(group: &str) -> String {
    match group {
        "lens" => "Lens flares have no native translation. A native lens flare event (80806F40) names a flare record (80806F68) whose size and color come from its parameter program (808071DC), and no converter authors one from a source lens flare".into(),
        "lsnd" => "Projectile looping sounds have no native flight sound translation".into(),
        "cpem" => "Cheap particle emitters in flight effects are not converted".into(),
        "prt3" => "A particle system attached outside an effect event is not converted".into(),
        other => format!("Attached {other} tags have no flight translation"),
    }
}

/// The parts, particle systems and looping sounds of the attached effect `tag`.
fn effect(
    cache: &Cache,
    scene: &Scene,
    (index, attachment, tag): (usize, &Value, &Tag),
    elements: &mut Vec<Element>,
) -> Result<()> {
    let effect = source::read(cache, tag.address()?)?;
    let locations = effect["locations"]
        .as_array()
        .context("Source effect locations")?;
    // A location is the projectile origin when it is unnamed or `root`, or `replace` with the
    // attachment's own marker there.
    let origin = |location: &Value| -> Result<bool> {
        let at = location.as_i64().context("Source effect location")?;
        let name = if at < 0 {
            ""
        } else {
            locations
                .get(usize::try_from(at)?)
                .and_then(|row| row["name"].as_str())
                .context("Source effect location name")?
        };
        let name = if matches!(name, "" | "replace") {
            attachment["marker"].as_str().unwrap_or_default()
        } else {
            name
        };
        Ok(matches!(name, "" | "root"))
    };
    for (event_index, event) in effect["events"]
        .as_array()
        .context("Source effect events")?
        .iter()
        .enumerate()
    {
        for (part_index, part) in event["parts"]
            .as_array()
            .context("Source effect parts")?
            .iter()
            .enumerate()
        {
            if part["tag"].is_null() {
                continue;
            }
            let source = json!({"attachment":index,"effect":tag,"event":event_index,
                "part":part_index,"tag":part["tag"]});
            let part_tag: Tag = serde_json::from_value(part["tag"].clone())?;
            match part_tag.group.clone().as_str() {
                "cntl" => {
                    let at = usize::try_from(part["offset"].as_u64().context("Source part")?)?;
                    let origin = source_frame(cache, tag, at, attachment, scene)
                        .map(|(origin, _)| origin)
                        .map_err(|error| format!("{error:#}"));
                    elements.push(Element::Contrail {
                        source,
                        tag: part_tag,
                        origin,
                    });
                }
                "ligh" | "gldf" => elements.push(Element::Light {
                    source,
                    tag: part_tag,
                }),
                "ltvl" => elements.push(if origin(&part["location"])? {
                    Element::Volume {
                        source,
                        tag: part_tag,
                    }
                } else {
                    Element::Other {
                        source,
                        reason: AT_MARKER.into(),
                    }
                }),
                group => elements.push(Element::Other {
                    source,
                    reason: reason(group),
                }),
            }
        }
        for (system_index, system) in event["particles"]
            .as_array()
            .context("Source effect particles")?
            .iter()
            .enumerate()
        {
            if system["tag"].is_null() {
                continue;
            }
            let source = json!({"attachment":index,"effect":tag,"event":event_index,
                "system":system_index,"tag":system["tag"]});
            elements.push(if origin(&system["location"])? {
                Element::Particles {
                    source,
                    system: system.clone(),
                }
            } else {
                Element::Other {
                    source,
                    reason: AT_MARKER.into(),
                }
            });
        }
    }
    for (sound_index, sound) in effect["looping_sounds"]
        .as_array()
        .context("Source effect looping sounds")?
        .iter()
        .enumerate()
    {
        if !sound["tag"].is_null() {
            elements.push(Element::Other {
                source: json!({"attachment":index,"effect":tag,"looping_sound":sound_index,
                    "tag":sound["tag"]}),
                reason: reason("lsnd"),
            });
        }
    }
    Ok(())
}

/// Every attached source flight element. An attached effect that does not read is one pending
/// element with its reason, so a survey never fails an import.
fn elements(cache: &Cache, scene: &Scene) -> Vec<Element> {
    let mut elements = Vec::new();
    for (index, attachment) in scene.object.attachments.iter().enumerate() {
        if attachment["tag"].is_null() {
            continue;
        }
        let source = json!({"attachment":index,"tag":attachment["tag"]});
        let tag = match serde_json::from_value::<Tag>(attachment["tag"].clone()) {
            Ok(tag) => tag,
            Err(error) => {
                elements.push(Element::Other {
                    source,
                    reason: format!("The attachment names no source tag: {error}"),
                });
                continue;
            }
        };
        match tag.group.clone().as_str() {
            "effe" => {
                let mut found = Vec::new();
                match effect(cache, scene, (index, attachment, &tag), &mut found) {
                    Ok(()) => elements.extend(found),
                    Err(error) => elements.push(Element::Other {
                        source,
                        reason: format!("The attached effect does not read: {error:#}"),
                    }),
                }
            }
            "ligh" | "gldf" => elements.push(Element::Light { source, tag }),
            "ltvl" if matches!(attachment["marker"].as_str(), None | Some("" | "root")) => {
                elements.push(Element::Volume { source, tag })
            }
            "ltvl" => elements.push(Element::Other {
                source,
                reason: AT_MARKER.into(),
            }),
            group => elements.push(Element::Other {
                source,
                reason: reason(group),
            }),
        }
    }
    elements
}

/// Each light volume of the attached light volume system `tag` as a native beam with its report,
/// or pending with its reasons. A beam that fails to emit takes back the nodes it placed.
fn volumes(
    cache: &Cache,
    native: &mut Reader,
    graph: &mut Graph,
    metres: f32,
    prefix: &str,
    (source, tag): (&Value, &Tag),
) -> Vec<std::result::Result<(Flight, Value), Value>> {
    let read = match volume::read(cache, tag, metres) {
        Ok(read) => read,
        Err(error) => {
            return vec![Err(
                json!({"source":source,"reasons":[format!("{error:#}")]}),
            )];
        }
    };
    read.into_iter()
        .enumerate()
        .map(|(index, (name, parsed))| {
            let source = json!({"source":source,"light_volume":index,"name":name});
            let parsed = parsed.map_err(|reason| json!({"source":source,"reasons":[reason]}))?;
            let start = graph.nodes.len();
            match volume::emit(
                native,
                graph,
                &parsed,
                &format!("{prefix}-volume-{index}"),
                metres,
            ) {
                Ok((symbol, report)) => Ok((
                    Flight {
                        symbol,
                        world: false,
                    },
                    json!({"source":source,"light_volume":report}),
                )),
                Err(error) => {
                    graph.nodes.truncate(start);
                    Err(json!({"source":source,"reasons":[format!("{error:#}")]}))
                }
            }
        })
        .collect()
}

/// The attached flight elements a modeled carrier's flight host leaves out, with their reasons.
pub(super) fn pending(cache: &Cache, scene: &Scene) -> Vec<Value> {
    elements(cache, scene)
        .into_iter()
        .filter_map(|element| match element {
            Element::Contrail { .. } => None,
            Element::Particles { source, .. }
            | Element::Volume { source, .. }
            | Element::Light { source, .. } => Some(
                json!({"source":source,"reasons":["A modeled carrier's flight host draws contrails only"]}),
            ),
            Element::Other { source, reason } => Some(json!({"source":source,"reasons":[reason]})),
        })
        .collect()
}

/// Each emitter of an attached source particle system at the projectile origin, converted to a
/// native system with its report, or pending with its reasons. A conversion that fails takes back
/// the nodes it placed. A world-space system also reads its particles' frame from a second binding,
/// which the host binds to its `world_space` locator, so its particles stay where they were
/// emitted. A local-space system's particles follow the projectile.
fn particles(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    metres: f32,
    prefix: &str,
    (source, system): (&Value, &Value),
) -> Result<Vec<std::result::Result<(Flight, Value), Value>>> {
    let frame = crate::halo_reach::rig::transform([0.; 3], [0., 0., 0., 1.]);
    let emitters = system["emitters"]
        .as_array()
        .context("Source particle emitters")?;
    let mut out = Vec::new();
    for (emitter_index, emitter) in emitters.iter().enumerate() {
        let start = graph.nodes.len();
        let source = json!({"source":source,"emitter":emitter_index});
        let result = particle::pending(cache, system, emitter).and_then(|reasons| {
            if !reasons.is_empty() {
                return Ok(Err(reasons));
            }
            let mut parsed = particle::read(cache, native, system, emitter, metres, &frame)?;
            let world = parsed.bind_world()?;
            let (symbol, report) = particle::emit(
                cache,
                pages,
                native,
                graph,
                &parsed,
                &format!("{prefix}-particle-{emitter_index}"),
            )?;
            Ok(Ok((Flight { symbol, world }, report)))
        });
        out.push(match result {
            Ok(Ok((flight, report))) => Ok((flight, json!({"source":source,"particles":report}))),
            Ok(Err(reasons)) => Err(json!({"source":source,"reasons":reasons})),
            Err(error) => {
                graph.nodes.truncate(start);
                Err(json!({"source":source,"reasons":[format!("{error:#}")]}))
            }
        });
    }
    Ok(out)
}

/// What a modeled carrier's flight host draws beside any contrail: the converted symbols of each
/// attached particle system and light volume at the projectile origin, their reports, every other
/// element but a contrail as pending, and with `lit`, the private light of the first source light
/// for the host's flight light event. The host marker sits at `origin`, the source contrail's
/// origin or, without a contrail, the projectile origin, facing the projectile's own axes, so it
/// hosts them only when that is the projectile origin.
pub(super) fn modeled(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    (origin, lit): ([f32; 3], bool),
) -> Result<FlightSystems> {
    let mut systems = Vec::new();
    let mut converted = Vec::new();
    let mut pending = Vec::new();
    let mut light = None;
    for (index, element) in elements(cache, scene).into_iter().enumerate() {
        match element {
            Element::Contrail { .. } => {}
            Element::Particles { source, system } if origin == [0.; 3] => {
                for result in particles(
                    cache,
                    pages,
                    native,
                    graph,
                    scene.metres(),
                    &format!("flight-{index}"),
                    (&source, &system),
                )? {
                    match result {
                        Ok((flight, report)) => {
                            systems.push(flight);
                            converted.push(report);
                        }
                        Err(entry) => pending.push(entry),
                    }
                }
            }
            Element::Particles { source, .. } => pending.push(json!({"source":source,
                "reasons":["The host marker sits at the contrail origin, away from the projectile origin the particle system needs"]})),
            Element::Volume { source, tag } if origin == [0.; 3] => {
                for result in volumes(
                    cache,
                    native,
                    graph,
                    scene.metres(),
                    &format!("flight-{index}"),
                    (&source, &tag),
                ) {
                    match result {
                        Ok((flight, report)) => {
                            systems.push(flight);
                            converted.push(report);
                        }
                        Err(entry) => pending.push(entry),
                    }
                }
            }
            Element::Volume { source, .. } => pending.push(json!({"source":source,
                "reasons":["The host marker sits at the contrail origin, away from the projectile origin the light volume needs"]})),
            Element::Light { source, tag } if lit && light.is_none() => {
                let start = graph.nodes.len();
                let result = light::source(cache, &tag, scene.metres()).and_then(|lit| {
                    let (placed, report) = light::template(native, lit.color)?
                        .reach(lit.radius)?
                        .add_named(native, graph, lit.seconds, "flight-light")?;
                    Ok((
                        placed,
                        json!({"source":source,"light":{"linear_color":lit.color,
                            "source_seconds":lit.seconds,"radius_metres":lit.radius,"native":report}}),
                    ))
                });
                match result {
                    Ok((placed, report)) => {
                        light = Some(placed.symbol);
                        converted.push(report);
                    }
                    Err(error) => {
                        graph.nodes.truncate(start);
                        pending.push(json!({"source":source,"reasons":[format!("{error:#}")]}));
                    }
                }
            }
            Element::Light { source, .. } if lit => pending.push(
                json!({"source":source,"reasons":["Only the first source light converts"]}),
            ),
            Element::Light { source, .. } => pending.push(json!({"source":source,
                "reasons":["A modeled carrier's flight host keeps no light event for the source light"]})),
            Element::Other { source, reason } => {
                pending.push(json!({"source":source,"reasons":[reason]}))
            }
        }
    }
    Ok((systems, converted, pending, light))
}

/// The carrier flow bound to the projectile: its owner, its `root` and `world_space` locators and
/// its one light event, if it has one.
struct Host {
    owner: u32,
    root: u8,
    world: u8,
    light: Option<usize>,
}

fn host(owners: &BTreeMap<u32, Payload>) -> Result<Host> {
    let mut found = Vec::new();
    for (&owner, p) in owners {
        let definition = p.pointer(24)?;
        if p.u32(definition - 4)? != FLOW {
            continue;
        }
        let locators = p.array(definition + 0x1c0, 16, Some(0x8080_8615))?;
        let names = p.array(definition + 0x1d0, 4, Some(0x8080_8614))?;
        if locators.len() != names.len() {
            continue;
        }
        let ordinal = |kind: u8, name: u32| {
            locators.iter().zip(&names).position(|(&row, &named)| {
                p.u8(row).ok() == Some(kind) && p.u32(named).ok() == Some(name)
            })
        };
        let (Some(root), Some(world)) = (ordinal(0, ROOT), ordinal(1, WORLD_SPACE)) else {
            continue;
        };
        let mut lights = Vec::new();
        for row in p.array(definition + 0x168, 24, Some(0x8080_93e6))? {
            let event = p.pointer(row + 16)?;
            if p.u32(event - 4)? == LIGHT {
                lights.push(event);
            }
        }
        found.push(Host {
            owner,
            root: u8::try_from(root)?,
            world: u8::try_from(world)?,
            light: match lights[..] {
                [event] => Some(event),
                _ => None,
            },
        });
    }
    ensure!(
        found.len() == 1,
        "The effect-only carrier has no single flow bound to its root and world space"
    );
    Ok(found.remove(0))
}

/// The flight presentation of an effect-only projectile, hosted by its carrier's own flow.
pub(super) fn build(
    cache: &Cache,
    pages: &mut Pages,
    native: &mut Reader,
    scene: &Scene,
    behavior: &Value,
    assembly: &mut super::super::assembly::Assembly,
    root: &std::path::Path,
) -> Result<Effects> {
    std::fs::create_dir_all(root)?;
    let mut graph = Graph::new(root.into(), Vec::new());
    let host = host(&assembly.owners)?;
    let speed = behavior["initial_velocity"]
        .as_f64()
        .context("Projectile trail reference speed")? as f32
        * scene.metres();
    let elements = elements(cache, scene);
    let declared = elements.len();
    let ConvertedFlight {
        systems,
        light,
        converted,
        pending,
    } = convert_elements(
        (cache, pages),
        native,
        &mut graph,
        scene,
        (speed, host.light.is_some()),
        elements,
    )?;
    let native_owner = format!("{:08X}", host.owner);
    if systems.is_empty() && light.is_none() {
        let report = if declared == 0 {
            super::super::effect_only::omit(native, assembly, &BTreeSet::from([host.owner]))?;
            json!({"status":"translated","presentation":"none","omitted_flow":native_owner,
                "reason":"The source projectile attaches no flight effect, so nothing draws its flight"})
        } else {
            // The stock events last one second and root completion would then retire the
            // projectile short of its source range, so they are held for the flight instead.
            let p = assembly
                .owners
                .get_mut(&host.owner)
                .context("Carrier flight flow")?;
            let definition = p.pointer(24)?;
            let events = p
                .array(definition + 0x168, 24, Some(0x8080_93e6))?
                .into_iter()
                .map(|row| p.pointer(row + 16))
                .collect::<Result<Vec<_>>>()?;
            for &event in &events {
                let flags = p.u32(event + 0x18)? | 8;
                put(&mut p.0, event + 0x18, &flags.to_le_bytes())?;
            }
            let retire = p.u8(definition + 0x2e0)? & !2;
            put(&mut p.0, definition + 0x2e0, &[retire])?;
            json!({"status":"native","presentation":"carrier","native_owner":native_owner,
                "held_events":events.len(),"pending":pending,
                "reason":"No attached source flight element converts, so the carrier's own flight effects stand in"})
        };
        return Ok(Effects {
            graph,
            patches: Vec::new(),
            report,
            owner: None,
            allocations: BTreeMap::new(),
        });
    }
    let allocation_tag = assembly.owners[&host.owner].u32(0x44)?;
    ensure!(
        assembly
            .owners
            .values()
            .filter(|owner| owner.u32(0x44).ok() == Some(allocation_tag))
            .count()
            == 1,
        "The carrier flight flow shares its allocation with another component"
    );
    let mut allocation = (*native.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
    let p = assembly
        .owners
        .get_mut(&host.owner)
        .context("Carrier flight flow")?;
    let stock = p.clone();
    let name = |index: usize| crate::tiger::audio::bank::hash(&format!("reach/flight/{index}"));
    // Flag 8 holds an event until its owner retires, as authored trail events do.
    let held = sequence::Timing {
        duration: [1.; 2],
        flags: 8,
        ..sequence::Timing::default()
    };
    let mut children = (0..systems.len())
        .map(|index| sequence::Node {
            name: name(index),
            timing: held,
            kind: sequence::Kind::Event(PARTICLES),
        })
        .collect::<Vec<_>>();
    if light.is_some() {
        children.push(sequence::Node {
            name: name(systems.len()),
            timing: held,
            kind: sequence::Kind::Event(LIGHT),
        });
    }
    let tree = sequence::Node {
        name: crate::tiger::audio::bank::hash("reach/flight/root"),
        timing: sequence::Timing::default(),
        kind: sequence::Kind::Parallel(children),
    };
    let written = sequence::write(p, &mut allocation, &tree, std::slice::from_ref(&stock))?;
    let mut declared_patches = Vec::new();
    for (index, flight) in systems.iter().enumerate() {
        let locators = if flight.world {
            vec![host.root, host.world]
        } else {
            vec![host.root]
        };
        for (offset, symbol) in sequence::particle::write_bound(
            p,
            written.events[&name(index)],
            &[(flight.symbol.clone(), locators)],
        )? {
            declared_patches.push(json!({"offset":offset,"symbol":symbol}));
        }
    }
    if let (Some(placed), Some(event)) = (&light, host.light) {
        // The stock light event keeps its body under the rebuilt controls, held for the flight,
        // and names its light record at `+0x40`.
        sequence::adopt(p, written.events[&name(systems.len())], event)?;
        let flags = p.u32(event + 0x18)? | 8;
        put(&mut p.0, event + 0x18, &flags.to_le_bytes())?;
        put(&mut p.0, event + 0x40, &u32::MAX.to_le_bytes())?;
        declared_patches.push(json!({"offset":event + 0x40,"symbol":placed.symbol}));
    }
    // Definition `+0x2E0` bit 1 retires the owner when the root completes. The projectile's own
    // contact and range end it instead.
    let definition = p.pointer(24)?;
    let retire = p.u8(definition + 0x2e0)? & !2;
    put(&mut p.0, definition + 0x2e0, &[retire])?;
    // The rebuilt controls and events are runtime records past the stock schema, which the
    // client's instance copy leaves out without a definition view after them.
    let end = p.0.len();
    crate::tiger::instance::seal_span(p, &mut declared_patches, end)?;
    let patches = declared_patches
        .iter()
        .map(|patch| {
            Ok((
                host.owner,
                usize::try_from(patch["offset"].as_u64().context("Flight patch offset")?)?,
                patch["symbol"]
                    .as_str()
                    .context("Flight patch symbol")?
                    .to_owned(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Effects {
        graph,
        patches,
        owner: Some(host.owner),
        allocations: BTreeMap::from([(allocation_tag, allocation)]),
        report: json!({"status":"translated","presentation":"source","native_owner":native_owner,
            "root_locator":host.root,"systems":systems.len(),"light":light.is_some(),
            "converted":converted,"pending":pending,
            "reference_speed_metres_per_second":speed,"gameplay_verified":false,
            "limits":["Converted systems start with the projectile and last until it retires, bound to its origin. Source event delays, loops and durations, attachment scale functions and source markers are not carried. A converted light has the native light's brightness in the source light's hue, and a cheap light's radius."]}),
    })
}

type FlightSystems = (Vec<Flight>, Vec<Value>, Vec<Value>, Option<String>);

struct ConvertedFlight {
    systems: Vec<Flight>,
    light: Option<Light>,
    converted: Vec<Value>,
    pending: Vec<Value>,
}

fn convert_elements(
    (cache, pages): (&Cache, &mut Pages),
    native: &mut Reader,
    graph: &mut Graph,
    scene: &Scene,
    (speed, light_slot): (f32, bool),
    elements: Vec<Element>,
) -> Result<ConvertedFlight> {
    let mut systems = Vec::new();
    let mut light = None;
    let mut converted = Vec::new();
    let mut pending = Vec::new();
    for (index, element) in elements.into_iter().enumerate() {
        // A conversion that fails takes back the nodes it placed.
        let start = graph.nodes.len();
        match element {
            Element::Contrail {
                source,
                tag,
                origin,
            } => {
                // Each profile converts on its own, and one that does not stays pending.
                let profiles = origin
                    .map_err(anyhow::Error::msg)
                    .and_then(|origin| {
                        ensure!(
                            origin == [0.; 3],
                            "A contrail away from the projectile origin needs a marker the carrier lacks"
                        );
                        trail::each(cache, &tag)
                    });
                let profiles = match profiles {
                    Ok(profiles) => profiles,
                    Err(error) => {
                        pending.push(json!({"source":source,"reasons":[format!("{error:#}")]}));
                        continue;
                    }
                };
                for (profile_index, (name, profile)) in profiles.into_iter().enumerate() {
                    let start = graph.nodes.len();
                    let result = profile.and_then(|profile| {
                        trail::emit(
                            cache,
                            pages,
                            native,
                            graph,
                            &profile,
                            &format!("flight-{index}-trail-{profile_index}"),
                            scene.metres(),
                            speed,
                        )
                    });
                    match result {
                        Ok((symbol, report)) => {
                            systems.push(Flight {
                                symbol,
                                world: false,
                            });
                            converted.push(json!({"source":source,"contrail":report}));
                        }
                        Err(error) if crate::cancellation::is_cancelled(&error) => {
                            return Err(error);
                        }
                        Err(error) => {
                            graph.nodes.truncate(start);
                            pending.push(json!({"source":source,"profile":name,
                                "reasons":[format!("{error:#}")]}));
                        }
                    }
                }
            }
            Element::Particles { source, system } => {
                for result in particles(
                    cache,
                    pages,
                    native,
                    graph,
                    scene.metres(),
                    &format!("flight-{index}"),
                    (&source, &system),
                )? {
                    match result {
                        Ok((flight, report)) => {
                            systems.push(flight);
                            converted.push(report);
                        }
                        Err(entry) => pending.push(entry),
                    }
                }
            }
            Element::Volume { source, tag } => {
                for result in volumes(
                    cache,
                    native,
                    graph,
                    scene.metres(),
                    &format!("flight-{index}"),
                    (&source, &tag),
                ) {
                    match result {
                        Ok((flight, report)) => {
                            systems.push(flight);
                            converted.push(report);
                        }
                        Err(entry) => pending.push(entry),
                    }
                }
            }
            Element::Light { source, tag } => {
                if light.is_some() {
                    pending.push(
                        json!({"source":source,"reasons":["Only the first source light converts"]}),
                    );
                    continue;
                }
                if !light_slot {
                    pending.push(json!({"source":source,
                        "reasons":["The carrier flow has no single light event to name it"]}));
                    continue;
                }
                let result = light::source(cache, &tag, scene.metres()).and_then(|lit| {
                    let (placed, report) = light::template(native, lit.color)?
                        .reach(lit.radius)?
                        .add_named(native, graph, lit.seconds, "flight-light")?;
                    Ok((
                        placed,
                        json!({"source":source,"light":{"linear_color":lit.color,
                            "source_seconds":lit.seconds,"radius_metres":lit.radius,"native":report}}),
                    ))
                });
                match result {
                    Ok((placed, report)) => {
                        light = Some(placed);
                        converted.push(report);
                    }
                    Err(error) => {
                        graph.nodes.truncate(start);
                        pending.push(json!({"source":source,"reasons":[format!("{error:#}")]}));
                    }
                }
            }
            Element::Other { source, reason } => {
                pending.push(json!({"source":source,"reasons":[reason]}));
            }
        }
    }
    Ok(ConvertedFlight {
        systems,
        light,
        converted,
        pending,
    })
}
