//! Source projectile controllers assembled for Shadowkeep through checked native
//! implementation envelopes. Compatibility substitutions are part of the output receipt.
pub(crate) use crate::tiger::projectile as assets;
pub(crate) mod compatibility;
mod resources;

use crate::d2_mot::{
    entity::{
        category, compiled, context,
        links::{Graph, Object},
        sequence, shared,
    },
    native::effects::controller::{self, Relocation, damage, movement, network, parameters},
    payload::Payload,
    reader::Reader,
};
use anyhow::{Context, Result, ensure};
use assets::{Assets, metadata, retag};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const ENTITY: u32 = 0x80BBC7DE;
const MOVEMENT: u32 = 0x815282E7;
const PARAMETER: u32 = 0x815282DF;
const SEQUENCE: u32 = 0x80F2E50A;

pub struct Profile<'a> {
    pub namespace: &'a str,
    pub element: f32,
    /// Compensation for a native weapon that supplies a lower launch input.
    pub speed_boost: f32,
}

fn put(payload: &mut Payload, at: usize, bytes: &[u8]) -> Result<()> {
    payload
        .0
        .get_mut(
            at..at
                .checked_add(bytes.len())
                .context("projectile write overflow")?,
        )
        .context("projectile write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

struct Component {
    source: u32,
    class: u32,
    template_tag: u32,
    template: Payload,
    entity: Payload,
    owner: u32,
    allocation: u32,
    payload: Option<Payload>,
}

fn template(class: u32) -> Result<(u32, u32)> {
    Ok(match class {
        0x80802AA1 => (MOVEMENT, 0x80BBB972),
        0x80803F64 => (0x80C70C0E, 0x80B3A568),
        0x808032AA => (0x80BFDC7A, 0x80B59A93),
        0x80808EDD => (0x80C709E1, 0x80B58D0E),
        0x80802960 => (0x80EF30CF, ENTITY),
        0x80808179 => (SEQUENCE, 0x80BBB972),
        0x80809775 => (0x815282E5, 0x80BBB972),
        0x80808757 => (PARAMETER, 0x815282E1),
        0x80809597 => (0x815282E0, 0x815282E1),
        _ => anyhow::bail!("unsupported source projectile component {class:08X}"),
    })
}

fn record(objects: &mut BTreeMap<Object, Object>, relocations: Vec<Relocation>) -> Result<()> {
    for row in relocations {
        if let Some(previous) = objects.insert(row.source, row.target) {
            ensure!(
                previous == row.target,
                "conflicting projectile object relocation"
            );
        }
    }
    Ok(())
}

/// Assemble the projectile named by a source barrel. Particle symbols must be
/// produced by the normal particle converter in this same graph. No source or
/// temporary tag is used as an installed projectile address.
pub fn convert(
    source: &mut Reader,
    native: &mut Reader,
    source_tag: u32,
    particles: &Value,
    graph_directory: &Path,
    profile: &Profile<'_>,
) -> Result<Value> {
    ensure!(
        !profile.namespace.trim().is_empty(),
        "projectile namespace is empty"
    );
    let entity = source.tag(source_tag, Some(0x80809AD8))?;
    let mut graph = Graph::read(&entity, true)?;
    let mut owners = BTreeMap::new();
    let mut components = Vec::new();
    let mut assets = Assets::default();
    let root = assets.reserve(native, "projectile-root".into())?;
    let mut metadata_table = BTreeMap::new();
    for &tag in &graph.components {
        let owner = source.tag(tag, None)?;
        let class = owner.u32(owner.pointer(16)? + 4)?;
        let (template_tag, parent) = template(class)?;
        let template = native.tag(template_tag, None)?;
        metadata(source, &[&owner], &mut metadata_table)?;
        metadata(native, &[&template], &mut metadata_table)?;
        components.push(Component {
            source: tag,
            class,
            template_tag,
            template: (*template).clone(),
            entity: (*native.tag(parent, Some(0x80809C0F))?).clone(),
            owner: assets.reserve(native, format!("projectile-controller-{tag:08X}"))?,
            allocation: assets.reserve(native, format!("projectile-allocation-{tag:08X}"))?,
            payload: None,
        });
        owners.insert(tag, (*owner).clone());
    }
    graph.validate_owners(&owners)?;
    let unique = |class| -> Result<usize> {
        let indices = components
            .iter()
            .enumerate()
            .filter(|(_, c)| c.class == class)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(
            indices.len() == 1,
            "source projectile needs exactly one {class:08X} component"
        );
        Ok(indices[0])
    };
    let movement_index = unique(0x80802AA1)?;
    let parameter_index = unique(0x80808757)?;
    let steering_index = unique(0x80802960)?;
    let bank_index = unique(0x80809597)?;
    let category_index = unique(0x80809775)?;
    let component_tags = components.iter().map(|c| (c.source, c.owner)).collect();
    let native_entity = native.tag(ENTITY, Some(0x80809C0F))?;
    let native_profile = native.tag(0x8152949F, None)?;
    let resources = resources::prepare(
        source,
        native,
        &owners[&components[movement_index].source],
        &owners[&components[category_index].source],
        &native_profile,
        &mut assets,
        &component_tags,
    )?;
    let steering = compatibility::steering(
        &owners[&components[steering_index].source],
        &owners[&components[parameter_index].source],
    )?;
    let sequence_template = native.tag(SEQUENCE, None)?;
    let sound = {
        let mut cues = Vec::new();
        for at in (0..sequence_template.0.len().saturating_sub(3)).step_by(4) {
            let tag = sequence_template.u32(at)?;
            if native.reference(tag).ok() == Some(0x80809802) {
                cues.push(tag);
            }
        }
        *cues
            .first()
            .context("native sequence has no compatibility sound cue")?
    };
    let mut objects = BTreeMap::new();
    let mut channels = BTreeMap::new();
    let mut differences = resources.report;
    let bank_component = &components[bank_index];
    let bank_allocation = native.tag(bank_component.template.u32(0x44)?, None)?;
    let bank = controller::bank(
        &owners[&bank_component.source],
        &bank_component.template,
        &bank_allocation,
        bank_component.owner,
        bank_component.allocation,
    )?;
    // Retain the bank for sequence notification-to-polling validation.
    let mut bank_parts = Some((bank.owner.clone(), bank.allocation.clone()));
    record(
        &mut objects,
        bank.objects
            .iter()
            .map(|r| Relocation {
                source: r.source,
                target: r.target,
            })
            .collect(),
    )?;
    for component in &mut components {
        let original = &owners[&component.source];
        let allocation_tag = component.template.u32(0x44)?;
        let allocation = native.tag(allocation_tag, None)?;
        let (payload, allocation_payload, relocations) = match component.class {
            0x80802AA1 => {
                let mut converted = movement::emit(
                    original,
                    &component.template,
                    &allocation,
                    &resources.movement,
                    component.owner,
                    component.allocation,
                )
                .context("convert projectile movement")?;
                ensure!(
                    converted.siblings.is_empty(),
                    "converted movement still names unbound resources: {:X?}",
                    converted.siblings
                );
                let interfaces = converted.interfaces(original, &metadata_table)?;
                differences.extend(compatibility::movement(
                    &interfaces.active_unmapped,
                    &graph,
                )?);
                record(&mut objects, interfaces.objects)?;
                differences.extend(compatibility::launch(
                    &mut converted.owner,
                    profile.speed_boost,
                )?);
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80802960 => {
                let converted = damage::emit(
                    &steering.payload,
                    &component.template,
                    &allocation,
                    &sequence_template,
                    &metadata_table,
                    component.owner,
                    component.allocation,
                )
                .context("convert projectile steering")?;
                for omitted in &converted.omitted_methods {
                    for edge in graph
                        .connections
                        .iter()
                        .chain(&graph.named_connections)
                        .filter(|e| e.provider.object == Some(omitted.object))
                    {
                        ensure!(
                            edge.channel >= 2,
                            "source projectile uses a modern-only steering method"
                        );
                        channels.insert((omitted.object, edge.channel), edge.channel - 2);
                    }
                }
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80809597 => {
                let (owner, allocation) = bank_parts.take().context("multiple projectile banks")?;
                (owner, allocation, Vec::new())
            }
            0x80808EDD => {
                let converted = network::emit(
                    original,
                    &component.template,
                    &allocation,
                    &metadata_table,
                    component.owner,
                    component.allocation,
                )?;
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80808757 => {
                let converted = parameters::emit(
                    original,
                    &component.template,
                    &allocation,
                    &metadata_table,
                    component.owner,
                    component.allocation,
                )?;
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80809775 => {
                let source_allocation = source.tag(original.u32(0x84)?, None)?;
                let converted = category::emit(
                    original,
                    &source_allocation,
                    &component.template,
                    &allocation,
                    &metadata_table,
                    &graph,
                    &resources.namespace,
                    component.owner,
                    component.allocation,
                )?;
                channels.extend(converted.channels);
                (converted.owner, converted.allocation, converted.objects)
            }
            0x80808179 => {
                let (adjusted, bindings, report) = compatibility::sequence(
                    original,
                    particles,
                    native,
                    &mut assets,
                    profile.namespace,
                    profile.element,
                    sound,
                )
                .context("prepare projectile sequence")?;
                differences.extend(report);
                let converted = sequence::emit(
                    &adjusted,
                    &component.template,
                    &allocation,
                    &bindings,
                    component.owner,
                    component.allocation,
                )
                .context("convert projectile sequence")?;
                graph = converted.link(&graph, &[&bank])?;
                (
                    converted.owner.clone(),
                    converted.allocation.clone(),
                    converted
                        .objects
                        .iter()
                        .map(|r| Relocation {
                            source: r.source,
                            target: r.target,
                        })
                        .collect(),
                )
            }
            0x80803F64 | 0x808032AA => {
                let mut payload = retag(&component.template, component.owner)?;
                put(&mut payload, 0x44, &component.allocation.to_le_bytes())?;
                let mapped = if component.class == 0x80803F64 {
                    context::interfaces(original, &payload, &metadata_table)?
                } else {
                    shared::globals(original, &payload, &metadata_table)?
                };
                (payload, (*allocation).clone(), mapped)
            }
            _ => unreachable!(),
        };
        record(&mut objects, relocations)?;
        assets.push(
            component.allocation,
            allocation_tag,
            allocation_payload,
            None,
        )?;
        let reference = (native.reference(component.template_tag)? == allocation_tag)
            .then_some(component.allocation);
        assets.push(
            component.owner,
            component.template_tag,
            payload.clone(),
            reference,
        )?;
        component.payload = Some(payload);
    }
    if let (Some(conditional), Some(constant)) = (steering.conditional, steering.constant) {
        let owner_tag = assets.reserve(native, "projectile-targeting-constant".into())?;
        let allocation_tag = assets.reserve(native, "projectile-targeting-allocation".into())?;
        let template = native.tag(PARAMETER, None)?;
        let allocation_template = template.u32(0x44)?;
        let allocation = native.tag(allocation_template, None)?;
        let converted = parameters::emit(
            &constant,
            &template,
            &allocation,
            &metadata_table,
            owner_tag,
            allocation_tag,
        )?;
        let provider = converted
            .objects
            .iter()
            .find(|r| r.source.class == 0x808098D2)
            .context("targeting constant provider")?
            .target;
        ensure!(
            objects.insert(conditional, provider).is_none(),
            "targeting provider already translated"
        );
        assets.push(
            allocation_tag,
            allocation_template,
            converted.allocation,
            None,
        )?;
        assets.push(
            owner_tag,
            PARAMETER,
            converted.owner.clone(),
            (native.reference(PARAMETER)? == allocation_template).then_some(allocation_tag),
        )?;
        components.push(Component {
            source: 0,
            class: 0x80808757,
            template_tag: PARAMETER,
            template: (*template).clone(),
            entity: (*native.tag(0x815282E1, Some(0x80809C0F))?).clone(),
            owner: owner_tag,
            allocation: allocation_tag,
            payload: Some(converted.owner),
        });
        differences.push(json!({"difference":"Modern conditional targeting uses a constant one multiplier. Modern faction-specific targeting exclusions are not reproduced."}));
    }
    let native_owners = components
        .iter()
        .map(|c| {
            Ok((
                c.owner,
                c.payload
                    .clone()
                    .context("projectile component was not emitted")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let target_components = components.iter().map(|c| c.owner).collect::<Vec<_>>();
    let rows = graph
        .native_rows(
            &target_components,
            &native_owners,
            |object| {
                objects.get(&object).copied().with_context(|| {
                    format!("source projectile endpoint {object:?} has no native object")
                })
            },
            |object, channel| Ok(channels.get(&(object, channel)).copied().unwrap_or(channel)),
            |_, _| anyhow::bail!("projectile external selectors require conversion"),
        )
        .context("assemble native projectile entity")?;
    let compiled_components = components
        .iter()
        .map(|c| compiled::Component {
            owner: c.owner,
            payload: &native_owners[&c.owner],
            template: &c.template,
            entity: &c.entity,
        })
        .collect::<Vec<_>>();
    let replication_template = components
        .iter()
        .map(|component| component.entity.u32(0x88))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find(|tag| ![0, u32::MAX].contains(tag))
        .context("projectile implementations have no native replication template")?;
    native.tag(replication_template, Some(0x80809BB6))?;
    let replication = assets.reserve(native, "projectile-replication".into())?;
    assets.push(
        replication,
        replication_template,
        compiled::replication::emit(root, &compiled_components)?,
        None,
    )?;
    let mut assembled = compiled::emit(&native_entity, &compiled_components, &rows)?;
    put(&mut assembled, 0x88, &replication.to_le_bytes())?;
    let readback = Graph::read(&assembled, false)?;
    readback.validate_owners(&native_owners)?;
    let report = json!({"source":format!("{source_tag:08X}"),"source_sha256":hex::encode(Sha256::digest(&entity.0)),
        "mode":"Source controllers with native compatibility substitutions","components":components.len(),
        "connections":rows.connections.len(),"named_connections":rows.named_connections.len(),
        "differences":differences,"gameplay_verified":false});
    assets.push(root, ENTITY, assembled, None)?;
    assets.write(graph_directory, root, report)
}
