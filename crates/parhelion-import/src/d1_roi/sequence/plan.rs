//! Whole event preparation has one commit boundary for topology, bodies and allocation data.
use super::{animation::Animation, completion::Completion, numeric::Numeric, sound::Sound, *};
use crate::d1_roi::{Source, ability::dependency};

enum Body {
    Numeric(Numeric),
    Animation(Animation),
    Completion(Completion),
    Sound(Sound),
}

/// Action names belong to an owner route and lane. The same source name can be selected by
/// both player and held-weapon animation consumers without sharing a translated state table.
pub type ActionKey = (u32, bool, bool);

#[derive(Default)]
pub struct Bindings {
    pub actions: BTreeMap<ActionKey, u32>,
    /// Completed private cue roots, indexed by source resource identity.
    pub cues: BTreeMap<u32, String>,
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
    pub resources: Vec<sound::CuePatch>,
    pub dependencies: Vec<dependency::Asset>,
}

impl Plan {
    /// Reject an unimplemented event before touching any native owner. Numeric output rows
    /// retain their original order and names rather than trusting a caller-supplied capacity.
    pub fn read(source: &Source, owner: &Payload) -> Result<Self> {
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
                    (Domain::Numeric, _, _) => {
                        Body::Numeric(Numeric::read(owner, event, outputs.len())?)
                    }
                    (Domain::External, 0x808025C1, 13) => {
                        Body::Animation(Animation::read(owner, event.offset)?)
                    }
                    (Domain::External, 0x808027EA, 14) => {
                        Body::Completion(Completion::read(owner, event.offset)?)
                    }
                    (Domain::External, 0x80801F00, 2) => {
                        Body::Sound(Sound::read(source, owner, event.offset)?)
                    }
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

    pub fn actions(&self) -> BTreeSet<ActionKey> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Animation(event) => Some((event.action(), event.owner_mode(), event.lane())),
                _ => None,
            })
            .collect()
    }

    pub fn cues(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Sound(event) => Some(event.cue()),
                _ => None,
            })
            .collect()
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
        ensure!(
            bindings.actions.iter().all(|(key, &name)| key.0 != name),
            "Imported sequence animation must use a private action name"
        );
        ensure!(
            self.cues() == bindings.cues.keys().copied().collect(),
            "Sequence cue bindings do not cover exactly the source resources"
        );
        if bindings.cues.is_empty() {
            ensure!(
                bindings.dependencies.is_empty(),
                "Sequence has unused effect dependencies"
            );
        } else {
            let roots = bindings.cues.values().cloned().collect::<Vec<_>>();
            dependency::validate(&bindings.dependencies, &roots)?;
            for root in roots {
                let cue = bindings
                    .dependencies
                    .iter()
                    .find(|asset| asset.symbol == root)
                    .context("Sequence cue dependency is absent")?;
                ensure!(
                    cue.class == 0x80809802,
                    "Sequence cue has the wrong native class"
                );
            }
        }
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
        let external = self
            .bodies
            .values()
            .filter_map(|body| match body {
                Body::Numeric(_) => None,
                Body::Animation(event) => Some(event.mapping()),
                Body::Completion(event) => Some(event.mapping()),
                Body::Sound(event) => Some(event.mapping()),
            })
            .collect();
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
                Body::Numeric(event) => event.write(&mut output, to)?,
                Body::Completion(event) => event.write(&mut output, to)?,
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
            dependencies: bindings.dependencies.clone(),
        })
    }
}
