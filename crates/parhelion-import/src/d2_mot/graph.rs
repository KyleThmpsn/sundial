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
        super::reader::write_json(&output.join("asset-graph.json"), &graph)?;
        self.validate(source_item)?;
        Self::new(&output, target_item)
    }

    pub fn new(directory: &Path, item: u32) -> Result<Self> {
        let directory = directory
            .canonicalize()
            .context("Open imported asset folder")?;
        let sha256 = fingerprint(&directory, item)?;
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
            fingerprint(&self.directory, item)? == self.sha256,
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
}

fn fingerprint(directory: &Path, item: u32) -> Result<String> {
    let bytes = fs::read(directory.join("asset-graph.json"))?;
    let graph: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        graph["item_hash"].as_u64() == Some(u64::from(item))
            || graph["ornament"]["target_weapon"].as_u64() == Some(u64::from(item)),
        "Imported graph belongs to a different weapon identity"
    );
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
    ensure!(
        symbols.contains("parent"),
        "Imported graph has no parent asset"
    );
    if let Some(first_person) = graph["animation"]["first_person"].as_object() {
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
                resolved.starts_with(&root),
                "Animation payload escapes its graph folder"
            );
            let payload = fs::read(resolved)?;
            digest.update((payload.len() as u64).to_le_bytes());
            digest.update(payload);
        }
    }
    if let Some(icon) = graph["ornament_icon_png"].as_str() {
        let path = root.join(icon).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "Ornament icon escapes its graph folder"
        );
        let payload = fs::read(path)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    Ok(format!("{:x}", digest.finalize()))
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
