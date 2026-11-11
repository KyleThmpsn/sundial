//! Private resources that every translated sequence of one ability shares.
//!
//! Each particle system and cue becomes one private closure, so a controller graph and the
//! effect graphs that play the same system name the same private root. Event kinds without a
//! qualified converter keep their scheduled timing as native delays and are reported.
use super::*;
use crate::d1_roi::{
    audio,
    dependency::resource::{self as resources, Node},
    sequence::{ActionKey, Bindings, hud::Keys, owner::Owner},
};

/// Source event classes and kinds whose bodies this path does not yet translate.
pub const OMITTED: [(u32, u32); 8] = [
    (0x808007A2, 1),
    (0x808003BB, 10),
    (0x808025C1, 13),
    (0x8080052F, 20),
    (0x80802402, 24),
    (0x808028A6, 26),
    (0x80801A6D, 30),
    (0x80801B64, 36),
];

/// Stock sequences whose event records supply runtime defaults for native event classes that a
/// presentation graph's own sequences may lack. They are read, never modified. Sequence
/// `80B3A75F` holds a kind-12 feedback event, `80B596A5` kind-18 animation events and
/// `80BC57DB`, the native port of the Incendiary Rounds explosion sequence, a kind-30 table event.
/// Sequence `80BC35FE` holds kind-27 profile event `BAA1BE86`, whose Rise of Iron namesake is a
/// kind-24 profile event. Every other authored class the sequence writer supports takes the
/// smallest stock flow whose instance of it passes the writer's envelope checks, from a survey of
/// every stock effect flow, in which each holding flow passes them. A base flow's own instance
/// still comes first.
const WITNESSES: [(u32, u32); 16] = [
    (0x8080658E, 0x80F45DDD),
    (0x80803D8D, 0x80B3A75F),
    (0x80803FE9, 0x80B596A5),
    (0x80804AB4, 0x80BC57DB),
    (0x80804AC4, 0x80BC35FE),
    (0x808093CB, 0x80B4C1D5),
    (0x808093C4, 0x80FC61CF),
    (0x80806CC6, 0x80B384FC),
    (0x80806F49, 0x80B6E241),
    (0x80806B38, 0x80C4A33D),
    (0x80803F24, 0x80BA002B),
    (0x80808881, 0x80FE6F9B),
    (0x80804595, 0x80F7A9F5),
    (0x808066A3, 0x80C10550),
    (0x80804AB0, 0x80FF0745),
    (0x80804AB9, 0x80BEA1C5),
];

/// Every effect sequence in a source component list, by its position in that list.
pub fn occurrences(source: &Source, components: &[u32]) -> Result<BTreeMap<usize, u32>> {
    let mut occurrences = BTreeMap::new();
    for (index, &tag) in components.iter().enumerate() {
        let payload = source.read(tag)?;
        if payload.u32(payload.pointer(16)? - 4)? == 0x8080079A {
            occurrences.insert(index, tag);
        }
    }
    Ok(occurrences)
}

/// Read every listed source sequence, omitting the listed kinds.
fn owners(
    source: &Source,
    tags: &BTreeSet<u32>,
    omitted: &BTreeSet<(u32, u32)>,
) -> Result<BTreeMap<u32, Owner>> {
    tags.iter()
        .map(|&tag| Ok((tag, Owner::read(source, &source.read(tag)?, omitted)?)))
        .collect()
}

pub struct Shared {
    owners: BTreeMap<u32, Owner>,
    particles: BTreeMap<u32, String>,
    pub nodes: Vec<Node>,
    /// Private particle system roots, all of class `80806E28`.
    pub systems: BTreeSet<String>,
    /// The complete converted cue closure and its `80809802` cue roots.
    pub(in crate::d1_roi) audio: Option<(audio::Prepared, BTreeSet<String>)>,
    hud_keys: Option<Keys>,
    /// Stock sequence envelopes for event classes the prepared sequences need. Callers append
    /// them to their own envelopes.
    pub witnesses: Vec<Payload>,
    symbol: String,
    /// Private translations a caller prepared for created graphs, query profiles and query
    /// feedback, by source tag. Sequences that name one of these resources need its symbol.
    resources: Resources,
    /// Native action names a caller published for animation events, by source name, route and
    /// lane.
    actions: BTreeMap<ActionKey, u32>,
    /// Whether the caller gives statuses whose HUD identity has no native row a private HUD
    /// entry, as an attach action does through its HUD status.
    pub private_hud: bool,
    pub evidence: Value,
}

