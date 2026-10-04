//! Portable native item assets at the recipe's load and save boundary.
//!
//! The editor keeps a small pinned graph reference. Only the saved JSON carries the native
//! payloads, and loading restores a checked local copy before the usual editor and build read it.
use crate::{WeaponRecipe, recipe::HexHash};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use parhelion_import::GraphReference;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path},
};

const SCHEMA: u32 = 1;
const MAX_GRAPH: usize = 4 * 1024 * 1024;
const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
const MAX_FILES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    sha256: String,
    #[serde(default)]
    attachments: Vec<GraphReference>,
    embedded_assets: Assets,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assets {
    schema: u32,
    /// Exact graph text, whose whitespace is part of the existing content pin.
    graph: String,
    files: BTreeMap<String, String>,
}

pub(crate) fn encode(recipe: &WeaponRecipe) -> Result<String, String> {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("No imported item selected")?;
    let kind = super::kind(recipe.kind).ok_or("This item kind cannot embed reusable assets")?;
    if !reference.attachments.is_empty() {
        return Err("Portable item assets cannot contain weapon attachments".into());
    }
    reference
        .validate_reusable(kind)
        .map_err(|e| e.to_string())?;
    let root = reference
        .directory
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let raw = read_bounded(&root.join("asset-graph.json"), MAX_GRAPH)?;
    let graph = String::from_utf8(raw).map_err(|e| e.to_string())?;
    let ordered = asset_files(&graph)?;
    let files = ordered.iter().collect::<BTreeSet<_>>();
    let mut payloads = BTreeMap::new();
    let mut total = graph.len();
    for name in files {
        let path = root
            .join(name)
            .canonicalize()
            .map_err(|e| format!("Could not read imported item asset {name:?}: {e}"))?;
        if !path.starts_with(&root) {
            return Err(format!("Item asset {name:?} escapes its source folder"));
        }
        let bytes = read_bounded(&path, MAX_FILE)?;
        total = checked_total(total, bytes.len())?;
        payloads.insert(name.clone(), bytes);
    }
    // Hash exactly the bytes being saved, in the graph's fingerprint order. Checking the
    // source folder alone could miss a file changed and restored while this copy was read.
    if content_pin(&graph, &ordered, &payloads) != reference.sha256 {
        return Err("Imported item assets changed while saving the recipe".into());
    }
    let encoded = payloads
        .into_iter()
        .map(|(name, bytes)| (name, STANDARD.encode(bytes)))
        .collect::<BTreeMap<_, _>>();
    // Reject a concurrent source edit rather than pinning different bytes in the export.
    reference
        .validate_reusable(kind)
        .map_err(|e| e.to_string())?;
    let mut document = serde_json::to_value(recipe).map_err(|e| e.to_string())?;
    let saved = document["overrides"]["imported_graph"]
        .as_object_mut()
        .ok_or("Imported item reference missing")?;
    saved.remove("directory");
    saved.insert(
        "embedded_assets".into(),
        json!({
            "schema": SCHEMA, "graph": graph, "files": encoded,
        }),
    );
    serde_json::to_string_pretty(&document).map_err(|e| e.to_string())
}

