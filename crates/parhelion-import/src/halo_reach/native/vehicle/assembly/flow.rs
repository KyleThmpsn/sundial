//! The vehicle's private effect flow on a standalone flow envelope: source-sized scalar inputs,
//! marker locators and gated controls. Its gates play the source contrails (`trail`) and create
//! the driver's secondary projectile (`fire::secondary`).
use super::*;
use crate::{
    halo_reach::{Scene, cache::Cache, resource::Pages},
    presentation::append,
    tiger::{
        allocation::Row,
        markers,
        sequence::{self, Kind, Node, Timing},
    },
};

pub(super) const OWNER: u32 = 0x80fc7ee1;
const ENTITY: u32 = 0x80c0dc18;
/// The stock flow whose events supply the object creation and delay bodies.
const CREATION_ENVELOPE: u32 = 0x80b8_25c8;
const CREATE: u32 = 0x8080_8881;
const DELAY: u32 = 0x8080_93cb;

/// FNV-1 of `world_space`, the name stock flows give the kind 1 locator that frames ribbons and
/// world-space particles.
const WORLD_SPACE: u32 = 0x9199_59c0;

/// One source trail attachment: the name of each of its particle events, the marker name of
/// each of its locators and the thrust its gate opens at. Each marker has a locator of its own,
/// so a ribbon system bound to one joins only that marker's samples. Each system has an event of
/// its own, as stock flows hold theirs: the native start gives every later system row of one
/// event the first row's lifetime as a start offset.
pub(super) struct Trail {
    pub events: Vec<u32>,
    pub markers: Vec<u32>,
    pub threshold: f32,
}

/// A secondary projectile created at one marker each time one of `inputs` becomes active, then
/// held for `recovery` seconds while that input stays active.
pub(super) struct Creation {
    pub locator: u32,
    pub inputs: Vec<u32>,
    pub recovery: f32,
}

pub(super) struct Flow {
    pub payload: Payload,
    pub allocation: Payload,
    pub allocation_tag: u32,
    pub registration: Payload,
    /// Definition offsets of the scalar inputs, in the order their channel names were given.
    pub inputs: Vec<usize>,
    /// The particle events of each trail, in the order their names were given.
    pub events: Vec<Vec<usize>>,
    pub gates: Vec<usize>,
    /// The creation event of each creation input.
    pub creations: Vec<usize>,
    /// The locator ordinals of each trail's markers, in marker order.
    pub locators: Vec<Vec<u8>>,
    /// The locator ordinal a creation's marker takes.
    pub creation_locator: Option<u32>,
    /// The ordinal of the `world_space` locator, present when the flow has trails.
    pub world: Option<u8>,
}

fn array(
    payload: &mut Payload,
    field: usize,
    class: u32,
    stride: usize,
    bytes: &[u8],
) -> Result<Vec<usize>> {
    append(&mut payload.0, field, u64::from(class), bytes, stride)?;
    payload.array(field, stride, Some(class))
}

fn inputs(payload: &mut Payload, names: &[u32]) -> Result<Vec<usize>> {
    let root = payload.pointer(16)?;
    let definition = payload.pointer(24)?;
    let instances = array(
        payload,
        root + 0xd0,
        0x80809788,
        96,
        &vec![0; names.len() * 96],
    )?;
    let definitions = array(
        payload,
        definition + 0x188,
        0x80809789,
        40,
        &vec![0; names.len() * 40],
    )?;
    for ((&i, &d), &name) in instances.iter().zip(&definitions).zip(names) {
        for (at, class, twin) in [(i, 0x80809789u32, d), (d, 0x80809788u32, i)] {
            put(&mut payload.0, at, &OWNER.to_le_bytes())?;
            put(&mut payload.0, at + 4, &class.to_le_bytes())?;
            put(&mut payload.0, at + 8, &(twin as u64).to_le_bytes())?;
        }
        sequence::relative(payload, i + 16, root)?;
        // Unbound scalar receiver state, shared with authored flight inputs.
        for offset in [0x20, 0x24, 0x30, 0x38, 0x3c] {
            put(&mut payload.0, i + offset, &u32::MAX.to_le_bytes())?;
        }
        put(&mut payload.0, d + 24, &0x808097c1u64.to_le_bytes())?;
        put(&mut payload.0, d + 32, &name.to_le_bytes())?;
    }
    Ok(definitions)
}

