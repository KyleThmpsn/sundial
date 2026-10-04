use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// A content-pinned local asset graph. Changing any payload requires reselection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphReference {
    pub directory: PathBuf,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<GraphReference>,
}

impl GraphReference {
    /// Copy an existing model into an independent authored identity without touching its source.
    pub fn copy_model(&self, source_item: u32, target_item: u32, output: &Path) -> Result<Self> {
        self.validate(source_item)?;
        ensure!(
            ![0, u32::MAX, 0x811C9DC5].contains(&target_item),
            "Invalid target identity"
        );
        let output = super::reader::outside(output, &self.directory)?;
        ensure!(!output.exists(), "Model output already exists");
        let mut graph: Value =
            serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
        ensure!(
            !matches!(
                graph["kind"].as_str(),
                Some("shader" | "armor" | "ghost_shell" | "ship" | "sparrow")
            ) && graph.get("gear_art").is_none(),
            "Choose imported weapon assets for a weapon model"
        );
        ensure!(
            graph.get("ornament").is_none(),
            "Choose a weapon model rather than an ornament attachment"
        );
        fs::create_dir_all(&output)?;
        for node in graph["nodes"].as_array().context("Missing asset nodes")? {
            let file = node["file"]
                .as_str()
                .context("Missing asset payload path")?;
            let destination = output.join(file);
            fs::create_dir_all(destination.parent().context("Model file parent")?)?;
            fs::copy(self.directory.join(file), destination)?;
        }
        if let Some(first_person) = graph["animation"]["first_person"].as_object() {
            let files = first_person["files"]
                .as_object()
                .context("animation files")?;
            let clips = first_person["clips"]
                .as_array()
                .context("animation clips")?;
            for value in files.values().chain(clips.iter().map(|clip| &clip["file"])) {
                let file = value.as_str().context("animation file")?;
                let destination = output.join(file);
                fs::create_dir_all(destination.parent().context("animation file parent")?)?;
                fs::copy(self.directory.join(file), destination)?;
            }
        }
        for media in graph["audio"]["transcoded_media"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                graph["audio"]["converted_banks"]
                    .as_array()
                    .into_iter()
                    .flatten(),
            )
            .chain(graph["particles"]["nodes"].as_array().into_iter().flatten())
        {
            let file = media["file"].as_str().context("audio or particle file")?;
            let destination = output.join(file);
            fs::create_dir_all(
                destination
                    .parent()
                    .context("audio or particle file parent")?,
            )?;
            fs::copy(self.directory.join(file), destination)?;
        }
        let key = |role: &str| {
            let mut hash = format!("parhelion/imported-model/{target_item:08X}/{role}")
                .bytes()
                .fold(0x811C9DC5u32, |hash, byte| {
                    hash.wrapping_mul(16777619) ^ u32::from(byte)
                });
            if [0, u32::MAX, 0x811C9DC5].contains(&hash) {
                hash ^= 0x10000;
            }
            hash
        };
        graph["item_hash"] = target_item.into();
        graph["art_key"] = key("art").into();
        if let Some(dyes) = graph["dyes"].as_array_mut() {
            for (index, dye) in dyes.iter_mut().enumerate() {
                dye["manifest"] = key(&format!("dye-{index}")).into();
            }
        }
        if let Some(parts) = graph["kept_parts"].as_array_mut() {
            for (index, part) in parts.iter_mut().enumerate() {
                part["key"] = key(&format!("kept-{index}")).into();
            }
        }
        if let Some(parts) = graph["source_parts"].as_array_mut() {
            for (index, part) in parts.iter_mut().enumerate() {
                part["key"] = key(&format!("source-part-{index}")).into();
            }
        }
        super::reader::write_json(&output.join("asset-graph.json"), &graph)?;
        self.validate(source_item)?;
        Self::new(&output, target_item)
    }

    pub fn new(directory: &Path, item: u32) -> Result<Self> {
        let directory = directory
            .canonicalize()
            .context("Open imported asset folder")?;
        let sha256 = fingerprint(&directory, Some(item))?;
        Ok(Self {
            directory,
            sha256,
            attachments: Vec::new(),
        })
    }

    pub fn validate(&self, item: u32) -> Result<()> {
        ensure!(
            self.directory.is_absolute(),
            "Imported asset folder must be absolute"
        );
        ensure!(
            fingerprint(&self.directory, Some(item))? == self.sha256,
            "Imported assets changed after selection. Select the prepared graph again"
        );
        for attachment in &self.attachments {
            ensure!(
                attachment.attachments.is_empty(),
                "Nested imported attachments are not supported"
            );
            attachment.validate(item)?;
        }
        Ok(())
    }

    /// Shader payloads are reusable source materials. Each authored recipe allocates its own
    /// item and dye registrations when linking, without rewriting these pinned source files.
    pub fn validate_shader(&self) -> Result<()> {
        self.validate_reusable("shader")
    }