#[derive(Default)]
pub struct Resources {
    pub graphs: BTreeMap<u32, String>,
    /// Created graphs that keep the stock graph a native port of their creating sequence makes,
    /// by source graph.
    pub stock_graphs: BTreeMap<u32, u32>,
    pub profiles: BTreeMap<u32, String>,
    pub feedback: BTreeMap<u32, String>,
}

impl Shared {
    /// `groups` lists each graph's sequences, most important first. Every cue converts with its
    /// complete bank and media closure. A group whose cues would make the ability's closure
    /// unconvertible keeps its sound timing as delays, with the reason recorded, while the
    /// groups before it keep their sound.
    pub fn prepare(
        source: &Source,
        native: &mut Reader,
        groups: &[BTreeSet<u32>],
        symbol: &str,
    ) -> Result<Self> {
        Self::prepare_with(
            source,
            native,
            groups,
            symbol,
            &OMITTED.into_iter().collect(),
            false,
        )
    }

    /// As [`Self::prepare`], keeping the listed event kinds as reported delays instead of
    /// [`OMITTED`]. A caller that translates query and creation events leaves them out. A particle
    /// system that does not convert keeps its events' timing as delays with the reason recorded,
    /// as the advanced-transparency impact particle of Rise of Iron hand cannon spawn `80A6A859`
    /// does, so the closure's gameplay still translates.
    pub fn prepare_omitting(
        source: &Source,
        native: &mut Reader,
        groups: &[BTreeSet<u32>],
        symbol: &str,
        omitted: &BTreeSet<(u32, u32)>,
    ) -> Result<Self> {
        Self::prepare_with(source, native, groups, symbol, omitted, true)
    }

    /// As [`Self::prepare`], translating the listed [`OMITTED`] kinds as well. The caller binds
    /// what they need, such as published actions for animation events. Unconvertible particle
    /// systems still refuse preparation.
    pub fn prepare_translating(
        source: &Source,
        native: &mut Reader,
        groups: &[BTreeSet<u32>],
        symbol: &str,
        translated: &[(u32, u32)],
    ) -> Result<Self> {
        let omitted = OMITTED
            .into_iter()
            .filter(|kind| !translated.contains(kind))
            .collect();
        Self::prepare_with(source, native, groups, symbol, &omitted, false)
    }

