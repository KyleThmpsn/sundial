//! A weapon ornament converted the way the weapon's own model is, from the ornament plug's art
//! rows, on the weapon import's native base and rig. An ornament carries model rows, dyes and an
//! icon but no runtime, so animation, audio and gameplay stay with the weapon. Its own sight
//! part keeps its sight markers and optic, which aim the ornament's scope.
use super::{
    batch, extract, geometry, kept_parts, mapping, markers, native, profile,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fs, path::Path, path::PathBuf};

pub struct Request<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    /// The weapon import's prepared base folder, holding its `profile.json` and `native` export.
    pub weapon: &'a Path,
    pub ornament: u32,
    /// The private plug identity the ornament graph becomes.
    pub target_item: u32,
    /// The `ornament` link: source weapon and socket, target weapon and socket, native plug.
    pub link: Value,
    pub art_key: u32,
    pub dye_key_base: u32,
    /// Keys for the kept sight and barrel parts, distinct from the weapon's own.
    pub ordinal: u32,
    pub out: &'a Path,
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| path.display().to_string())?)
        .with_context(|| path.display().to_string())
}

fn selectors(model: &Value) -> Vec<u64> {
    model["art_parts"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|part| part["placements"].as_array().into_iter().flatten())
        .filter_map(|placement| placement["selector"].as_u64())
        .collect()
}

/// The first source model is the primary one, whose carrier's channel bank hosts the object
/// channels. Put the ornament's model in the weapon's primary art slot first, so it hosts on
/// the carrier the weapon import already proved, rather than on whatever its art lists first.
fn primary_slot_first(weapon_profile: &Value, ornament: &mut Value) -> Result<()> {
    let Some(weapon_source) = weapon_profile["rig"]["source"].as_str() else {
        return Ok(());
    };
    let weapon = load(&Path::new(weapon_source).join("report.json"))?;
    let Some(slot) = weapon["models"]
        .as_array()
        .and_then(|models| models.first())
        .and_then(|model| selectors(model).first().copied())
    else {
        return Ok(());
    };
    let models = ornament["models"]
        .as_array_mut()
        .context("ornament models")?;
    if let Some(index) = models
        .iter()
        .position(|model| selectors(model).contains(&slot))
    {
        let model = models.remove(index);
        models.insert(0, model);
    }
    Ok(())
}

