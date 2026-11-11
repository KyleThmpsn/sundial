//! Private graph nodes use temporary identities only while translating. Every occurrence
//! is replaced by a symbol before the graph reaches Parhelion's package allocator.
use super::{payload::Payload, reader::Reader};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

fn put(payload: &mut Payload, offset: usize, bytes: &[u8]) -> Result<()> {
    payload
        .0
        .get_mut(
            offset
                ..offset
                    .checked_add(bytes.len())
                    .context("projectile write overflow")?,
        )
        .context("projectile write outside payload")?
        .copy_from_slice(bytes);
    Ok(())
}

#[derive(Clone)]
pub(crate) struct Node {
    pub id: u32,
    pub symbol: String,
    pub template: u32,
    pub payload: Payload,
    pub reference: Option<u32>,
    storage: Option<&'static str>,
    patches: Vec<(usize, String)>,
    linking: Linking,
}

/// What the item's linked audio namespace binds in an audio node: a bank's converted form and its
/// media rows, each a source sample identity and the reserved node of its recording, or a cue's
/// event fields, each an offset and the bank-local event identity linking rewrites there.
#[derive(Clone, Default)]
struct Linking {
    bank: Option<Value>,
    media: Vec<(u32, u32)>,
    events: Vec<(usize, u32)>,
}

#[derive(Clone, Default)]
pub(crate) struct Assets {
    pub names: BTreeMap<u32, String>,
    pub nodes: Vec<Node>,
}

impl Assets {
    pub fn reserve(&mut self, native: &Reader, symbol: String) -> Result<u32> {
        let id = 0x81FF0000u32
            .checked_add(u32::try_from(self.names.len() + 1)?)
            .context("projectile relocation capacity")?;
        ensure!(
            id <= 0x81FFFFFF && native.reference(id).is_err(),
            "projectile temporary identity collides with native content"
        );
        ensure!(
            !self.names.values().any(|name| name == &symbol),
            "duplicate projectile symbol"
        );
        self.names.insert(id, symbol);
        Ok(id)
    }

    pub fn external(&mut self, native: &Reader, symbol: &str) -> Result<u32> {
        if let Some((&id, _)) = self.names.iter().find(|(_, name)| name.as_str() == symbol) {
            return Ok(id);
        }
        self.reserve(native, symbol.into())
    }

    pub fn push(
        &mut self,
        id: u32,
        template: u32,
        payload: Payload,
        reference: Option<u32>,
    ) -> Result<()> {
        ensure!(
            !self.nodes.iter().any(|node| node.id == id),
            "projectile node emitted twice"
        );
        self.nodes.push(Node {
            id,
            symbol: self
                .names
                .get(&id)
                .context("unreserved projectile node")?
                .clone(),
            template,
            payload,
            reference,
            storage: None,
            patches: Vec::new(),
            linking: Linking::default(),
        });
        Ok(())
    }

    pub fn audio(
        &mut self,
        id: u32,
        template: u32,
        payload: Vec<u8>,
        bank: bool,
        patches: Vec<(usize, String)>,
    ) -> Result<()> {
        self.push(id, template, Payload(payload), None)?;
        let node = self.nodes.last_mut().context("missing audio node")?;
        node.storage = Some(if bank { "audio_bank" } else { "audio_media" });
        for (offset, _) in &patches {
            ensure!(
                node.payload.0.get(*offset..offset + 4) == Some(&u32::MAX.to_le_bytes()[..]),
                "audio media reference is not a placeholder"
            );
        }
        node.patches = patches;
        Ok(())
    }

    /// Declares the converted bank `bank` of emitted audio bank node `id` and its media rows,
    /// each a source sample identity and the reserved node of its recording.
    pub fn bank(&mut self, id: u32, bank: Value, media: Vec<(u32, u32)>) -> Result<()> {
        ensure!(
            media
                .iter()
                .all(|(_, medium)| self.names.contains_key(medium)),
            "unreserved audio medium"
        );
        let node = self
            .nodes
            .iter_mut()
            .find(|node| node.id == id)
            .context("unemitted audio bank")?;
        ensure!(
            node.storage == Some("audio_bank") && node.linking.bank.is_none() && !bank.is_null(),
            "audio bank node differs"
        );
        node.linking.bank = Some(bank);
        node.linking.media = media;
        Ok(())
    }

    /// Declares the event fields of emitted cue node `id`, each an offset and the bank-local event
    /// identity stored there.
    pub fn events(&mut self, id: u32, events: Vec<(usize, u32)>) -> Result<()> {
        let node = self
            .nodes
            .iter_mut()
            .find(|node| node.id == id)
            .context("unemitted audio cue")?;
        ensure!(
            node.storage.is_none() && node.linking.events.is_empty(),
            "audio cue node differs"
        );
        for &(offset, event) in &events {
            ensure!(
                node.payload.u32(offset)? == event,
                "audio cue event differs"
            );
        }
        node.linking.events = events;
        Ok(())
    }

    pub fn write(self, graph: &Path, root: u32, report: Value) -> Result<Value> {
        self.write_in(graph, "projectile", root, report)
    }

