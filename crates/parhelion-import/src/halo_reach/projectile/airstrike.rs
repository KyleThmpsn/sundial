//! A source projectile whose contact calls the scenario's airstrike, translated as a private strike
//! host in the marker's finish graph.
//!
//! A Material Response row of response 8, Airstrike, hands the contact point to the airstrike
//! system. The scenario names that system's definition at `scnr +0x2E0`, Airstrike Globals, an
//! `airs` tag whose battery rows hold launch radius, height, warmup, arrival, cooldown and
//! duration, shots per launch, initial launches, the fire effect, the fire offsets and the request
//! weapon. The fire effect's object part creates the bomb projectile at its `gravity` location.
//! The behavior closure follows the definition and the fire effect, so the bomb is a source
//! projectile of the weapon.
//!
//! The marker's finish graph at movement definition `+0x180` becomes a private copy of the stock
//! rocket's detonation emitter `80BBD11C`. That kind-17 graph's effect flow creates cluster
//! bomblets at its `down` locator and tracking missiles at its `up` locator through kind-15
//! creation events. The copy's controls are rebuilt to create the converted bomb once per shot:
//! after the battery's warmup, evenly over the launch duration, at the launch height above the
//! mark and each fire offset in turn, launched down at the bomb's source speed. The host then
//! retires. The bomb converts through the ordinary projectile pipeline into the marker's section.
use super::*;
use crate::tiger::sequence::{self, Kind, Node, Timing, spawn};

/// Reach Material Response 8, Airstrike.
const AIRSTRIKE: i64 = 8;
/// The stock rocket's detonation emitter, the strike host's template.
const HOST: u32 = 0x80bb_d11c;
/// FNV-1 of `down`, the locator name the stock emitter creates its cluster bomblets at.
const DOWN: u32 = 0x87ed_d469;
/// The point kind the stock `down` locator declares.
const DOWN_KIND: u8 = 12;
const FLOW: u32 = 0x8080_84e9;
const MOVEMENT: u32 = 0x8080_388f;
const CREATE: u32 = 0x8080_8881;
const DELAY: u32 = 0x8080_93cb;
const RETIRE: u32 = 0x8080_4ab0;
/// Stock owners whose flows supply the delay and retirement event bodies, as the private effect
/// hosts of the vehicle damage effects take them.
const ENVELOPES: [u32; 3] = [0x815b_901a, 0x80bc_308d, 0x80fc_7dfb];
/// The symbol prefix of the bomb's nodes in the marker's section, before the section's own prefix.
const BOMB: &str = "reach-projectile-airstrike-bomb-";
const PREFIX: &str = "reach-projectile-airstrike-host-";
/// A bound on the shots one launch creates, which keeps the rebuilt controls small.
const SHOTS: i64 = 64;

/// The battery a weapon requests, in source units.
struct Battery {
    definition: Value,
    index: usize,
    radius: f32,
    height: f32,
    warmup: f32,
    arrival: f32,
    cooldown: f32,
    duration: f32,
    shots: usize,
    launches: i64,
    offsets: Vec<[f32; 2]>,
    fire_effect: Value,
}

/// The bomb a marker's strike creates, converted into the marker's section, with its battery.
pub(super) struct Bomb {
    root: u32,
    battery: Battery,
    /// The bomb's source launch speed, metres per second.
    speed: f32,
    /// The fire effect location the source creates the bomb at.
    location: Value,
    report: Value,
}

/// The airstrike a source projectile calls, prepared before its own conversion.
pub(super) enum Prepared {
    None,
    Refused(Value),
    Bomb(Box<Bomb>),
}

fn number(value: &Value, what: &str) -> Result<f32> {
    let number = value
        .as_f64()
        .with_context(|| format!("Airstrike source {what}"))? as f32;
    ensure!(number.is_finite(), "Nonfinite airstrike source {what}");
    Ok(number)
}

fn class(p: &Payload) -> Result<u32> {
    p.u32(p.pointer(24)? - 4)
}

