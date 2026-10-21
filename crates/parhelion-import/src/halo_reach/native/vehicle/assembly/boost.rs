//! Source boost attachments with an independent native sequence lifecycle.
mod flow;
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
    tiger::{markers, sequence},
};

#[derive(Default)]
pub(in crate::halo_reach::native::vehicle) struct Trails {
    pub events: BTreeMap<u32, BTreeSet<usize>>,
    pub report: Value,
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
        let owner = flow::OWNER;
        ensure!(
            !self.payloads.contains_key(&owner),
            "Source trail owner is already present"
        );
        let declared = patches(graph, "vehicle-motion")?;
        let provider = declared
            .iter()
            .find(|patch| patch["symbol"] == "vehicle-flight-thrust-output")
            .context("Flight thrust provider")?;
        let offset = provider["offset"]
            .as_u64()
            .context("Flight thrust provider offset")?
            .checked_sub(8)
            .context("Flight thrust provider prefix")?;
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
        let (cruise, boost) = super::super::flight::thrust(controls)?;
        let threshold = (cruise + boost) * 0.5;
        let groups = source
            .iter()
            .enumerate()
            .map(|(index, attachment)| {
                (
                    super::super::damage::hash(&format!("source/boost/{index}")),
                    attachment.markers.len(),
                    if attachment.scale == "wingtips" {
                        cruise * 0.25
                    } else {
                        threshold
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut flow = flow::build(native, channel.name, &groups)?;
        let first = usize::try_from(
            bones["source_bone_first"]
                .as_u64()
                .context("Boost source rig")?,
        )?;
        let mut markers = Vec::new();
        let mut report = Vec::new();
        let mut resources = Vec::new();
        for (index, source) in source.iter().enumerate() {
            let (name, _, activation) = groups[index];
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
                placements.push(json!({"source_marker":source_marker.name,"bone":bone,
                    "position":position,"source_position":world.0}));
            }
            let speed = super::super::motion::number(controls, "forward_source_units_per_second")?
                * scene.metres();
            let mut profiles = Vec::new();
            let mut systems = Vec::new();
            for (profile_index, profile) in source.profiles.iter().enumerate() {
                let prefix = format!("vehicle-boost-{index}-{profile_index}");
                let (symbol, profile) = trail::emit(
                    cache,
                    pages,
                    native,
                    graph,
                    profile,
                    flow::SYSTEM,
                    &prefix,
                    scene.metres(),
                    speed,
                )?;
                systems.push(symbol.clone());
                profiles.push(json!({"system":symbol,"profile":profile}));
            }
            let event = flow.events[index];
            resources.extend(sequence::particle::write(
                &mut flow.payload,
                event,
                &systems,
                u8::try_from(index)?,
            )?);
            report.push(
                json!({"event":event,"profiles":profiles,"source":source.source,
                "scale":source.scale,"placements":placements,"marker":format!("{name:08X}"),
                "locator":index,"thrust_threshold":activation,"gate":flow.gates[index],
                "lifetime":"Active until the thrust gate closes or the owner retires"}),
            );
        }
        let symbol = "vehicle-boost-flow";
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
        self.links.connections.push(links::Connection {
            consumer: links::Endpoint {
                namespace: 0x811c9dc5,
                object: Some(links::Object {
                    owner,
                    class: 0x80809789,
                    offset: flow.input as u64,
                }),
                selector: ordinal as u64,
            },
            provider: channel.provider.clone(),
            channel: channel.index,
            flags: 0,
        });
        let marker_owner = self.owner(markers::NATIVE_COMPONENT)?;
        let payload = markers::append_skeletal(
            &self.payloads[&marker_owner],
            &markers,
            bones["bones"].as_array().context("Boost target rig")?.len(),
        )?;
        self.payloads.insert(marker_owner, payload);
        Ok(Trails {
            events: BTreeMap::from([(owner, flow.events.into_iter().collect())]),
            report: json!({"owner":format!("{owner:08X}"),"attachments":report,
                "thrust_threshold":threshold,"input":flow.input,"gate":flow.gates[0],
                "parallel_entry":0,"independent_flow":true,"gameplay_verified":false,
                "limits":["Source profiles and bitmaps use straight ribbon segments. Thrust gates are authored independently of the vehicle carrier. Historical bending and light volumes are not translated. Live visibility and release cleanup require gameplay acceptance."]}),
        })
    }
}
