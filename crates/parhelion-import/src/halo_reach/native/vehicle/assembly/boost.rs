//! Source boost attachments on a checked native CPU trail and sequence lifecycle.
use super::*;
use crate::{
    halo_reach::{
        Scene,
        cache::{Cache, Tag},
        model,
        native::seat,
    },
    presentation::append,
    tiger::markers,
};

#[derive(Default)]
pub(in crate::halo_reach::native::vehicle) struct Trails {
    pub events: BTreeMap<u32, BTreeSet<usize>>,
    pub report: Value,
}

struct Target {
    event: usize,
    system_row: usize,
    system: u32,
    locator: usize,
}

struct Attachment<'a> {
    source: Tag,
    scale: String,
    markers: Vec<&'a model::Marker>,
}

fn attachments<'a>(cache: &Cache, scene: &'a Scene) -> Result<Vec<Attachment<'a>>> {
    let effects = scene
        .object
        .attachments
        .iter()
        .filter(|value| {
            value["tag"]["group"] == "effe"
                && matches!(
                    value["primary_scale"].as_str(),
                    Some("boost_fx" | "wingtips")
                )
        })
        .collect::<Vec<_>>();
    if effects.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut scales = BTreeSet::new();
    for effect in effects {
        let scale = effect["primary_scale"]
            .as_str()
            .context("Source trail scale")?;
        ensure!(scales.insert(scale), "Source trail scale is ambiguous");
        let tag: Tag = serde_json::from_value(effect["tag"].clone())?;
        let at = tag.address()?;
        let locations = cache.block(at + 0x20, 8)?;
        ensure!(
            locations.len() == 1,
            "Source boost effect needs one marker group"
        );
        let name = cache.string_id(cache.u32(locations[0])?)?;
        let mut contrails = 0;
        for event in cache.block(at + 0x2c, 0x40)? {
            for part in cache.block(event + 0x1c, 0x64)? {
                if let Some(reference) = cache.reference(part + 0x14, None)?
                    && reference.group == "cntl"
                {
                    ensure!(
                        cache.i16(part + 6)? == -1,
                        "Boost contrail selects a separate location"
                    );
                    contrails += cache.block(reference.address()?, 0x280)?.len();
                }
            }
        }
        ensure!(contrails > 0, "Source boost effect has no contrail");
        let model = &scene.models[0].model;
        let mut selected = Vec::new();
        for marker in model.markers.iter().filter(|marker| marker.name == name) {
            if model.marker_selected(marker)? {
                selected.push(marker);
            }
        }
        ensure!(
            !selected.is_empty() && selected.len() <= 8,
            "Source boost marker count is unsupported"
        );
        result.push(Attachment {
            source: tag,
            scale: scale.to_owned(),
            markers: selected,
        });
    }
    Ok(result)
}

