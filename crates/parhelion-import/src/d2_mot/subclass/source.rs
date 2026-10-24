//! Retained socket membership, paired ability catalogs, grants and innate/selected perk traces.

use crate::d2_mot::{
    ability::{animation, grants, source::tag},
    assets::item,
    gameplay::perks,
    ornaments,
    reader::Reader,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT: u32 = 1;
pub const DECODER: &str = "Destiny2TheEdgeOfFate";

#[derive(Debug, Deserialize, Serialize)]
pub struct Provenance {
    pub decoder: String,
    /// Cache identity over package paths, sizes and modification times, not a content digest.
    pub package_stamp: String,
    /// None until the mounted packages are independently matched to an executable build.
    pub executable_build: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Choice {
    pub hash: u32,
    pub tag: String,
    pub category: u32,
    pub default: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Socket {
    pub index: u32,
    pub socket_type_index: u16,
    pub plug_sets: Vec<u32>,
    pub choices: Vec<Choice>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PayloadEvidence {
    pub tag: String,
    /// Package entry reference. Only typed tag entries use this field as a class.
    pub reference: u32,
    pub file_type: u32,
    pub file_subtype: u32,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Capture {
    pub format: u32,
    pub provenance: Provenance,
    pub item_hash: u32,
    pub item_tag: String,
    pub sockets: Vec<Socket>,
    /// Original trace documents preserve source fields not understood by the planner.
    pub traces: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub innate_trace: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ability_catalog: Option<grants::Catalog>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<animation::Evidence>,
    /// Mounted capture records every payload read. Replay does not re-read those packages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub payloads: Vec<PayloadEvidence>,
}

fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

impl Capture {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(self.format == FORMAT, "Unsupported subclass capture format");
        ensure!(
            self.provenance.decoder == DECODER,
            "Capture uses another source decoder"
        );
        ensure!(
            hash(&self.provenance.package_stamp),
            "Invalid source package stamp"
        );
        ensure!(
            self.provenance
                .executable_build
                .as_ref()
                .is_none_or(|v| !v.trim().is_empty()),
            "Empty executable build identity"
        );
        tag(&self.item_tag)?;
        if let Some(catalog) = &self.ability_catalog {
            catalog.validate()?;
        }
        ensure!(!self.sockets.is_empty(), "Source candidate has no sockets");
        let mut indexes = BTreeSet::new();
        let mut definitions = BTreeMap::new();
        for socket in &self.sockets {
            ensure!(
                indexes.insert(socket.index),
                "Duplicate source socket index"
            );
            let mut choices = BTreeSet::new();
            for choice in &socket.choices {
                let identity = (tag(&choice.tag)?, choice.category);
                ensure!(
                    choices.insert(choice.hash),
                    "Duplicate plug within source socket"
                );
                if let Some(previous) = definitions.insert(choice.hash, identity) {
                    ensure!(
                        previous == identity,
                        "Source plug has conflicting definitions"
                    );
                }
            }
            ensure!(
                socket
                    .choices
                    .iter()
                    .filter(|choice| choice.default)
                    .count()
                    <= 1,
                "Source socket has multiple defaults"
            );
        }
        let mut payloads = BTreeSet::new();
        for payload in &self.payloads {
            ensure!(
                payloads.insert(tag(&payload.tag)?),
                "Duplicate payload evidence"
            );
            ensure!(hash(&payload.sha256), "Invalid payload digest");
        }
        let mut animations = BTreeSet::new();
        for evidence in &self.animations {
            ensure!(
                animations.insert(tag(&evidence.entity_tag)?),
                "Duplicate animation entity evidence"
            );
            evidence.validate()?;
        }
        Ok(())
    }

    pub(crate) fn choice(&self, socket: u32, plug: u32) -> Result<&Choice> {
        self.sockets
            .iter()
            .find(|row| row.index == socket)
            .with_context(|| format!("Missing source socket {socket}"))?
            .choices
            .iter()
            .find(|choice| choice.hash == plug)
            .with_context(|| format!("Plug {plug:08X} is not a member of source socket {socket}"))
    }
}

fn trace(
    reader: &mut Reader,
    hash: u32,
    source_tag: &str,
    catalog: &grants::Catalog,
) -> Result<Value> {
    let item = reader.tag(tag(source_tag)?, Some(0x8080799D))?;
    // A resource can grant an active ability with no standalone perk controller.
    let mut trace = if item.u64(0x68)? == 0 {
        json!({"format":1, "plug_hash":hash, "item_tag":source_tag,
            "installable":false, "perks":[]})
    } else {
        perks::trace(reader, hash).with_context(|| format!("Tracing source item {hash:08X}"))?
    };
    trace["ability_grants"] = serde_json::to_value(grants::capture(reader, &item, catalog)?)?;
    Ok(trace)
}

pub(crate) fn capture(
    reader: &mut Reader,
    request: &super::Request,
    stamp: String,
) -> Result<Capture> {
    let (_, item_tag) = item::find(reader, request.item_hash)?;
    reader.tag(item_tag, Some(0x8080799D))?;
    // Reuse the bounded modern socket reader, including inline and reusable plug sets.
    // No weapon extraction, donor selection or presentation import runs on this route.
    let sockets = ornaments::gameplay_sockets(reader, request.item_hash)?;
    let mut result = Capture {
        format: FORMAT,
        provenance: Provenance {
            decoder: DECODER.into(),
            package_stamp: stamp,
            executable_build: None,
        },
        item_hash: request.item_hash,
        item_tag: format!("{item_tag:08X}"),
        sockets: serde_json::from_value(sockets["sockets"].clone())
            .context("Source socket membership")?,
        traces: Vec::new(),
        innate_trace: None,
        ability_catalog: None,
        animations: Vec::new(),
        payloads: Vec::new(),
    };
    result.validate()?;
    // Validate every requested membership before following runtime dependencies.
    for selection in &request.selections {
        result.choice(selection.socket_index, selection.plug_hash)?;
    }
    let catalog = grants::catalog(reader)?;
    result.innate_trace = Some(trace(
        reader,
        request.item_hash,
        &result.item_tag,
        &catalog,
    )?);
    let mut captured = BTreeSet::new();
    for selection in &request.selections {
        crate::cancellation::check()?;
        if captured.insert(selection.plug_hash) {
            let choice = result.choice(selection.socket_index, selection.plug_hash)?;
            let selected = trace(reader, choice.hash, &choice.tag, &catalog)?;
            result.traces.push(selected);
        }
    }
    result.animations = animation::capture(
        reader,
        result.innate_trace.iter().chain(result.traces.iter()),
        &catalog,
    )?;
    result.ability_catalog = Some(catalog);
    reader.finish()?;
    Ok(result)
}
