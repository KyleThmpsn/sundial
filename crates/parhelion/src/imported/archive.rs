//! Recipes that earlier versions saved with their native item assets embedded.
//!
//! A recipe names its imported graph folder, as a weapon recipe does. Earlier versions saved gear,
//! shader and universal ornament recipes with the graph's payloads embedded instead. Loading one
//! checks every payload against its content pin and restores them into the `graph` folder of a
//! conversion of the source item, as its importer would have named one, with each attachment in
//! that conversion's `ornaments/<HASH>/graph`. A later load of the same assets reuses them.
//! Saving the recipe then names those folders.
#[cfg(test)]
use crate::WeaponRecipe;
use crate::recipe::HexHash;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use parhelion_import::{
    GraphReference,
    d2_mot::service::{IMPORTERS, IMPORTS, item_directory, reserve_assets},
};
use serde::Deserialize;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::io::Read;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

const SCHEMA: u32 = 1;
const MAX_GRAPH: usize = 4 * 1024 * 1024;
const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
// Complete vehicle graphs include separate damage forms and effect resources.
// The independent graph, per-file and aggregate byte bounds still apply.
const MAX_FILES: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    sha256: String,
    #[serde(default)]
    converter_revision: Option<u32>,
    #[serde(default)]
    material_inputs: Option<parhelion_import::marathon::MaterialInputs>,
    #[serde(default)]
    attachments: Vec<Value>,
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

/// Writes the embedded form earlier versions saved, so tests can load one.
#[cfg(test)]
pub(crate) fn encode(recipe: &WeaponRecipe) -> Result<String, String> {
    let reference = recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("No imported item selected")?;
    let kind = super::kind(recipe.kind).ok_or("This item kind cannot embed reusable assets")?;
    reference
        .validate_reusable(kind)
        .map_err(|e| e.to_string())?;
    let mut total = 0;
    let mut document = serde_json::to_value(recipe).map_err(|e| e.to_string())?;
    document["overrides"]["imported_graph"] = encode_reference(reference, kind, &mut total, true)?;
    serde_json::to_string_pretty(&document).map_err(|e| e.to_string())
}

