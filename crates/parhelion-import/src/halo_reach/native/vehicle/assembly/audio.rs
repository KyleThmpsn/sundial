//! Source engine loops on the carrier's native engine flow, gated by its occupancy gate and
//! pitched by the translated or retained drive's engine output.
//!
//! The engine flow is the carrier's effect flow (`808084E9`) whose occupancy gate, a range control
//! (`808093CD`) on its `vehicle_power` input (`157AA4E5`), starts a sound event (`80806B38`)
//! directly. In Shadowkeep `86657.20.08.23.1800.d2_rc` these are the Heavy Pike's `80F2F25A`, the
//! Interceptor's `80FC6BAB` and the tank's `80F8310E`. The gate then starts only that event, whose
//! cue plays the source loops, so the carrier's own engine sound and the other cues under the gate
//! stop. The event passes one flow input to the private bank as its game parameter, fed by:
//!
//! - a translated car, the car's RPM output (`80803CE6` method 7)
//! - a translated flight, the flight thrust output (`80803CF2` method 9), at full from cruise thrust
//! - a retained hover drive, its engine output (`80803CDE` method 14, `084044E3`), the value the
//!   Heavy Pike's RPM channel forwards and from which the Interceptor's native engine pitch follows
//! - a retained tread drive, the body's linear speed (`808092D8` method 26), at full from the
//!   drive's highest gear ratio times its engine speed limit. With the stock tank's one forward gear
//!   that speed is the engine speed's fraction of its limit.
use super::*;
use crate::{
    halo_reach::Scene,
    presentation::append,
    tiger::audio::bank::{
        Bank,
        looping::{Curve, Layer, Phase},
    },
};

/// The Heavy Pike engine flow's RPM input. Its stock channel forwards the hover engine output.
const RPM: u32 = 0x5daeb921;
/// `vehicle_power`, the occupancy every checked engine flow gates on.
const OCCUPIED: u32 = 0x157aa4e5;
const BUS: u32 = 2_945_946_222;
const FLOW: u32 = 0x808084e9;
const SOUND: u32 = 0x80806b38;
const RANGE: u32 = 0x808093cd;
const HOVER: u32 = 0x80803cde;
const TREAD: u32 = 0x80803caa;
const BODY: u32 = 0x808092d8;
/// The source vehicle functions a Reach engine loop is scaled by, exported by its physics.
const ENGINE_FUNCTIONS: [&str; 2] = ["engine_rpm", "engine_power"];
/// The source function that silences an engine loop without a driver, as the native occupancy gate
/// does.
const DRIVER_OCCUPIED: &str = "driver_seat_occupied";
/// A source sound class that among the MCC U13 vehicles' loops holds only distant LOD copies, such
/// as the Banshee's, Ghost's, Falcon's, Pelican's, Phantom's and Scorpion's.
const ENGINE_LOD_CLASS: u64 = 25;

fn scalar(value: &Value, label: &str) -> Result<f32> {
    let value = value.as_f64().with_context(|| format!("Engine {label}"))? as f32;
    ensure!(value.is_finite(), "Engine {label} is not finite");
    Ok(value)
}

/// The fade's duration. Source modes one to three select curve shapes whose meaning
/// is not established, so the native action keeps its linear curve for every mode.
fn fade(value: &Value) -> Result<u32> {
    let seconds = scalar(&value["seconds"], "fade")?;
    ensure!(
        value["mode"].as_u64().is_some_and(|mode| mode <= 3) && (0.0..=60.0).contains(&seconds),
        "Unsupported engine fade"
    );
    Ok((seconds * 1000.).round() as u32)
}

/// A range's recording pitch, then its bend bounds, which order ranges that share a recording
/// pitch, such as the Banshee's idle and hi at 200 cents.
fn pitch_key(range: &Value) -> Result<[f32; 3]> {
    let parameters = &range["parameters"];
    Ok([
        scalar(&parameters["natural_pitch_cents"], "recording pitch")?,
        scalar(&parameters["bend_bounds_cents"][0], "lower bend")?,
        scalar(&parameters["bend_bounds_cents"][1], "upper bend")?,
    ])
}