    /// Model gear and shaders allocate private registrations for each authored recipe.
    /// Their immutable source graph can therefore survive a rename or a duplicate.
    pub fn validate_reusable(&self, kind: &str) -> Result<()> {
        ensure!(
            matches!(
                kind,
                "shader" | "armor" | "ghost_shell" | "ship" | "sparrow"
            ),
            "This item kind does not use reusable imported assets"
        );
        ensure!(
            self.directory.is_absolute(),
            "Imported asset folder must be absolute"
        );
        ensure!(
            self.attachments.is_empty(),
            "Reusable item assets cannot contain attachments"
        );
        ensure!(
            fingerprint(&self.directory, None)? == self.sha256,
            "Imported assets changed after selection. Select the prepared graph again"
        );
        let graph: Value =
            serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
        ensure!(
            graph["kind"] == kind,
            "Imported asset kind differs from its recipe"
        );
        Ok(())
    }
}

fn hash_animation(graph: &Value, root: &Path, digest: &mut Sha256) -> Result<()> {
    for first_person in graph["animation"]["first_person"]
        .as_object()
        .into_iter()
        .chain(
            graph
                .get("equipment_animation")
                .filter(|a| a["status"] == "linked")
                .and_then(Value::as_object),
        )
    {
        let mut files = Vec::new();
        for (_, file) in first_person["files"]
            .as_object()
            .context("animation files")?
        {
            files.push(file.as_str().context("animation file")?);
        }
        for clip in first_person["clips"]
            .as_array()
            .context("animation clips")?
        {
            files.push(clip["file"].as_str().context("animation clip file")?);
        }
        for name in files {
            let path = Path::new(name);
            ensure!(
                !path.as_os_str().is_empty()
                    && path
                        .components()
                        .all(|part| matches!(part, Component::Normal(_))),
                "Animation payload path must stay inside its graph folder"
            );
            let resolved = root.join(path).canonicalize()?;
            ensure!(
                resolved.starts_with(root),
                "Animation payload escapes its graph folder"
            );
            let payload = fs::read(resolved)?;
            digest.update((payload.len() as u64).to_le_bytes());
            digest.update(payload);
        }
    }
    Ok(())
}