/// The projectile's Airstrike rows.
fn rows(projectile: &Value) -> Result<Vec<Value>> {
    Ok(projectile["behavior"]["responses"]
        .as_array()
        .context("Source material responses")?
        .iter()
        .filter(|row| row["response"].as_i64() == Some(AIRSTRIKE))
        .map(|row| json!({"material":row["material"],"chance":row["chance"],"flags":row["flags"]}))
        .collect())
}

/// The airstrike definitions the closure holds for `marker`, by identity, each with its node and
/// the scenarios that name it.
fn definitions(behavior: &Value, marker: &Value) -> Result<BTreeMap<u64, (Value, Vec<Value>)>> {
    let mut out = BTreeMap::<u64, (Value, Vec<Value>)>::new();
    for edge in behavior["edges"]
        .as_array()
        .context("Behavior edges")?
        .iter()
        .filter(|e| e["owner"] == marker["tag"]["datum"] && e["role"] == "airstrike")
    {
        let id = edge["tag"]["datum"]
            .as_u64()
            .context("Airstrike definition identity")?;
        let node = &behavior["nodes"][format!("{id:08X}")];
        ensure!(
            node["tag"] == edge["tag"],
            "Airstrike definition is absent from the behavior closure"
        );
        out.entry(id)
            .or_insert_with(|| (node.clone(), Vec::new()))
            .1
            .push(edge["scenario"].clone());
    }
    Ok(out)
}

/// The one battery of `definitions` whose request weapon is the importing weapon.
fn battery(
    behavior: &Value,
    definitions: &BTreeMap<u64, (Value, Vec<Value>)>,
) -> Result<std::result::Result<Battery, &'static str>> {
    let weapon = &behavior["root"]["path"];
    let mut found = Vec::new();
    for (node, _) in definitions.values() {
        for (index, row) in node["data"]["batteries"]
            .as_array()
            .context("Airstrike batteries")?
            .iter()
            .enumerate()
        {
            if row["request_weapon"]["path"] == *weapon {
                found.push((&node["tag"], index, row));
            }
        }
    }
    let &[(definition, index, row)] = found.as_slice() else {
        return Ok(Err(if found.is_empty() {
            "No battery of the scenario's airstrike definition requests this weapon"
        } else {
            "Several batteries of the scenario's airstrike definitions request this weapon"
        }));
    };
    let shots = row["shots_per_launch"]
        .as_i64()
        .context("Airstrike shots per launch")?;
    let offsets = row["fire_offsets"]
        .as_array()
        .context("Airstrike fire offsets")?
        .iter()
        .map(|offset| {
            Ok([
                number(&offset[0], "fire offset")?,
                number(&offset[1], "fire offset")?,
            ])
        })
        .collect::<Result<Vec<_>>>()?;
    let battery = Battery {
        definition: definition.clone(),
        index,
        radius: number(&row["launch_radius"], "launch radius")?,
        height: number(&row["launch_height"], "launch height")?,
        warmup: number(&row["warmup_seconds"], "warmup")?,
        arrival: number(&row["arrival_seconds"], "arrival")?,
        cooldown: number(&row["cooldown_seconds"], "cooldown")?,
        duration: number(&row["duration_seconds"], "duration")?,
        shots: usize::try_from(shots.clamp(0, SHOTS))?,
        launches: row["initial_launches"]
            .as_i64()
            .context("Airstrike initial launches")?,
        offsets,
        fire_effect: row["fire_effect"].clone(),
    };
    if !(battery.height > 0.
        && battery.radius >= 0.
        && battery.warmup >= 0.
        && battery.duration >= 0.
        && (1..=SHOTS).contains(&shots))
    {
        return Ok(Err(
            "The battery's height, radius, timing or shot count has no native strike",
        ));
    }
    Ok(Ok(battery))
}