/// The engine's pitch ranges from lowest to highest recording pitch. The Warthog's four are idle,
/// low, med and hi. Other engines record one to three, such as the forklift's idle and hi.
fn ordered(ranges: &[Value]) -> Result<Vec<&Value>> {
    let mut ordered = ranges
        .iter()
        .map(|range| Ok((pitch_key(range)?, range)))
        .collect::<Result<Vec<_>>>()?;
    let order = |a: &[f32; 3], b: &[f32; 3]| {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.total_cmp(b))
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    ordered.sort_by(|a, b| order(&a.0, &b.0));
    ensure!(
        ordered
            .windows(2)
            .all(|pair| order(&pair[0].0, &pair[1].0).is_lt()),
        "Engine pitch ranges share a recording pitch and bend bounds"
    );
    Ok(ordered.into_iter().map(|(_, range)| range).collect())
}

/// The pitch the blend sweeps, from the lowest range's recording pitch to the highest range's
/// upper bend. A sweep that does not rise, as with the Scorpion's ranges, whose bend bounds lie
/// below their recording pitches, or a single range without bend, keeps each recording's pitch.
fn pitch_span(ordered: &[&Value]) -> Result<Option<[f32; 2]>> {
    let idle = ordered.first().context("Engine idle range")?;
    let high = ordered.last().context("Engine high range")?;
    let pitch = [
        scalar(&idle["parameters"]["natural_pitch_cents"], "idle pitch")?,
        scalar(&high["parameters"]["bend_bounds_cents"][1], "high pitch")?,
    ];
    Ok((pitch[1] > pitch[0]).then_some(pitch))
}

/// The driver parameter and the value it reaches at full engine, where the blend ends.
#[derive(Clone, Copy)]
struct Sweep {
    parameter: u32,
    full: f32,
}

fn curves(
    range: &Value,
    index: usize,
    count: usize,
    pitch: Option<[f32; 2]>,
    sweep: Sweep,
) -> Result<Vec<Curve>> {
    // An authored blend that spaces the ranges evenly over engine speed, from the lowest
    // recording pitch to the highest. The source's inverted gain bounds are preserved in the
    // receipt and are not treated as a proven curve.
    let mut curves = Vec::new();
    if count > 1 {
        let spacing = (count - 1) as f32;
        curves.push(Curve {
            parameter: sweep.parameter,
            target: 0,
            points: (0..=12)
                .map(|point| {
                    let x = point as f32 / 12.;
                    let weight = (1. - (x * spacing - index as f32).abs()).max(0.);
                    let gain = if weight == 0. {
                        -96.
                    } else {
                        20. * (weight * std::f32::consts::FRAC_PI_2).sin().log10()
                    };
                    [x * sweep.full, gain]
                })
                .collect(),
        });
    }
    if let Some(pitch) = pitch {
        let natural = scalar(
            &range["parameters"]["natural_pitch_cents"],
            "recording pitch",
        )?;
        curves.push(Curve {
            parameter: sweep.parameter,
            target: 2,
            points: vec![[0., pitch[0] - natural], [sweep.full, pitch[1] - natural]],
        });
    }
    Ok(curves)
}

