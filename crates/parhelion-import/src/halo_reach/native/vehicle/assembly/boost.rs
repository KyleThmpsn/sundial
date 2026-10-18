//! Source boost attachments on a checked native CPU trail and sequence lifecycle.
use super::*;
use crate::{
    halo_reach::{
        Scene,
        cache::{Cache, Tag},
        effect::trail,
        model,
        native::seat,
        resource::Pages,
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
    system: u32,
    locator: usize,
}

struct Attachment<'a> {
    source: Tag,
    scale: String,
    markers: Vec<&'a model::Marker>,
    profiles: Vec<trail::Profile>,
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
        let mut profiles = Vec::new();
        for event in cache.block(at + 0x2c, 0x40)? {
            for part in cache.block(event + 0x1c, 0x64)? {
                if let Some(reference) = cache.reference(part + 0x14, None)?
                    && reference.group == "cntl"
                {
                    ensure!(
                        cache.i16(part + 6)? == -1,
                        "Boost contrail selects a separate location"
                    );
                    profiles.extend(trail::read(cache, &reference)?);
                }
            }
        }
        ensure!(!profiles.is_empty(), "Source boost effect has no contrail");
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
            profiles,
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
            system,
            locator: usize::from(payload.u8(locators[0])?),
        });
    }
    Ok(result)
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
        pages: &mut Pages,
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
        let entry = original.u32(definition + 0x2d0)?;
        ensure!(entry & 0xffff == 0, "Boost flow entry is not a control");
        let entry = original.pointer(
            *nodes
                .get((entry >> 16) as usize)
                .context("Boost flow entry")?
                + 16,
        )?;
        let children = original.array(entry + 0x38, 4, Some(0x808093fb))?;
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
        // Kind zero waits for a blocking child before advancing. Keep the native
        // allocation and paired classes of an existing kind-one parallel control
        // so both indefinitely repeating scalar gates receive updates.
        let (root_index, root) = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let at = original.pointer(row + 16).ok()?;
                (original.u32(at - 4).ok() == Some(0x808093d7)
                    && original.u8(at + 0x30).ok() == Some(1))
                .then_some((index, at))
            })
            .next()
            .context("Boost flow lacks an allocated parallel control")?;
        {
            let payload = self.payloads.get_mut(&owner).unwrap();
            put(&mut payload.0, root + 0x14, &0xffff00ffu32.to_le_bytes())?;
            put(&mut payload.0, root + 0x18, &[0; 24])?;
            put(&mut payload.0, root + 0x31, &[1, 0])?;
            put(
                &mut payload.0,
                definition + 0x2d0,
                &(u32::try_from(root_index)? << 16).to_le_bytes(),
            )?;
        }
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
            // Child completion and release follow the node's stored parent,
            // not the array that scheduled it. Reparent both ends of this
            // shortened path along with the root's child list.
            put(
                &mut payload.0,
                gate + 0x14,
                &(u32::try_from(root_index)? << 16).to_le_bytes(),
            )?;
            put(
                &mut payload.0,
                target.event + 0x14,
                &(u32::try_from(gate_index)? << 16).to_le_bytes(),
            )?;
            // These source attachments last while their scalar gate is open.
            // The carrier event otherwise expires after its stored one-second
            // duration even though the gate remains qualified. Native flag 8
            // suppresses that event timer. Parent retirement still stops it.
            let flags = payload.u32(target.event + 0x18)? | 8;
            put(&mut payload.0, target.event + 0x18, &flags.to_le_bytes())?;
            // Retain the native scalar gate and event lifecycle, with no dependency
            // on contact or impact branches removed by the new flight controller.
            put(&mut payload.0, gate + 0x58, &activation.to_le_bytes())?;
            put(&mut payload.0, gate + 0x5c, &f32::MAX.to_le_bytes())?;
            put(&mut payload.0, gate + 0x60, &0u32.to_le_bytes())?;
            ensure!(
                original.u8(gate + 0x32)? == 1,
                "Native trail gate does not repeat after release"
            );
            let ordinal = flow_events
                .iter()
                .position(|row| original.pointer(row + 16).ok() == Some(target.event))
                .context("Trail event ordinal")?;
            let mut child = 1u16.to_le_bytes().to_vec();
            child.extend(u16::try_from(ordinal)?.to_le_bytes());
            append(&mut payload.0, gate + 0x38, 0x808093fb, &child, 4)?;
            root_children.extend(0u16.to_le_bytes());
            root_children.extend(u16::try_from(gate_index)?.to_le_bytes());
            // Particle-event instances store an allocated group handle. Each
            // source profile gets its own system in that group's definition.
            let mut systems = vec![0; source.profiles.len() * 24];
            for row in systems.chunks_exact_mut(24) {
                row[16..24].copy_from_slice(&[0xff; 8]);
            }
            let payload = self.payloads.get_mut(&owner).unwrap();
            append(
                &mut payload.0,
                target.event + 0x40,
                0x80806cc8,
                &systems,
                24,
            )?;
            let rows = payload.array(target.event + 0x40, 24, Some(0x80806cc8))?;
            let speed = super::super::motion::number(controls, "forward_source_units_per_second")?
                * scene.metres();
            let mut profiles = Vec::new();
            for (profile_index, (profile, row)) in source.profiles.iter().zip(rows).enumerate() {
                append(
                    &mut self.payloads.get_mut(&owner).unwrap().0,
                    row,
                    0x80800009,
                    &[u8::try_from(target.locator)?],
                    1,
                )?;
                let prefix = format!("vehicle-boost-{index}-{profile_index}");
                let (symbol, profile) = trail::emit(
                    cache,
                    pages,
                    native,
                    graph,
                    profile,
                    target.system,
                    &prefix,
                    scene.metres(),
                    speed,
                )?;
                self.resource(owner, row + 16, symbol.clone())?;
                profiles.push(json!({"system":symbol,"profile":profile}));
            }
            events.insert(target.event);
            report.push(json!({"event":target.event,"profiles":profiles,"source":source.source,"scale":source.scale,"placements":placements,
                "marker":format!("{name:08X}"),"locator":target.locator,"thrust_threshold":activation,"gate":gate,
                "lifetime":"Active until the thrust gate closes or the owner retires"}));
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
            "attachments":report,"thrust_threshold":threshold,"input":input,"gate":gate,
            "parallel_entry":root_index,"gameplay_verified":false,
            "limits":["Source profiles and bitmaps use straight ribbon segments. Thrust gates are authored for the native controller. Historical bending and light volumes are not translated. Live visibility and release cleanup require gameplay acceptance."]}),
        })
    }
}
