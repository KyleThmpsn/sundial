use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
mod opened;
mod presentation;
pub use opened::forget_opened;

/// The importer revision a new import records in its graph. Raise it with any change to what
/// conversion writes. Raise `MINIMUM_CONVERTER_REVISION` to it as well when imports made before
/// the change are unsafe or wrong to install, so Parhelion asks for those to be imported again.
pub const CONVERTER_REVISION: u32 = 212;
/// Imports from an earlier revision need importing again. Imports made before revisions were
/// recorded count as revision 0.
pub const MINIMUM_CONVERTER_REVISION: u32 = 0;

/// A content-pinned local asset graph. Changing any payload requires reselection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphReference {
    pub directory: PathBuf,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<GraphReference>,
    /// The importer revision that converted this graph, read from its manifest when it was
    /// pinned. 0 for imports made before revisions were recorded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub converter_revision: u32,
    /// Authored values applied to private material copies without changing the source pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_inputs: Option<crate::marathon::MaterialInputs>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Every declared model, including attachments saved before nodes carried a model flag.
/// Use declarations rather than symbol prefixes, which can also name buffers and materials.
pub fn model_symbols(graph: &Value) -> Result<BTreeSet<&str>> {
    let nodes = graph["nodes"].as_array().context("graph nodes missing")?;
    let mut models = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    for node in nodes {
        let name = node["symbol"].as_str().context("graph symbol missing")?;
        ensure!(symbols.insert(name), "duplicate graph symbol {name}");
        if name == "model" || node["model"] == true {
            models.insert(name);
        }
    }
    for part in graph["source_parts"].as_array().into_iter().flatten() {
        let name = part["model"]
            .as_str()
            .context("source part model missing")?;
        ensure!(
            symbols.contains(name),
            "source part model {name} is missing"
        );
        models.insert(name);
    }
    Ok(models)
}

/// The importer revision a graph's manifest records, 0 when it records none.
#[must_use]
pub fn manifest_converter_revision(directory: &Path) -> u32 {
    fs::read(directory.join("asset-graph.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|graph| graph["converter_revision"].as_u64())
        .and_then(|revision| u32::try_from(revision).ok())
        .unwrap_or(0)
}

/// Writes the current [`CONVERTER_REVISION`] into a freshly converted graph's manifest, before
/// it is pinned.
pub fn record_converter_revision(directory: &Path) -> Result<()> {
    let path = directory.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&path)?)?;
    include_animation_payloads(&mut graph)?;
    graph["converter_revision"] = CONVERTER_REVISION.into();
    crate::io::write_json(&path, &graph)
}