fn targets(native: &mut Reader, payload: &Payload) -> Result<Vec<Target>> {
    let definition = payload.pointer(24)?;
    if payload.u32(definition - 4)? != 0x808084e9 {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for row in payload.array(definition + 0x168, 24, Some(0x808093e6))? {
        let event = payload.pointer(row + 16)?;
        match payload.u32(event - 4)? {
            0x80806f49 | 0x808093cb => continue,
            0x80806cc6 => {}
            _ => return Ok(Vec::new()),
        }
        let systems = payload.array(event + 0x40, 24, Some(0x80806cc8))?;
        if systems.len() != 1 {
            continue;
        }
        let system_row = systems[0];
        let system = payload.u32(system_row + 16)?;
        if system == u32::MAX || payload.u32(system_row + 20)? != u32::MAX {
            continue;
        }
        let data = native.tag(system, Some(0x80806e28))?;
        if [4, 8, 16]
            .iter()
            .any(|&at| data.u32(at).ok() != Some(u32::MAX))
        {
            continue;
        }
        let particle = native.tag(data.u32(0)?, Some(0x80806e2c))?;
        if particle.u8(0x14b)? != 5
            || (0..8)
                .any(|i| particle.u16(0x70 + i * 2).ok() != Some([0, 25, 8, 51, 24, 225, 10, 0][i]))
            || particle.array(0x60, 16, Some(0x80800090))?.len() != 41
            || particle.array(0x100, 2, Some(0x80800007))?.len() != 4
        {
            continue;
        }
        let locators = payload.array(system_row, 1, Some(0x80800009))?;
        ensure!(
            locators.len() == 1,
            "Native boost trail needs one attachment group"
        );
        result.push(Target {
            event,
            system_row,
            system,
            locator: usize::from(payload.u8(locators[0])?),
        });
    }
    Ok(result)
}

fn particle(
    native: &mut Reader,
    graph: &mut Graph,
    target: &Target,
    index: usize,
    wingtip: bool,
) -> Result<String> {
    let system = native.tag(target.system, Some(0x80806e28))?;
    let tag = system.u32(0)?;
    let original = native.tag(tag, Some(0x80806e2c))?;
    let mut payload = (*original).clone();
    let constants = payload.array(0x60, 16, Some(0x80800090))?;
    let mut values = constants
        .iter()
        .flat_map(|&at| payload.0[at..at + 16].iter().copied())
        .collect::<Vec<_>>();
    // The native ribbon supplies age, width and breakup. Its contact inputs are
    // replaced by one so it remains independent of the removed hover contacts.
    let constant = u8::try_from(constants.len())?;
    for _ in 0..4 {
        values.extend(1f32.to_le_bytes());
    }
    let colors = if wingtip {
        [[0.58f32, 0.76, 1., 0.65], [0.35, 0.55, 0.95, 0.35]]
    } else {
        [[0.38, 0.22, 1., 1.], [0.18, 0.10, 0.95, 1.]]
    };
    for (row, color) in [12, 13].into_iter().zip(colors) {
        for (axis, value) in color.into_iter().enumerate() {
            put(&mut values, row * 16 + axis * 4, &value.to_le_bytes())?;
        }
    }
    let code = payload.array(0x50, 1, Some(0x80800009))?;
    let mut cursor = 0;
    let mut replaced = 0;
    while cursor < code.len() {
        let op = payload.u8(code[cursor])?;
        let width = match op {
            0x22 | 0x34..=0x3b | 0x40..=0x47 => 2,
            0x3e | 0x3f => 4,
            1..=0x21 | 0x23..=0x33 | 0x3d | 0x49..=0x52 => 1,
            _ => anyhow::bail!("Unsupported native boost particle instruction {op:02X}"),
        };
        ensure!(
            cursor + width <= code.len(),
            "Truncated native boost particle instruction"
        );
        if op == 0x47 {
            ensure!(
                payload.u8(code[cursor + 1])? < 4,
                "Boost particle input is outside its table"
            );
            payload.0[code[cursor]] = 0x34;
            payload.0[code[cursor + 1]] = constant;
            replaced += 1;
        }
        cursor += width;
    }
    ensure!(
        replaced == 4,
        "Native boost particle input contract differs"
    );
    append(&mut payload.0, 0x60, 0x80800090, &values, 16)?;
    put(&mut payload.0, 0x100, &[0; 16])?;
    let symbol = format!("vehicle-boost-system-{index}");
    let definition = format!("vehicle-boost-particle-{index}");
    graph.add(&definition, tag, &payload.0, None, Vec::new())?;
    let mut bytes = system.0.clone();
    put(&mut bytes, 0, &u32::MAX.to_le_bytes())?;
    graph.add(
        &symbol,
        target.system,
        &bytes,
        None,
        vec![json!({"offset":0,"symbol":definition})],
    )?;
    Ok(symbol)
}

impl Assembly {
    #[expect(
        clippy::too_many_arguments,
        reason = "Trail authoring joins source attachments, native physics and the private assembly"
    )]
    pub fn boost(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        scene: &Scene,
        bones: &Value,
        controls: &Value,
        cache: &Cache,
        motion: &motion::Motion,
    ) -> Result<Trails> {
        let source = attachments(cache, scene)?;
        if source.is_empty() {
            return Ok(Trails::default());
        }
        let mut candidates = Vec::new();
        for (&owner, payload) in &self.payloads {
            let selected = targets(native, payload)?;
            if selected.len() >= source.len() {
                candidates.push((owner, selected));
            }
        }
        ensure!(
            candidates.len() == 1,
            "Compatible native boost trail flow is absent or ambiguous"
        );
        let (owner, selected) = candidates.pop().unwrap();
        let original = self.payloads[&owner].clone();
        let definition = original.pointer(24)?;
        let nodes = original.array(definition + 0x158, 24, Some(0x808093e6))?;
        let root = original.pointer(*nodes.first().context("Boost flow root")? + 16)?;
        let children = original.array(root + 0x38, 4, Some(0x808093fb))?;
        ensure!(
            children.len() == 1 && original.u16(children[0])? == 0,
            "Boost flow entry differs"
        );
        let gate = original.pointer(
            *nodes
                .get(usize::from(original.u16(children[0] + 2)?))
                .context("Boost flow gate")?
                + 16,
        )?;
        ensure!(
            original.u32(gate - 4)? == 0x808093cd && original.u32(gate + 0x60)? == 0,
            "Boost flow lacks its scalar entry gate"
        );
        let input = *original
            .array(definition + 0x188, 40, Some(0x80809789))?
            .first()
            .context("Boost flow input")?;
        let declared = patches(graph, "vehicle-motion")?;
        let provider = declared
            .iter()
            .find(|patch| patch["symbol"] == "vehicle-flight-thrust-output")
            .context("Flight thrust provider")?;
        let offset = provider["offset"]
            .as_u64()
            .context("Flight thrust provider offset")?
            - 8;
        let object = links::Object {
            owner: motion::OWNER,
            class: 0x80809ae1,
            offset,
        };
        let contract = links::Interface::read(
            &motion.payload,
            object,
            &graph.read("vehicle-flight-thrust-output")?,
            false,
        )?;
        ensure!(
            contract.methods.len() == 1
                && contract.methods[0].index == 9
                && contract.methods[0].implementation_class == 0x80803cf2,
            "Flight thrust output contract differs"
        );
        let channels =
            self.channel_bridge(native, graph, "vehicle-boost-channels", &[(object, 0)])?;
        let channel = &channels[0];
        let mut count = 0;
        for wire in self
            .links
            .connections
            .iter_mut()
            .chain(&mut self.links.named_connections)
        {
            if wire.consumer.object.is_some_and(|v| {
                v.owner == owner && v.offset == input as u64 && v.class == 0x80809789
            }) {
                wire.provider = channel.provider.clone();
                wire.channel = channel.index;
                count += 1;
            }
        }
        ensure!(
            count == 1,
            "Boost flow input binding is absent or ambiguous"
        );
        let (cruise, boost) = super::super::flight::thrust(controls)?;
        let threshold = (cruise + boost) * 0.5;
        put(
            &mut self.payloads.get_mut(&owner).unwrap().0,
            input + 32,
            &channel.name.to_le_bytes(),
        )?;
        let names = original.array(definition + 0x1d0, 4, Some(0x80808614))?;
        let locators = original.array(definition + 0x1c0, 16, Some(0x80808615))?;
        let spans = original.array(definition + 0x1b0, 6, Some(0x80808617))?;
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Boost source rig")?,
        )?;
        let mut markers = Vec::new();
        let mut report = Vec::new();
        let mut events = BTreeSet::new();
        let flow_events = original.array(definition + 0x168, 24, Some(0x808093e6))?;
        let gates = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let at = original.pointer(row + 16).ok()?;
                (original.u32(at - 4).ok() == Some(0x808093cd)).then_some((index, at))
            })
            .collect::<Vec<_>>();
        ensure!(
            gates.len() >= source.len(),
            "Native trail gates are insufficient"
        );
        let mut root_children = Vec::new();
        for (index, (target, source)) in selected.iter().zip(&source).enumerate() {
            let name = format!("source/boost/{index}")
                .bytes()
                .fold(0x811c9dc5u32, |h, b| {
                    h.wrapping_mul(0x01000193) ^ u32::from(b)
                });
            let mut placements = Vec::new();
            for source_marker in &source.markers {
                let bone = source_marker.bone.map_or(0, |bone| first + bone);
                let world = seat::marker_pose(&scene.models[0].model, source_marker)?;
                let (position, orientation) = seat::frame(bones, bone, world)?;
                markers.push(markers::Marker {
                    name,
                    binding: [1, u32::try_from(bone)?],
                    position: [position[0], position[1], position[2], 1.],
                    orientation,
                    row: 0,
                });
                placements.push(json!({"source_marker":source_marker.name,"bone":bone,"position":position,"source_position":world.0}));
            }
            let locator = *locators.get(target.locator).context("Boost locator")?;
            let span = *spans.get(target.locator).context("Boost locator span")?;
            ensure!(
                original.u8(locator)? == 2
                    && original.u8(locator + 2)? == original.u8(span + 1)?
                    && original.u8(locator + 1)? == original.u8(span)?
                    && original.u16(locator + 4)? == 0x3c,
                "Boost attachment is not a native marker group"
            );
            ensure!(
                source.markers.len() <= usize::from(original.u8(locator + 2)?),
                "Source trail group exceeds the native transform allocation"
            );
            let payload = self.payloads.get_mut(&owner).unwrap();
            payload.0[locator + 2] = u8::try_from(source.markers.len())?;
            payload.0[span + 1] = u8::try_from(source.markers.len())?;
            put(&mut payload.0, locator + 8, &name.to_le_bytes())?;
            put(&mut payload.0, names[target.locator], &name.to_le_bytes())?;
            let wingtip = source.scale == "wingtips";
            let activation = if wingtip { cruise * 0.25 } else { threshold };
            let (gate_index, gate) = gates[index];
            // Retain the native scalar gate and event lifecycle, with no dependency
            // on contact or impact branches removed by the new flight controller.
            put(&mut payload.0, gate + 0x58, &activation.to_le_bytes())?;
            put(&mut payload.0, gate + 0x5c, &f32::MAX.to_le_bytes())?;
            put(&mut payload.0, gate + 0x60, &0u32.to_le_bytes())?;
            let ordinal = flow_events
                .iter()
                .position(|row| original.pointer(row + 16).ok() == Some(target.event))
                .context("Trail event ordinal")?;
            let mut child = 1u16.to_le_bytes().to_vec();
            child.extend(u16::try_from(ordinal)?.to_le_bytes());
            append(&mut payload.0, gate + 0x38, 0x808093fb, &child, 4)?;
            root_children.extend(0u16.to_le_bytes());
            root_children.extend(u16::try_from(gate_index)?.to_le_bytes());
            let symbol = particle(native, graph, target, index, wingtip)?;
            self.resource(owner, target.system_row + 16, symbol.clone())?;
            events.insert(target.event);
            report.push(json!({"event":target.event,"system":symbol,"source":source.source,"scale":source.scale,"placements":placements,
                "marker":format!("{name:08X}"),"locator":target.locator,"thrust_threshold":activation,"gate":gate}));
        }
        append(
            &mut self.payloads.get_mut(&owner).unwrap().0,
            root + 0x38,
            0x808093fb,
            &root_children,
            4,
        )?;
        let marker_owner = self.owner(markers::NATIVE_COMPONENT)?;
        let payload = markers::append_skeletal(
            &self.payloads[&marker_owner],
            &markers,
            bones["bones"].as_array().context("Boost target rig")?.len(),
        )?;
        self.payloads.insert(marker_owner, payload);
        Ok(Trails {
            events: BTreeMap::from([(owner, events)]),
            report: json!({"owner":format!("{owner:08X}"),
            "attachments":report,"thrust_threshold":threshold,"input":input,"gate":gate,"gameplay_verified":false,
            "limits":["Native ribbons approximate source engine and wingtip contrails. Thrust gates are authored for the native controller. Exact source contrail shading and light volumes are not translated. Live visibility and release cleanup require gameplay acceptance."]}),
        })
    }
}
