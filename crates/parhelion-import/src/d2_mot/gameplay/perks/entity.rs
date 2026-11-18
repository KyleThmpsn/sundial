//! A whole source entity as a private attachment group.
//!
//! An attachment whose entity the row forms of [`super::modifier`] refuse, one with
//! presentation, a status with inputs or creation parameters, and the entity a spawn creates,
//! are handed to the whole-entity converter of the single perk import, one entity at a time. The group is written under the
//! import's folder, which the recipe then names, and the attachment names the group through a
//! stand-in graph that the package build replaces. A controller an entity hosts is compiled
//! into a private native action inside the group, as the single perk import does.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result};
use base64::Engine;
use serde_json::{Value, json};
use sundial::package_authoring::{
    fnv1,
    sandbox_perk::program::{Action, Asset, AttachmentTarget, HudStatus, ImportedAsset, Position},
};

use super::{
    lower,
    modifier::{BUFF_ICON, Whole},
    presentation,
};
use crate::d2_mot::{native::attachment, payload::Payload, reader::Reader};

/// The graph a private attachment group stands in for while its action compiles.
const GROUP_ROOT: u32 = 0x80BC_57DD;
const EMPTY: u32 = 0x811C_9DC5;

/// What a conversion needs beside the readers: both package folders, the import's folder for
/// the groups and the plug they belong to.
#[derive(Clone, Copy)]
pub struct Groups<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    pub output: &'a Path,
    pub plug: u32,
}

/// Prefix every symbol of an attachment group, so the groups of one recipe stay apart.
fn rename(manifest: &mut Value, prefix: &str) -> Result<()> {
    let name = |value: &str| format!("{prefix}-{value}");
    for section in ["particles", "attachments"] {
        for node in manifest[section]["nodes"]
            .as_array_mut()
            .context("attachment group nodes")?
        {
            node["symbol"] = json!(name(node["symbol"].as_str().context("node symbol")?));
            if let Some(reference) = node["reference"].as_str() {
                node["reference"] = json!(name(reference));
            }
            for patch in node["patches"].as_array_mut().context("node patches")? {
                patch["symbol"] = json!(name(patch["symbol"].as_str().context("patch symbol")?));
            }
        }
    }
    for root in manifest["attachments"]["roots"]
        .as_array_mut()
        .context("attachment group roots")?
    {
        *root = json!(name(root.as_str().context("root symbol")?));
    }
    for system in manifest["particles"]["systems"]
        .as_object_mut()
        .context("attachment group particles")?
        .values_mut()
    {
        *system = json!(name(system.as_str().context("particle symbol")?));
    }
    Ok(())
}