pub(crate) fn expand(document: &mut Value) -> Result<(), String> {
    let field = "/overrides/imported_graph";
    if document
        .pointer(field)
        .is_none_or(|v| v.get("embedded_assets").is_none())
    {
        return Ok(());
    }
    let kind = document["kind"]
        .as_str()
        .ok_or("Embedded assets require an item kind")?
        .to_owned();
    if !matches!(
        kind.as_str(),
        "shader" | "armor" | "ghost_shell" | "ship" | "sparrow"
    ) {
        return Err("Embedded assets require a supported gear recipe".into());
    }
    let item: HexHash = serde_json::from_value(document["identity"]["item_hash"].clone())
        .map_err(|e| e.to_string())?;
    item.parse_u32().map_err(|e| e.to_string())?;
    let reference: Reference = serde_json::from_value(
        document
            .pointer_mut(field)
            .ok_or("Imported item reference missing")?
            .take(),
    )
    .map_err(|e| e.to_string())?;
    if !reference.attachments.is_empty() {
        return Err("Portable item assets cannot contain weapon attachments".into());
    }
    if reference.sha256.len() != 64
        || !reference
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Imported item content pin must be a lowercase SHA-256 hash".into());
    }
    let assets = reference.embedded_assets;
    if assets.schema != SCHEMA {
        return Err(format!(
            "Unsupported embedded item asset schema {}",
            assets.schema
        ));
    }
    let ordered = asset_files(&assets.graph)?;
    let expected = ordered.iter().cloned().collect::<BTreeSet<_>>();
    if assets.files.len() != expected.len()
        || assets.files.keys().any(|name| !expected.contains(name))
    {
        return Err("Embedded item assets do not match the pinned graph's files".into());
    }
    let mut total = assets.graph.len();
    let mut files = BTreeMap::new();
    for (name, encoded) in assets.files {
        if encoded.len() > MAX_FILE.div_ceil(3) * 4 {
            return Err(format!("Embedded item asset {name:?} is too large"));
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|e| format!("Invalid embedded item asset {name:?}: {e}"))?;
        if bytes.len() > MAX_FILE {
            return Err(format!("Embedded item asset {name:?} is too large"));
        }
        total = checked_total(total, bytes.len())?;
        files.insert(name, bytes);
    }
    if content_pin(&assets.graph, &ordered, &files) != reference.sha256 {
        return Err("Embedded item assets differ from their content pin".into());
    }
    // Incoming bytes are checked even when cached. Only a missing cache needs extraction,
    // avoiding hundreds of temporary file writes on every library scan or save.
    let root = std::env::temp_dir().join(if kind == "shader" {
        "sundial-native-shaders"
    } else {
        "sundial-native-gear"
    });
    fs::create_dir_all(&root).map_err(|e| format!("Could not create item asset cache: {e}"))?;
    // Serialize publication so concurrent loads cannot partially publish each other's assets.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".publish.lock"))
        .map_err(|e| e.to_string())?;
    fs2::FileExt::lock_exclusive(&lock).map_err(|e| e.to_string())?;
    let destination = root.join(&reference.sha256);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("Cached item assets are not a local folder".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let temporary = tempfile::tempdir_in(&root).map_err(|e| e.to_string())?;
            fs::write(temporary.path().join("asset-graph.json"), assets.graph)
                .map_err(|e| e.to_string())?;
            for (name, bytes) in files {
                let path = temporary.path().join(name);
                fs::create_dir_all(path.parent().ok_or("Item asset has no parent folder")?)
                    .map_err(|e| e.to_string())?;
                // A path alias must not overwrite another payload or the graph.
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| format!("Could not restore item asset: {e}"))?;
                std::io::Write::write_all(&mut file, &bytes).map_err(|e| e.to_string())?;
            }
            GraphReference {
                directory: temporary.path().to_owned(),
                sha256: reference.sha256.clone(),
                attachments: Vec::new(),
            }
            .validate_reusable(&kind)
            .map_err(|e| format!("Embedded item assets are invalid: {e}"))?;
            fs::rename(temporary.path(), &destination).map_err(|e| e.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    let directory = destination.canonicalize().map_err(|e| e.to_string())?;
    if directory.parent() != Some(root.canonicalize().map_err(|e| e.to_string())?.as_path()) {
        return Err("Cached item assets must stay inside their cache folder".into());
    }
    let restored = GraphReference {
        directory,
        sha256: reference.sha256,
        attachments: Vec::new(),
    };
    restored
        .validate_reusable(&kind)
        .map_err(|e| format!("Cached item assets are invalid: {e}"))?;
    *document
        .pointer_mut(field)
        .ok_or("Imported item reference missing")? =
        serde_json::to_value(restored).map_err(|e| e.to_string())?;
    Ok(())
}

/// The existing graph fingerprint, including repeated references in manifest order.
fn content_pin(graph: &str, ordered: &[String], files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut digest = Sha256::new();
    digest.update((graph.len() as u64).to_le_bytes());
    digest.update(graph.as_bytes());
    for name in ordered {
        let bytes = &files[name];
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())
}

fn asset_files(raw: &str) -> Result<Vec<String>, String> {
    if raw.len() > MAX_GRAPH {
        return Err("Embedded item graph is too large".into());
    }
    let graph: Value = sundial::package_authoring::parse_json(raw).map_err(|e| e.to_string())?;
    if !matches!(
        graph["kind"].as_str(),
        Some("shader" | "armor" | "ghost_shell" | "ship" | "sparrow")
    ) {
        return Err("Embedded native asset graph is not reusable gear".into());
    }
    let mut files = Vec::new();
    let mut add = |file: &Value| -> Result<(), String> {
        let name = file.as_str().ok_or("Item asset path missing")?;
        let path = Path::new(name);
        if name.is_empty()
            || name.contains([':', '\0'])
            || name
                .split(['/', '\\'])
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || name.eq_ignore_ascii_case("asset-graph.json")
        {
            return Err(format!(
                "Item asset path {name:?} must stay inside its graph folder"
            ));
        }
        files.push(name.to_owned());
        if files.len() > MAX_FILES {
            return Err("Embedded item graph contains too many files".into());
        }
        Ok(())
    };
    for node in graph["nodes"]
        .as_array()
        .ok_or("Item asset nodes missing")?
    {
        add(&node["file"])?;
    }
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
        for file in first_person["files"]
            .as_object()
            .ok_or("Animation files missing")?
            .values()
        {
            add(file)?;
        }
        for clip in first_person["clips"]
            .as_array()
            .ok_or("Animation clips missing")?
        {
            add(&clip["file"])?;
        }
    }
    for file in graph["audio"]["transcoded_media"]
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
        add(&file["file"])?;
    }
    for key in ["ornament_icon_png", "source_icon_png"] {
        if let Some(file) = graph.get(key).filter(|value| !value.is_null()) {
            add(file)?;
        }
    }
    Ok(files)
}

fn checked_total(total: usize, size: usize) -> Result<usize, String> {
    total
        .checked_add(size)
        .filter(|size| *size <= MAX_TOTAL)
        .ok_or_else(|| "Embedded item assets are too large".into())
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file =
        fs::File::open(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!(
            "Imported item asset {} is too large",
            path.display()
        ));
    }
    Ok(bytes)
}