/// Convert the ornament and return its graph folder.
pub fn prepare(request: &Request, progress: &mut dyn FnMut(String)) -> Result<PathBuf> {
    let out = request.out;
    ensure!(!out.exists(), "ornament output already exists");
    fs::create_dir_all(out)?;
    let weapon_profile = load(&request.weapon.join("profile.json"))?;
    // Work on a copy so carriers added for the ornament never change the weapon's export.
    let native_path = out.join("native");
    copy_tree(&request.weapon.join("native"), &native_path)?;
    let source_path = out.join("source");
    progress("Extracting the ornament's models and textures…".into());
    let mut r = Reader::new(request.modern, &source_path, true)?;
    let mut source = extract::extract_with_progress(
        &mut r,
        request.ornament,
        None,
        geometry::Detail::default(),
        progress,
    )?;
    primary_slot_first(&weapon_profile, &mut source)?;
    write_json(&source_path.join("report.json"), &source)?;
    r.finish()?;
    drop(r);
    let mut template = load(&native_path.join("template-report.json"))?;
    let stride = if weapon_profile.get("rig").is_some() {
        24
    } else {
        20
    };
    progress("Checking the ornament's model parts against the base's materials…".into());
    if let Err(error) =
        mapping::check_carriers(&source_path, &native_path, &source, &template, stride)
    {
        // The carrier reader's manifest would list only its own reads. Keep the base's tags in
        // it, since channel banks and runtime components are looked up through it.
        let manifest_path = native_path.join("source-manifest.json");
        let base_manifest = load(&manifest_path)?;
        let mut r = Reader::new(request.native, &native_path, false)?;
        batch::extend_carriers(
            &source_path,
            request.native,
            &mut r,
            &source,
            stride,
            &mut template,
            progress,
        )
        .with_context(|| format!("{error:#}"))?;
        r.finish()?;
        drop(r);
        let mut manifest = load(&manifest_path)?;
        for (tag, entry) in base_manifest["tags"]
            .as_object()
            .context("base manifest tags")?
        {
            if manifest["tags"].get(tag).is_none() {
                manifest["tags"][tag] = entry.clone();
            }
        }
        write_json(&manifest_path, &manifest)?;
        mapping::check_carriers(&source_path, &native_path, &source, &template, stride)?;
    }
    // Host the object channels on the first model, in slot preference order, whose carrier's
    // channel bank adapts. A carrier found outside the base can lack a bank entirely.
    let mut hosted = None;
    let count = source["models"].as_array().map_or(0, Vec::len);
    for index in 0..count {
        let mut candidate = source.clone();
        let models = candidate["models"]
            .as_array_mut()
            .context("ornament models")?;
        let model = models.remove(index);
        models.insert(0, model);
        let owner = mapping::primary_carrier_owner(
            &source_path,
            &native_path,
            &candidate,
            &template,
            stride,
        )?;
        match native::effects::check_channels(&source_path, &native_path, owner) {
            Ok(()) => {
                hosted = Some(candidate);
                break;
            }
            Err(error) => {
                progress(format!(
                    "Ornament model {} cannot host channels: {error:#}",
                    candidate["models"][0]["model"]
                ));
            }
        }
    }
    let source = hosted.context("no ornament model's carrier has an adaptable channel bank")?;
    write_json(&source_path.join("report.json"), &source)?;
    write_json(&native_path.join("template-report.json"), &template)?;
    let mut ornament_profile = weapon_profile.clone();
    ornament_profile["source_item"] = request.ornament.into();
    ornament_profile["target_item"] = format!("0x{:08X}", request.target_item).into();
    ornament_profile["art_key"] = request.art_key.into();
    ornament_profile["dye_key_base"] = request.dye_key_base.into();
    ornament_profile["ornament"] = request.link.clone();
    if ornament_profile.get("rig").is_some() {
        // The rig's bones come from the weapon. Its skinned geometry is the ornament's.
        ornament_profile["rig"]["native"] = native_path.display().to_string().into();
        ornament_profile["rig"]["source_geometry"] = source_path.display().to_string().into();
        let weapon_source = weapon_profile["rig"]["source"]
            .as_str()
            .context("weapon rig export")?;
        ornament_profile["rig"]["rig_item_tag"] =
            load(&Path::new(weapon_source).join("report.json"))?["item_tag"].clone();
    }
    write_json(&out.join("profile.json"), &ornament_profile)?;
    let prepared = out.join("prepared");
    profile::prepare_reusing(
        &out.join("profile.json"),
        request.modern,
        request.native,
        &prepared,
        Some(profile::Extracted {
            source: &source_path,
            native: &native_path,
        }),
        progress,
    )?;
    let assembled = native::automatic::build(
        &prepared,
        &source_path,
        &native_path,
        &out.join("assembled"),
        progress,
    )?;
    let graph_dir = PathBuf::from(assembled["graph"].as_str().context("ornament graph")?);
    let graph_path = graph_dir.join("asset-graph.json");
    let mut graph = load(&graph_path)?;
    progress("Keeping the ornament's sight and barrel markers…".into());
    let mut kept = Reader::new(request.native, &out.join("kept"), false)?;
    kept_parts::build(
        &mut kept,
        &source_path,
        &template,
        &graph_dir,
        &mut graph,
        request.ordinal,
    )
    .context("ornament kept parts")?;
    markers::author(
        &mut kept,
        &source_path,
        &template,
        &graph_dir,
        &mut graph,
        super::artwork::native_selector,
    )
    .context("ornament part markers")?;
    kept.finish()?;
    // A pinned graph holds its own files, so the inventory icon moves inside it.
    if let Some(icon) = graph["ornament_icon_png"].as_str().map(PathBuf::from)
        && icon.is_absolute()
    {
        fs::copy(&icon, graph_dir.join("ornament-icon.png"))?;
        graph["ornament_icon_png"] = "ornament-icon.png".into();
    }
    write_json(&graph_path, &graph)?;
    crate::graph::record_converter_revision(&graph_dir)?;
    Ok(graph_dir)
}
