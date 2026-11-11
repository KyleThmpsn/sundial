//! Whole event preparation has one commit boundary for topology, bodies and allocation data.
use super::{
    animation::Animation, camera::Camera, completion::Completion, feedback::Feedback, hud::Hud,
    numeric::Numeric, particle::Particle, profile::Profile, query::Query, signal::Signal,
    sound::Sound, spawn::Spawn, table::Table, *,
};
use crate::d1_roi::{Source, dependency};

enum Body {
    Numeric(Numeric),
    Animation(Animation),
    Completion(Completion),
    Signal(Signal),
    Sound(Sound),
    Hud(Hud),
    Particle(Particle),
    /// Boxed, since a query carries its whole program set and is far larger than other bodies.
    Query(Box<Query>),
    Spawn(Spawn),
    Camera(Camera),
    /// Boxed for its ten programs.
    Feedback(Box<Feedback>),
    Table(Table),
    Profile(Profile),
    /// A caller-listed external event keeps its scheduled timing as a native delay.
    Omitted(NativeEvent),
}

/// Action names belong to an owner route and lane. The same source name can be selected by
/// both player and held-weapon animation consumers without sharing a translated state table.
pub type ActionKey = (u32, bool, bool);

#[derive(Default)]
pub struct Bindings {
    pub actions: BTreeMap<ActionKey, u32>,
    /// Completed private cue roots, indexed by source resource identity.
    pub cues: BTreeMap<u32, String>,
    /// Private symbols for generated HUD override resources, indexed by source identity.
    pub overrides: BTreeMap<u32, String>,
    /// Completed private particle systems with primary and secondary locator bindings.
    pub particles: BTreeMap<u32, String>,
    /// Private translations of graphs that creation events make, by source graph.
    pub graphs: BTreeMap<u32, String>,
    /// Stock graphs that creation events make instead, by source graph.
    pub stock_graphs: BTreeMap<u32, u32>,
    /// Private outgoing profile owners named by query actions, by source owner.
    pub profiles: BTreeMap<u32, String>,
    /// Private feedback resources of query damage blocks and feedback events, by source resource.
    pub feedback: BTreeMap<u32, String>,
    pub hud_keys: Option<hud::Keys>,
    pub inputs: BTreeMap<u32, u32>,
    pub locators: BTreeMap<u8, u8>,
    pub dependencies: Vec<dependency::Asset>,
}

pub struct Plan {
    topology: Topology,
    bodies: BTreeMap<usize, Body>,
    outputs: Vec<[u8; 8]>,
}

/// Event bodies and their allocation state have both completed. Resource patches still need
/// the containing graph's private allocations, and action bindings need both animation owners.
pub struct Prepared {
    pub topology: Written,
    pub actions: Vec<animation::Action>,
    pub resources: Vec<ResourcePatch>,
    pub dependencies: Vec<dependency::Asset>,
}

impl Plan {
    /// Reject an unimplemented event before touching any native owner. Numeric output rows
    /// retain their original order and names rather than trusting a caller-supplied capacity.
    pub fn read(source: &Source, owner: &Payload) -> Result<Self> {
        Self::read_omitting(source, owner, &BTreeSet::new())
    }