/// Whether the entity `graph` holds a status component, which times it.
fn timed(source: &mut Reader, graph: u32) -> Result<bool> {
    let entity = source.tag(graph, Some(0x8080_9AD8))?;
    for entry in entity.array(8, 12, Some(0x8080_9ACD))? {
        let owner = source.tag(entity.u32(entry)?, Some(0x8080_9B06))?;
        let settings = owner.pointer(24)?;
        if settings >= 4 && owner.u32(settings - 4)? == 0x8080_B11F {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The name a converted status owner shows on the HUD: the report's name word, which the
/// owner holds in the record its instance names and in its definition. The package build
/// binds a HUD status at both, so an owner whose two words differ is passed over.
fn shown_name(owner: &Payload) -> Option<u32> {
    let instance = owner.pointer(16).ok()?;
    let definition = owner.pointer(24).ok()?;
    if instance < 4
        || definition < 4
        || owner.u32(instance - 4).ok()? != 0x8080_4209
        || owner.u32(definition - 4).ok()? != 0x8080_4205
    {
        return None;
    }
    let named = usize::try_from(owner.u64(instance + 8).ok()?).ok()?;
    let name = owner.u32(definition + 0x160).ok()?;
    (owner.u32(named.checked_add(0x160)?).ok()? == name).then_some(name)
}

/// HUD bindings for the group's status owners. A converted status keeps the source's HUD
/// name, a row of the source HUD table that Shadowkeep lacks, so the game would draw its
/// count with no text and no icon. Each such owner takes a HUD status of the project's own
/// with the source's text and icon. A name Shadowkeep has a row for keeps that row. A name
/// whose text or icon the source does not hold stays as it is.
fn hud_statuses(
    source: &mut Reader,
    native: &mut Reader,
    directory: &Path,
    manifest: &Value,
) -> Result<Vec<Value>> {
    let statuses = native.tag(0x80D5_FFD3, Some(0x8080_4A55))?;
    let stock = statuses
        .array(8, 112, Some(0x8080_4A59))?
        .into_iter()
        .map(|row| statuses.u32(row))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut bindings = Vec::new();
    for node in manifest["attachments"]["nodes"]
        .as_array()
        .context("attachment group nodes")?
    {
        let (Some(symbol), Some(file), true) = (
            node["symbol"].as_str(),
            node["file"].as_str(),
            node["reference"].is_null(),
        ) else {
            continue;
        };
        let owner = Payload(std::fs::read(directory.join(file))?);
        let Some(name) = shown_name(&owner).filter(|name| *name != EMPTY) else {
            continue;
        };
        if stock.contains(&name) {
            continue;
        }
        let (Some(text), Some(icon)) = (
            presentation::status_text(source, name)?,
            presentation::status_icon(source, name)?,
        ) else {
            continue;
        };
        bindings.push(json!({"owner":symbol,"name":name,"status":HudStatus {
            name: text,
            icon: Some(BUFF_ICON),
            image: Some(base64::engine::general_purpose::STANDARD.encode(icon)),
        }}));
    }
    Ok(bindings)
}

/// One converted group: where it is, its digest and the symbol of its root.
struct Group {
    directory: std::path::PathBuf,
    sha256: String,
    symbol: String,
}

/// Convert the entity `graph` into a group under the import's folder.
fn group(
    source: &mut Reader,
    native: &mut Reader,
    groups: Groups<'_>,
    graph: u32,
    parameters: BTreeMap<u32, [u8; 16]>,
) -> Result<Group> {
    let directory = groups
        .output
        .join("attachments")
        .join(format!("{:08X}-{graph:08X}", groups.plug));
    std::fs::create_dir_all(&directory)?;
    let namespace = format!("d2_mot/perk/{:08X}/{graph:08X}", groups.plug);
    // A controller the entity hosts becomes a private native action, in the envelope of the
    // first stock action, as the single perk import compiles it.
    let template = native
        .classes(0x8080_40B5)
        .into_iter()
        .min()
        .context("native action envelope is absent")?;
    let mut converted = attachment::convert_gameplay(
        source,
        native,
        &attachment::Request {
            source_tag: graph,
            source_packages: groups.modern,
            native_packages: groups.native,
            directory: &directory,
            namespace: &namespace,
        },
        &[graph],
        &BTreeMap::from([(graph, parameters)]),
        &BTreeSet::new(),
        |input| super::attached::compile_hosted(input, template, &namespace),
    )?;
    let prefix = format!("{:08X}-{graph:08X}", groups.plug);
    rename(&mut converted.manifest, &prefix)?;
    // An entity with no particle system leaves an empty particle section that nothing
    // marked, and the package build installs only marked sections.
    if converted.manifest["particles"]["nodes"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        converted.manifest["particles"]["installable"] = json!(true);
    }
    let hud = hud_statuses(source, native, &directory, &converted.manifest)?;
    if !hud.is_empty() {
        converted.manifest["hud_statuses"] = Value::Array(hud);
    }
    std::fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&converted.manifest)?,
    )?;
    let root = converted
        .roots
        .get(&graph)
        .context("converted group has no root for its entity")?;
    Ok(Group {
        sha256: attachment::fingerprint(&directory)?,
        directory,
        symbol: format!("{prefix}-{root}"),
    })
}

/// Convert the entity that the plain attachment or the spawn at `at` of `p` names.
pub fn convert(
    source: &mut Reader,
    native: &mut Reader,
    groups: Groups<'_>,
    p: &Payload,
    at: usize,
) -> Result<Whole> {
    anyhow::ensure!(at >= 4, "source effect has no class word");
    let asset = Asset {
        graph: GROUP_ROOT,
        ..Asset::default()
    };
    let (action, group) = if p.u32(at - 4)? == 0x8080_3128 {
        // Most modern spawns point at a placement record `8080312D` that is all zero: no
        // offset and no locator. That is read as the plain spawn, which has no record. No
        // twin shows the pair.
        let mut plain = Payload(p.0.clone());
        if p.u64(at + 8)? != 0 {
            let placement = p.pointer(at + 8)?;
            anyhow::ensure!(
                placement >= 4
                    && p.u32(placement - 4)? == 0x8080_312D
                    && p.bytes::<20>(placement)? == [0; 20],
                "spawn places its entity with an offset or a locator"
            );
            plain.0[at + 8..at + 16].fill(0);
        }
        // Creation parameters are constants the converter bakes into the entity, as it does
        // for an attachment. Twin spawns with parameters hold no field for them.
        let parameters = lower::attachment::parameters(p, at + 128)?;
        plain.0[at + 128..at + 144].fill(0);
        let spawn = lower::spawn::read(source, &plain, at)?;
        let group = group(source, native, groups, spawn.graph, parameters)?;
        let position = if spawn.position == 1 {
            Position::Event
        } else {
            Position::Owner
        };
        (Action::Spawn { asset, position }, group)
    } else {
        let attached = lower::attachment::read(source, p, at)?;
        anyhow::ensure!(
            attached.retained && attached.stacks_name == EMPTY,
            "attachment retention or creation stacks need translation"
        );
        let group = group(
            source,
            native,
            groups,
            attached.graph,
            attached.parameters.clone(),
        )?;
        // Twin attachments hold a lifetime key exactly when the source keeps the entity
        // unless its parent is the host and the entity holds a status: 81 of 82, against
        // none of the 172 status attachments with automatic cleanup and none of the
        // modifier entities. An independent entity always outlives its action.
        let keyed = match attached.cleanup {
            lower::attachment::Cleanup::Independent => true,
            lower::attachment::Cleanup::UnlessParentIsHost => timed(source, attached.graph)?,
            lower::attachment::Cleanup::Automatic => false,
        };
        let lifetime = if keyed {
            let key = fnv1(&format!(
                "d2_mot/perk/{:08X}/{:08X}/lifetime",
                groups.plug, attached.graph
            ));
            anyhow::ensure!(
                !matches!(key, 0 | u32::MAX | EMPTY),
                "the attachment lifetime key is reserved"
            );
            key
        } else {
            EMPTY
        };
        (
            Action::Attach {
                asset,
                mode: AttachmentTarget::from_byte(attached.target),
                keys: [lifetime, attached.removal_name],
                float_bits: [attached.removal_value.to_bits(); 4],
            },
            group,
        )
    };
    Ok(Whole {
        action,
        asset: ImportedAsset {
            action_index: 0,
            directory: group.directory,
            sha256: group.sha256,
            symbol: group.symbol,
            graph: None,
        },
    })
}
