//! A source Super with every exclusive upgrade on the retained native controller, with original
//! source presentation. A revival choice anchors the Super: its equipped foundation, its
//! exclusive upgrades, the passives that grant only its own properties, its sequences, audio and
//! human actions all become one private graph. Input authority and completed resurrection
//! require gameplay.
use super::{Choice, Source, animation, controller, dependency, effect, grants, property_choice};
use crate::{
    d1_roi::dependency::resource::{self as resources, Node},
    presentation::put,
    tiger::{
        entity::{compiled, links},
        payload::Payload,
        reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use sundial::package_authoring::{
    PackageManager, ability_modifier, declared_payload_object_references,
    declared_payload_references, runtime,
};
use tiger_pkg::TagHash;
mod retained;
mod source;

/// One exclusive source upgrade. An available upgrade's complete property groups are in the
/// private bank. An unavailable one still names effect graphs without a private translation.
#[derive(Clone, Serialize)]
pub struct Upgrade {
    pub choice: Choice,
    /// Source display row, top to bottom.
    pub row: i16,
    pub keys: Vec<u32>,
    pub revival: bool,
    pub available: bool,
    /// Source effect graphs the upgrade's properties select.
    pub resources: Vec<u32>,
}

struct Audio {
    graph: Value,
    files: Vec<(String, Vec<u8>)>,
    cues: BTreeSet<String>,
}

pub struct Prepared {
    hud: Vec<Value>,
    pub root: String,
    /// The equipped foundation and the selected modifier are different source choices.
    pub ability: Choice,
    /// The anchoring revival choice's keys.
    pub keys: Vec<u32>,
    /// Every exclusive upgrade in source row order, each with only its own keys.
    pub upgrades: Vec<Upgrade>,
    /// Other grid choices that grant only the Super's own properties, such as a duration
    /// passive. Their keys join the private bank, and the caller places them on path nodes.
    pub passives: Vec<Upgrade>,
    pub assets: Vec<dependency::Asset>,
    /// Particle systems and their render resources, with package-reference edges.
    pub nodes: Vec<Node>,
    /// Converted source cues: the audio graph section, its payload files and the cue roots
    /// that sequence patches name.
    audio: Option<Audio>,
    pub templates: BTreeMap<String, u32>,
    /// Private human actions the build adds to the shared native human tables.
    human: Option<animation::Human>,
    pub evidence: Value,
}

/// Bind every prepared animation event. Cached-provider events name source human actions,
/// whose clips convert into private actions for the shared human tables. Player-route events keep
/// their source names, which the held weapon's first-person table resolves, as stock sequences'
/// `13D833A4` events do.
fn human_actions(
    source: &Source,
    native: &mut Reader,
    shared: &mut effect::Shared,
    symbol: &str,
) -> Result<(Option<animation::Human>, Value)> {
    let keys = shared.actions();
    let body = keys
        .iter()
        .filter(|key| !key.1)
        .map(|key| key.0)
        .collect::<BTreeSet<_>>();
    let prepared = if body.is_empty() {
        None
    } else {
        let plan = animation::read(source, animation::SOURCE_HUMAN_LOOKUP, &body)?;
        Some(animation::prepare(
            source,
            native,
            &plan,
            &animation::HUMAN,
            &format!("{symbol}-human"),
            &BTreeMap::new(),
            &[],
        )?)
    };
    let mut actions = BTreeMap::new();
    for &key in &keys {
        let name = if key.1 {
            key.0
        } else {
            *prepared
                .as_ref()
                .and_then(|prepared| prepared.actions.get(&key.0))
                .context("Human animation action has no private route")?
        };
        actions.insert(key, name);
    }
    shared.set_actions(actions.clone());
    let evidence = json!({"actions":actions.iter().map(|(key, name)| json!({
        "source":key.0,"player_route":key.1,"lane":key.2,"native":name})).collect::<Vec<_>>(),
        "human":prepared.as_ref().map(|prepared| &prepared.evidence)});
    let Some(prepared) = prepared else {
        return Ok((None, evidence));
    };
    Ok((Some(animation::Human::new(native, prepared)?), evidence))
}

impl Prepared {
    fn systems(&self) -> BTreeMap<String, u32> {
        self.nodes
            .iter()
            .filter(|node| {
                self.assets.iter().any(|asset| {
                    asset
                        .patches
                        .iter()
                        .any(|patch| patch.symbol == node.symbol && patch.class == 0x80806E28)
                })
            })
            .map(|node| (node.symbol.clone(), 0x80806E28))
            .collect()
    }

    /// Publish a checked preparation for the existing private ability-root allocator. This
    /// writes source payloads and a manifest only, never a package or installed game content.
    pub fn save(&self, directory: &Path) -> Result<()> {
        let systems = self.systems();
        let mut external = systems.clone();
        for cue in self.audio.iter().flat_map(|audio| &audio.cues) {
            external.insert(cue.clone(), 0x80809802);
        }
        if let Some(audio) = &self.audio {
            let symbols = audio.graph["nodes"]
                .as_array()
                .context("Ability audio nodes are absent")?
                .iter()
                .filter_map(|node| node["symbol"].as_str())
                .collect::<BTreeSet<_>>();
            ensure!(
                audio.cues.iter().all(|cue| symbols.contains(cue.as_str()))
                    && symbols.iter().all(|symbol| {
                        !self.templates.contains_key(*symbol)
                            && self.nodes.iter().all(|node| node.symbol != *symbol)
                    }),
                "Ability cue closure is incomplete or collides with other assets"
            );
        }
        dependency::validate_external(&self.assets, std::slice::from_ref(&self.root), &external)?;
        if !self.nodes.is_empty() {
            let roots = systems.keys().map(String::as_str).collect::<Vec<_>>();
            resources::validate(&self.nodes, &roots)?;
        }
        ensure!(
            self.assets
                .iter()
                .all(|asset| self.templates.contains_key(&asset.symbol)),
            "Ability asset template is absent"
        );
        ensure!(
            !directory.exists(),
            "Ability preparation output already exists"
        );
        fs::create_dir_all(directory)?;
        let mut nodes = Vec::new();
        for asset in &self.assets {
            let template = self
                .templates
                .get(&asset.symbol)
                .context("Ability asset template is absent")?;
            let file = format!("{}.bin", asset.symbol);
            fs::write(directory.join(&file), &asset.payload.0)?;
            let patches = asset
                .patches
                .iter()
                .map(|patch| json!({"offset":patch.offset,"symbol":patch.symbol}))
                .collect::<Vec<_>>();
            nodes.push(
                json!({"symbol":asset.symbol,"template":template,"file":file,"patches":patches}),
            );
        }
        for node in &self.nodes {
            let file = format!("{}.bin", node.symbol);
            fs::write(directory.join(&file), &node.payload)?;
            let patches = node
                .patches
                .iter()
                .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
                .collect::<Vec<_>>();
            nodes.push(
                json!({"symbol":node.symbol,"template":node.template,"file":file,
                "reference":node.reference,"patches":patches}),
            );
        }
        fs::write(
            directory.join("provenance.json"),
            serde_json::to_vec_pretty(&self.evidence)?,
        )?;
        let mut graph = json!({
            "attachments":{"installable":true,"roots":[self.root],"nodes":nodes},
            "hud_statuses":self.hud
        });
        if let Some(audio) = &self.audio {
            let folder = directory.join("audio-assets");
            fs::create_dir_all(&folder)?;
            for (file, payload) in &audio.files {
                fs::write(folder.join(file), payload)?;
            }
            let mut section = audio.graph.clone();
            for node in section["nodes"]
                .as_array_mut()
                .context("Ability audio nodes are absent")?
            {
                node["file"] = json!(format!(
                    "audio-assets/{}",
                    node["file"]
                        .as_str()
                        .context("Ability audio file is absent")?
                ));
            }
            section["installable"] = json!(true);
            graph["audio_assets"] = section;
        }
        if let Some(human) = &self.human {
            human.write(directory, &mut graph)?;
        }
        // The manifest is the commit marker. A partial directory has no importable root.
        fs::write(
            directory.join("asset-graph.json"),
            serde_json::to_vec_pretty(&graph)?,
        )?;
        Ok(())
    }
}

fn read(manager: &PackageManager, tag: u32, class: u32) -> Result<Payload> {
    ensure!(
        manager
            .get_entry(TagHash(tag))
            .is_some_and(|entry| entry.reference == class),
        "Native revival template {tag:08X} has another resource class"
    );
    Ok(Payload(
        manager.read_tag(TagHash(tag)).map_err(anyhow::Error::msg)?,
    ))
}

struct Provider {
    key: u32,
    offset: usize,
    condition: u16,
    metadata: Payload,
}

fn providers(
    manager: &PackageManager,
    graph: &Payload,
    tag: u32,
    old: &Payload,
    new: &Payload,
    mut relocate: impl FnMut(usize) -> Result<Option<usize>>,
) -> Result<Vec<Provider>> {
    let old_definition = usize::try_from(old.u64(old.pointer(16)? + 8)?)?;
    let maps = graph.array(88, 40, Some(0x80809C22))?;
    let descriptors = graph.array(104, 24, Some(0x80809C20))?;
    ensure!(
        maps.len() == descriptors.len(),
        "Native provider tables have different lengths"
    );
    let mut result = Vec::new();
    for binding in graph.array(72, 8, Some(0x80809C25))? {
        let key = graph.u32(binding)?;
        if key == u32::MAX {
            continue;
        }
        let span = graph.u32(binding + 4)?;
        let start = (span & 0xFFFF) as usize;
        let count = ((span >> 16) & 0x7FFF) as usize;
        ensure!(
            count > 0 && start + count <= maps.len(),
            "Native provider span exceeds its table"
        );
        for index in start..start + count {
            if graph.u32(descriptors[index])? != tag {
                continue;
            }
            let row = maps[index];
            ensure!(
                graph.u32(row + 16)? == tag && graph.u32(row + 20)? == 0x80809C50,
                "Native provider registration ownership differs"
            );
            let old_offset = usize::try_from(graph.u64(row + 24)?)?;
            let metadata_tag = old.u32(old_offset + 8)?;
            ensure!(
                metadata_tag == graph.u32(row + 32)?,
                "Native provider registration has different metadata"
            );
            let metadata = read(manager, metadata_tag, 0x80809C54)?;
            let original = links::Interface::read(
                old,
                links::Object {
                    owner: tag,
                    class: metadata.u32(8)?,
                    offset: old_offset as u64,
                },
                &metadata,
                false,
            )?;
            let Some(offset) = relocate(old_offset)? else {
                ensure!(
                    original.definition_offset == old_definition,
                    "Discarded donor provider does not belong to its replaced root"
                );
                continue;
            };
            let definition = relocate(original.definition_offset)?
                .context("Retained provider targets a replaced definition")?;
            let old_state = usize::try_from(old.u64(original.definition_offset + 8)?)?;
            let state = relocate(old_state)?
                .context("Retained provider targets replaced runtime storage")?;
            let implementation = new.u32(definition + 4)?;
            let boundary = new.pointer(24)?;
            ensure!(
                new.pointer(offset)? == definition
                    && new.u32(offset + 8)? == metadata_tag
                    && new.u32(definition)? == tag
                    && new.u32(state)? == tag
                    && new.u64(definition + 8)? == state as u64
                    && new.u64(state + 8)? == definition as u64
                    && state >= new.pointer(16)?
                    && state.checked_add(16).is_some_and(|end| end <= boundary)
                    && !original.methods.is_empty()
                    && original.methods.iter().all(|method| {
                        runtime::native_type_inherits(implementation, method.implementation_class)
                    }),
                "Native provider does not dispatch its retained reciprocal object"
            );
            result.push(Provider {
                key,
                offset,
                condition: graph.u16(row + 2)?,
                metadata,
            });
        }
    }
    ensure!(
        !result.is_empty(),
        "Native component publishes no provider contracts"
    );
    Ok(result)
}

impl Source {
    /// Discover complete self-revive property choices separately from movement imports.
    pub fn revival_choices(&self) -> Result<Vec<Choice>> {
        let mut result = Vec::new();
        for catalog in self.subclass_choices()? {
            if catalog.controller_class != 0x808025B8 {
                continue;
            }
            for node in &catalog.nodes {
                for step in &node.steps {
                    let Some((row, keys, _)) = property_choice(&step.source)? else {
                        continue;
                    };
                    if row != catalog.ability_row
                        || !controller::revival_keys(
                            self,
                            catalog.bank,
                            &keys.into_iter().collect(),
                        )?
                    {
                        continue;
                    }
                    result.push(Choice {
                        subclass: catalog.item,
                        subclass_name: catalog.name.clone(),
                        grid: catalog.grid,
                        node: node.node,
                        step: step.step,
                        name: step.name.clone(),
                        description: step.description.clone(),
                    });
                }
            }
        }
        self.unchanged()?;
        Ok(result)
    }

    /// Accept a source choice by its complete grant group, never by its displayed name.
    /// The native graph supplies the common controller, its helpers and the channel bank, and
    /// must own a single Super bank. Its presentation sequences are replaced by the source's.
    /// Every exclusive upgrade whose effect resources are translated joins the private bank.
    pub fn prepare_upgrades(
        &self,
        manager: &PackageManager,
        native: &mut Reader,
        choice: &Choice,
        native_graph: u32,
        symbol: &str,
    ) -> Result<Prepared> {
        dependency::symbol(symbol)?;
        let anchor = self.anchor(choice)?;
        let catalog = &anchor.catalog;
        let mut graph = retained::NativeGraph::read(manager, native_graph)?;
        let tags = (graph.controller_tag, graph.bank_tag);
        let names = retained::Names::new(symbol);
        ensure!(
            self.reference(catalog.controller)? == 0x80800861,
            "Source revival controller has another owner resource class"
        );
        let source_controller = self.read(catalog.controller)?;
        let mut controller = controller::native::prepare_source(
            manager,
            tags.0,
            &names.controller,
            self,
            &source_controller,
        )?;
        controller.source_energy(catalog.controller, &source_controller)?;
        let glyph_keys = sundial::package_authoring::ability_hud::glyph_keys(manager)
            .map_err(anyhow::Error::msg)?;
        controller.source_glyph(&source_controller, &glyph_keys)?;
        let activation = source_controller.u32(source_controller.pointer(24)? + 0x708)?;
        let effects = self.effects(manager, native, &anchor, activation, &graph, symbol)?;
        let (upgrades, passives) = upgrades(&anchor, &effects.resources, choice)?;
        let bank_keys = upgrades
            .iter()
            .filter(|upgrade| upgrade.available)
            .chain(&passives)
            .flat_map(|upgrade| upgrade.keys.iter().copied())
            .collect::<BTreeSet<_>>();
        let bank_template = graph
            .originals
            .get(&tags.1)
            .context("Native bank is outside its component graph")?;
        let bank_allocation_tag = bank_template.u32(0x44)?;
        let bank = controller::prepare_selected_bank(
            self,
            catalog.bank,
            &bank_keys,
            bank_template,
            &read(manager, bank_allocation_tag, 0x80809BBB)?,
            tags.1,
            &names.bank,
            &effects.resources,
        )?;
        let presentation = source::prepare(
            self,
            native,
            catalog.graph,
            catalog.controller,
            &graph.connections,
            &graph.originals,
            &effects.shared,
            symbol,
        )?;
        let channel_tag = retained::retain(&mut graph.connections, &presentation, tags)?;
        let remap = retained::Remap::new(
            &graph.originals[&tags.0],
            bank_template,
            &controller.offsets,
            bank.definition,
            tags,
        )?;
        remap.connections(&mut graph.connections)?;
        retained::controller_inputs(
            (&controller.payload, &mut controller.patches),
            &mut graph.connections,
            &presentation,
            tags.0,
            effects.resources.get(&activation),
        )?;
        let mut owners = retained::owners(manager, &graph.originals, &presentation, &remap, tags)?;
        owners.insert(tags.0, controller.payload.clone());
        owners.insert(tags.1, bank.payload.clone());
        let sequences = presentation
            .sequences
            .iter()
            .map(|sequence| (sequence.id, sequence))
            .collect::<BTreeMap<_, _>>();
        retained::add_sequences(manager, &sequences, &mut owners, &mut graph.connections)?;
        let context = retained::Retained {
            graph: &graph.graph,
            connections: &graph.connections,
            originals: &graph.originals,
            owners: &owners,
            sequences: &sequences,
            tags,
            channel_tag,
        };
        let providers = retained::Providers::read(
            manager,
            &context,
            &controller,
            &bank,
            (bank_template, remap.old_bank_definition),
        )?;
        let channels = presentation.channel_bank.as_ref();
        let emitted = retained::emit(
            manager,
            &context,
            &providers,
            &controller,
            (
                bank_allocation_tag,
                channels.map(|channels| channels.allocation_template),
            ),
            &names,
            native_graph,
        )?;
        let symbols = retained::symbols(&context, &names, native_graph);
        let mut pending = retained::owner_assets(&context, &symbols, &controller, &bank, &names)?;
        pending.extend(retained::graph_assets(
            emitted,
            &controller,
            &bank,
            (&context, bank_allocation_tag, channels),
            &names,
            native_graph,
        )?);
        let mut dependencies = BTreeMap::new();
        effect::merge(&mut dependencies, presentation.dependencies)?;
        let (hud, effect_evidence) =
            merge_effects(&mut dependencies, effects.graphs, &mut pending)?;
        let feedback_roots = merge_feedback(&mut dependencies, effects.feedback, &pending)?;
        let audio = effects.shared.audio.map(|(prepared, cues)| {
            let (graph, files) = prepared.into_parts();
            Audio { graph, files, cues }
        });
        let external = external(&effects.shared.systems, audio.as_ref());
        let (assets, templates) =
            assets(manager, native, pending, &symbols, dependencies, &external)?;
        let closure = dependency::validate_external(&assets, &[symbol.into()], &external)?;
        let nodes = particle_nodes(&assets, effects.shared.nodes, &templates)?;
        self.unchanged()?;
        let evidence = json!({
            "source":"d1_roi","choice":choice,"source_variant":anchor.variant,"native_graph":native_graph,
            "ability":anchor.ability,"foundation_source":anchor.foundation_source,
            "controller":controller.evidence,"bank":bank.evidence,"closure":closure,
            "upgrades":upgrades,"passives":passives,
            "presentation":presentation.evidence,"shared_resources":effects.shared.evidence,
            "activation_entity":{"source":activation,"symbol":effects.resources.get(&activation)},
            "effects":effect_evidence,"refused_effects":effects.refused,
            "feedback":feedback_roots,
            "animation":effects.actions,
            "gameplay_verified":false,
            "limits":["Each exclusive upgrade keeps its own complete property groups. Upgrades whose effect graphs are not translated are excluded.",
                "The foundation identifies the equipped Super. Its additional source grants are recorded, not certified by this preparation.",
                "Source charge count, activation cost, active drain and qualified flags replace the presentation donor's settings.",
                "Source presentation sequences replace the native ones. Their untranslated event kinds keep timing as native delays.",
                "The remaining native common ability values, query helper and channel bank are retained."]
        });
        Ok(Prepared {
            hud,
            root: symbol.into(),
            ability: anchor.ability,
            keys: anchor.keys,
            upgrades,
            passives,
            assets,
            nodes,
            audio,
            templates,
            human: effects.human,
            evidence,
        })
    }

    /// The revival choice a preparation anchors on, checked against the source as it is now.
    fn anchor(&self, choice: &Choice) -> Result<Anchor> {
        let catalog = self
            .subclass_choices()?
            .into_iter()
            .find(|catalog| catalog.item == choice.subclass && catalog.grid == choice.grid)
            .context("Source revival subclass changed")?;
        ensure!(
            catalog.controller_class == 0x808025B8,
            "Source subclass has another revival controller family"
        );
        let step = catalog
            .nodes
            .iter()
            .find(|node| node.node == choice.node)
            .and_then(|node| node.steps.iter().find(|step| step.step == choice.step))
            .context("Source revival choice changed")?;
        ensure!(
            catalog.name == choice.subclass_name
                && step.name == choice.name
                && step.description == choice.description,
            "Source revival choice metadata changed"
        );
        let (row, keys, variant) = property_choice(&step.source)?
            .context("Source revival choice has unsupported companions")?;
        let selected = keys.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            row == catalog.ability_row && controller::revival_keys(self, catalog.bank, &selected)?,
            "Source choice is not a complete self-revive flag and energy-drain group"
        );
        let selection = self.radiance_selection(choice)?;
        let (node, foundation) = foundation(&catalog)?;
        let ability = Choice {
            subclass: catalog.item,
            subclass_name: catalog.name.clone(),
            grid: catalog.grid,
            node: node.node,
            step: foundation.step,
            name: foundation.name.trim().to_owned(),
            description: foundation.description.clone(),
        };
        let foundation_source = foundation.source.clone();
        Ok(Anchor {
            catalog,
            keys,
            variant,
            selection,
            ability,
            foundation_source,
        })
    }

    /// The definition's activation entity and every upgrade's resources are effect graphs. Their
    /// sequences share particle systems and cues with the Super graph's, so all of them are
    /// prepared together before any owner is written. A graph whose translation is refused is
    /// reported.
    fn effects(
        &self,
        manager: &PackageManager,
        native: &mut Reader,
        anchor: &Anchor,
        activation: u32,
        graph: &retained::NativeGraph,
        symbol: &str,
    ) -> Result<Effects> {
        let mut tags = BTreeSet::new();
        if !matches!(activation, 0 | u32::MAX) {
            tags.insert(activation);
        }
        for upgrade in &anchor.selection.upgrades {
            tags.extend(upgrade.resources.iter().copied());
        }
        // The Super graph's own sequences come first, so its sound survives a refused buff cue.
        let mut groups = vec![effect::sequences(self, anchor.catalog.graph)?];
        for &tag in &tags {
            groups.push(effect::sequences(self, tag)?);
        }
        let mut shared = effect::Shared::prepare_translating(
            self,
            native,
            &groups,
            symbol,
            &[(0x808025C1, 13)],
        )?;
        let feedback = self.feedback(&mut shared, symbol)?;
        let (human, actions) = human_actions(self, native, &mut shared, symbol)?;
        let envelopes = graph
            .connections
            .components
            .iter()
            .filter(|tag| effect::is_sequence(&graph.originals[tag]).unwrap_or(false))
            .map(|tag| graph.originals[tag].clone())
            .collect::<Vec<_>>();
        let mut graphs = Vec::new();
        let mut resources = BTreeMap::new();
        let mut refused = Vec::new();
        for (index, &tag) in tags.iter().enumerate() {
            let effect_symbol = format!("{symbol}-effect-{tag:08X}");
            let base = 0x7D00_0000u32 + u32::try_from(index)? * 0x100;
            match effect::translate(self, native, &shared, &envelopes, tag, &effect_symbol, base) {
                Ok(translated) => {
                    for &id in &translated.components {
                        ensure!(
                            manager.get_entry(TagHash(id)).is_none(),
                            "Temporary effect identity collides with a native resource"
                        );
                    }
                    resources.insert(tag, effect_symbol);
                    graphs.push(translated);
                }
                Err(error) => {
                    refused.push(json!({"graph":tag,"error":format!("{error:#}")}));
                }
            }
        }
        Ok(Effects {
            shared,
            feedback,
            human,
            actions,
            graphs,
            resources,
            refused,
        })
    }

    /// Feedback events name complete feedback resources, each a private closure. Created graphs
    /// and query profiles belong to event kinds this path keeps as delays.
    fn feedback(
        &self,
        shared: &mut effect::Shared,
        symbol: &str,
    ) -> Result<Vec<Vec<dependency::Asset>>> {
        let (graphs, profiles, feedback) = shared.named_resources();
        ensure!(
            graphs.is_empty() && profiles.is_empty(),
            "Radiance sequences name created graphs or query profiles"
        );
        let mut named = effect::Resources::default();
        let mut closures = Vec::new();
        for tag in feedback {
            let closure =
                crate::d1_roi::native::damage_profile::feedback::translate(self, tag, symbol)?;
            named.feedback.insert(tag, closure[0].symbol.clone());
            closures.push(closure);
        }
        shared.set_resources(named);
        Ok(closures)
    }
}

/// The revival choice a preparation anchors on: its subclass, its keys and source variant, the
/// subclass's exclusive upgrade selection, and the one foundation that equips the Super.
struct Anchor {
    catalog: super::SubclassCatalog,
    keys: Vec<u32>,
    variant: Value,
    selection: super::radiance::Selection,
    ability: Choice,
    foundation_source: Value,
}

/// The effect graphs the Super's activation and its upgrades name, translated over the resources
/// they share with the Super graph's sequences.
struct Effects {
    shared: effect::Shared,
    /// Feedback closures. Each joins once a written event names its root.
    feedback: Vec<Vec<dependency::Asset>>,
    human: Option<animation::Human>,
    /// How each source human action resolved.
    actions: Value,
    graphs: Vec<effect::Graph>,
    /// The private symbol of each source effect graph with a translation.
    resources: BTreeMap<u32, String>,
    refused: Vec<Value>,
}

/// The one foundation step that equips the Super's source row, with its node.
fn foundation(
    catalog: &super::SubclassCatalog,
) -> Result<(&super::SubclassNode, &super::SubclassStep)> {
    let mut foundations = Vec::new();
    for node in &catalog.nodes {
        for step in node
            .steps
            .iter()
            .filter(|step| step.foundation == Some(true))
        {
            let variants = step.source["variants"]
                .as_array()
                .context("Source Super foundation variants are absent")?;
            let mut equips_super = false;
            for variant in variants {
                if grants(variant)?
                    .iter()
                    .any(|record| record.u8(11).ok() == Some(catalog.ability_row))
                {
                    equips_super = true;
                }
            }
            if equips_super {
                foundations.push((node, step));
            }
        }
    }
    let [foundation] = foundations.as_slice() else {
        anyhow::bail!("Self-revive has no unique equipped Super foundation");
    };
    Ok(*foundation)
}

/// The exclusive upgrades and the passives. An upgrade joins the bank only when every effect
/// graph it selects has a private translation, and the revival choice must be one that does.
fn upgrades(
    anchor: &Anchor,
    resources: &BTreeMap<u32, String>,
    choice: &Choice,
) -> Result<(Vec<Upgrade>, Vec<Upgrade>)> {
    let upgrades = anchor
        .selection
        .upgrades
        .iter()
        .map(|upgrade| Upgrade {
            choice: upgrade.choice.clone(),
            row: upgrade.row,
            keys: upgrade.keys.clone(),
            revival: upgrade.revival,
            available: upgrade
                .resources
                .iter()
                .all(|resource| resources.contains_key(resource)),
            resources: upgrade.resources.clone(),
        })
        .collect::<Vec<_>>();
    ensure!(
        upgrades
            .iter()
            .any(|upgrade| upgrade.revival && upgrade.available && upgrade.choice == *choice),
        "Radiance revival upgrade is not available to the private bank"
    );
    let passives = passives(&anchor.catalog, &upgrades)?;
    Ok((upgrades, passives))
}

/// Other grid choices that grant only the Super's own properties join the same bank. The caller
/// decides which path selects each one.
fn passives(catalog: &super::SubclassCatalog, upgrades: &[Upgrade]) -> Result<Vec<Upgrade>> {
    let mut passives = Vec::new();
    for node in &catalog.nodes {
        for step in &node.steps {
            let Some((row, keys, _)) = property_choice(&step.source)? else {
                continue;
            };
            let passive = Choice {
                subclass: catalog.item,
                subclass_name: catalog.name.clone(),
                grid: catalog.grid,
                node: node.node,
                step: step.step,
                name: step.name.clone(),
                description: step.description.clone(),
            };
            if row != catalog.ability_row
                || upgrades.iter().any(|upgrade| upgrade.choice == passive)
            {
                continue;
            }
            passives.push(Upgrade {
                choice: passive,
                row: node.row,
                keys,
                revival: false,
                available: true,
                resources: Vec::new(),
            });
        }
    }
    Ok(passives)
}

/// Merges each translated effect graph's dependencies and adds its owners, returning their HUD
/// statuses and evidence.
fn merge_effects(
    dependencies: &mut BTreeMap<String, dependency::Asset>,
    graphs: Vec<effect::Graph>,
    pending: &mut Vec<retained::PendingAsset>,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut hud = Vec::new();
    let mut evidence = Vec::new();
    for graph in graphs {
        hud.extend(graph.hud);
        effect::merge(dependencies, graph.dependencies)?;
        for entry in graph.pending {
            pending.push((entry.symbol, entry.template, entry.payload, entry.patches));
        }
        evidence.push(json!({"symbol":graph.symbol,"translation":graph.evidence}));
    }
    Ok((hud, evidence))
}

/// A feedback closure joins only when a written event names its root. An effect graph whose
/// translation was refused leaves its feedback unreferenced. Returns the roots that joined.
fn merge_feedback(
    dependencies: &mut BTreeMap<String, dependency::Asset>,
    closures: Vec<Vec<dependency::Asset>>,
    pending: &[retained::PendingAsset],
) -> Result<Vec<String>> {
    let named = pending
        .iter()
        .flat_map(|(_, _, _, patches)| patches.iter().map(|(_, symbol)| symbol.as_str()))
        .collect::<BTreeSet<_>>();
    let mut roots = Vec::new();
    for closure in closures {
        if named.contains(closure[0].symbol.as_str()) {
            roots.push(closure[0].symbol.clone());
            effect::merge(dependencies, closure)?;
        }
    }
    Ok(roots)
}

/// The particle systems and cues whose closures are checked apart from the typed assets, with
/// their classes.
fn external(systems: &BTreeSet<String>, audio: Option<&Audio>) -> BTreeMap<String, u32> {
    systems
        .iter()
        .map(|system| (system.clone(), 0x80806E28))
        .chain(
            audio
                .into_iter()
                .flat_map(|audio| &audio.cues)
                .map(|cue| (cue.clone(), 0x80809802)),
        )
        .collect()
}

/// Every private asset: each sequence dependency at its class's native template, and each pending
/// owner with its references as patches. Returns them with their templates.
fn assets(
    manager: &PackageManager,
    native: &Reader,
    pending: Vec<retained::PendingAsset>,
    symbols: &BTreeMap<u32, String>,
    dependencies: BTreeMap<String, dependency::Asset>,
    external: &BTreeMap<String, u32>,
) -> Result<(Vec<dependency::Asset>, BTreeMap<String, u32>)> {
    let mut classes = pending
        .iter()
        .map(|(symbol, template, _, _)| {
            Ok((
                symbol.clone(),
                manager
                    .get_entry(TagHash(*template))
                    .context("Private ability template disappeared")?
                    .reference,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (system, class) in external {
        ensure!(
            classes.insert(system.clone(), *class).is_none(),
            "Particle system or cue symbol collides with an ability asset"
        );
    }
    let mut templates = BTreeMap::new();
    let mut assets = Vec::new();
    for asset in dependencies.into_values() {
        let template = native
            .classes(asset.class)
            .into_iter()
            .min()
            .context("Native sequence dependency template is absent")?;
        ensure!(
            classes.insert(asset.symbol.clone(), asset.class).is_none(),
            "Sequence resource collides with an ability asset"
        );
        templates.insert(asset.symbol.clone(), template);
        assets.push(asset);
    }
    for entry in pending {
        templates.insert(entry.0.clone(), entry.1);
        assets.push(private_asset(manager, entry, symbols, &classes)?);
    }
    Ok((assets, templates))
}

/// A pending owner as a typed asset. Every declared reference to a private component and every
/// explicit reference becomes an unresolved patch of its target's class.
fn private_asset(
    manager: &PackageManager,
    (symbol, template, mut payload, explicit): retained::PendingAsset,
    symbols: &BTreeMap<u32, String>,
    classes: &BTreeMap<String, u32>,
) -> Result<dependency::Asset> {
    let mut patches = BTreeMap::new();
    for (at, tag) in
        declared_payload_references(manager, template, &payload.0).map_err(anyhow::Error::msg)?
    {
        if let Some(symbol) = symbols.get(&tag) {
            patches.insert(at, symbol.clone());
        }
    }
    for (at, symbol) in explicit {
        ensure!(
            patches
                .insert(at, symbol.clone())
                .is_none_or(|old| old == symbol),
            "Conflicting private ability reference"
        );
    }
    let patches = patches
        .into_iter()
        .map(|(offset, symbol)| {
            put(&mut payload.0, offset, &u32::MAX.to_le_bytes())?;
            Ok(dependency::Patch {
                offset,
                class: *classes
                    .get(&symbol)
                    .context("Private ability dependency is absent")?,
                symbol,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(dependency::Asset {
        class: classes[&symbol],
        symbol,
        payload,
        patches,
    })
}

/// The particle resources that emitted owners reach. An effect graph whose translation was
/// refused leaves its prepared systems unreferenced.
fn particle_nodes(
    assets: &[dependency::Asset],
    nodes: Vec<Node>,
    templates: &BTreeMap<String, u32>,
) -> Result<Vec<Node>> {
    let referenced = assets
        .iter()
        .flat_map(|asset| &asset.patches)
        .filter(|patch| patch.class == 0x80806E28)
        .map(|patch| patch.symbol.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let nodes = resources::closure(nodes, &referenced);
    ensure!(
        nodes
            .iter()
            .all(|node| !templates.contains_key(&node.symbol)),
        "Particle resource symbols collide with ability assets"
    );
    Ok(nodes)
}