    fn prepare_with(
        source: &Source,
        native: &mut Reader,
        groups: &[BTreeSet<u32>],
        symbol: &str,
        omitted: &BTreeSet<(u32, u32)>,
        unconverted_particles: bool,
    ) -> Result<Self> {
        let omitted = omitted.clone();
        let sequences = groups.iter().flatten().copied().collect::<BTreeSet<_>>();
        let mut owners = owners(source, &sequences, &omitted)?;
        let mut cues = BTreeSet::new();
        let mut prepared = None;
        let mut audio_refusals = Vec::new();
        let mut silent = BTreeSet::new();
        for group in groups {
            let group_cues = group
                .iter()
                .flat_map(|tag| owners[tag].plan().cues())
                .collect::<BTreeSet<_>>();
            if group_cues.is_empty() {
                continue;
            }
            let mut trial = cues.clone();
            trial.extend(group_cues);
            match audio::plan(source, native, &trial) {
                Ok(plan) => {
                    cues = trial;
                    prepared = Some(plan);
                }
                Err(error) => {
                    audio_refusals.push(json!({"sequences":group,"error":format!("{error:#}")}));
                    silent.extend(group.iter().copied());
                }
            }
        }
        if !silent.is_empty() {
            let mut without_sound = omitted.clone();
            without_sound.insert((0x80801F00, 2));
            owners.extend(self::owners(source, &silent, &without_sound)?);
        }
        let audio = prepared.map(|plan| {
            (
                plan,
                cues.iter()
                    .map(|&tag| audio::cue_symbol(tag))
                    .collect::<BTreeSet<_>>(),
            )
        });
        let mut particles = BTreeMap::new();
        let mut nodes = Vec::new();
        let mut identities = BTreeSet::new();
        let mut particle_evidence = Vec::new();
        let mut converted = BTreeMap::new();
        let mut particle_refusals = BTreeMap::new();
        for system in owners
            .values()
            .flat_map(|owner| owner.plan().particles())
            .collect::<BTreeSet<_>>()
        {
            let namespace = format!("{symbol}-particle-{system:08X}");
            let definition = source.read(system)?.u32(0)?;
            let bound = source.particle_bindings(definition)?;
            let inputs = bound
                .inputs()
                .iter()
                .map(|&index| (index, index))
                .collect::<BTreeMap<_, _>>();
            // The source and native CPU frame callers both supply a null runtime pool, so these
            // rows keep their indices without selecting producer values.
            let pool_rows = inputs.keys().max().map_or(1, |max| *max as usize + 1);
            let digest = Sha256::digest(namespace.as_bytes());
            let identity =
                0x8280_0000 | (u32::from_le_bytes(digest[..4].try_into()?) & 0x007F_FFFF);
            ensure!(
                identities.insert(identity),
                "Private particle compiled identities collide"
            );
            match source
                .particle_resources(system, native, &inputs, pool_rows, identity, &namespace)
            {
                Ok(resources) => {
                    converted.insert(system, resources);
                }
                Err(error)
                    if unconverted_particles && !crate::cancellation::is_cancelled(&error) =>
                {
                    particle_refusals.insert(system, format!("{error:#}"));
                }
                Err(error) => return Err(error),
            }
        }
        // Sequences that play a refused system keep every particle event's timing as a delay.
        let dark = owners
            .iter()
            .filter(|(_, owner)| {
                owner
                    .plan()
                    .particles()
                    .iter()
                    .any(|system| particle_refusals.contains_key(system))
            })
            .map(|(&tag, _)| tag)
            .collect::<BTreeSet<_>>();
        for &tag in &dark {
            let mut without = omitted.clone();
            without.insert((0x8080037A, 0));
            if silent.contains(&tag) {
                without.insert((0x80801F00, 2));
            }
            owners.insert(tag, Owner::read(source, &source.read(tag)?, &without)?);
        }
        // Only systems a sequence still plays are kept.
        let playing = owners
            .values()
            .flat_map(|owner| owner.plan().particles())
            .collect::<BTreeSet<_>>();
        for (system, resources) in converted {
            if playing.contains(&system) {
                particle_evidence.push(resources.evidence.clone());
                particles.insert(system, resources.system.clone());
                nodes.extend(resources.nodes);
            }
        }
        let systems = particles.values().cloned().collect::<BTreeSet<_>>();
        if !systems.is_empty() {
            let roots = systems.iter().map(String::as_str).collect::<Vec<_>>();
            resources::validate(&nodes, &roots)?;
        }
        let hud_keys = if owners
            .values()
            .all(|owner| owner.plan().overrides().is_empty())
        {
            None
        } else {
            Some(Keys::read(native)?)
        };
        let classes = owners
            .values()
            .flat_map(|owner| owner.plan().native_classes())
            .collect::<BTreeSet<_>>();
        let witnesses = WITNESSES
            .into_iter()
            .filter(|(class, _)| classes.contains(class))
            .map(|(_, tag)| Ok((*native.tag(tag, Some(0x80809C36))?).clone()))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            owners,
            particles,
            nodes,
            systems,
            audio,
            hud_keys,
            witnesses,
            symbol: symbol.to_owned(),
            resources: Resources::default(),
            actions: BTreeMap::new(),
            private_hud: false,
            evidence: json!({"cues":cues,"audio_refusals":audio_refusals,
                "particles":particle_evidence,
                "particle_refusals":particle_refusals.iter().map(|(system, error)| json!({"system":system,"error":error}))
                    .collect::<Vec<_>>(),
                "sequences_without_particles":dark,
                "omitted_kinds":omitted.iter().map(|(class, kind)| json!({"class":class,"kind":kind}))
                    .collect::<Vec<_>>(),
                "sequences_without_sound":silent}),
        })
    }

    /// Source cues of every prepared sequence, including sequences whose sound became delays.
    pub fn cues(&self) -> BTreeSet<u32> {
        self.owners
            .values()
            .flat_map(|owner| owner.plan().cues())
            .collect()
    }

    /// Graphs, profiles and feedback resources the prepared sequences name.
    pub fn named_resources(&self) -> (BTreeSet<u32>, BTreeSet<u32>, BTreeSet<u32>) {
        let mut out = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for owner in self.owners.values() {
            out.0.extend(owner.plan().graphs());
            out.1.extend(owner.plan().profiles());
            out.2.extend(owner.plan().feedback());
        }
        out
    }

    /// Supply the private symbols of the resources [`Self::named_resources`] lists.
    pub fn set_resources(&mut self, resources: Resources) {
        self.resources = resources;
    }

    /// The private symbol of a translated graph, which owners other than sequences may name.
    pub fn graph(&self, tag: u32) -> Option<&str> {
        self.resources.graphs.get(&tag).map(String::as_str)
    }

    /// Source action names, routes and lanes of every prepared animation event.
    pub fn actions(&self) -> BTreeSet<ActionKey> {
        self.owners
            .values()
            .flat_map(|owner| owner.plan().actions())
            .collect()
    }

    /// Supply the native action each [`Self::actions`] entry names.
    pub fn set_actions(&mut self, actions: BTreeMap<ActionKey, u32>) {
        self.actions = actions;
    }

    /// One prepared source sequence with its private resource symbols. The owner writer keeps
    /// every scalar input and locator at its source ordinal.
    pub fn bindings(&self, tag: u32) -> Result<(&Owner, Bindings)> {
        let owner = self
            .owners
            .get(&tag)
            .context("Source sequence was not prepared with the ability")?;
        let symbol = &self.symbol;
        let pick = |tags: BTreeSet<u32>,
                    map: &BTreeMap<u32, String>|
         -> Result<BTreeMap<u32, String>> {
            tags.into_iter()
                .map(|tag| {
                    Ok((
                        tag,
                        map.get(&tag)
                            .with_context(|| {
                                format!("Sequence resource {tag:08X} has no private translation")
                            })?
                            .clone(),
                    ))
                })
                .collect()
        };
        let (stock, private) = owner
            .plan()
            .graphs()
            .into_iter()
            .partition::<BTreeSet<_>, _>(|tag| self.resources.stock_graphs.contains_key(tag));
        let bindings = Bindings {
            graphs: pick(private, &self.resources.graphs)?,
            stock_graphs: stock
                .into_iter()
                .map(|tag| (tag, self.resources.stock_graphs[&tag]))
                .collect(),
            profiles: pick(owner.plan().profiles(), &self.resources.profiles)?,
            feedback: pick(owner.plan().feedback(), &self.resources.feedback)?,
            particles: owner
                .plan()
                .particles()
                .into_iter()
                .map(|system| (system, self.particles[&system].clone()))
                .collect(),
            overrides: owner
                .plan()
                .overrides()
                .into_iter()
                .map(|resource| (resource, format!("{symbol}-hud-{resource:08X}")))
                .collect(),
            cues: owner
                .plan()
                .cues()
                .into_iter()
                .map(|cue| (cue, audio::cue_symbol(cue)))
                .collect(),
            inputs: (0..owner.inputs().len())
                .map(|index| Ok((u32::try_from(index)?, u32::try_from(index)?)))
                .collect::<Result<_>>()?,
            hud_keys: (!owner.plan().overrides().is_empty())
                .then(|| self.hud_keys.clone())
                .flatten(),
            locators: (0..owner.locator_count())
                .map(|index| Ok((u8::try_from(index)?, u8::try_from(index)?)))
                .collect::<Result<_>>()?,
            actions: owner
                .plan()
                .actions()
                .into_iter()
                .map(|key| {
                    Ok((
                        key,
                        *self.actions.get(&key).with_context(|| {
                            format!("Animation action {:08X} has no published route", key.0)
                        })?,
                    ))
                })
                .collect::<Result<_>>()?,
            ..Bindings::default()
        };
        Ok((owner, bindings))
    }
}

/// Merge translated sequence resources, which sequences of different graphs may share.
pub fn merge(
    dependencies: &mut BTreeMap<String, dependency::Asset>,
    assets: Vec<dependency::Asset>,
) -> Result<()> {
    for asset in assets {
        match dependencies.get(&asset.symbol) {
            Some(existing) => ensure!(
                existing.payload.0 == asset.payload.0 && existing.class == asset.class,
                "Shared sequence resource translations differ"
            ),
            None => {
                dependencies.insert(asset.symbol.clone(), asset);
            }
        }
    }
    Ok(())
}