#[cfg(test)]
fn encode_reference(
    reference: &GraphReference,
    kind: &str,
    total: &mut usize,
    owner: bool,
) -> Result<Value, String> {
    if reference.attachments.len() > 32
        || (!reference.attachments.is_empty() && (!owner || kind != "armor"))
    {
        return Err("Only armor supports a bounded list of direct appearance attachments".into());
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
    *total = checked_total(*total, graph.len())?;
    for name in files {
        let path = root
            .join(name)
            .canonicalize()
            .map_err(|e| format!("Could not read imported item asset {name:?}: {e}"))?;
        if !path.starts_with(&root) {
            return Err(format!("Item asset {name:?} escapes its source folder"));
        }
        let bytes = read_bounded(&path, MAX_FILE)?;
        *total = checked_total(*total, bytes.len())?;
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
    let attachments = reference
        .attachments
        .iter()
        .map(|attachment| encode_reference(attachment, kind, total, false))
        .collect::<Result<Vec<_>, _>>()?;
    let mut document = serde_json::to_value(reference).map_err(|e| e.to_string())?;
    let saved = document
        .as_object_mut()
        .ok_or("Imported item reference missing")?;
    saved.insert("attachments".into(), Value::Array(attachments));
    saved.remove("directory");
    saved.insert(
        "embedded_assets".into(),
        json!({
            "schema": SCHEMA, "graph": graph, "files": encoded,
        }),
    );
    Ok(document)
}

pub(crate) fn expand(document: &mut Value) -> Result<(), String> {
    let field = "/overrides/imported_graph";
    if document
        .pointer(field)
        .is_none_or(|v| v.get("embedded_assets").is_none())
    {
        return Ok(());
    }
    document["kind"]
        .as_str()
        .ok_or("Embedded assets require an item kind")?;
    // The recipe's kind names the kind of assets it embeds, as a mod's names a universal ornament.
    let kind = serde_json::from_value::<crate::ItemKind>(document["kind"].clone())
        .ok()
        .and_then(super::kind)
        .ok_or("Embedded assets require a supported gear recipe")?;
    let item: HexHash = serde_json::from_value(document["identity"]["item_hash"].clone())
        .map_err(|e| e.to_string())?;
    item.parse_u32().map_err(|e| e.to_string())?;
    let value = document
        .pointer_mut(field)
        .ok_or("Imported item reference missing")?
        .take();
    let restored = expand_reference(value, kind, &mut 0, None)?;
    *document
        .pointer_mut(field)
        .ok_or("Imported item reference missing")? =
        serde_json::to_value(restored).map_err(|e| e.to_string())?;
    Ok(())
}

/// Restores one embedded graph and its attachments. `owner` is the conversion folder of the graph
/// an attachment belongs to, `None` for the recipe's own graph.
fn expand_reference(
    value: Value,
    kind: &str,
    total: &mut usize,
    owner: Option<&Path>,
) -> Result<GraphReference, String> {
    let reference: Reference = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if reference.attachments.len() > 32
        || (!reference.attachments.is_empty() && (owner.is_some() || kind != "armor"))
    {
        return Err("Only armor supports a bounded list of direct appearance attachments".into());
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
    *total = checked_total(*total, assets.graph.len())?;
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
        *total = checked_total(*total, bytes.len())?;
        files.insert(name, bytes);
    }
    if content_pin(&assets.graph, &ordered, &files) != reference.sha256 {
        return Err("Embedded item assets differ from their content pin".into());
    }
    let directory = match owner {
        None => restore(&reference.sha256, &assets.graph, files, kind)?,
        Some(conversion) => {
            restore_ornament(conversion, &reference.sha256, &assets.graph, files, kind)?
        }
    };
    let conversion = directory
        .parent()
        .ok_or("Restored graph has no conversion folder")?
        .to_owned();
    let converter_revision = parhelion_import::manifest_converter_revision(&directory);
    if reference
        .converter_revision
        .is_some_and(|revision| revision != converter_revision)
    {
        return Err("Embedded item converter revision differs from its pinned graph".into());
    }
    let attachments = reference
        .attachments
        .into_iter()
        .map(|attachment| expand_reference(attachment, kind, total, Some(&conversion)))
        .collect::<Result<Vec<_>, _>>()?;
    let restored = GraphReference {
        directory,
        sha256: reference.sha256,
        attachments,
        converter_revision,
        material_inputs: reference.material_inputs,
    };
    restored
        .validate_reusable(kind)
        .map_err(|e| format!("Cached item assets are invalid: {e}"))?;
    Ok(restored)
}

/// The graph folder the checked assets with content pin `sha256` are restored in: the `graph`
/// folder of a conversion of the source item, of the kind its importer gives such a graph. A
/// conversion there whose graph is the same text and whose payloads match the pin is reused.
/// Otherwise a new one is reserved and filled, and a restore that stops partway is removed by the
/// sweep once nothing names it.
fn restore(
    sha256: &str,
    graph: &str,
    files: BTreeMap<String, Vec<u8>>,
    kind: &str,
) -> Result<PathBuf, String> {
    let (item, conversion) = restore_folder(graph)?;
    if let Some(found) = restored(&item, graph, sha256, kind) {
        return Ok(found);
    }
    let folder = reserve_assets(&item, conversion).map_err(|e| e.to_string())?;
    let directory = folder.join(parhelion_import::GRAPH);
    if let Err(error) = write_graph(&directory, sha256, graph, files, kind) {
        // Removing it only saves space. The sweep removes it otherwise.
        let _ = fs::remove_dir_all(&folder);
        return Err(error);
    }
    directory.canonicalize().map_err(|e| e.to_string())
}

/// The graph folder an attachment of the graph restored in the conversion folder `conversion` is
/// restored in, `ornaments/<HASH>/graph` there, named by its source item. One holding the same
/// graph text whose payloads match the pin `sha256` is reused.
fn restore_ornament(
    conversion: &Path,
    sha256: &str,
    graph: &str,
    files: BTreeMap<String, Vec<u8>>,
    kind: &str,
) -> Result<PathBuf, String> {
    let parsed: Value = sundial::package_authoring::parse_json(graph).map_err(|e| e.to_string())?;
    let source = [&parsed["ornament"]["source_item"], &parsed["source_item"]]
        .into_iter()
        .find_map(|value| u32::try_from(value.as_u64()?).ok())
        .ok_or("Embedded ornament graph names no source item")?;
    let directory = conversion
        .join(parhelion_import::ORNAMENTS)
        .join(format!("{source:08X}"))
        .join(parhelion_import::GRAPH);
    if directory.exists() {
        if holds(&directory, graph, sha256, kind) {
            return directory.canonicalize().map_err(|e| e.to_string());
        }
        return Err(format!("{} holds another ornament", directory.display()));
    }
    if let Err(error) = write_graph(&directory, sha256, graph, files, kind) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    directory.canonicalize().map_err(|e| e.to_string())
}

/// Writes the payloads and then the graph text into the new folder `directory`, and checks them
/// against the content pin `sha256`.
fn write_graph(
    directory: &Path,
    sha256: &str,
    graph: &str,
    files: BTreeMap<String, Vec<u8>>,
    kind: &str,
) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for (name, bytes) in files {
        let path = directory.join(name);
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
    fs::write(directory.join("asset-graph.json"), graph).map_err(|e| e.to_string())?;
    pinned(directory, sha256)
        .validate_reusable(kind)
        .map_err(|e| format!("Embedded item assets are invalid: {e}"))
}

/// The import folder of the item the embedded graph `graph` was converted from, its importer's,
/// named by the graph's `source`, or the modern importer's when it names none, and the kind of
/// conversion folder that importer gives such a graph.
fn restore_folder(graph: &str) -> Result<(PathBuf, &'static str), String> {
    let graph: Value = sundial::package_authoring::parse_json(graph).map_err(|e| e.to_string())?;
    let importer = graph["source"]
        .as_str()
        .filter(|source| IMPORTERS.contains(source))
        .unwrap_or("d2_mot");
    let conversion = match (importer, graph["kind"].as_str()) {
        (_, Some("shader")) => "shader",
        ("d1_roi", _) => "armor",
        ("halo_reach", _) => "vehicle",
        _ => "gear",
    };
    let item = ["source_item", "item_hash"]
        .into_iter()
        .find_map(|key| u32::try_from(graph[key].as_u64()?).ok())
        .ok_or("Embedded item graph names no item")?;
    let imports = sundial::package_authoring::parhelion_data_directory()
        .ok_or("No data folder to restore embedded item assets in")?
        .join(IMPORTS);
    let folder = item_directory(&imports, importer, item).map_err(|e| e.to_string())?;
    Ok((folder, conversion))
}

/// The graph folder of a conversion of `item` that holds the graph text `graph` with payloads
/// matching the content pin `sha256`.
fn restored(item: &Path, graph: &str, sha256: &str, kind: &str) -> Option<PathBuf> {
    fs::read_dir(item).ok()?.find_map(|entry| {
        let directory = entry.ok()?.path().join(parhelion_import::GRAPH);
        holds(&directory, graph, sha256, kind)
            .then(|| directory.canonicalize().ok())
            .flatten()
    })
}

/// Whether `directory` holds the graph text `graph` with payloads matching the pin `sha256`.
fn holds(directory: &Path, graph: &str, sha256: &str, kind: &str) -> bool {
    fs::read(directory.join("asset-graph.json")).is_ok_and(|text| text == graph.as_bytes())
        && pinned(directory, sha256).validate_reusable(kind).is_ok()
}

/// A reference to the assets in `folder` that must match the content pin `sha256`.
fn pinned(folder: &Path, sha256: &str) -> GraphReference {
    GraphReference {
        directory: folder.to_owned(),
        sha256: sha256.to_owned(),
        attachments: Vec::new(),
        converter_revision: 0,
        material_inputs: None,
    }
}

/// Leaves out the embedded assets of a recipe that is only listed. Lists and searches of saved
/// recipes read names, icons and sockets, never the model, and restoring decodes and hashes
/// every embedded payload twice.
pub(crate) fn skip(document: &mut Value) {
    if let Some(graph) = document.pointer_mut("/overrides/imported_graph")
        && graph.get("embedded_assets").is_some()
    {
        *graph = Value::Null;
    }
}

/// Removes the payloads of an embedded model and of its attachments, keeping their graph text,
/// content pins and converter revisions, which a listing still reads. Whether there were any.
pub(crate) fn strip(document: &mut Value) -> bool {
    fn strip_reference(reference: &mut Value) -> bool {
        let mut stripped = reference
            .get_mut("embedded_assets")
            .and_then(Value::as_object_mut)
            .is_some_and(|assets| assets.remove("files").is_some());
        for attachment in reference
            .get_mut("attachments")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            stripped |= strip_reference(attachment);
        }
        stripped
    }
    document
        .pointer_mut("/overrides/imported_graph")
        .is_some_and(strip_reference)
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
    hex::encode(digest.finalize())
}

fn asset_files(raw: &str) -> Result<Vec<String>, String> {
    if raw.len() > MAX_GRAPH {
        return Err("Embedded item graph is too large".into());
    }
    let graph: Value = sundial::package_authoring::parse_json(raw).map_err(|e| e.to_string())?;
    if !matches!(
        graph["kind"].as_str(),
        Some("shader" | "armor" | "ghost_shell" | "ship" | "sparrow" | "ornament")
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
        .chain(graph["animation"]["character"].as_object())
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
        .chain(
            graph["audio_assets"]["nodes"]
                .as_array()
                .into_iter()
                .flatten(),
        )
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

#[cfg(test)]
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