    /// Like [`Self::read`], but external events whose class and kind the caller lists become
    /// native delays with the same timing. The caller records each omission it requests.
    pub fn read_omitting(
        source: &Source,
        owner: &Payload,
        omitted: &BTreeSet<(u32, u32)>,
    ) -> Result<Self> {
        let topology = Topology::read(owner)?;
        let outputs = owner
            .array(owner.pointer(24)? + 0x190, 56, Some(0x80800788))?
            .into_iter()
            .map(|row| owner.bytes::<8>(row + 48))
            .collect::<Result<Vec<_>>>()?;
        let mut bodies = BTreeMap::new();
        for event in topology.events() {
            let read = || -> Result<Body> {
                Ok(match (event.domain, event.class, event.kind) {
                    (Domain::External, class, kind) if omitted.contains(&(class, kind)) => {
                        Body::Omitted(NativeEvent {
                            source_class: class,
                            source_kind: kind,
                            class: 0x808093CB,
                        })
                    }
                    (Domain::Numeric, _, _) => {
                        Body::Numeric(Numeric::read(owner, event, outputs.len())?)
                    }
                    (Domain::External, 0x808025C1, 13) => {
                        Body::Animation(Animation::read(owner, event.offset)?)
                    }
                    (Domain::External, 0x808027EA, 14) => {
                        Body::Completion(Completion::read(owner, event.offset)?)
                    }
                    (Domain::External, 0x8080050E, 4) => {
                        Body::Signal(Signal::read(owner, event.offset)?)
                    }
                    // A sound event that names no cue has nothing to play, as a particle event
                    // without a system has not. It keeps its timing as a reported delay.
                    (Domain::External, 0x80801F00, 2)
                        if owner.u32(event.offset + 0x20)? == u32::MAX =>
                    {
                        Body::Omitted(NativeEvent {
                            source_class: 0x80801F00,
                            source_kind: 2,
                            class: 0x808093CB,
                        })
                    }
                    (Domain::External, 0x80801F00, 2) => {
                        Body::Sound(Sound::read(source, owner, event.offset)?)
                    }
                    (Domain::External, 0x80802573, 8) => {
                        Body::Hud(Hud::read(source, owner, event.offset)?)
                    }
                    // A particle event that names no system has nothing to play. It keeps its
                    // timing as a delay and is counted with the omitted events.
                    (Domain::External, 0x8080037A, 0)
                        if owner.u32(event.offset + 0x20)? == u32::MAX =>
                    {
                        Body::Omitted(NativeEvent {
                            source_class: 0x8080037A,
                            source_kind: 0,
                            class: 0x808093CB,
                        })
                    }
                    (Domain::External, 0x8080037A, 0) => {
                        Body::Particle(Particle::read(source, owner, event.offset)?)
                    }
                    (Domain::External, 0x808028A6, 26) => {
                        Body::Query(Box::new(Query::read(source, owner, event.offset)?))
                    }
                    (Domain::External, 0x808003BB, 10) => {
                        Body::Spawn(Spawn::read(source, owner, event.offset)?)
                    }
                    (Domain::External, 0x80802045, 18) => {
                        Body::Camera(Camera::read(source, owner, event.offset)?)
                    }
                    (Domain::External, 0x80802891, 7) => {
                        Body::Feedback(Box::new(Feedback::read(source, owner, event.offset)?))
                    }
                    // A sequence whose sound became delays leaves its table's cues empty too.
                    (Domain::External, 0x80802402, 24) => {
                        Body::Profile(Profile::read(source, owner, event.offset)?)
                    }
                    (Domain::External, 0x8080237E, 27) => Body::Table(Table::read(
                        source,
                        owner,
                        event.offset,
                        omitted.contains(&(0x80801F00, 2)),
                    )?),
                    _ => anyhow::bail!("No complete body converter"),
                })
            };
            bodies.insert(
                event.offset,
                read().with_context(|| {
                    format!(
                        "Source sequence event {:08X} kind {} at {:X}",
                        event.class, event.kind, event.offset
                    )
                })?,
            );
        }
        source.unchanged()?;
        Ok(Self {
            topology,
            bodies,
            outputs,
        })
    }

    pub fn topology(&self) -> &Topology {
        &self.topology
    }

    /// Source external events written as native delays, by class and kind.
    pub fn omitted(&self) -> BTreeMap<(u32, u32), usize> {
        let mut counts = BTreeMap::new();
        for body in self.bodies.values() {
            if let Body::Omitted(event) = body {
                *counts
                    .entry((event.source_class, event.source_kind))
                    .or_default() += 1;
            }
        }
        counts
    }

