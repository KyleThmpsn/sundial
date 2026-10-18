//! Source debris and particles scheduled by private native effect entities.
use super::super::{fragment, secondary::Bundle};
use super::*;
use crate::{
    halo_reach::{
        cache::{Cache, Tag},
        effect::{particle, source},
        native::seat,
        resource::Pages,
        rig,
    },
    tiger::{
        markers,
        sequence::{self, Kind, Node, Timing, spawn},
    },
};
use std::collections::BTreeMap;

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

fn host_locators(payload: &mut Payload, parent_marker: Option<u32>, owner: u32) -> Result<()> {
    let d = payload.pointer(24)?;
    for field in [0x178, 0x188, 0x200, 0x210, 0x220, 0x230] {
        ensure!(
            payload.u64(d + field)? == 0,
            "Damage effect host has additional dependencies"
        );
    }
    let locators = payload.array(d + 0x1c0, 16, Some(0x80808615))?;
    if let Some(marker) = parent_marker {
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
    }
    Ok(())
}

struct Parts {
    children: Vec<Node>,
    creations: Vec<Creation>,
    pending: Vec<Value>,
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

struct Compiler<'a> {
    cache: &'a Cache,
    pages: &'a mut Pages,
    native: &'a mut Reader,
    graph: &'a mut Graph,
    scene: &'a Scene,
    fragments: BTreeMap<(u32, [u32; 4]), (String, Value)>,
}

impl Compiler<'_> {
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
        if matches!(marker, "" | "replace") {
            poses.push(anchor);
        } else {
            for source in model.markers.iter().filter(|source| source.name == marker) {
                if model.marker_selected(source)? {
                    poses.push(seat::marker_pose(model, source)?);
                }
            }
        }
        ensure!(
            !poses.is_empty(),
            "Source damage marker {marker} is absent from its model"
        );
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

    fn parts(
        &mut self,
        effect: &Value,
        event: &Value,
        event_name: &str,
        event_index: usize,
        anchor: ([f32; 3], [f32; 4]),
    ) -> Result<Parts> {
        let mut creations = Vec::new();
        let mut pending = Vec::new();
        let mut children = Vec::new();
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
            if source.group != "bloc" {
                pending.push(json!({"event":event_index,"part":part_index,"source":source,"status":"Separate effect part translation"}));
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
            let cone = part["velocity_cone_radians"]
                .as_f64()
                .context("Source debris cone")? as f32;
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
        Ok(Parts {
            children,
            creations,
            pending,
        })
    }

    fn effect(
        &mut self,
        tag: &Tag,
        prefix: &str,
        anchor: ([f32; 3], [f32; 4]),
        skip: f32,
        parent_marker: Option<u32>,
    ) -> Result<(Option<String>, Value)> {
        ensure!(
            tag.group == "effe",
            "Detached-piece source is not an effect"
        );
        let effect = source::read(self.cache, tag.address()?)?;
        let loop_start = effect["loop_start"]
            .as_i64()
            .context("Source effect loop entry")?;
        if loop_start >= 0 && parent_marker.is_none() {
            return Ok((
                None,
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
            } = self.parts(&effect, event, &event_name, event_index, anchor)?;
            creations.extend(objects);
            pending.extend(omitted);
            let mut systems = Vec::new();
            for (system_index, system) in event["particles"]
                .as_array()
                .context("Source particle systems")?
                .iter()
                .enumerate()
            {
                if system["tag"].is_null() {
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
                        maximum_life = maximum_life.max(parsed.life);
                        let name = format!(
                            "{prefix}-event-{event_index}-particle-{system_index}-{emitter_index}-{marker_index}"
                        );
                        let (symbol, report) = particle::emit(
                            self.cache,
                            self.pages,
                            self.native,
                            self.graph,
                            &parsed,
                            &name,
                        )?;
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
        if creations.is_empty() && particle_events.is_empty() {
            return Ok((
                None,
                json!({"source":tag,"pending":pending,"emitted_objects":0,"emitted_particle_systems":0}),
            ));
        }
        repeat(&mut scheduled, &effect, prefix, loop_start)?;
        let content = chance(
            &format!("{prefix}/response-choice"),
            skip,
            node(&format!("{prefix}/source"), Kind::Serial(scheduled)),
        )?;
        let root = if parent_marker.is_some() {
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
        let mut bundle = Bundle::read(
            self.native,
            if parent_marker.is_some() {
                0x80c0d93e
            } else {
                0x80c0dc18
            },
        )?;
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
        let payload = bundle.owners.get_mut(&owner).unwrap();
        host_locators(payload, parent_marker, owner)?;
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
        // Kind 38 uses +42 to delete its selected object. With selector FF at
        // +38, 0058E590 selects this flow's host. Zero flags would do nothing.
        if parent_marker.is_none() {
            let retire = written.events[&hash(&format!("{prefix}/retire"))];
            payload.0[retire + 0x42] = 1;
        }
        bundle.seal(owner)?;
        let entity = bundle.emit(self.graph, prefix)?;
        Ok((
            Some(entity),
            json!({"source":tag,"emitted_objects":creations.len(),"pending":pending,
            "emitted_particle_systems":particle_reports.len(),"particles":particle_reports,"maximum_particle_life":maximum_life,
            "response_skip_fraction":skip,"loop_start":loop_start,"managed":parent_marker.is_some(),"gameplay_verified":false,
            "limits":["Effect-local marker offsets use the source bind pose relative to the live damage marker. Source spin magnitudes use the native local forward axis. Cone bounds use the native polar distribution. Source first-person suppression and exact random streams remain separate acceptance boundaries."]}),
        ))
    }
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
                let source_marker = match marker_field {
                    Some(field) => response[field].as_str().context("Source damage marker")?,
                    None => "",
                };
                let model = &scene.models[0].model;
                let (anchor, bone) = if source_marker.is_empty() {
                    (([0.; 3], [0., 0., 0., 1.]), first)
                } else {
                    let marker = seat::selected_marker(model, source_marker)?;
                    (
                        seat::marker_pose(model, marker)?,
                        marker.bone.map_or(first, |bone| first + bone),
                    )
                };
                let prefix = format!("damage-{section_index}-{response_index}-{field}");
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
                report.push(json!({"section":section_index,"response":response_index,"field":field,"effect":effect}));
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