fn sound_layers(source: &Value, track: &Value, phase: Phase, sweep: Sweep) -> Result<Vec<Layer>> {
    let gain = scalar(&track["gain_db"], "track gain")?
        + scalar(&source["playback"]["gain_db"], "sound gain")?;
    let ranges = source["pitch_ranges"]
        .as_array()
        .context("Engine pitch ranges")?;
    ensure!(!ranges.is_empty(), "Engine sound has no pitch ranges");
    let sustain = matches!(phase, Phase::Sustain);
    let order = if sustain {
        ordered(ranges)?
    } else {
        Vec::new()
    };
    let pitch = if sustain { pitch_span(&order)? } else { None };
    let mut seen = BTreeSet::new();
    ranges
        .iter()
        .map(|range| {
            let curves = if sustain {
                ensure!(
                    seen.insert(range["name"].as_str()),
                    "Repeated engine pitch range"
                );
                let index = order
                    .iter()
                    .position(|item| std::ptr::eq(*item, range))
                    .context("Engine pitch range order")?;
                curves(range, index, order.len(), pitch, sweep)?
            } else {
                Vec::new()
            };
            let variants = range["variants"].as_array().context("Engine variations")?;
            ensure!(!variants.is_empty(), "Engine range has no recordings");
            let variation_gain = scalar(&variants[0]["gain_encoded"], "variation gain")?;
            // A variation's skip fraction lowers how often it plays, which the native random
            // container's equal weights do not express. The Phantom's hover loop holds one.
            ensure!(
                variants
                    .iter()
                    .all(|variant| variant["gain_encoded"] == variants[0]["gain_encoded"]),
                "Engine variations require individual gains"
            );
            let media = variants
                .iter()
                .map(|variant| {
                    u32::try_from(
                        variant["fsb_info"]
                            .as_u64()
                            .context("Engine recording ID")?,
                    )
                    .map_err(Into::into)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Layer {
                media,
                gain_db: gain + variation_gain,
                phase,
                fade_in_ms: if sustain { fade(&track["fade_in"])? } else { 0 },
                fade_out_ms: fade(&track["fade_out"])?,
                curves,
            })
        })
        .collect()
}

/// A source engine track and the distance band, in metres, its looping sound plays over.
struct Track<'a> {
    source: &'a Value,
    track: &'a Value,
    band: [f32; 2],
    /// Whether Reach keeps the loop playing without a driver.
    driverless: bool,
}

/// The source engine tracks: the looping sounds the object attaches that its engine scales,
/// directly or through one object function, and that only lose their driver to silence. A distant
/// copy is left out, a loop of the engine LOD class or one that starts farther than another engine
/// loop and either takes over where that loop falls silent or reaches no farther. The Warthog's
/// `warthog_engine_lod` and the Sabre's `sabre_eng_lod` are copies, while the Seraph's main loop,
/// beyond its close layer, is not. Loops scaled by boost, turning, reversing, vertical thrust,
/// treads or contrails follow inputs this flow does not have.
fn engine_tracks<'a>(
    scene: &'a Scene,
    behavior: &'a Value,
) -> Result<(Vec<Track<'a>>, Vec<Value>)> {
    let nodes = behavior["nodes"]
        .as_object()
        .context("Engine source nodes")?;
    let mut seen = BTreeSet::new();
    let mut loops = Vec::new();
    let mut omitted = Vec::new();
    for attachment in &scene.object.attachments {
        let tag = &attachment["tag"];
        if tag["group"] != "lsnd" {
            continue;
        }
        let id = tag["datum"].as_u64().context("Engine loop identity")?;
        // One looping sound attached at several markers still plays once on the vehicle.
        if !seen.insert(id) {
            continue;
        }
        let node = nodes
            .get(&format!("{id:08X}"))
            .context("Engine loop closure")?;
        let scale = attachment["primary_scale"].as_str().unwrap_or("");
        let (input, off) = scene
            .object
            .functions
            .iter()
            .find(|function| function["output"] == scale)
            .map_or((scale, ""), |function| {
                (
                    function["input"].as_str().unwrap_or(""),
                    function["turn_off_with"].as_str().unwrap_or(""),
                )
            });
        if !ENGINE_FUNCTIONS.contains(&input) || !(off.is_empty() || off == DRIVER_OCCUPIED) {
            omitted.push(json!({"loop":tag["path"],"scale":scale,"input":input,"turn_off_with":off,
                "reason":"Not scaled by the source engine, or silenced by a function the native occupancy gate does not express"}));
            continue;
        }
        let bounds = &node["data"]["distance_bounds_source"];
        let band = [
            scalar(&bounds[0], "near distance")? * super::super::METRES,
            scalar(&bounds[1], "far distance")? * super::super::METRES,
        ];
        loops.push((tag, &node["data"], band, off.is_empty()));
    }
    let bands = loops.iter().map(|row| row.2).collect::<Vec<_>>();
    let mut tracks = Vec::new();
    for (tag, data, band, driverless) in loops {
        let lod = data["class"].as_u64() == Some(ENGINE_LOD_CLASS);
        let copy = bands
            .iter()
            .any(|other| other[0] < band[0] && (band[0] >= other[1] || band[1] <= other[1]));
        if lod || copy {
            omitted.push(json!({"loop":tag["path"],"distance_metres":band,
                "reason":if lod {"The source engine LOD class"} else {"A distant copy of a nearer engine loop"}}));
            continue;
        }
        for track in data["track_controls"].as_array().context("Engine tracks")? {
            if track["loop"].is_null() {
                omitted.push(
                    json!({"loop":tag["path"],"track":track["name"],"reason":"The track has no loop"}),
                );
                continue;
            }
            ensure!(
                track["output_effect"] == 0
                    && track["layer"] == 0
                    && track["transition_in"].is_null()
                    && track["transition_out"].is_null(),
                "Engine track requires source transitions or effects"
            );
            tracks.push(Track {
                source: tag,
                track,
                band,
                driverless,
            });
        }
    }
    Ok((tracks, omitted))
}