    /// Replaces the prefix `from` of every symbol with `to`. A graph flattens its sections into
    /// one symbol space, so a second conversion by the same converter, such as another barrel's
    /// projectile, takes a prefix of its own.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let renamed = |name: &str| -> Result<String> {
            let rest = name
                .strip_prefix(from)
                .with_context(|| format!("projectile symbol {name} lacks the prefix {from}"))?;
            Ok(format!("{to}{rest}"))
        };
        let mut seen = BTreeSet::new();
        for name in self.names.values_mut() {
            *name = renamed(name)?;
            ensure!(seen.insert(name.clone()), "duplicate projectile symbol");
        }
        for node in &mut self.nodes {
            node.symbol = self
                .names
                .get(&node.id)
                .context("unreserved projectile node")?
                .clone();
            for (_, symbol) in &mut node.patches {
                *symbol = renamed(symbol)?;
            }
        }
        Ok(())
    }

    pub fn write_in(self, graph: &Path, folder: &str, root: u32, report: Value) -> Result<Value> {
        // A barrel's own folder holds a projectile fired in place of the weapon's projectile.
        let barrel = folder
            .strip_prefix("projectile-barrel-")
            .is_some_and(|index| index.parse::<u8>().is_ok());
        ensure!(
            matches!(folder, "projectile" | "sword") || barrel,
            "Unsupported private graph folder"
        );
        let root = self
            .names
            .get(&root)
            .context("missing projectile root")?
            .clone();
        let nodes = self.write_nodes(graph, folder)?;
        Ok(json!({"installable":true,"root":root,"nodes":nodes,"conversion":report}))
    }

    /// Replaces every reserved identity in `payload`, a payload outside these nodes, with the
    /// `FFFFFFFF` placeholder and returns the patches that name its symbols.
    pub fn placeholders(&self, payload: &mut Payload) -> Result<Vec<(usize, String)>> {
        let mut patches = Vec::new();
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            if let Some(symbol) = self.names.get(&payload.u32(at)?) {
                patches.push((at, symbol.clone()));
                put(payload, at, &u32::MAX.to_le_bytes())?;
            }
        }
        Ok(patches)
    }

    /// Writes every node's payload beneath `graph/folder` and returns the nodes' graph entries.
    pub fn write_nodes(self, graph: &Path, folder: &str) -> Result<Vec<Value>> {
        let directory = graph.join(folder);
        ensure!(!directory.exists(), "projectile output already exists");
        fs::create_dir_all(&directory)?;
        let media = self
            .nodes
            .iter()
            .filter(|node| node.storage == Some("audio_media"))
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        let mut nodes = Vec::new();
        for mut node in self.nodes {
            let mut patches = node
                .patches
                .iter()
                .map(|(offset, symbol)| json!({"offset":offset,"symbol":symbol}))
                .collect::<Vec<_>>();
            // PCM samples and Wwise IDs are not package tags. Bank patches came from
            // the checked bank parser, whose fields need not be four-byte aligned.
            for at in (0..if node.storage.is_none() {
                node.payload.0.len().saturating_sub(3)
            } else {
                0
            })
                .step_by(4)
            {
                if let Some(symbol) = self.names.get(&node.payload.u32(at)?) {
                    patches.push(json!({"offset":at,"symbol":symbol}));
                    put(&mut node.payload, at, &u32::MAX.to_le_bytes())?;
                }
            }
            let file = format!("{folder}/{}.bin", node.symbol);
            fs::write(graph.join(&file), &node.payload.0)?;
            let mut entry = json!({"symbol":node.symbol,"template":node.template,"file":file,
                "reference":node.reference.map(|id| self.names.get(&id).cloned().context("unreserved projectile reference")).transpose()?,
                "storage":node.storage,
                "patches":patches});
            if let Some(bank) = node.linking.bank {
                let rows = node
                    .linking
                    .media
                    .iter()
                    .map(|&(id, medium)| {
                        ensure!(media.contains(&medium), "audio medium was not emitted");
                        Ok(json!({"id":id,"symbol":self.names[&medium]}))
                    })
                    .collect::<Result<Vec<_>>>()?;
                entry["audio_bank"] = bank;
                entry["audio_media"] = json!(rows);
            }
            if !node.linking.events.is_empty() {
                entry["audio_events"] = node
                    .linking
                    .events
                    .iter()
                    .map(|&(offset, id)| json!({"offset":offset,"id":id}))
                    .collect();
            }
            nodes.push(entry);
        }
        Ok(nodes)
    }
}

pub(crate) fn metadata(
    reader: &mut Reader,
    payloads: &[&Payload],
    output: &mut BTreeMap<u32, Payload>,
) -> Result<()> {
    let mut tags = BTreeSet::new();
    for payload in payloads {
        // Network owners use package-defined root schemas. Their converters check
        // the schema's base type, size and name in addition to provider metadata.
        for field in [16, 24] {
            let class = payload.u32(payload.pointer(field)? + 4)?;
            if (0x80810000..=0x81FFFFFF).contains(&class) {
                reader.reference(class)?;
                tags.insert(class);
            }
        }
        for at in (0..payload.0.len().saturating_sub(3)).step_by(4) {
            let tag = payload.u32(at)?;
            if (0x80800001..=0x81FFFFFF).contains(&tag)
                && matches!(reader.reference(tag), Ok(0x80809B23 | 0x80809C54))
            {
                tags.insert(tag);
            }
        }
    }
    for tag in tags {
        let payload = reader.tag(tag, None)?;
        if let Some(previous) = output.insert(tag, (*payload).clone()) {
            ensure!(
                previous.0 == payload.0,
                "cross-version metadata identity collision"
            );
        }
    }
    Ok(())
}

pub(crate) fn retag(payload: &Payload, tag: u32) -> Result<Payload> {
    let mut result = payload.clone();
    let previous = payload.u32(payload.pointer(16)?)?;
    for at in (0..payload.0.len().saturating_sub(15)).step_by(4) {
        if payload.u32(at)? == previous {
            ensure!(
                payload.u32(at + 4)? & 0xFFFF0000 == 0x80800000
                    && payload.u64(at + 8)? < payload.0.len() as u64,
                "untyped component owner occurrence"
            );
            put(&mut result, at, &tag.to_le_bytes())?;
        }
    }
    Ok(result)
}