    /// Ordered locator pairs of every particle event, which index the source declarations.
    pub fn particle_locators(&self) -> BTreeSet<u8> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Particle(event) => Some(event.locators()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    pub fn actions(&self) -> BTreeSet<ActionKey> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Animation(event) => Some((event.action(), event.owner_mode(), event.lane())),
                _ => None,
            })
            .collect()
    }

    /// Cues of sound events and presentation table leaves.
    pub fn cues(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .flat_map(|body| match body {
                Body::Sound(event) => vec![event.cue()],
                Body::Table(event) => event.cues().collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    pub fn overrides(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Hud(event) => Some(event.resource()),
                _ => None,
            })
            .collect()
    }

    pub fn particles(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Particle(event) => Some(event.system()),
                _ => None,
            })
            .collect()
    }

    /// Graphs that creation events make, query actions attach and presentation table leaves name.
    pub fn graphs(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .flat_map(|body| match body {
                Body::Spawn(event) => vec![event.graph()],
                Body::Query(event) => event.graphs().collect(),
                Body::Table(event) => event.graphs().collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// The origin locator ordinal a presentation table event without a locator names. The owner
    /// writer appends it after the source locators.
    pub fn origin(&self) -> Option<u8> {
        self.bodies.values().find_map(|body| match body {
            Body::Table(event) => event.origin(),
            _ => None,
        })
    }

    /// Outgoing profile owners named by query actions and profile events.
    pub fn profiles(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .flat_map(|body| match body {
                Body::Query(event) => event.profiles().collect::<Vec<_>>(),
                Body::Profile(event) => vec![event.profile()],
                _ => Vec::new(),
            })
            .collect()
    }

    /// Feedback resources of query damage blocks and feedback events.
    pub fn feedback(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .flat_map(|body| match body {
                Body::Query(event) => event.feedback().into_iter().collect::<Vec<_>>(),
                Body::Feedback(event) => event.resources().collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// Native classes of the external events this plan writes, delays included.
    pub fn native_classes(&self) -> BTreeSet<u32> {
        self.external()
            .into_values()
            .map(|event| event.class)
            .collect()
    }

    fn external(&self) -> BTreeMap<usize, NativeEvent> {
        self.bodies
            .iter()
            .filter_map(|(&offset, body)| match body {
                Body::Numeric(_) => None,
                Body::Animation(event) => Some(event.mapping()),
                Body::Completion(event) => Some(event.mapping()),
                Body::Signal(event) => Some(event.mapping()),
                Body::Sound(event) => Some(event.mapping()),
                Body::Hud(event) => Some(event.mapping()),
                Body::Particle(event) => Some(event.mapping()),
                Body::Query(event) => Some(event.mapping()),
                Body::Spawn(event) => Some(event.mapping()),
                Body::Camera(event) => Some(event.mapping()),
                Body::Feedback(event) => Some(event.mapping()),
                Body::Table(event) => Some(event.mapping()),
                Body::Profile(event) => Some(event.mapping()),
                Body::Omitted(event) => Some((offset, *event)),
            })
            .collect()
    }

    /// Locators addressed by query, creation, camera and table events, which need native mappings.
    pub fn event_locators(&self) -> BTreeSet<u8> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Query(event) => Some(event.locator()),
                Body::Spawn(event) => Some(event.locator()),
                Body::Camera(event) => event.locator(),
                Body::Table(event) => Some(event.locator()),
                _ => None,
            })
            .collect()
    }

    fn dependencies(&self, bindings: &Bindings) -> Result<Vec<dependency::Asset>> {
        ensure!(
            self.graphs()
                == bindings
                    .graphs
                    .keys()
                    .chain(bindings.stock_graphs.keys())
                    .copied()
                    .collect()
                && bindings
                    .graphs
                    .keys()
                    .all(|tag| !bindings.stock_graphs.contains_key(tag))
                && self.profiles() == bindings.profiles.keys().copied().collect()
                && self.feedback() == bindings.feedback.keys().copied().collect(),
            "Sequence graph, profile or feedback bindings do not cover exactly the source resources"
        );
        ensure!(
            self.event_locators()
                .iter()
                .all(|index| bindings.locators.contains_key(index)),
            "Sequence query, creation or camera locator has no native mapping"
        );
        ensure!(
            self.particles() == bindings.particles.keys().copied().collect(),
            "Sequence particle bindings do not cover exactly the source systems"
        );
        ensure!(
            self.cues() == bindings.cues.keys().copied().collect(),
            "Sequence cue bindings do not cover exactly the source resources"
        );
        ensure!(
            self.overrides() == bindings.overrides.keys().copied().collect()
                && bindings.overrides.is_empty() == bindings.hud_keys.is_none(),
            "Sequence HUD bindings do not cover exactly the source resources"
        );
        let mut dependencies = bindings.dependencies.clone();
        let mut converted = BTreeSet::new();
        for body in self.bodies.values() {
            if let Body::Camera(event) = body {
                for asset in event.assets() {
                    if let Some(existing) = dependencies.iter().find(|a| a.symbol == asset.symbol) {
                        ensure!(
                            existing.class == asset.class && existing.payload.0 == asset.payload.0,
                            "Camera rail translations disagree"
                        );
                    } else {
                        dependencies.push(asset.clone());
                    }
                }
            }
            if let Body::Hud(event) = body
                && converted.insert(event.resource())
            {
                dependencies.push(
                    event.translate(
                        &bindings.overrides[&event.resource()],
                        bindings
                            .hud_keys
                            .as_ref()
                            .context("Native HUD keys are absent")?,
                    )?,
                );
            }
            if let Body::Table(event) = body
                && !dependencies.iter().any(|a| a.symbol == event.symbol())
            {
                dependencies.push(event.translate(
                    &bindings.cues,
                    &bindings.graphs,
                    &bindings.stock_graphs,
                )?);
            }
        }
        // Particle systems and cues have package-reference and storage edges that typed
        // dependency assets cannot express. The caller validates each complete closure
        // with its own resources: particle systems with their render resources, and cues
        // with the audio graph's banks and media.
        ensure!(
            dependencies.iter().all(|asset| {
                !bindings
                    .particles
                    .values()
                    .chain(bindings.cues.values())
                    .chain(bindings.graphs.values())
                    .chain(bindings.profiles.values())
                    .chain(bindings.feedback.values())
                    .any(|s| *s == asset.symbol)
            }),
            "Particle systems, cues, graphs, profiles and feedback belong to their own closures"
        );
        let mut roots = bindings.overrides.values().cloned().collect::<Vec<_>>();
        roots.extend(
            self.bodies
                .values()
                .filter_map(|body| match body {
                    Body::Camera(event) => Some(event.assets().map(|asset| asset.symbol.clone())),
                    _ => None,
                })
                .flatten(),
        );
        roots.extend(self.bodies.values().filter_map(|body| match body {
            Body::Table(event) => Some(event.symbol()),
            _ => None,
        }));
        roots.sort();
        roots.dedup();
        // Presentation tables name cues and graphs, whose closures their callers validate.
        let external = bindings
            .cues
            .values()
            .map(|symbol| (symbol.clone(), 0x80809802))
            .chain(
                bindings
                    .graphs
                    .values()
                    .map(|symbol| (symbol.clone(), 0x80809C0F)),
            )
            .collect();
        if roots.is_empty() {
            ensure!(
                dependencies.is_empty(),
                "Sequence has unused effect dependencies"
            );
        } else {
            dependency::validate_external(&dependencies, &roots, &external)?;
            for (root, class) in bindings.overrides.values().map(|s| (s, 0x80803F26)) {
                let asset = dependencies
                    .iter()
                    .find(|asset| &asset.symbol == root)
                    .context("Sequence resource dependency is absent")?;
                ensure!(
                    asset.class == class,
                    "Sequence resource has the wrong native class"
                );
            }
        }
        Ok(dependencies)
    }

    /// Complete every supported event on copies, then replace both caller buffers together.
    /// No unsupported body or missing typed resource is treated as an empty event.
    pub fn write(
        &self,
        source: &Payload,
        target: &mut Payload,
        allocation: &mut Payload,
        envelopes: &[Payload],
        bindings: &Bindings,
    ) -> Result<Prepared> {
        ensure!(
            self.actions() == bindings.actions.keys().copied().collect(),
            "Sequence animation bindings do not cover exactly the selected owner routes"
        );
        // Player-route actions resolve in the held weapon's first-person table, as source
        // actions did, so they keep the name that table publishes. Other routes resolve in an
        // owner's table, where an imported action needs a private name.
        ensure!(
            bindings
                .actions
                .iter()
                .all(|(key, &name)| key.1 || key.0 != name),
            "Imported sequence animation must use a private action name"
        );
        let dependencies = self.dependencies(bindings)?;
        let outputs = target.array(target.pointer(24)? + 0x178, 48, Some(0x808084DF))?;
        ensure!(
            outputs.len() == self.outputs.len(),
            "Native sequence output count differs"
        );
        for (row, expected) in outputs.into_iter().zip(&self.outputs) {
            ensure!(
                target.bytes::<8>(row + 40)? == *expected,
                "Native sequence output identity or policy differs"
            );
        }
        let external = self.external();
        let mut output = target.clone();
        let mut storage = allocation.clone();
        let topology =
            self.topology
                .write(source, &mut output, &mut storage, envelopes, &external)?;
        let mut actions = Vec::new();
        let mut resources = Vec::new();
        for (&from, body) in &self.bodies {
            let to = *topology
                .events
                .get(&from)
                .context("Allocated event is absent")?;
            match body {
                // The common writer already gave the delay its source timing and parent.
                Body::Omitted(_) => {}
                Body::Numeric(event) => event.write(&mut output, to)?,
                Body::Completion(event) => event.write(&mut output, to)?,
                Body::Signal(event) => event.write(&mut output, to)?,
                Body::Camera(event) => {
                    resources.extend(event.write(&mut output, to, &bindings.locators)?)
                }
                Body::Feedback(event) => resources.extend(event.write(
                    &mut output,
                    to,
                    &bindings.feedback,
                    &bindings.inputs,
                )?),
                Body::Table(event) => {
                    resources.push(event.write(&mut output, to, &bindings.locators)?)
                }
                Body::Profile(event) => resources.push(event.write(
                    &mut output,
                    to,
                    &bindings.profiles[&event.profile()],
                )?),
                Body::Animation(event) => {
                    let key = (event.action(), event.owner_mode(), event.lane());
                    let selected = BTreeMap::from([(event.action(), bindings.actions[&key])]);
                    actions.push(event.write(&mut output, to, &selected)?);
                }
                Body::Sound(event) => resources.push(event.write(
                    &mut output,
                    to,
                    &bindings.cues[&event.cue()],
                    &bindings.inputs,
                    &bindings.locators,
                )?),
                Body::Hud(event) => resources.push(event.write(
                    &mut output,
                    to,
                    &bindings.overrides[&event.resource()],
                )?),
                Body::Particle(event) => resources.push(event.write(
                    &mut output,
                    to,
                    &bindings.particles[&event.system()],
                    &bindings.locators,
                )?),
                Body::Query(event) => resources.extend(event.write(
                    &mut output,
                    to,
                    &bindings.profiles,
                    (&bindings.graphs, &bindings.stock_graphs),
                    event.feedback().map(|tag| bindings.feedback[&tag].as_str()),
                    (&bindings.locators, &bindings.inputs),
                )?),
                Body::Spawn(event) => match bindings.stock_graphs.get(&event.graph()) {
                    Some(&graph) => {
                        event.write_stock(&mut output, to, graph, &bindings.locators)?
                    }
                    None => resources.push(event.write(
                        &mut output,
                        to,
                        &bindings.graphs[&event.graph()],
                        &bindings.locators,
                    )?),
                },
            }
        }
        let size = u64::try_from(output.0.len())?;
        put(&mut output.0, 0, &size.to_le_bytes())?;
        *target = output;
        *allocation = storage;
        Ok(Prepared {
            topology,
            actions,
            resources,
            dependencies,
        })
    }
}