impl GraphReference {
    /// Whether an importer fix since this graph was converted means it has to be imported again.
    #[must_use]
    #[allow(
        clippy::absurd_extreme_comparisons,
        reason = "the minimum is 0 until an importer fix makes older imports unsafe"
    )]
    pub fn needs_reimport(&self) -> bool {
        self.converter_revision < MINIMUM_CONVERTER_REVISION
    }

    /// Copy an existing model into an independent authored identity without touching its source.
    pub fn copy_model(&self, source_item: u32, target_item: u32, output: &Path) -> Result<Self> {
        self.validate(source_item)?;
        ensure!(
            ![0, u32::MAX, 0x811C9DC5].contains(&target_item),
            "Invalid target identity"
        );
        let output = crate::io::outside(output, &self.directory)?;
        ensure!(!output.exists(), "Model output already exists");
        let mut graph: Value =
            serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
        include_animation_payloads(&mut graph)?;
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
        let plan = copy_plan(&graph, &self.directory, &output)?;
        fs::create_dir_all(&output)?;
        for (source, destination) in plan {
            fs::create_dir_all(destination.parent().context("Model file parent")?)?;
            fs::copy(source, destination)?;
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
        crate::io::write_json(&output.join("asset-graph.json"), &graph)?;
        self.validate(source_item)?;
        let mut copied = Self::new(&output, target_item)?;
        copied.material_inputs = self.material_inputs.clone();
        Ok(copied)
    }

    pub fn new(directory: &Path, item: u32) -> Result<Self> {
        let directory = directory
            .canonicalize()
            .context("Open imported asset folder")?;
        let sha256 = fingerprint(&directory, Some(item))?;
        let converter_revision = manifest_converter_revision(&directory);
        let graph: Value = serde_json::from_slice(&fs::read(directory.join("asset-graph.json"))?)?;
        let material_inputs = graph
            .get("material_inputs")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?;
        Ok(Self {
            directory,
            sha256,
            attachments: Vec::new(),
            converter_revision,
            material_inputs,
        })
    }

    pub fn validate(&self, item: u32) -> Result<()> {
        self.validate_material_inputs()?;
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

    /// A prepared vehicle appearance is bound to the native runtime used for retargeting.
    pub fn validate_vehicle(&self, entity: u32) -> Result<()> {
        self.validate_reusable("sparrow")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
        ensure!(
            graph["runtime_presentation"]["installable"] == true
                && graph["runtime_presentation"]["source_entity"].as_u64()
                    == Some(u64::from(entity)),
            "Imported appearance was prepared for a different summoned vehicle. Reimport it for this vehicle"
        );
        Ok(())
    }

    /// Model gear and shaders allocate private registrations for each authored recipe.
    /// Their immutable source graph can therefore survive a rename or a duplicate.
    pub fn validate_reusable(&self, kind: &str) -> Result<()> {
        self.validate_material_inputs()?;
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
            fingerprint(&self.directory, None)? == self.sha256,
            "Imported assets changed after selection. Select the prepared graph again"
        );
        let graph: Value =
            serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
        ensure!(
            graph["kind"] == kind,
            "Imported asset kind differs from its recipe"
        );
        let mut choices = BTreeSet::new();
        for attachment in &self.attachments {
            ensure!(
                kind == "armor" && attachment.attachments.is_empty(),
                "Only armor supports reusable appearance attachments"
            );
            attachment.validate_reusable(kind)?;
            let child: Value =
                serde_json::from_slice(&fs::read(attachment.directory.join("asset-graph.json"))?)?;
            ensure!(
                child["ornament"]["target_item"] == graph["item_hash"]
                    && child["native_item"] == graph["native_item"],
                "Armor ornament belongs to another source item"
            );
            let socket = child["ornament"]["target_socket"]
                .as_u64()
                .context("Armor ornament socket")?;
            let choice = child["ornament"]["choice_index"]
                .as_u64()
                .context("Armor ornament choice")?;
            ensure!(
                choice > 0 && choices.insert((socket, choice)),
                "Duplicate armor ornament choice"
            );
        }
        Ok(())
    }

    fn validate_material_inputs(&self) -> Result<()> {
        if let Some(inputs) = &self.material_inputs {
            inputs.validate()?;
            let graph: Value =
                serde_json::from_slice(&fs::read(self.directory.join("asset-graph.json"))?)?;
            ensure!(
                graph["source"] == "marathon" && graph.get("material_inputs").is_some(),
                "Imported graph has no editable material inputs. Reimport it"
            );
        }
        Ok(())
    }
}

// New manifests name every animation payload in the established files map. This keeps
// existing saved fingerprints valid while copies and newly prepared graphs pin private
// rigs and supplemental clips as well as their state and clip banks.
fn include_animation_payloads(graph: &mut Value) -> Result<()> {
    for section in [
        "/animation/first_person",
        "/animation/character",
        "/equipment_animation",
    ] {
        let Some(animation) = graph.pointer_mut(section) else {
            continue;
        };
        if !animation.is_object() {
            continue;
        }
        let mut additions = Vec::new();
        for field in ["rigs", "extra_clips"] {
            for (index, row) in animation[field]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let file = row["file"].as_str().context("animation payload file")?;
                additions.push((format!("{field}_{index}"), Value::from(file)));
                for template in ["template_file", "original_file"] {
                    if let Some(file) = row.get(template) {
                        ensure!(file.is_string(), "Animation template file must be a path");
                        additions.push((format!("{field}_{index}_{template}"), file.clone()));
                    }
                }
            }
        }
        if let Some(layers) = animation.get("pose_layers").filter(|v| !v.is_null()) {
            for field in ["file", "template_file"] {
                let file = layers[field]
                    .as_str()
                    .context("animation pose layer file")?;
                additions.push((format!("pose_layers_{field}"), Value::from(file)));
            }
        }
        if additions.is_empty() {
            continue;
        }
        let files = animation["files"]
            .as_object_mut()
            .context("animation files")?;
        for (key, file) in additions {
            if let Some(existing) = files.get(&key) {
                ensure!(existing == &file, "Conflicting animation payload {key}");
            } else {
                if !files.values().any(|existing| existing == &file) {
                    files.insert(key, file);
                }
            }
        }
    }
    Ok(())
}