/// The native signal that stands in for the source engine function.
struct Drive {
    source: links::Object,
    value_type: u16,
    /// Its value at full engine, where the authored blend reaches the highest range.
    full: f32,
    report: Value,
}

/// The carrier's engine flow and the parts of it this translation uses.
struct Flow {
    owner: u32,
    definition: usize,
    inputs: Vec<usize>,
    /// The occupancy gate's definition and the controls from the flow's entry down to it.
    gate: usize,
    path: Vec<usize>,
    /// The gate's sound event, by ordinal and definition.
    event: usize,
    event_at: usize,
}

fn children(payload: &Payload, control: usize) -> Result<Vec<u32>> {
    payload
        .array(control + 0x38, 4, Some(0x808093fb))?
        .into_iter()
        .map(|at| payload.u32(at))
        .collect()
}

/// The controls from `from` down to `target`, following control children only.
fn descend(
    payload: &Payload,
    controls: &[usize],
    from: usize,
    target: usize,
    depth: usize,
) -> Result<Option<Vec<usize>>> {
    if from == target {
        return Ok(Some(vec![from]));
    }
    if depth >= controls.len() {
        return Ok(None);
    }
    for selector in children(payload, controls[from])? {
        let next = (selector >> 16) as usize;
        if selector & 0xffff != 0 || next >= controls.len() {
            continue;
        }
        if let Some(mut path) = descend(payload, controls, next, target, depth + 1)? {
            path.insert(0, from);
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// The motion output a translated drive exports under `symbol`.
fn output(
    graph: &Graph,
    motion: &motion::Motion,
    symbol: &str,
    implementation: u32,
    method: u32,
) -> Result<links::Object> {
    let offset = patches(graph, "vehicle-motion")?
        .iter()
        .find(|patch| patch["symbol"] == symbol)
        .and_then(|patch| patch["offset"].as_u64())
        .and_then(|offset| offset.checked_sub(8))
        .with_context(|| format!("Engine output {symbol}"))?;
    let object = links::Object {
        owner: motion::OWNER,
        class: 0x80809ae1,
        offset,
    };
    let contract = links::Interface::read(&motion.payload, object, &graph.read(symbol)?, false)?;
    ensure!(
        contract.methods.len() == 1
            && contract.methods[0].index == method
            && contract.methods[0].implementation_class == implementation,
        "Engine output {symbol} differs"
    );
    Ok(object)
}

impl Assembly {
    fn has(&self, class: u32) -> bool {
        self.payloads.values().any(|payload| {
            payload
                .pointer(16)
                .ok()
                .and_then(|at| payload.u32(at.checked_sub(4)?).ok())
                == Some(class)
        })
    }

    /// The highest speed the retained tread drive reaches: its highest gear ratio times its
    /// engine's speed limit, the engine (`80803CF7`) at definition `+0x370` with its limit at
    /// `+0x34` and its `80803CFA` gears at `+0x48`, the layout the car translation reads.
    fn tread_speed(&self) -> Result<f32> {
        let owner = self.owner(TREAD)?;
        let payload = &self.payloads[&owner];
        let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
        let engine = definition + 0x370;
        ensure!(
            payload.u32(definition - 4)? == 0x80803cb2 && payload.u32(engine + 4)? == 0x80803cf7,
            "Tread engine layout differs"
        );
        let limit = payload.f32(engine + 0x34)?;
        let mut ratio = 0f32;
        for gear in payload.array(engine + 0x48, 0x88, Some(0x80803cfa))? {
            ratio = ratio.max(payload.f32(gear + 0x80)?);
        }
        ensure!(
            limit > 0. && ratio > 0.,
            "Tread engine has no forward speed"
        );
        Ok(limit * ratio)
    }

    fn drive(
        &self,
        native: &mut Reader,
        graph: &Graph,
        motion: Option<&motion::Motion>,
    ) -> Result<Drive> {
        if let Some(motion) = motion {
            return match motion.payload.u32(motion::INSTANCE - 4)? {
                0x80803ce6 => Ok(Drive {
                    source: output(graph, motion, "vehicle-car-output-1", 0x80803ce6, 7)?,
                    value_type: 0,
                    full: 1.,
                    report: json!({"kind":"car","provider":"Car RPM output, method 7","full":1}),
                }),
                0x80803cf2 => {
                    let cruise = motion.report["cruise_thrust_fraction"]
                        .as_f64()
                        .context("Flight cruise thrust")? as f32;
                    ensure!(
                        cruise.is_finite() && cruise > 0.,
                        "Flight cruise thrust is outside the engine domain"
                    );
                    Ok(Drive {
                        source: output(
                            graph,
                            motion,
                            "vehicle-flight-thrust-output",
                            0x80803cf2,
                            9,
                        )?,
                        value_type: 0,
                        full: cruise,
                        report: json!({"kind":"flight","provider":"Flight thrust output, method 9","full":cruise,
                            "limits":["Reach pitches the loops by its engine function, which follows the source physics. The native sweep follows flight thrust, idle at zero and full from cruise, so boost holds the highest range."]}),
                    })
                }
                class => anyhow::bail!("Vehicle motion {class:08X} has no engine output"),
            };
        }
        if self.has(HOVER) {
            return Ok(Drive {
                source: self.numeric_provider(native, HOVER, 14)?,
                value_type: 0,
                full: 1.,
                report: json!({"kind":"hover","provider":"Retained hover engine output 084044E3, method 14","full":1,
                    "limits":["The hover engine output's fraction is inferred from the stock flows, the Heavy Pike's RPM gates at 0.2 and the Interceptor's engine pitch over zero to one, not from a decoded consumer."]}),
            });
        }
        let full = self.tread_speed()?;
        Ok(Drive {
            source: self.numeric_provider(native, BODY, 26)?,
            value_type: 4,
            full,
            report: json!({"kind":"tread","provider":"Body linear speed, method 26","full_metres_per_second":full,
                "limits":["The stock tank's engine sound also follows linear speed. A tread that slips or a drive with several forward gears separates speed from engine speed."]}),
        })
    }

    /// The carrier's engine flow, the one effect flow whose occupancy gate starts a sound event
    /// directly, reached from the flow's entry through controls. Flows with other events, such as
    /// delays, are not engine sound flows.
    fn engine_flow(&self) -> Result<Option<Flow>> {
        let mut found = Vec::new();
        for (&owner, payload) in &self.payloads {
            if !self.links.components.contains(&owner) {
                continue;
            }
            let definition = usize::try_from(payload.u64(payload.pointer(16)? + 8)?)?;
            if payload.u32(definition - 4)? != FLOW {
                continue;
            }
            let inputs = payload.array(definition + 0x188, 40, Some(0x80809789))?;
            let Some(occupied) = inputs
                .iter()
                .position(|&row| payload.u32(row + 32).ok() == Some(OCCUPIED))
            else {
                continue;
            };
            let events = payload
                .array(definition + 0x168, 24, Some(0x808093e6))?
                .into_iter()
                .map(|row| payload.pointer(row + 16))
                .collect::<Result<Vec<_>>>()?;
            if events.is_empty()
                || !events
                    .iter()
                    .all(|&at| payload.u32(at - 4).ok() == Some(SOUND))
            {
                continue;
            }
            let controls = payload
                .array(definition + 0x158, 24, Some(0x808093e6))?
                .into_iter()
                .map(|row| payload.pointer(row + 16))
                .collect::<Result<Vec<_>>>()?;
            let entry = payload.u32(definition + 0x2d0)?;
            for (index, &control) in controls.iter().enumerate() {
                if payload.u32(control - 4)? != RANGE
                    || payload.u32(control + 0x60)? as usize != occupied
                {
                    continue;
                }
                // The event's own parent is the gate, so its completion returns to it.
                let parent = u32::try_from(index)? << 16;
                let mut event = None;
                for selector in children(payload, control)? {
                    let ordinal = (selector >> 16) as usize;
                    if selector & 0xffff == 1
                        && ordinal < events.len()
                        && payload.u32(events[ordinal] + 0x14)? == parent
                    {
                        event = Some(ordinal);
                        break;
                    }
                }
                let Some(event) = event else {
                    continue;
                };
                if entry & 0xffff != 0 || (entry >> 16) as usize >= controls.len() {
                    continue;
                }
                let Some(path) = descend(payload, &controls, (entry >> 16) as usize, index, 0)?
                else {
                    continue;
                };
                found.push(Flow {
                    owner,
                    definition,
                    inputs: inputs.clone(),
                    gate: control,
                    path: path.into_iter().map(|ordinal| controls[ordinal]).collect(),
                    event,
                    event_at: events[event],
                });
            }
        }
        ensure!(found.len() <= 1, "Native engine flow is ambiguous");
        Ok(found.pop())
    }

    /// The flow input that carries the drive: the Heavy Pike flow's RPM input, or else the
    /// event's first input that no control on the way to it reads.
    fn engine_input(&self, flow: &Flow) -> Result<usize> {
        let payload = &self.payloads[&flow.owner];
        let mut gated = BTreeSet::new();
        for &control in &flow.path {
            if payload.u32(control - 4)? == RANGE {
                gated.insert(payload.u32(control + 0x60)? as usize);
            }
            if payload.u64(control + 0x48)? != 0 {
                let condition = payload.pointer(control + 0x48)?;
                gated.insert(payload.u32(condition + 0xc)? as usize);
            }
        }
        let rpm = flow
            .inputs
            .iter()
            .position(|&row| payload.u32(row + 32).ok() == Some(RPM));
        let input = match rpm {
            Some(rpm) => rpm,
            None => payload
                .array(flow.event_at + 0x48, 4, Some(0x80800007))?
                .into_iter()
                .map(|at| payload.u32(at).map(|value| value as usize))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .find(|input| !gated.contains(input))
                .context("Native engine event has no input free to carry the engine")?,
        };
        ensure!(
            input < flow.inputs.len() && !gated.contains(&input),
            "Native engine input is gated"
        );
        Ok(input)
    }

    /// Replace the carrier's engine sound with the source engine loops, or remove the carrier's
    /// engine flow when the source has none, as with a stationary gun. `motion` is the translated
    /// drive, `None` on a retained carrier.
    pub fn engine_audio(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        behavior: &Value,
        motion: Option<&motion::Motion>,
    ) -> Result<Value> {
        let (tracks, omitted) = engine_tracks(scene, behavior)?;
        let flow = self.engine_flow()?;
        if tracks.is_empty() {
            let Some(flow) = flow else {
                return Ok(json!({"status":"none","omitted_loops":omitted,
                    "reason":"The source has no engine loop and the carrier no engine flow"}));
            };
            self.remove_component(flow.owner)?;
            return Ok(
                json!({"status":"silenced","flow_owner":format!("{:08X}", flow.owner),
                "omitted_loops":omitted,
                "reason":"The source has no engine loop, so the carrier's engine flow leaves the vehicle"}),
            );
        }
        let flow = flow.context("Native engine flow is absent")?;
        let drive = self.drive(native, graph, motion)?;
        let input = self.engine_input(&flow)?;
        let owner = flow.owner;
        let parameter = self.payloads[&owner].u32(flow.inputs[input] + 32)?;
        let sweep = Sweep {
            parameter,
            full: drive.full,
        };
        let nodes = behavior["nodes"]
            .as_object()
            .context("Engine source nodes")?;
        let sound = |tag: &Value| -> Result<&Value> {
            let id = tag["datum"].as_u64().context("Engine track sound")?;
            let node = nodes
                .get(&format!("{id:08X}"))
                .context("Engine sound closure")?;
            ensure!(
                node["tag"]["group"] == "snd!",
                "Engine track is not a sound"
            );
            Ok(&node["data"])
        };
        let mut layers = Vec::new();
        let mut bands = Vec::new();
        let mut skips = 0;
        let mut source = Vec::new();
        for track in &tracks {
            for (key, phase) in [
                ("start", Phase::Start),
                ("loop", Phase::Sustain),
                ("end", Phase::Stop),
            ] {
                if track.track[key].is_null() {
                    continue;
                }
                let data = sound(&track.track[key])?;
                skips += data["pitch_ranges"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|range| range["variants"].as_array().into_iter().flatten())
                    .filter(|variant| variant["skip_fraction_encoded"] != 0)
                    .count();
                let added = sound_layers(data, track.track, phase, sweep)?;
                bands.extend(std::iter::repeat_n(track.band, added.len()));
                layers.extend(added);
            }
            source.push(
                json!({"loop":track.source,"track":track.track,"distance_metres":track.band}),
            );
        }
        let original = &self.payloads[&owner];
        let event = flow.event_at;
        let cue_tag = original.u32(event + 0x40)?;
        let cue = native.tag(cue_tag, Some(0x80809802))?;
        ensure!(
            ![0, u32::MAX, 0x811c9dc5].contains(&cue.u32(12)?),
            "Engine cue has no stop event"
        );
        let bank_template = cue.u32(0x14)?;
        let native_bank = native.tag(bank_template, None)?;
        crate::halo_reach::audio::verify_bus(&native_bank.0, BUS)?;
        let media_template = cue.u32(
            *cue.array(0x18, 4, Some(0x80800014))?
                .first()
                .context("Engine media template")?,
        )?;
        let ids = layers
            .iter()
            .flat_map(|layer| layer.media.iter().copied())
            .collect::<BTreeSet<_>>();
        let source_root = graph
            .root
            .parent()
            .context("Engine source directory")?
            .join("source/audio");
        let mut media = Vec::new();
        for id in &ids {
            let symbol = format!("vehicle-engine-media-{id:08X}");
            let bytes = crate::tiger::audio::transcode::normalize_pcm_wem(&std::fs::read(
                source_root.join(format!("{id:08X}.wav")),
            )?)?;
            graph.add(&symbol, media_template, &bytes, None, Vec::new())?;
            graph.nodes.last_mut().unwrap()["storage"] = json!("audio_media");
            media.push(json!({"id":id,"symbol":symbol}));
        }
        // Generated banks all use bank 1 and objects from 1, and the item build checks every bank
        // of a graph in one linked namespace. A flight vehicle's graph also holds its launch cue's
        // bank, and the Banshee's stage failed on their shared object 1 (2026-11-06). The engine
        // bank takes its own 8192-wide range, as `once` cues do, and its events move with it.
        let base = 0x1000_0000 | (crate::tiger::audio::bank::hash("vehicle-engine") & 0x0FFF_E000);
        let bank = Bank::looping_bands(&layers, &bands, BUS)?.rebased(base)?;
        graph.add(
            "vehicle-engine-bank",
            bank_template,
            &bank.bytes,
            None,
            Vec::new(),
        )?;
        let node = graph.nodes.last_mut().unwrap();
        node["storage"] = json!("audio_bank");
        node["audio_bank"] = serde_json::to_value(&bank)?;
        node["audio_media"] = json!(media);
        ensure!(
            cue.0.len() >= 0x50 && cue.pointer(0x20)? == 0x40,
            "Engine cue envelope differs"
        );
        let mut bytes = cue.0[..0x50].to_vec();
        bytes.resize(0x50 + ids.len() * 4, 0xff);
        let size = bytes.len() as u64;
        put(&mut bytes, 0, &size.to_le_bytes())?;
        put(&mut bytes, 8, &(base + 1).to_le_bytes())?;
        put(&mut bytes, 12, &(base + 2).to_le_bytes())?;
        put(&mut bytes, 20, &u32::MAX.to_le_bytes())?;
        for at in [0x18, 0x40] {
            put(&mut bytes, at, &(ids.len() as u64).to_le_bytes())?;
        }
        let mut patches = vec![json!({"offset":20,"symbol":"vehicle-engine-bank"})];
        for (index, id) in ids.iter().enumerate() {
            patches.push(
                json!({"offset":0x50+index*4,"symbol":format!("vehicle-engine-media-{id:08X}")}),
            );
        }
        graph.add("vehicle-engine-cue", cue_tag, &bytes, None, patches)?;
        graph.nodes.last_mut().unwrap()["audio_events"] =
            json!([{"offset":8,"id":base + 1},{"offset":12,"id":base + 2}]);
        self.resource(owner, event + 0x40, "vehicle-engine-cue".into())?;
        let channel = self
            .channel_bridge(
                native,
                graph,
                "vehicle-engine-input",
                &[(drive.source, drive.value_type)],
            )?
            .remove(0);
        let consumer = flow.inputs[input] as u64;
        let mut rewired = 0;
        for connection in self
            .links
            .connections
            .iter_mut()
            .chain(&mut self.links.named_connections)
        {
            if connection
                .consumer
                .object
                .is_some_and(|value| value.owner == owner && value.offset == consumer)
            {
                connection.provider = channel.provider.clone();
                connection.channel = channel.index;
                rewired += 1;
            }
        }
        ensure!(rewired == 1, "Engine input binding is absent or ambiguous");
        let payload = self.payloads.get_mut(&owner).unwrap();
        // The gate starts only the engine event, and the event passes only the drive.
        let selector = (u32::try_from(flow.event)? << 16) | 1;
        append(
            &mut payload.0,
            flow.gate + 0x38,
            0x808093fb,
            &selector.to_le_bytes(),
            4,
        )?;
        append(
            &mut payload.0,
            event + 0x48,
            0x80800007,
            &u32::try_from(input)?.to_le_bytes(),
            4,
        )?;
        let symbol = &self.symbols[&owner];
        let declared = super::patches(graph, symbol)?;
        // Only immutable definition arrays were appended. The existing mutable
        // event instances keep their original allocation and copied span.
        let length = payload.0.len() as u64;
        put(&mut payload.0, 0, &length.to_le_bytes())?;
        graph.replace(symbol, &payload.0, declared)?;
        let mut limits = vec![json!(
            "The source PCM, track gains, distance bands and lifecycle fade durations are translated. Nonzero source fade modes keep a linear native curve. Pitch-range selection is an authored blend, not verified Reach playback parity. Coasting alternate tracks, distant LOD copies and source scale function curves are not used."
        )];
        if tracks.iter().any(|track| track.driverless) {
            limits.push(json!("Reach plays some of these loops without a driver. The native occupancy gate plays them only while the vehicle is occupied."));
        }
        if skips > 0 {
            limits.push(json!("Source variation skip fractions are not translated, so every variation plays at equal weight."));
        }
        if let Some(drive_limits) = drive.report["limits"].as_array() {
            limits.extend(drive_limits.iter().cloned());
        }
        let mut distinct = Vec::new();
        for band in bands {
            if !distinct.contains(&band) {
                distinct.push(band);
            }
        }
        Ok(
            json!({"status":"translated","source_tracks":source,"omitted_loops":omitted,
            "layers":layers.len(),"recordings":ids.len(),"distance_bands_metres":distinct,
            "flow_owner":format!("{owner:08X}"),"flow_definition":flow.definition,"event":event,"event_ordinal":flow.event,
            "engine_input":input,"engine_parameter":format!("{parameter:08X}"),"engine_channel":channel.index,
            "drive":drive.report,"skipped_variations":skips,"output_bus":BUS,
            "blend":"Authored crossfades of the source pitch ranges, spaced evenly over the native engine drive",
            "gameplay_verified":false,"limits":limits}),
        )
    }
}