fn fingerprint(directory: &Path, item: Option<u32>) -> Result<String> {
    let bytes = fs::read(directory.join("asset-graph.json"))?;
    let graph: Value = serde_json::from_slice(&bytes)?;
    if let Some(item) = item {
        ensure!(
            graph["item_hash"].as_u64() == Some(u64::from(item))
                || graph["ornament"]["target_weapon"].as_u64() == Some(u64::from(item)),
            "Imported graph belongs to a different item identity"
        );
    } else {
        ensure!(
            matches!(
                graph["kind"].as_str(),
                Some("shader" | "armor" | "ghost_shell" | "ship" | "sparrow")
            ),
            "Imported graph is not reusable item assets"
        );
        ensure!(
            graph["item_hash"]
                .as_u64()
                .and_then(|hash| u32::try_from(hash).ok())
                .is_some_and(|hash| ![0, u32::MAX, 0x811C9DC5].contains(&hash)),
            "Imported graph has an invalid source identity"
        );
    }
    let nodes = graph["nodes"].as_array().context("Missing asset nodes")?;
    ensure!(!nodes.is_empty(), "Imported graph has no assets");
    let mut digest = Sha256::new();
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(&bytes);
    let root = directory.canonicalize()?;
    let mut symbols = BTreeSet::new();
    for node in nodes {
        let symbol = node["symbol"].as_str().context("Missing asset symbol")?;
        ensure!(
            !symbol.is_empty()
                && symbol
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
            "Invalid asset symbol {symbol}"
        );
        ensure!(symbols.insert(symbol), "Duplicate asset symbol {symbol}");
        let name = node["file"]
            .as_str()
            .context("Missing asset payload path")?;
        let path = Path::new(name);
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Asset payload path must stay inside its graph folder"
        );
        let resolved = root.join(path).canonicalize()?;
        ensure!(
            resolved.starts_with(&root),
            "Asset payload escapes its graph folder"
        );
        let payload = fs::read(resolved)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    if graph["kind"] == "shader" {
        let dyes = graph["dyes"]
            .as_array()
            .context("Imported shader has no dyes")?;
        ensure!(
            !dyes.is_empty() && dyes.len() <= 15,
            "Invalid imported shader dye count"
        );
        let mut channels = BTreeSet::new();
        let mut manifests = BTreeSet::new();
        for dye in dyes {
            let channel = dye["channel"].as_u64().context("Missing shader channel")?;
            let key = dye["manifest"]
                .as_u64()
                .context("Missing shader dye identity")?;
            ensure!(
                matches!(channel, 0..=2 | 4..=15) && channels.insert(channel),
                "Invalid or duplicate shader channel"
            );
            ensure!(
                key <= u64::from(u32::MAX)
                    && ![0, u64::from(u32::MAX), 0x811C9DC5].contains(&key)
                    && manifests.insert(key),
                "Invalid or duplicate shader dye identity"
            );
            ensure!(
                symbols.contains(dye["parent"].as_str().context("Missing shader parent")?),
                "Imported shader has no dye parent asset"
            );
        }
    } else if graph.get("gear_art").is_some()
        || matches!(
            graph["kind"].as_str(),
            Some("armor" | "ghost_shell" | "ship" | "sparrow")
        )
    {
        validate_gear(&graph, &symbols)?;
    } else {
        ensure!(
            symbols.contains("parent"),
            "Imported graph has no parent asset"
        );
    }
    hash_animation(&graph, &root, &mut digest)?;
    for media in graph["audio"]["transcoded_media"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            graph["audio"]["converted_banks"]
                .as_array()
                .into_iter()
                .flatten(),
        )
    {
        let name = media["file"].as_str().context("audio media file")?;
        let path = Path::new(name);
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Audio payload path must stay inside its graph folder"
        );
        let resolved = root.join(path).canonicalize()?;
        ensure!(
            resolved.starts_with(&root),
            "Audio payload escapes its graph folder"
        );
        let payload = fs::read(resolved)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    for node in graph["particles"]["nodes"].as_array().into_iter().flatten() {
        let name = node["file"].as_str().context("particle payload file")?;
        let path = Path::new(name);
        ensure!(
            !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Particle payload path must stay inside its graph folder"
        );
        let resolved = root.join(path).canonicalize()?;
        ensure!(
            resolved.starts_with(&root),
            "Particle payload escapes its graph folder"
        );
        let payload = fs::read(resolved)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    for icon in [
        graph["ornament_icon_png"].as_str(),
        graph["source_icon_png"].as_str(),
    ]
    .into_iter()
    .flatten()
    {
        let path = root.join(icon).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "Imported icon escapes its graph folder"
        );
        let payload = fs::read(path)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_gear(graph: &Value, symbols: &BTreeSet<&str>) -> Result<()> {
    ensure!(
        matches!(
            graph["kind"].as_str(),
            Some("armor" | "ghost_shell" | "ship" | "sparrow")
        ),
        "Gear art requires a model gear recipe"
    );
    ensure!(
        symbols.contains(graph["parent"].as_str().context("Gear primary parent")?),
        "Gear parent asset missing"
    );
    let dyes = graph["dyes"].as_array().context("Gear dyes")?;
    let layers = graph["dye_rows"].as_array().context("Gear dye layers")?;
    ensure!(
        layers.len() == 3 && dyes.len() <= 45,
        "Invalid gear dye layers"
    );
    let mut manifests = BTreeSet::new();
    for dye in dyes {
        let channel = dye["channel"].as_u64().context("Gear dye channel")?;
        let key = super::profile::hash(dye, "manifest")?;
        ensure!(
            matches!(channel, 0..=2 | 4..=15) && manifests.insert(key),
            "Invalid gear dye identity"
        );
        ensure!(
            symbols.contains(dye["parent"].as_str().context("Gear dye parent")?),
            "Gear dye parent asset missing"
        );
    }
    let mut used = BTreeSet::new();
    for layer in layers {
        let mut channels = BTreeSet::new();
        for row in layer.as_array().context("Gear dye layer")? {
            let index = usize::try_from(row["dye"].as_u64().context("Gear dye index")?)?;
            let dye = dyes
                .get(index)
                .context("Gear dye index outside converted dyes")?;
            let channel = row["channel"].as_u64().context("Gear dye channel")?;
            ensure!(
                channels.insert(channel) && dye["channel"].as_u64() == Some(channel),
                "Gear dye channel differs from its layer"
            );
            used.insert(index);
        }
    }
    ensure!(used.len() == dyes.len(), "Unregistered gear dye material");
    let rows = graph["gear_art"]["rows"]
        .as_array()
        .context("Gear art rows")?;
    let parts = graph["gear_art"]["parts"]
        .as_array()
        .context("Gear art parts")?;
    ensure!(
        !rows.is_empty() && rows.len() <= 64 && !parts.is_empty() && parts.len() <= 256,
        "Gear art count outside native limits"
    );
    let mut assignments = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for part in parts {
        let source = super::profile::hash(part, "source_assignment")?;
        let key = super::profile::hash(part, "key")?;
        ensure!(
            assignments.insert(u64::from(source)) && keys.insert(key),
            "Duplicate gear art assignment"
        );
        ensure!(
            symbols.contains(part["parent"].as_str().context("Gear art parent")?),
            "Gear art parent asset missing"
        );
    }
    for row in rows {
        ensure!(
            row["class"].as_i64().is_some_and(|v| (-1..=2).contains(&v)),
            "Unsupported gear class selector"
        );
        ensure!(
            row["flags"].as_u64().is_some_and(|v| v <= 255),
            "Invalid gear art flags"
        );
        ensure!(
            row["template_index"]
                .as_u64()
                .is_some_and(|v| v < u64::from(u16::MAX)),
            "Invalid native art template index"
        );
        let singles = row["singles"]
            .as_array()
            .context("Gear direct assignments")?;
        let slots = row["slots"].as_array().context("Gear art selectors")?;
        ensure!(
            singles.len() == 2 && slots.len() <= 32,
            "Invalid gear art layout"
        );
        let mut selectors = BTreeSet::new();
        let mut selected = singles.iter().collect::<Vec<_>>();
        for slot in slots {
            ensure!(
                selectors.insert(slot["selector"].as_u64().context("Gear selector")?),
                "Duplicate gear selector"
            );
            let values = slot["assignments"]
                .as_array()
                .context("Gear alternatives")?;
            ensure!(values.len() <= 256, "Too many gear alternatives");
            selected.extend(values);
        }
        for value in selected {
            let key = value.as_u64().context("Gear assignment")?;
            ensure!(
                [0, u64::from(u32::MAX), 0x811C9DC5].contains(&key) || assignments.contains(&key),
                "Gear art refers to an unconverted source assignment"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copying_a_model_preserves_payloads_and_allocates_independent_identity_keys() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("parent.bin"), [1, 2, 3]).unwrap();
        let original = r#"{"item_hash":42,"art_key":9,"nodes":[{"symbol":"parent","file":"parent.bin"}],"dyes":[{"manifest":10,"channel":0,"parent":"parent"}]}"#;
        fs::write(source.join("asset-graph.json"), original).unwrap();
        let reference = GraphReference::new(&source, 42).unwrap();
        let copied = reference
            .copy_model(42, 43, &root.path().join("copy"))
            .unwrap();
        copied.validate(43).unwrap();
        assert!(copied.validate(42).is_err());
        reference.validate(42).unwrap();
        assert_eq!(
            fs::read(source.join("asset-graph.json")).unwrap(),
            original.as_bytes()
        );
        assert_eq!(
            fs::read(copied.directory.join("parent.bin")).unwrap(),
            [1, 2, 3]
        );
        let graph: Value =
            serde_json::from_slice(&fs::read(copied.directory.join("asset-graph.json")).unwrap())
                .unwrap();
        assert_ne!(graph["art_key"], 9);
        assert_ne!(graph["dyes"][0]["manifest"], 10);
        assert_ne!(graph["art_key"], graph["dyes"][0]["manifest"]);
    }

    #[test]
    fn content_changes_and_wrong_identity_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("parent.bin"), [1, 2, 3]).unwrap();
        fs::write(
            root.path().join("asset-graph.json"),
            r#"{"item_hash":42,"nodes":[{"symbol":"parent","file":"parent.bin"}]}"#,
        )
        .unwrap();
        let reference = GraphReference::new(root.path(), 42).unwrap();
        reference.validate(42).unwrap();
        assert!(reference.validate(43).is_err());
        fs::write(root.path().join("parent.bin"), [1, 2, 4]).unwrap();
        assert!(reference.validate(42).is_err());
    }

    #[test]
    fn converted_audio_is_pinned_and_copied_with_the_graph() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir_all(source.join("audio")).unwrap();
        fs::write(source.join("parent.bin"), [1, 2, 3]).unwrap();
        fs::write(source.join("audio/source.pcm.wem"), [4, 5, 6]).unwrap();
        fs::write(source.join("asset-graph.json"), r#"{"item_hash":42,"nodes":[{"symbol":"parent","file":"parent.bin"}],"audio":{"transcoded_media":[{"file":"audio/source.pcm.wem"}]}}"#).unwrap();
        let reference = GraphReference::new(&source, 42).unwrap();
        let copied = reference
            .copy_model(42, 43, &root.path().join("copy"))
            .unwrap();
        assert_eq!(
            fs::read(copied.directory.join("audio/source.pcm.wem")).unwrap(),
            [4, 5, 6]
        );
        fs::write(source.join("audio/source.pcm.wem"), [4, 5, 7]).unwrap();
        assert!(reference.validate(42).is_err());
        copied.validate(43).unwrap();
    }

    #[test]
    fn traversal_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("asset-graph.json"),
            r#"{"item_hash":42,"nodes":[{"symbol":"parent","file":"../outside.bin"}]}"#,
        )
        .unwrap();
        assert!(
            GraphReference::new(root.path(), 42)
                .unwrap_err()
                .to_string()
                .contains("inside")
        );
    }
}
