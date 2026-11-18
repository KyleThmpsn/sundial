//! Source boost and wingtip contrails as thrust-gated particle events of the vehicle's effect
//! flow. Additive profiles draw as native ribbons and the others as camera-facing quads.
use super::*;
use crate::{
    halo_reach::{
        Scene,
        cache::{Cache, Tag},
        effect::trail as contrail,
        model,
        native::seat,
        projectile::effect::bolt,
        resource::Pages,
    },
    tiger::{markers, sequence},
};

/// One source effect attachment that carries contrails, with the markers of its location.
pub(super) struct Attachment<'a> {
    source: Tag,
    scale: String,
    markers: Vec<&'a model::Marker>,
    profiles: Vec<contrail::Profile>,
}

/// The source boost and wingtip effects that carry contrails, and the paths of those without one,
/// such as the Sabre's boost light volume, which this route does not translate. Each contrail part
/// names its marker group by its primary location index at `+4`.
pub(super) fn attachments<'a>(
    cache: &Cache,
    scene: &'a Scene,
) -> Result<(Vec<Attachment<'a>>, Vec<String>)> {
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
    let mut result = Vec::new();
    let mut skipped = Vec::new();
    let mut scales = BTreeSet::new();
    for effect in effects {
        let scale = effect["primary_scale"]
            .as_str()
            .context("Source trail scale")?;
        let tag: Tag = serde_json::from_value(effect["tag"].clone())?;
        let at = tag.address()?;
        let locations = cache.block(at + 0x20, 8)?;
        let mut profiles = Vec::new();
        let mut used = BTreeSet::new();
        for event in cache.block(at + 0x2c, 0x40)? {
            for part in cache.block(event + 0x1c, 0x64)? {
                if let Some(reference) = cache.reference(part + 0x14, None)?
                    && reference.group == "cntl"
                {
                    ensure!(
                        cache.i16(part + 6)? == -1,
                        "Source contrail selects a separate location"
                    );
                    used.insert(cache.i16(part + 4)?);
                    profiles.extend(contrail::read(cache, &reference)?);
                }
            }
        }
        if profiles.is_empty() {
            skipped.push(tag.path.clone());
            continue;
        }
        ensure!(scales.insert(scale), "Source trail scale is ambiguous");
        let [location] = used.into_iter().collect::<Vec<_>>()[..] else {
            anyhow::bail!("Source contrails of one effect use several marker groups");
        };
        let location = locations
            .get(usize::try_from(location).context("Source contrail location")?)
            .context("Source contrail location is outside its effect")?;
        let name = cache.string_id(cache.u32(*location)?)?;
        let model = &scene.models[0].model;
        let mut selected = Vec::new();
        for marker in model.markers.iter().filter(|marker| marker.name == name) {
            if model.marker_selected(marker)? {
                selected.push(marker);
            }
        }
        ensure!(
            !selected.is_empty() && selected.len() <= 8,
            "Source trail marker count is unsupported"
        );
        result.push(Attachment {
            source: tag,
            scale: scale.to_owned(),
            markers: selected,
            profiles,
        });
    }
    Ok((result, skipped))
}

/// The flow gate of each attachment and the boost thrust threshold. Wingtip trails open at a
/// quarter of cruise thrust, and boost trails halfway between cruise and boost thrust.
pub(super) fn gates(
    source: &[Attachment<'_>],
    controls: &Value,
) -> Result<(Vec<flow::Trail>, f32)> {
    let (cruise, boost) = super::super::flight::thrust(controls)?;
    let threshold = (cruise + boost) * 0.5;
    let trails = source
        .iter()
        .enumerate()
        .map(|(index, attachment)| {
            let markers = attachment.markers.len();
            flow::Trail {
                // One event for each profile at each marker, in the order `write` emits systems.
                events: (0..attachment.profiles.len())
                    .flat_map(|profile| {
                        (0..markers).map(move |marker| {
                            super::super::damage::hash(&format!(
                                "source/trail/{index}/event/{profile}/{marker}"
                            ))
                        })
                    })
                    .collect(),
                markers: (0..markers)
                    .map(|marker| {
                        super::super::damage::hash(&format!("source/trail/{index}/{marker}"))
                    })
                    .collect(),
                threshold: if attachment.scale == "wingtips" {
                    cruise * 0.25
                } else {
                    threshold
                },
            }
        })
        .collect();
    Ok((trails, threshold))
}

/// Where the trail systems are emitted and what they read.
#[derive(Clone, Copy)]
pub(super) struct Inputs<'a> {
    pub scene: &'a Scene,
    pub bones: &'a Value,
    pub controls: &'a Value,
    pub cache: &'a Cache,
    /// The first source bone of the vehicle rig.
    pub first: usize,
}