/// The source receipt row of the one projectile the battery's fire effect creates, and the
/// effect location it is created at.
fn bomb_source(
    cache: &cache::Cache,
    behavior: &Value,
    battery: &Battery,
) -> Result<std::result::Result<(Value, Value), &'static str>> {
    let Some(id) = battery.fire_effect["datum"].as_u64() else {
        return Ok(Err("The battery names no fire effect"));
    };
    let effect = &behavior["nodes"][format!("{id:08X}")];
    if effect["tag"] != battery.fire_effect {
        return Ok(Err("The fire effect is absent from the behavior closure"));
    }
    let mut bombs = BTreeMap::new();
    for event in effect["data"]["events"]
        .as_array()
        .context("Fire effect events")?
    {
        for part in event["parts"].as_array().context("Fire effect parts")? {
            if part["tag"]["group"] != "proj" {
                continue;
            }
            let location = part["location"]
                .as_i64()
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| effect["data"]["locations"].get(index))
                .map_or(Value::Null, |location| location["name"].clone());
            bombs.insert(
                part["tag"]["datum"]
                    .as_u64()
                    .context("Fire effect bomb identity")?,
                location,
            );
        }
    }
    let Some((&id, location)) = bombs.iter().next() else {
        return Ok(Err("The fire effect creates no projectile"));
    };
    if bombs.len() > 1 {
        return Ok(Err("The fire effect creates several projectiles"));
    }
    let node = &behavior["nodes"][format!("{id:08X}")];
    if node["tag"]["datum"].as_u64() != Some(id) {
        return Ok(Err(
            "The bomb projectile is absent from the behavior closure",
        ));
    }
    let path = node["tag"]["path"].as_str().context("Bomb path")?;
    let object = Object::read(cache, cache.find("proj", path)?)?;
    let modeled = object
        .world
        .as_ref()
        .and_then(|branch| branch.model.as_ref())
        .is_some();
    Ok(Ok((
        json!({"tag":node["tag"],"behavior":node["data"],"has_model":modeled}),
        location.clone(),
    )))
}

/// The carrier the bomb flies on, by the rules a charged barrel's projectile follows.
fn carrier(source: &Value) -> Result<std::result::Result<u32, &'static str>> {
    let behavior = &source["behavior"];
    let shards = shard::Shards::read(behavior)?.is_some();
    Ok(Ok(if source["has_model"] != true {
        if shards {
            return Ok(Err(
                "An effect-only bomb with attached shards needs a modeled carrier's assembly",
            ));
        }
        effect_only::CARRIER
    } else if shards || tracking::enabled(behavior)? {
        0x8152_82e1
    } else {
        0x80bb_b917
    }))
}

/// Converts the bomb a marker's airstrike creates into `assets`, under its own symbol prefix,
/// before the marker itself converts. A source projectile that calls no airstrike prepares none.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    cache: &cache::Cache,
    pages: &mut resource::Pages,
    reader: &mut Reader,
    behavior: &Value,
    marker: &Value,
    entry: &ImportRequest,
    root: &Path,
    layout: &Layout<'_>,
    assets: &mut Assets,
) -> Result<Prepared> {
    if rows(marker)?.is_empty() {
        return Ok(Prepared::None);
    }
    let refuse = |reason: String| -> Result<Prepared> {
        Ok(Prepared::Refused(
            json!({"status":"omitted","reason":reason,"gameplay_verified":false}),
        ))
    };
    let definitions = definitions(behavior, marker)?;
    if definitions.is_empty() {
        return refuse("The cache's scenario names no airstrike definition".into());
    }
    let battery = match battery(behavior, &definitions)? {
        Ok(battery) => battery,
        Err(reason) => return refuse(reason.into()),
    };
    let (source, location) = match bomb_source(cache, behavior, &battery)? {
        Ok(found) => found,
        Err(reason) => return refuse(reason.into()),
    };
    let carrier = match carrier(&source)? {
        Ok(carrier) => carrier,
        Err(reason) => return refuse(reason.into()),
    };
    if let Err(error) = Template::read(reader) {
        return refuse(format!(
            "The strike host template has another structure: {error:#}"
        ));
    }
    let speed =
        number(&source["behavior"]["initial_velocity"], "bomb speed")? * rig::METRES_PER_UNIT;
    if speed <= 0. {
        return refuse("The bomb has no launch speed to fall from the launch height".into());
    }
    let presentation = format!("{}-airstrike-bomb", layout.presentation);
    let effects = format!("{}-airstrike-bomb", layout.effects);
    let folder = format!("{}-airstrike-bomb", layout.folder);
    let bomb_layout = Layout {
        presentation: &presentation,
        effects: &effects,
        folder: &folder,
        prefix: None,
        unguided: true,
        curve: false,
        launch: None,
        trajectories: 1,
    };
    let mut bomb_entry = entry.clone();
    bomb_entry.projectile_entity = Some(carrier);
    let snapshot = assets.clone();
    match super::assemble(
        cache,
        pages,
        reader,
        behavior,
        &source,
        &bomb_entry,
        root,
        &bomb_layout,
        assets,
        &Prepared::None,
    ) {
        Ok((id, report)) => {
            // The bomb's symbols take a prefix of their own before the marker reserves its own.
            assets.rename("reach-projectile-", BOMB)?;
            Ok(Prepared::Bomb(Box::new(Bomb {
                root: id,
                battery,
                speed,
                location,
                report,
            })))
        }
        Err(error) if crate::cancellation::is_cancelled(&error) => Err(error),
        Err(error) => {
            *assets = snapshot;
            refuse(format!("The bomb projectile did not convert: {error:#}"))
        }
    }
}