/// Declares one kind 2 marker locator per group and, with `world`, a kind 1 `world_space`
/// locator after them, as stock flows such as the Fallen bolt's `80FF0231` store theirs: a
/// declaration, span and name per locator, a transform and flag row per point, a slot per
/// locator, and one absolute transform for the world locator, whose allocation row `82C28E51`
/// leads the locator rows.
fn locators(payload: &mut Payload, groups: &[(u32, usize)], world: bool) -> Result<Row> {
    let root = payload.pointer(16)?;
    let d = payload.pointer(24)?;
    let markers = groups.iter().map(|(_, count)| count).sum::<usize>();
    let points = markers + usize::from(world);
    let count = groups.len() + usize::from(world);
    ensure!(
        !groups.is_empty() && count <= 255 && points <= 255,
        "Source trail locators exceed native byte selectors"
    );
    let mut declarations = Vec::new();
    let mut spans = Vec::new();
    let mut names = Vec::new();
    let mut first = 0u8;
    for (index, &(name, count)) in groups.iter().enumerate() {
        let count = u8::try_from(count)?;
        ensure!(count > 0, "Source trail locator has no markers");
        declarations.extend([2, first, count, 0, 0x3c, 0, 0, 0]);
        declarations.extend(name.to_le_bytes());
        declarations.extend([0xff, 0, 0, 0]);
        spans.extend([first, count, u8::try_from(index)?, 1, 0x3c, 0]);
        names.extend(name.to_le_bytes());
        first = first
            .checked_add(count)
            .context("Trail locator point overflow")?;
    }
    let mut flags = [0x3c, 0].repeat(markers);
    if world {
        declarations.extend([1, first, 1, 0, 0x5f, 0, 0, 0]);
        declarations.extend(0x811c_9dc5u32.to_le_bytes());
        declarations.extend([0xff, 0, 0, 0]);
        spans.extend([first, 1, u8::try_from(groups.len())?, 1, 0x5f, 0]);
        names.extend(WORLD_SPACE.to_le_bytes());
        flags.extend([0x5f, 0]);
    }
    array(payload, d + 0x1b0, 0x80808617, 6, &spans)?;
    array(payload, d + 0x1c0, 0x80808615, 16, &declarations)?;
    array(payload, d + 0x1d0, 0x80808614, 4, &names)?;
    ensure!(
        payload.u64(root + 0x170)? == 0,
        "Trail host has an absolute transform pool"
    );
    let transforms = [0f32, 0., 0., 1., 0., 0., 0., 1.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    if world {
        array(payload, root + 0x170, 0x80809f75, 32, &transforms)?;
    }
    array(
        payload,
        root + 0x180,
        0x80809f75,
        32,
        &transforms.repeat(points),
    )?;
    array(payload, root + 0x190, 0x80808618, 2, &flags)?;
    let mut slot = [0u8; 24];
    slot[..8].fill(0xff);
    array(payload, root + 0x1a0, 0x80809bff, 24, &slot.repeat(count))?;
    for (index, count) in [usize::from(world), points, points, count]
        .into_iter()
        .enumerate()
    {
        put(
            &mut payload.0,
            d + 0x1e0 + index * 4,
            &u32::try_from(count)?.to_le_bytes(),
        )?;
    }
    let mut rows = Vec::new();
    if world {
        rows.push(Row::named(0x82c28e51, 0x80809f75, 1, Vec::new()));
    }
    rows.extend([
        Row::named(0x4c2e87a1, 0x80809f75, points, Vec::new()),
        Row::named(0x6e385590, 0x80808618, points, Vec::new()),
        Row::named(0xa6e3dd20, 0x80809bff, count, Vec::new()),
    ]);
    Ok(Row::named(0x846fde3f, u32::MAX, 0, rows))
}

fn hash(name: &str) -> u32 {
    super::super::damage::hash(name)
}

/// Builds the flow over `channels`, the bank channel names its scalar inputs read in order. Every
/// trail reads input zero. A creation's locator follows the trails' and holds one marker.
fn build(
    native: &mut Reader,
    channels: &[u32],
    trails: &[Trail],
    creation: Option<&Creation>,
) -> Result<Flow> {
    let registration = (*native.tag(ENTITY, Some(0x80809c0f))?).clone();
    let links = links::Graph::read(&registration, false)?;
    ensure!(
        links.components.contains(&OWNER)
            && links.connections.is_empty()
            && links.named_connections.is_empty(),
        "Standalone trail host has external connections"
    );
    ensure!(
        !channels.is_empty() && (!trails.is_empty() || creation.is_some()),
        "Vehicle effect flow has nothing to play"
    );
    let mut payload = (*native.tag(OWNER, Some(0x80809c36))?).clone();
    let d = payload.pointer(24)?;
    for field in [0x148, 0x178, 0x188, 0x200, 0x210, 0x220, 0x230, 0x2b8] {
        ensure!(
            payload.u64(d + field)? == 0,
            "Standalone trail host has additional dependencies"
        );
    }
    let allocation_tag = payload.u32(0x44)?;
    let mut allocation = (*native.tag(allocation_tag, Some(0x80809bbb))?).clone();
    let inputs = inputs(&mut payload, channels)?;
    // Trail markers first, one locator each, then the creation's marker, then `world_space`.
    let mut groups = Vec::new();
    let mut trail_locators = Vec::new();
    for trail in trails {
        ensure!(
            !trail.markers.is_empty() && !trail.events.is_empty(),
            "Source trail has no markers or events"
        );
        let first = groups.len();
        groups.extend(trail.markers.iter().map(|&name| (name, 1)));
        trail_locators.push(
            (first..groups.len())
                .map(u8::try_from)
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let creation_locator = creation
        .map(|creation| {
            groups.push((creation.locator, 1));
            u32::try_from(groups.len() - 1)
        })
        .transpose()?;
    let world = if trails.is_empty() {
        None
    } else {
        Some(u8::try_from(groups.len())?)
    };
    let locator_allocation = locators(&mut payload, &groups, world.is_some())?;
    Row::write(
        &[
            Row::named(0x86374c3c, 0x80809788, channels.len(), Vec::new()),
            locator_allocation,
        ],
        &mut allocation,
        32,
    )?;
    let mut gates = trails
        .iter()
        .enumerate()
        .map(|(index, trail)| Node {
            name: hash(&format!("source/trail/gate/{index}")),
            timing: Timing {
                flags: 0x10,
                ..Timing::default()
            },
            kind: Kind::Range {
                input: 0,
                bounds: [trail.threshold, f32::MAX],
                children: trail
                    .events
                    .iter()
                    .map(|&name| Node {
                        name,
                        timing: Timing {
                            duration: [1.; 2],
                            flags: 8,
                            ..Timing::default()
                        },
                        kind: Kind::Event(0x80806cc6),
                    })
                    .collect(),
            },
        })
        .collect::<Vec<_>>();
    let mut created = Vec::new();
    for (index, &input) in creation
        .iter()
        .flat_map(|creation| &creation.inputs)
        .enumerate()
    {
        let recovery = creation.map_or(0., |creation| creation.recovery);
        ensure!(
            (input as usize) < channels.len() && recovery.is_finite() && recovery >= 0.,
            "Secondary creation input or recovery is invalid"
        );
        let create = hash(&format!("source/secondary/create/{index}"));
        created.push(create);
        // The gate starts its children each time its input enters the range and retires them when
        // it leaves. A maneuver state is one pulse a press, so each press creates once, and the
        // carrier's maneuver energy holds the source recovery between presses. The delay after
        // the creation keeps a state that stays active from creating again before that recovery.
        gates.push(Node {
            name: hash(&format!("source/secondary/gate/{index}")),
            timing: Timing {
                flags: 0x10,
                ..Timing::default()
            },
            kind: Kind::Range {
                input,
                bounds: [0.5, f32::MAX],
                children: vec![Node {
                    name: hash(&format!("source/secondary/shot/{index}")),
                    timing: Timing::default(),
                    kind: Kind::Serial(vec![
                        Node {
                            name: create,
                            timing: Timing::default(),
                            kind: Kind::Event(CREATE),
                        },
                        Node {
                            name: hash(&format!("source/secondary/recover/{index}")),
                            timing: Timing {
                                duration: [recovery; 2],
                                flags: 4,
                                ..Timing::default()
                            },
                            kind: Kind::Event(DELAY),
                        },
                    ]),
                }],
            },
        });
    }
    let count = gates.len();
    let root = Node {
        name: hash("source/flow/root"),
        timing: Timing::default(),
        kind: Kind::Parallel(gates),
    };
    let mut envelopes = vec![(*native.tag(0x80fc7dfb, Some(0x80809c36))?).clone()];
    if creation.is_some() {
        envelopes.push((*native.tag(CREATION_ENVELOPE, Some(0x80809c36))?).clone());
    }
    let written = sequence::write(&mut payload, &mut allocation, &root, &envelopes)?;
    let controls = payload.array(d + 0x158, 24, Some(0x808093e6))?;
    let parallel = payload.pointer(controls[0] + 16)?;
    payload.0[parallel + 0x31] = 1;
    // The root's gates follow it in the order they were given, each before its own children.
    let mut gate_offsets = Vec::new();
    for row in &controls[1..] {
        let control = payload.pointer(row + 16)?;
        if payload.u32(control - 4)? == 0x808093cd {
            gate_offsets.push(control);
        }
    }
    ensure!(
        gate_offsets.len() == count,
        "Vehicle effect flow gate count differs"
    );
    let events = trails
        .iter()
        .map(|trail| {
            trail
                .events
                .iter()
                .map(|name| written.events[name])
                .collect::<Vec<usize>>()
        })
        .collect::<Vec<_>>();
    let creations = created.iter().map(|name| written.events[name]).collect();
    Ok(Flow {
        payload,
        allocation,
        allocation_tag,
        registration,
        inputs,
        events,
        gates: gate_offsets,
        creations,
        locators: trail_locators,
        creation_locator,
        world,
    })
}

/// What the flow plays: the trail events its owner keeps, the trail report and the secondary
/// weapon report, an object when the flow creates the projectile.
#[derive(Default)]
pub(in crate::halo_reach::native::vehicle) struct Played {
    pub events: BTreeMap<u32, BTreeSet<usize>>,
    pub trails: Value,
    pub secondary: Value,
}

impl Assembly {
    /// Adds the vehicle's private effect flow when the source has contrails or a secondary
    /// projectile for it to play. The trails read the flight thrust at input zero, and the
    /// maneuver states that create the secondary follow it.
    #[expect(
        clippy::too_many_arguments,
        reason = "The flow joins source attachments, native physics and the private assembly"
    )]
    pub fn effect_flow(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        controls: &Value,
        cache: &Cache,
        pages: &mut Pages,
        motion: &motion::Motion,
        secondary: Option<fire::secondary::Secondary<'_>>,
    ) -> Result<Played> {
        let (source, skipped) = trail::attachments(cache, scene)?;
        let untranslated = if skipped.is_empty() {
            Value::Null
        } else {
            json!({"untranslated_effects":skipped})
        };
        if source.is_empty() && secondary.is_none() {
            return Ok(Played {
                trails: untranslated,
                ..Played::default()
            });
        }
        let owner = OWNER;
        ensure!(
            !self.payloads.contains_key(&owner),
            "Vehicle effect flow owner is already present"
        );
        let thrust = if source.is_empty() {
            None
        } else {
            Some(self.thrust_channel(native, graph, motion)?)
        };
        let (trails, threshold) = trail::gates(&source, controls)?;
        let states: &[Channel] = match &secondary {
            Some(secondary) => secondary.states,
            None => &[],
        };
        let inputs = thrust.iter().chain(states).collect::<Vec<_>>();
        let first_state = u32::try_from(inputs.len() - states.len())?;
        let creation = secondary
            .as_ref()
            .map(|secondary| fire::secondary::creation(secondary, first_state))
            .transpose()?;
        let mut flow = build(
            native,
            &inputs
                .iter()
                .map(|channel| channel.name)
                .collect::<Vec<_>>(),
            &trails,
            creation.as_ref(),
        )?;
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Vehicle effect flow source rig")?,
        )?;
        let mut markers = Vec::new();
        let mut resources = Vec::new();
        let attachments = trail::write(
            native,
            graph,
            pages,
            &trail::Inputs {
                scene,
                bones,
                controls,
                cache,
                first,
            },
            &mut flow,
            &source,
            &trails,
            &mut markers,
            &mut resources,
        )?;
        let mut secondary_report = Value::Null;
        if let (Some(secondary), Some(creation)) = (&secondary, &creation) {
            let locator = flow
                .creation_locator
                .context("Vehicle effect flow has no creation locator")?;
            let written = fire::secondary::write(
                &mut flow, secondary, creation, scene, bones, first, locator,
            )?;
            markers.push(written.marker);
            resources.extend(written.resources);
            secondary_report = written.report;
            secondary_report["maneuver"] = self.maneuver_fire(creation.recovery)?;
        }
        let symbol = "vehicle-effect-flow";
        let end = flow.payload.0.len();
        put(&mut flow.payload.0, 0, &(end as u64).to_le_bytes())?;
        graph.add(symbol, owner, &flow.payload.0, None, Vec::new())?;
        let ordinal = self.links.components.len();
        self.links.components.push(owner);
        self.symbols.insert(owner, symbol.into());
        self.payloads.insert(owner, flow.payload);
        self.registration_entities.insert(owner, flow.registration);
        self.set_allocation(graph, owner, flow.allocation_tag, &flow.allocation)?;
        self.runtime_end(owner, end)?;
        for (offset, symbol) in resources {
            self.resource(owner, offset, symbol)?;
        }
        for (&input, channel) in flow.inputs.iter().zip(&inputs) {
            self.links.connections.push(links::Connection {
                consumer: links::Endpoint {
                    namespace: 0x811c9dc5,
                    object: Some(links::Object {
                        owner,
                        class: 0x80809789,
                        offset: input as u64,
                    }),
                    selector: ordinal as u64,
                },
                provider: channel.provider.clone(),
                channel: channel.index,
                flags: 0,
            });
        }
        let marker_owner = self.owner(markers::NATIVE_COMPONENT)?;
        let payload = markers::append_skeletal(
            &self.payloads[&marker_owner],
            &markers,
            bones["bones"]
                .as_array()
                .context("Vehicle effect flow target rig")?
                .len(),
        )?;
        self.payloads.insert(marker_owner, payload);
        let trails_report = if source.is_empty() {
            untranslated
        } else {
            json!({"owner":format!("{owner:08X}"),"attachments":attachments,"untranslated_effects":skipped,
                "thrust_threshold":threshold,"input":flow.inputs[0],"gate":flow.gates[0],
                "parallel_entry":0,"independent_flow":true,"gameplay_verified":false,
                "limits":["Additive source profiles draw the Fallen bolt's single ribbon from each marker, in the source colors, sample lifetime and half width, framed by the flow's world_space locator. Other profiles keep ordinary CPU samples with camera-facing quads. Thrust gates are authored independently of the vehicle carrier. Light volumes are not translated."]})
        };
        Ok(Played {
            events: BTreeMap::from([(owner, flow.events.into_iter().flatten().collect())]),
            trails: trails_report,
            secondary: secondary_report,
        })
    }
}