/// Emits each attachment's particle systems, writes one event of `flow` for each and adds its
/// markers and system references. Returns the attachment reports.
#[expect(
    clippy::too_many_arguments,
    reason = "Trail authoring joins source attachments, the flow and the graph's particle assets"
)]
pub(super) fn write(
    native: &mut Reader,
    graph: &mut Graph,
    pages: &mut Pages,
    inputs: &Inputs<'_>,
    flow: &mut flow::Flow,
    source: &[Attachment<'_>],
    trails: &[flow::Trail],
    markers: &mut Vec<markers::Marker>,
    resources: &mut Vec<(usize, String)>,
) -> Result<Vec<Value>> {
    let Inputs {
        scene,
        bones,
        controls,
        cache,
        first,
    } = *inputs;
    let model = &scene.models[0].model;
    let mut report = Vec::new();
    for (index, source) in source.iter().enumerate() {
        let trail = &trails[index];
        let locators = flow.locators[index].clone();
        ensure!(
            trail.markers.len() == source.markers.len() && locators.len() == source.markers.len(),
            "Source trail locators differ from its markers"
        );
        let mut placements = Vec::new();
        for (source_marker, &name) in source.markers.iter().zip(&trail.markers) {
            let bone = source_marker.bone.map_or(0, |bone| first + bone);
            let world = seat::marker_pose(model, source_marker)?;
            let (position, orientation) = seat::frame(bones, bone, world)?;
            markers.push(markers::Marker {
                name,
                binding: [markers::EFFECT_GROUP, u32::try_from(bone)?],
                position: [position[0], position[1], position[2], 1.],
                orientation,
                row: 0,
            });
            placements.push(json!({"source_marker":source_marker.name,"bone":bone,
                "marker":format!("{name:08X}"),"position":position,"source_position":world.0}));
        }
        let speed = super::super::motion::number(controls, "forward_source_units_per_second")?
            * scene.metres();
        let mut profiles = Vec::new();
        // One system row per marker, so each marker's samples form a trail of their own.
        let mut systems = Vec::new();
        for (profile_index, profile) in source.profiles.iter().enumerate() {
            let prefix = format!("vehicle-trail-{index}-{profile_index}");
            // An additive profile draws the Fallen bolt's ribbon, which follows the marker's
            // path. A profile the ribbon cannot take, such as a black one, keeps the quads.
            let ribbon = if profile.additive() {
                bolt::ribbon(
                    cache,
                    pages,
                    native,
                    graph,
                    profile,
                    &prefix,
                    scene.metres(),
                )
                .map_err(|error| format!("{error:#}"))
            } else {
                Err("The source profile is not additive".to_owned())
            };
            match ribbon {
                Ok(ribbon) => {
                    for &marker in &locators {
                        let bindings = ribbon
                            .locators
                            .iter()
                            .map(|&ordinal| match ordinal {
                                0 => Ok(marker),
                                _ => flow
                                    .world
                                    .context("Vehicle effect flow has no world_space locator"),
                            })
                            .collect::<Result<Vec<_>>>()?;
                        systems.push((ribbon.symbol.clone(), bindings));
                    }
                    profiles.push(json!({"system":ribbon.symbol,"presentation":"ribbon",
                        "ribbon":ribbon.report}));
                }
                Err(refusal) => {
                    let (symbol, profile) = contrail::emit(
                        cache,
                        pages,
                        native,
                        graph,
                        profile,
                        &prefix,
                        scene.metres(),
                        speed,
                    )?;
                    systems.extend(
                        locators
                            .iter()
                            .map(|&marker| (symbol.clone(), vec![marker])),
                    );
                    profiles.push(json!({"system":symbol,"presentation":"quads",
                        "profile":profile,"ribbon_refusal":refusal}));
                }
            }
        }
        let events = flow.events[index].clone();
        ensure!(
            events.len() == systems.len(),
            "Source trail systems differ from its events"
        );
        // One system to an event. The native start gives every later row of one event the first
        // row's lifetime as a start offset, so a second marker's ribbon would not start as the
        // first's does.
        for (&event, system) in events.iter().zip(&systems) {
            resources.extend(sequence::particle::write_bound(
                &mut flow.payload,
                event,
                std::slice::from_ref(system),
            )?);
        }
        report.push(
            json!({"events":events,"profiles":profiles,"source":source.source,
            "scale":source.scale,"placements":placements,
            "body_group":markers::EFFECT_GROUP,
            "locators":locators,"world_space_locator":flow.world,
            "thrust_threshold":trail.threshold,"gate":flow.gates[index],
            "lifetime":"Active until the thrust gate closes or the owner retires"}),
        );
    }
    Ok(report)
}

impl Assembly {
    /// The private flight-thrust bank channel the trail gates read.
    pub(super) fn thrust_channel(
        &mut self,
        native: &mut Reader,
        graph: &mut Graph,
        motion: &motion::Motion,
    ) -> Result<Channel> {
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
        self.channel_bridge(native, graph, "vehicle-thrust-channels", &[(object, 0)])?
            .pop()
            .context("Flight thrust channel")
    }
}
