//! A bounded source self-revive choice on retained native controller and presentation contracts.
//! This prepares a private graph. Input authority and completed resurrection require gameplay.
use super::{Choice, Source, controller, dependency, grants, property_choice};
use crate::{
    presentation::put,
    tiger::{
        entity::{compiled, links},
        payload::Payload,
    },
};
use anyhow::{Context, Result, ensure};
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
mod presentation;

pub struct Prepared {
    pub root: String,
    /// The equipped foundation and the selected modifier are different source choices.
    pub ability: Choice,
    pub keys: Vec<u32>,
    pub assets: Vec<dependency::Asset>,
    pub templates: BTreeMap<String, u32>,
    pub evidence: Value,
}

impl Prepared {
    /// Publish a checked preparation for the existing private ability-root allocator. This
    /// writes source payloads and a manifest only, never a package or installed game content.
    pub fn save(&self, directory: &Path) -> Result<()> {
        dependency::validate(&self.assets, std::slice::from_ref(&self.root))?;
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
        fs::write(
            directory.join("provenance.json"),
            serde_json::to_vec_pretty(&self.evidence)?,
        )?;
        // The manifest is the commit marker. A partial directory has no importable root.
        fs::write(
            directory.join("asset-graph.json"),
            serde_json::to_vec_pretty(&json!({
                "attachments":{"installable":true,"roots":[self.root],"nodes":nodes}
            }))?,
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
    /// The caller explicitly chooses native presentation. The native graph must satisfy the
    /// direct common-controller contract and own a single Super bank.
    #[allow(clippy::cognitive_complexity)]
    pub fn prepare_revival(
        &self,
        manager: &PackageManager,
        choice: &Choice,
        native_graph: u32,
        symbol: &str,
    ) -> Result<Prepared> {
        dependency::symbol(symbol)?;
        let catalogs = self.subclass_choices()?;
        let catalog = catalogs
            .iter()
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
        let mut foundations = Vec::new();
        for node in &catalog.nodes {
            for step in &node.steps {
                if step.foundation != Some(true) {
                    continue;
                }
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
        let [(foundation_node, foundation)] = foundations.as_slice() else {
            anyhow::bail!("Self-revive has no unique equipped Super foundation");
        };
        let ability = Choice {
            subclass: catalog.item,
            subclass_name: catalog.name.clone(),
            grid: catalog.grid,
            node: foundation_node.node,
            step: foundation.step,
            name: foundation.name.trim().to_owned(),
            description: foundation.description.clone(),
        };
        let graph = read(manager, native_graph, 0x80809C0F)?;
        let mut connections = links::Graph::read(&graph, false)?;
        let bank_tag = ability_modifier::entity_bank(&graph.0)
            .map_err(anyhow::Error::msg)?
            .context("Native presentation has no ability bank")?;
        ensure!(
            ability_modifier::bank_slot(bank_tag)
                == Some(sundial::package_authoring::ability_bank::AbilityTarget::Super),
            "Self-revive requires a native Super presentation graph"
        );
        let mut originals = BTreeMap::new();
        let mut controllers = Vec::new();
        for &tag in &connections.components {
            let owner = read(manager, tag, 0x80809C36)?;
            let class = owner.u32(owner.pointer(16)? - 4)?;
            if runtime::native_type_inherits(class, 0x80803BB7) {
                controllers.push(tag);
            }
            originals.insert(tag, owner);
        }
        let [controller_tag] = controllers.as_slice() else {
            anyhow::bail!("Native presentation needs one controller");
        };
        let controller_tag = *controller_tag;
        let controller_symbol = format!("{symbol}-controller");
        let bank_symbol = format!("{symbol}-bank");
        let controller = controller::native::prepare(manager, controller_tag, &controller_symbol)?;
        let bank_template = originals
            .get(&bank_tag)
            .context("Native bank is outside its component graph")?;
        let bank_allocation_tag = bank_template.u32(0x44)?;
        let bank = controller::prepare_selected_bank(
            self,
            catalog.bank,
            &selected,
            bank_template,
            &read(manager, bank_allocation_tag, 0x80809BBB)?,
            bank_tag,
            &bank_symbol,
            &BTreeMap::new(),
        )?;
        let old_bank_definition =
            usize::try_from(bank_template.u64(bank_template.pointer(16)? + 8)?)?;
        let original_controller = &originals[&controller_tag];
        let old_runtime = original_controller.pointer(16)?;
        let old_definition = original_controller.pointer(24)?;
        let old_classes = (
            original_controller.u32(old_runtime - 4)?,
            original_controller.u32(old_definition - 4)?,
        );
        let remap = |mut object: links::Object| -> Result<links::Object> {
            if object.owner == controller_tag {
                if object.class == old_classes.0 {
                    ensure!(
                        object.offset == old_runtime as u64,
                        "Native concrete controller reference is not its root"
                    );
                    object.class = 0x80804176;
                }
                if object.class == old_classes.1 {
                    ensure!(
                        object.offset == old_definition as u64,
                        "Native concrete definition reference is not its root"
                    );
                    object.class = 0x80804177;
                }
                object.offset = u64::try_from(
                    controller
                        .offsets
                        .retained(usize::try_from(object.offset)?)?,
                )?;
            } else if object.owner == bank_tag {
                let delta = usize::try_from(object.offset)?
                    .checked_sub(old_bank_definition)
                    .context("Bank endpoint precedes its definition")?;
                ensure!(
                    matches!(delta, 0x48 | 0x60),
                    "Native graph references a donor-specific bank interface"
                );
                object.offset = u64::try_from(bank.definition + delta)?;
            }
            Ok(object)
        };
        for connection in connections
            .connections
            .iter_mut()
            .chain(&mut connections.named_connections)
        {
            for endpoint in [&mut connection.consumer, &mut connection.provider] {
                if let Some(object) = endpoint.object {
                    endpoint.object = Some(remap(object)?);
                }
            }
        }
        for &(common, specialized) in &controller.script_aliases {
            let matches = connections
                .connections
                .iter()
                .filter(|row| {
                    row.consumer.object.is_some_and(|object| {
                        object.owner == controller_tag
                            && object.class == 0x808092F4
                            && object.offset == common as u64
                    })
                })
                .collect::<Vec<_>>();
            let [row] = matches.as_slice() else {
                anyhow::bail!("Native common sequence has no unique provider connection");
            };
            let mut alias = (**row).clone();
            alias
                .consumer
                .object
                .as_mut()
                .context("Native sequence consumer is absent")?
                .offset = specialized as u64;
            connections.connections.push(alias);
        }
        let mut owners = originals.clone();
        let mut presentation = BTreeMap::new();
        for (&tag, owner) in &mut owners {
            if owner.u32(owner.pointer(16)? - 4)? == 0x808084D7 {
                presentation.insert(format!("{tag:08X}"), presentation::retain(owner)?);
            }
        }
        // Component-local references can address another component directly. Graph connection
        // rows are not the only places an embedded controller address can appear.
        for (&tag, owner) in &mut owners {
            if tag == controller_tag || tag == bank_tag {
                continue;
            }
            for (at, target) in declared_payload_object_references(manager, tag, &owner.0)
                .map_err(anyhow::Error::msg)?
            {
                if target != controller_tag && target != bank_tag {
                    continue;
                }
                let object = remap(links::Object {
                    owner: target,
                    class: owner.u32(at + 4)?,
                    offset: owner.u64(at + 8)?,
                })?;
                put(&mut owner.0, at + 4, &object.class.to_le_bytes())?;
                put(&mut owner.0, at + 8, &object.offset.to_le_bytes())?;
            }
        }
        owners.insert(controller_tag, controller.payload.clone());
        owners.insert(bank_tag, bank.payload.clone());
        let mut controller_providers = providers(
            manager,
            &graph,
            controller_tag,
            &originals[&controller_tag],
            &controller.payload,
            |offset| controller.offsets.provider(offset),
        )?;
        let binding = controller_providers
            .iter_mut()
            .filter(|provider| provider.offset == controller.provider)
            .collect::<Vec<_>>();
        ensure!(
            binding.len() == 1,
            "Native slot-binding provider is missing or ambiguous"
        );
        for provider in binding {
            provider.metadata = controller.metadata.clone();
        }
        let bank_providers = providers(
            manager,
            &graph,
            bank_tag,
            bank_template,
            &bank.payload,
            |offset| {
                if offset == bank_template.pointer(16)? {
                    return Ok(Some(bank.runtime));
                }
                let delta = offset
                    .checked_sub(old_bank_definition)
                    .context("Native bank provider precedes its definition")?;
                ensure!(
                    delta.checked_add(24).is_some_and(|end| end <= 0x88),
                    "Native bank provider leaves its common definition"
                );
                Ok(Some(bank.definition + delta))
            },
        )?;
        let authored = [
            (controller_tag, &controller_providers),
            (bank_tag, &bank_providers),
        ]
        .into_iter()
        .map(|(tag, rows)| {
            (
                tag,
                rows.iter()
                    .map(|row| compiled::authored::Provider {
                        key: row.key,
                        offset: row.offset,
                        metadata: &row.metadata,
                        condition: row.condition,
                    })
                    .collect(),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let rows = connections.native_rows(
            &connections.components,
            &owners,
            Ok,
            |_, channel| Ok(channel),
            |_, selector| Ok(selector),
        )?;
        let components = connections
            .components
            .iter()
            .map(|&tag| compiled::Component {
                owner: tag,
                payload: &owners[&tag],
                template: &originals[&tag],
                entity: &graph,
            })
            .collect::<Vec<_>>();
        let emitted = compiled::emit_authored(&graph, &components, &rows, &authored)?;
        let mut graph_patches = vec![(0x88, format!("{symbol}-replication"))];
        let mut binding_count = 0;
        for row in emitted.array(88, 40, Some(0x80809C22))? {
            if emitted.u32(row + 16)? == controller_tag
                && emitted.u64(row + 24)? == controller.provider as u64
            {
                ensure!(
                    emitted.u32(row + 32)? == controller.metadata_template,
                    "Authored slot-binding metadata identity differs"
                );
                graph_patches.push((row + 32, format!("{controller_symbol}-interface")));
                binding_count += 1;
            }
        }
        ensure!(
            binding_count == 1,
            "Authored slot-binding registration is missing or ambiguous"
        );
        let replication_template = graph.u32(0x88)?;
        read(manager, replication_template, 0x80809BB6)?;
        let allocations = BTreeMap::from([
            (controller_tag, original_controller.u32(0x44)?),
            (bank_tag, bank_allocation_tag),
        ]);
        let replication =
            compiled::replication::emit_authored(native_graph, &components, &allocations)?;
        let mut replication_patches = vec![(8, symbol.to_owned())];
        for (&tag, row) in
            connections
                .components
                .iter()
                .zip(replication.array(0x30, 32, Some(0x80809BB8))?)
        {
            if tag == controller_tag {
                replication_patches.push((row + 4, format!("{controller_symbol}-allocation")));
            }
            if tag == bank_tag {
                replication_patches.push((row + 4, format!("{bank_symbol}-allocation")));
            }
        }
        let symbols = connections
            .components
            .iter()
            .map(|&tag| {
                (
                    tag,
                    if tag == controller_tag {
                        controller_symbol.clone()
                    } else if tag == bank_tag {
                        bank_symbol.clone()
                    } else {
                        format!("{symbol}-{tag:08X}")
                    },
                )
            })
            .chain([(native_graph, symbol.to_owned())])
            .collect::<BTreeMap<_, _>>();
        let mut pending = Vec::new();
        for (&tag, payload) in &owners {
            let mut patches = if tag == controller_tag {
                controller.patches.clone()
            } else if tag == bank_tag {
                bank.patches.clone()
            } else {
                Vec::new()
            };
            if tag == bank_tag {
                patches.push((0x44, format!("{bank_symbol}-allocation")));
            }
            pending.push((symbols[&tag].clone(), tag, payload.clone(), patches));
        }
        pending.push((symbol.into(), native_graph, emitted, graph_patches));
        pending.push((
            format!("{controller_symbol}-interface"),
            controller.metadata_template,
            controller.metadata,
            Vec::new(),
        ));
        pending.push((
            format!("{symbol}-replication"),
            replication_template,
            replication,
            replication_patches,
        ));
        pending.push((
            format!("{controller_symbol}-allocation"),
            originals[&controller_tag].u32(0x44)?,
            controller.allocation,
            Vec::new(),
        ));
        pending.push((
            format!("{bank_symbol}-allocation"),
            bank_allocation_tag,
            bank.allocation,
            Vec::new(),
        ));
        let mut templates = BTreeMap::new();
        let classes = pending
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
        let mut assets = Vec::new();
        for (symbol, template, mut payload, explicit) in pending {
            let mut patches = BTreeMap::new();
            for (at, tag) in declared_payload_references(manager, template, &payload.0)
                .map_err(anyhow::Error::msg)?
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
            templates.insert(symbol.clone(), template);
            assets.push(dependency::Asset {
                class: classes[&symbol],
                symbol,
                payload,
                patches,
            });
        }
        let closure = dependency::validate(&assets, &[symbol.into()])?;
        self.unchanged()?;
        Ok(Prepared {
            root: symbol.into(),
            ability: ability.clone(),
            keys,
            assets,
            templates,
            evidence: json!({
                "source":"d1_roi","choice":choice,"source_variant":variant,"native_graph":native_graph,
                "ability":ability,"foundation_source":foundation.source,
                "controller":controller.evidence,"bank":bank.evidence,"closure":closure,
                "presentation":presentation,"gameplay_verified":false,
                "limits":["Only the selected source self-revive property group is imported.",
                    "The foundation identifies the equipped Super. Its additional source grants are recorded, not certified by this preparation.",
                    "The remaining native common ability values and presentation are retained."]
            }),
        })
    }
}