/// A payload must have a normal relative name and resolve inside the canonical graph root.
fn checked_payload(root: &Path, name: &str, role: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "{role} payload path must stay inside its graph folder"
    );
    let resolved = root
        .join(path)
        .canonicalize()
        .with_context(|| format!("Locate {role} payload {}", root.join(path).display()))?;
    ensure!(
        resolved.starts_with(root),
        "{role} payload escapes its graph folder"
    );
    ensure!(resolved.is_file(), "{role} payload is not a file");
    Ok(resolved)
}

fn animation_files(graph: &Value) -> Result<Vec<&str>> {
    let mut files = Vec::new();
    for animation in graph["animation"]["first_person"]
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
        for file in animation["files"]
            .as_object()
            .context("animation files")?
            .values()
        {
            files.push(file.as_str().context("animation file")?);
        }
        for clip in animation["clips"].as_array().context("animation clips")? {
            files.push(clip["file"].as_str().context("animation clip file")?);
        }
    }
    Ok(files)
}

/// Resolve the complete augmented graph before creating any destination directory.
fn copy_plan(graph: &Value, root: &Path, output: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut files = BTreeSet::new();
    for node in graph["nodes"].as_array().context("Missing asset nodes")? {
        files.insert(
            node["file"]
                .as_str()
                .context("Missing asset payload path")?,
        );
    }
    if let Some(file) = graph["source_icon_png"].as_str() {
        files.insert(file);
    }
    files.extend(animation_files(graph)?);
    for pointer in [
        "/audio/transcoded_media",
        "/audio/converted_banks",
        "/particles/nodes",
        "/audio_assets/nodes",
        "/projectile/nodes",
    ] {
        for node in graph
            .pointer(pointer)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            files.insert(node["file"].as_str().context("audio or particle file")?);
        }
    }
    files
        .into_iter()
        .map(|file| {
            let source = checked_payload(root, file, "Model")?;
            let destination = crate::io::resolved(&output.join(file))?;
            ensure!(
                destination.starts_with(output),
                "Model payload escapes its output folder"
            );
            Ok((source, destination))
        })
        .collect()
}