/// The stock emitter the strike host copies, with its one effect flow and that flow's `down`
/// locator, whose allocation no other component shares.
struct Template {
    entity: Payload,
    rows: Vec<usize>,
    owners: BTreeMap<u32, Payload>,
    flow: u32,
    locator: u32,
}

impl Template {
    fn read(reader: &mut Reader) -> Result<Self> {
        let entity = (*reader.tag(HOST, Some(0x8080_9c0f))?).clone();
        ensure!(entity.u16(0x96)? == 17, "Strike host template kind differs");
        let mut owners = BTreeMap::new();
        for row in entity.array(16, 12, Some(0x8080_9c04))? {
            let tag = entity.u32(row)?;
            let owner = (*reader.tag(tag, Some(0x8080_9c36))?).clone();
            ensure!(
                owners.insert(tag, owner).is_none(),
                "Strike host template repeats a component"
            );
        }
        // The unreplicated marker creates the host, which flies unreplicated as well.
        let (entity, network) = super::replication::strip(reader, &entity, &owners)?;
        owners.retain(|tag, _| !network.contains(tag));
        let rows = entity.array(16, 12, Some(0x8080_9c04))?;
        let mut flows = Vec::new();
        for (&tag, p) in &owners {
            if class(p)? == FLOW {
                flows.push(tag);
            }
        }
        let [flow] = flows.as_slice() else {
            anyhow::bail!("Strike host template has no single effect flow")
        };
        let p = &owners[flow];
        let allocation = p.u32(0x44)?;
        ensure!(
            owners
                .values()
                .filter(|owner| owner.u32(0x44).ok() == Some(allocation))
                .count()
                == 1,
            "Strike host flow shares its allocation"
        );
        let d = p.pointer(24)?;
        let names = p
            .array(d + 0x1d0, 4, Some(0x8080_8614))?
            .into_iter()
            .map(|row| p.u32(row))
            .collect::<Result<Vec<_>>>()?;
        let ranges = p.array(d + 0x1b0, 6, Some(0x8080_8617))?;
        let points = p.array(d + 0x1c0, 16, Some(0x8080_8615))?;
        let index = names
            .iter()
            .position(|&name| name == DOWN)
            .context("Strike host flow has no down locator")?;
        let range = *ranges.get(index).context("Strike host locator range")?;
        let point = *points
            .get(usize::from(p.u8(range + 2)?))
            .context("Strike host locator point")?;
        ensure!(
            p.u8(point)? == DOWN_KIND,
            "Strike host down locator kind differs"
        );
        Ok(Self {
            entity,
            rows,
            owners,
            flow: *flow,
            locator: u32::try_from(index)?,
        })
    }
}

