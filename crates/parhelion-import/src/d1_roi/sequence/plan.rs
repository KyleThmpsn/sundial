//! Whole event preparation has one commit boundary for topology, bodies and allocation data.
use super::{
    animation::Animation, completion::Completion, hud::Hud, numeric::Numeric, sound::Sound, *,
};
use crate::d1_roi::{Source, ability::dependency};

enum Body {
    Numeric(Numeric),
    Animation(Animation),
    Completion(Completion),
    Sound(Sound),
    Hud(Hud),
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
                    (Domain::External, 0x80802573, 8) => {
                        Body::Hud(Hud::read(source, owner, event.offset)?)
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

    pub fn overrides(&self) -> BTreeSet<u32> {
        self.bodies
            .values()
            .filter_map(|body| match body {
                Body::Hud(event) => Some(event.resource()),
                _ => None,
            })
            .collect()
    }

    fn dependencies(&self, bindings: &Bindings) -> Result<Vec<dependency::Asset>> {
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
        }
        let roots = bindings
            .cues
            .values()
            .chain(bindings.overrides.values())
            .cloned()
            .collect::<Vec<_>>();
        if roots.is_empty() {
            ensure!(
                dependencies.is_empty(),
                "Sequence has unused effect dependencies"
            );
        } else {
            dependency::validate(&dependencies, &roots)?;
            for (root, class) in bindings
                .cues
                .values()
                .map(|s| (s, 0x80809802))
                .chain(bindings.overrides.values().map(|s| (s, 0x80803F26)))
            {
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
        ensure!(
            bindings.actions.iter().all(|(key, &name)| key.0 != name),
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
        let external = self
            .bodies
            .values()
            .filter_map(|body| match body {
                Body::Numeric(_) => None,
                Body::Animation(event) => Some(event.mapping()),
                Body::Completion(event) => Some(event.mapping()),
                Body::Sound(event) => Some(event.mapping()),
                Body::Hud(event) => Some(event.mapping()),
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
                Body::Hud(event) => resources.push(event.write(
                    &mut output,
                    to,
                    &bindings.overrides[&event.resource()],
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
            dependencies,
        })
    }
}