fn hash_animation(graph: &Value, root: &Path, digest: &mut Sha256) -> Result<()> {
    for name in animation_files(graph)? {
        let payload = fs::read(checked_payload(root, name, "Animation")?)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    Ok(())
}

fn fingerprint(directory: &Path, item: Option<u32>) -> Result<String> {
    let bytes = fs::read(directory.join("asset-graph.json"))?;
    let graph: Value = serde_json::from_slice(&bytes)?;
    if let Some(item) = item {
        ensure!(
            graph["item_hash"].as_u64() == Some(u64::from(item))
                || graph["ornament"]["target_weapon"].as_u64() == Some(u64::from(item))
                || graph["ornament"]["target_item"].as_u64() == Some(u64::from(item)),
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
        let payload = fs::read(checked_payload(&root, name, "Asset")?)?;
        if graph["source"] == "d1_roi" && graph.get("gear_art").is_some() {
            presentation::validate(&payload).with_context(|| format!("Asset {symbol}"))?;
        }
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
        let resolved = root.join(path).canonicalize().with_context(|| {
            format!(
                "Locate prepared audio payload {}",
                root.join(path).display()
            )
        })?;
        ensure!(
            resolved.starts_with(&root),
            "Audio payload escapes its graph folder"
        );
        let payload = fs::read(&resolved)
            .with_context(|| format!("Read prepared audio payload {}", resolved.display()))?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    for node in graph["particles"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            graph["audio_assets"]["nodes"]
                .as_array()
                .into_iter()
                .flatten(),
        )
        .chain(
            graph["projectile"]["nodes"]
                .as_array()
                .into_iter()
                .flatten(),
        )
    {
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
        ensure!(
            Path::new(icon)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "Imported icon path must stay inside its graph folder"
        );
        let path = root.join(icon).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "Imported icon escapes its graph folder"
        );
        let payload = fs::read(path)?;
        digest.update((payload.len() as u64).to_le_bytes());
        digest.update(payload);
    }
    Ok(hex::encode(digest.finalize()))
}

fn validate_gear(graph: &Value, symbols: &BTreeSet<&str>) -> Result<()> {
    ensure!(
        matches!(
            graph["kind"].as_str(),
            Some("weapon" | "armor" | "ghost_shell" | "ship" | "sparrow")
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
        let key = hash(dye, "manifest")?;
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
        let source = hash(part, "source_assignment")?;
        let key = hash(part, "key")?;
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

/// The namespace an import takes, which names its authored identity: the source family, the
/// source item's name, and its hash, which tells apart items that share a name, such as
/// `parhelion.d2_mot.zephyr-c223445e`.
pub(crate) fn import_namespace(family: &str, name: &str, source: u32) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        format!("parhelion.{family}.{source:08x}")
    } else {
        format!("parhelion.{family}.{slug}-{source:08x}")
    }
}

/// The authored hashes an import's namespace names: its item, collectible, unlock, pattern and
/// strings. Each role hashes `parhelion/<namespace>/<role>` with FNV-1, adding a nonce until the
/// hash is free, and the string roles stay above the reserved stock range.
pub(crate) fn import_identity(namespace: &str) -> Result<Value> {
    let mut values = serde_json::Map::new();
    let mut occupied = BTreeSet::from([0x2DF71B81u32, 0x304D56FE]);
    for (i, role) in [
        "item",
        "collectible",
        "unlock",
        "pattern_global",
        "name",
        "type",
        "flavor",
        "source",
        "collection_name",
        "collection_description",
        "inventory_hint",
        "collection_requirement",
    ]
    .iter()
    .enumerate()
    {
        let mut allocated = None;
        for nonce in 0..65536 {
            let key = format!(
                "parhelion/{namespace}/{role}{}",
                if nonce == 0 {
                    String::new()
                } else {
                    format!("/{nonce}")
                }
            );
            let hash = key.bytes().fold(0x811C9DC5u32, |h, b| {
                h.wrapping_mul(16777619) ^ u32::from(b)
            });
            if hash == 0
                || hash == 0x811C9DC5
                || occupied.contains(&hash)
                || (i >= 4 && hash <= 0x304D56FE)
            {
                continue;
            }
            occupied.insert(hash);
            allocated = Some(hash);
            break;
        }
        let field = if *role == "pattern_global" {
            "pattern_global_id_hash".to_owned()
        } else {
            format!("{role}_hash")
        };
        values.insert(
            field,
            json!(format!(
                "0x{:08X}",
                allocated.context("private identity exhausted")?
            )),
        );
    }
    Ok(Value::Object(values))
}

pub fn hash(p: &Value, key: &str) -> Result<u32> {
    let v = &p[key];
    let n = if let Some(s) = v.as_str() {
        u32::from_str_radix(s.trim_start_matches("0x"), 16)?
    } else {
        u32::try_from(v.as_u64().with_context(|| format!("missing {key}"))?)?
    };
    ensure!(![0, u32::MAX, 0x811C9DC5].contains(&n), "invalid {key}");
    Ok(n)
}
pub(crate) fn add(
    out: &Path,
    nodes: &mut Vec<Value>,
    symbol: &str,
    template: u32,
    data: &[u8],
    reference: Option<&str>,
    patches: Vec<Value>,
) -> Result<()> {
    let file = format!("{symbol}.bin");
    fs::write(out.join(&file), data)?;
    nodes.push(json!({"symbol":symbol,"template":template,"file":file,"reference":reference,"patches":patches}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    mod copy;

    #[test]
    fn identity_matches_existing_imported_chroma_recipe() {
        let id = import_identity("parhelion.bulk.42bdcc00").unwrap();
        assert_eq!(id["item_hash"], "0x39A0CE76");
    }

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