fn hash(name: &str) -> u32 {
    crate::tiger::audio::bank::hash(name)
}

fn delay(name: &str, seconds: f32) -> Node {
    Node {
        name: hash(name),
        timing: Timing {
            duration: [seconds; 2],
            flags: 4,
            ..Timing::default()
        },
        kind: Kind::Event(DELAY),
    }
}

fn event(name: &str, class: u32) -> Node {
    Node {
        name: hash(name),
        timing: Timing::default(),
        kind: Kind::Event(class),
    }
}

/// Builds the strike host into `assets` and names it as the finish graph of the marker's movement
/// in `owners`.
fn strike(
    reader: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    bomb: &Bomb,
) -> Result<Value> {
    let host = Template::read(reader)?;
    let battery = &bomb.battery;
    let metres = rig::METRES_PER_UNIT;
    let height = battery.height * metres;
    let cone = (battery.radius / battery.height).atan();
    let interval = battery.duration / battery.shots as f32;
    let mut shots = Vec::new();
    let mut nodes = Vec::new();
    for index in 0..battery.shots {
        let seconds = battery.warmup + interval * index as f32;
        let offset = battery
            .offsets
            .get(index % battery.offsets.len().max(1))
            .copied()
            .unwrap_or([0.; 2]);
        // The `down` frame's forward axis is taken to point down, so the launch height lies
        // along its negative forward axis and the fire offsets along its other two axes.
        let position = [-height, offset[0] * metres, offset[1] * metres];
        let create = format!("airstrike/shot/{index}/create");
        nodes.push(Node {
            name: hash(&format!("airstrike/shot/{index}")),
            timing: Timing::default(),
            kind: Kind::Serial(vec![
                delay(&format!("airstrike/shot/{index}/delay"), seconds),
                event(&create, CREATE),
            ]),
        });
        shots.push((
            hash(&create),
            position,
            json!({"seconds":seconds,"position_metres":position,"source_offset":offset}),
        ));
    }
    let root = Node {
        name: hash("airstrike/root"),
        timing: Timing::default(),
        kind: Kind::Serial(vec![
            Node {
                name: hash("airstrike/shots"),
                timing: Timing::default(),
                kind: Kind::Parallel(nodes),
            },
            event("airstrike/retire", RETIRE),
        ]),
    };
    let stock = host.owners[&host.flow].clone();
    let allocation_tag = stock.u32(0x44)?;
    let mut allocation = (*reader.tag(allocation_tag, Some(0x8080_9bbb))?).clone();
    let mut envelopes = vec![stock.clone()];
    for tag in ENVELOPES {
        envelopes.push((*reader.tag(tag, Some(0x8080_9c36))?).clone());
    }
    let mut flow = stock;
    let written = sequence::write(&mut flow, &mut allocation, &root, &envelopes)?;
    for (name, position, _) in &shots {
        let field = spawn::write(
            &mut flow,
            *written.events.get(name).context("Strike creation event")?,
            &spawn::Parameters {
                locator: host.locator,
                position: *position,
                rotation: [0., 0., 0., 1.],
                scale: [1.; 2],
                speed: [bomb.speed; 2],
                cone,
                angular_min: [0.; 3],
                angular_max: [0.; 3],
            },
        )?;
        put(&mut flow.0, field, &bomb.root.to_le_bytes())?;
    }
    // Kind 38 deletes its selected object, this flow's host, when `+42` is set.
    let retire = *written
        .events
        .get(&hash("airstrike/retire"))
        .context("Strike retirement event")?;
    flow.0[retire + 0x42] = 1;
    // The rebuilt controls are runtime records past the stock schema.
    let end = flow.0.len();
    let seal = crate::tiger::instance::seal_span(&mut flow, &mut Vec::new(), end)?;
    let mut ids = BTreeMap::new();
    let mut allocations = BTreeMap::new();
    for (&tag, p) in &host.owners {
        ids.insert(
            tag,
            assets.reserve(reader, format!("{PREFIX}component-{tag:08X}"))?,
        );
        let allocation = p.u32(0x44)?;
        if let std::collections::btree_map::Entry::Vacant(slot) = allocations.entry(allocation) {
            slot.insert(assets.reserve(reader, format!("{PREFIX}allocation-{allocation:08X}"))?);
        }
    }
    let mut converted = host.owners.clone();
    converted.insert(host.flow, flow);
    // Typed references are checked against the components as emitted. The sealed flow's own
    // records now reach past the stock flow's end.
    let emitted = converted.clone();
    for (&tag, p) in &mut converted {
        super::relocate(p, &emitted, &ids, &[])?;
        let allocation = allocations[&host.owners[&tag].u32(0x44)?];
        put(&mut p.0, 0x44, &allocation.to_le_bytes())?;
        assets.push(ids[&tag], tag, p.clone(), None)?;
    }
    for (&tag, &id) in &allocations {
        let payload = if tag == allocation_tag {
            allocation.clone()
        } else {
            (*reader.tag(tag, Some(0x8080_9bbb))?).clone()
        };
        assets.push(id, tag, payload, None)?;
    }
    let root_id = assets.reserve(reader, format!("{PREFIX}entity"))?;
    let mut entity = host.entity.clone();
    super::relocate(&mut entity, &emitted, &ids, &host.rows)?;
    let replication = host.entity.u32(0x88)?;
    if replication != u32::MAX {
        reader.tag(replication, Some(0x8080_9bb6))?;
        let id = assets.reserve(reader, format!("{PREFIX}replication"))?;
        let components = host
            .rows
            .iter()
            .map(|&row| {
                let tag = host.entity.u32(row)?;
                Ok(compiled::Component {
                    owner: ids[&tag],
                    payload: &converted[&tag],
                    template: &host.owners[&tag],
                    entity: &host.entity,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        assets.push(
            id,
            replication,
            compiled::replication::emit(root_id, &components)?,
            None,
        )?;
        put(&mut entity.0, 0x88, &id.to_le_bytes())?;
    }
    crate::tiger::entity::links::Graph::read(&entity, false)?;
    assets.push(root_id, HOST, entity, None)?;
    // The marker's finish graph becomes the host in every definition view.
    let mut movements = owners
        .values_mut()
        .filter(|p| class(p).ok() == Some(MOVEMENT))
        .collect::<Vec<_>>();
    let [m] = movements.as_mut_slice() else {
        anyhow::bail!("Marker movement is absent or ambiguous")
    };
    let views = BTreeSet::from([m.pointer(24)?, usize::try_from(m.u64(m.pointer(16)? + 8)?)?]);
    let mut replaced = BTreeSet::new();
    for view in views {
        replaced.insert(format!("{:08X}", m.u32(view + 0x180)?));
        put(&mut m.0, view + 0x180, &root_id.to_le_bytes())?;
    }
    let report = &bomb.report;
    Ok(json!({"status":"partial","template":format!("{HOST:08X}"),
        "flow":format!("{:08X}", host.flow),"locator":{"index":host.locator,"name":format!("{DOWN:08X}"),"kind":DOWN_KIND},
        "replaced_finish":replaced,"seal":seal,
        "battery":{"definition":battery.definition,"index":battery.index,"launch_radius":battery.radius,
            "launch_height":battery.height,"warmup_seconds":battery.warmup,"arrival_seconds":battery.arrival,
            "cooldown_seconds":battery.cooldown,"duration_seconds":battery.duration,
            "shots_per_launch":battery.shots,"initial_launches":battery.launches,
            "fire_offsets":battery.offsets,"length_unit_metres":metres},
        "bomb":{"source":report["source"],"native_carrier":report["native_carrier"],
            "source_location":bomb.location,"speed_metres_per_second":bomb.speed,
            "detonation":report["detonation"],"conversion":report},
        "shots":shots.iter().map(|(_, _, row)| row.clone()).collect::<Vec<_>>(),
        "launch_cone_radians":cone,
        "limits":[
            "The bombs are created at the host's down locator, kind 12. That its forward axis points down along gravity is inferred from its name, the FNV-1 preimage of down, and from the stock emitter creating cluster bomblets there. The native locator resolver fills kinds 6 to 16 only from cached points, so the frame comes from the graph's creation, which is not traced. The launch height lies along the frame's negative forward axis and the fire offsets along its other two axes, whose turn about the vertical is unknown.",
            "Shots start after the battery's warmup, follow evenly over its launch duration and cycle its fire offsets. The launch radius becomes a launch cone of its angle seen from the launch height. The spacing, offset order and radius use are inferred from the field names.",
            "One marker calls one launch. The battery's initial launches and cooldown are not carried.",
            "The marker ends at its first contact, where the host is created, rather than staying as the source's beacon until it arms.",
            "Bombs are created at the source's launch height, so under a ceiling or overhang they meet it before the mark.",
            "The host's other stock components stay, and its rebuilt flow leaves the stock emitter's damage, presentation and creation events unreachable."
        ],
        "gameplay_verified":false}))
}

/// Builds the strike a marker prepared into `assets` and the marker's movement in `owners`, or
/// reports why it has none. A failed host leaves both as they were.
pub(super) fn host(
    reader: &mut Reader,
    assets: &mut Assets,
    owners: &mut BTreeMap<u32, Payload>,
    prepared: &Prepared,
) -> Result<Value> {
    let bomb = match prepared {
        Prepared::None => {
            return Ok(
                json!({"status":"not_applicable","reason":"The source projectile calls no airstrike"}),
            );
        }
        Prepared::Refused(receipt) => return Ok(receipt.clone()),
        Prepared::Bomb(bomb) => bomb,
    };
    let snapshot = (assets.clone(), owners.clone());
    match strike(reader, assets, owners, bomb) {
        Ok(report) => Ok(report),
        Err(error) if crate::cancellation::is_cancelled(&error) => Err(error),
        Err(error) => {
            (*assets, *owners) = snapshot;
            Ok(json!({"status":"omitted","bomb":bomb.report["source"],
                "reason":format!("The strike host did not build: {error:#}"),
                "limits":["The converted bomb stays in the section with nothing that creates it."],
                "gameplay_verified":false}))
        }
    }
}

/// The weapon's airstrike: every source projectile with an Airstrike row, the definitions its
/// scenario names and the strike its conversion builds. `fired` is the fired projectile's
/// conversion receipt. A marker another barrel fires builds its strike in its own conversion.
pub(super) fn receipt(behavior: &Value, projectiles: &Value, fired: &Value) -> Result<Value> {
    let mut markers = Vec::new();
    let mut statuses = BTreeSet::new();
    for projectile in projectiles.as_array().context("Projectile presentations")? {
        let rows = rows(projectile)?;
        if rows.is_empty() {
            continue;
        }
        let definitions = definitions(behavior, projectile)?
            .into_values()
            .map(|(node, scenarios)| {
                json!({"definition":node["tag"],"scenarios":scenarios,
                    "batteries":node["data"]["batteries"]})
            })
            .collect::<Vec<_>>();
        let strike = if projectile["tag"]["datum"] == fired["source"]["datum"] {
            fired["strike"].clone()
        } else {
            json!({"status":"pending",
                "reason":"The marker is another barrel's projectile, so its own conversion builds the strike, as the charged release's additional projectile receipt records"})
        };
        statuses.insert(strike["status"].as_str().unwrap_or("omitted").to_owned());
        markers.push(
            json!({"source":projectile["tag"],"rows":rows,"definitions":definitions,
            "strike":strike}),
        );
    }
    if markers.is_empty() {
        return Ok(json!({"status":"not_applicable",
            "reason":"No source projectile of the weapon answers a contact with Airstrike"}));
    }
    let status = ["partial", "pending", "omitted"]
        .into_iter()
        .find(|status| statuses.contains(*status))
        .unwrap_or("omitted");
    Ok(json!({"status":status,"markers":markers,"gameplay_verified":false}))
}
