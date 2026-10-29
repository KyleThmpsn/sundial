//! A weapon ornament converted the way the weapon's own model is, from the ornament plug's art
//! rows, on the weapon import's native base and rig. An ornament carries model rows, dyes and an
//! icon but no runtime, so animation, audio and gameplay stay with the weapon. Its own sight
//! part keeps its sight markers and optic, which aim the ornament's scope.
use super::{
    batch, extract, geometry, kept_parts, mapping, markers, native, profile,
    reader::{Reader, write_json},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, path::PathBuf};

pub struct Request<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    /// The weapon import's prepared base folder, holding its `profile.json` and `native` export.
    pub weapon: &'a Path,
    pub ornament: u32,
    /// The private plug identity the ornament graph becomes.
    pub target_item: u32,
    /// The `ornament` link: source weapon and socket, target weapon and socket, native plug. An
    /// ornament that becomes a mod of its own has none, since a shared plug set offers it rather
    /// than an authored weapon's socket.
    pub link: Option<Value>,
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

/// The ornament's report with its first model, in slot preference order, whose carrier's channel
/// bank adapts placed first, or `None` when no model's does. A carrier found outside the base can
/// lack a bank entirely.
fn host(
    source_path: &Path,
    native_path: &Path,
    source: &Value,
    template: &Value,
    stride: i16,
    progress: &mut dyn FnMut(String),
) -> Result<Option<Value>> {
    let count = source["models"].as_array().map_or(0, Vec::len);
    for index in 0..count {
        let mut candidate = source.clone();
        let models = candidate["models"]
            .as_array_mut()
            .context("ornament models")?;
        let model = models.remove(index);
        models.insert(0, model);
        let owner =
            mapping::primary_carrier_owner(source_path, native_path, &candidate, template, stride)?;
        match native::effects::check_channels(source_path, native_path, owner) {
            Ok(()) => return Ok(Some(candidate)),
            Err(error) => progress(format!(
                "Ornament model {} cannot host channels: {error:#}",
                candidate["models"][0]["model"]
            )),
        }
    }
    Ok(None)
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
    // Host the object channels on the first model whose carrier's channel bank adapts. When the
    // body slot's carriers have none that does, the base's models are tried in table order, as a
    // weapon import does when its body owner's bank cannot adapt.
    let mut hosted = host(
        &source_path,
        &native_path,
        &source,
        &template,
        stride,
        progress,
    )?;
    if hosted.is_none() && mapping::body_host(&template).is_some() {
        mapping::use_table_order(
            &mut template,
            "no ornament model's body slot carrier has an adaptable channel bank",
        )?;
        mapping::check_carriers(&source_path, &native_path, &source, &template, stride)?;
        hosted = host(
            &source_path,
            &native_path,
            &source,
            &template,
            stride,
            progress,
        )?;
    }
    let source = hosted.context("no ornament model's carrier has an adaptable channel bank")?;
    write_json(&source_path.join("report.json"), &source)?;
    write_json(&native_path.join("template-report.json"), &template)?;
    let mut ornament_profile = weapon_profile.clone();
    ornament_profile["source_item"] = request.ornament.into();
    ornament_profile["target_item"] = format!("0x{:08X}", request.target_item).into();
    ornament_profile["art_key"] = request.art_key.into();
    ornament_profile["dye_key_base"] = request.dye_key_base.into();
    match &request.link {
        Some(link) => ornament_profile["ornament"] = link.clone(),
        None => {
            if let Some(profile) = ornament_profile.as_object_mut() {
                profile.remove("ornament");
            }
        }
    }
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

/// The art keys and dye key bases an import takes from its 16-bit art ordinal, as the weapon's
/// own conversion does.
const ART_KEYS: u32 = 0xE2A0_0000;
const DYE_KEYS: u32 = 0xE2B0_0000;

/// What an import of a weapon's own ornaments needs.
pub struct Ornaments<'a> {
    pub modern: &'a Path,
    pub native: &'a Path,
    /// The source weapon, whose ornament socket offers the ornaments.
    pub source: u32,
    /// The native base the weapon was built on.
    pub donor: u32,
    /// Other stock weapons of the base's type in preference order, which can lend the ornament
    /// socket type a base without an ornament socket lacks.
    pub kin: &'a [u32],
    /// The weapon import's prepared base folder.
    pub weapon: &'a Path,
    /// The weapon's prepared recipe, whose ornament socket offers the converted ornaments.
    pub recipe: &'a Path,
    /// The weapon's own art ordinal, which no ornament takes.
    pub ordinal: u32,
    /// Each ornament converts into a folder here named by its hash, and the package reads that
    /// find them go in [`crate::tiger::ornament::reads`] beside it.
    pub out: &'a Path,
}

/// Convert the source weapon's own ornaments on the weapon's base and offer them in the recipe's
/// ornament socket, laid out as [`crate::tiger::ornament::layout`] lays out imported appearances.
/// An ornament that does not convert is left out with its reason and the next takes its choice.
/// The weapon keeps no ornament column when none converts, and the report says why. Only a
/// cancellation is an error.
pub fn import_all(request: &Ornaments, progress: &mut dyn FnMut(String)) -> Result<Value> {
    let mut report = json!({"omitted":[]});
    match convert_all(request, &mut report, progress) {
        Ok(true) => {}
        Ok(false) => {
            remove(request.out);
            remove(&crate::tiger::ornament::reads(request.out));
        }
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(error) => {
            report["skipped"] = json!(format!("{error:#}"));
            remove(request.out);
            remove(&crate::tiger::ornament::reads(request.out));
        }
    }
    Ok(report)
}

/// Removing unused conversions only saves space, so a failure is not an error.
fn remove(folder: &Path) {
    if folder.exists()
        && let Err(error) = fs::remove_dir_all(folder)
    {
        eprintln!(
            "Unused ornament conversion {} kept: {error}",
            folder.display()
        );
    }
}

/// Convert and bind the ornaments, returning whether any was bound.
fn convert_all(
    request: &Ornaments,
    report: &mut Value,
    progress: &mut dyn FnMut(String),
) -> Result<bool> {
    progress("Reading the weapon's ornaments…".into());
    let reads = crate::tiger::ornament::reads(request.out);
    let mut source = Reader::discovery(request.modern, &reads.join("source"), true)?;
    let (socket, ornaments) = source_ornaments(&mut source, request.source, report)?;
    drop(source);
    report["ornaments"] = json!(ornaments.len());
    if ornaments.is_empty() {
        return Ok(false);
    }
    let mut native = Reader::discovery(request.native, &reads.join("native"), false)?;
    let mut layout =
        crate::tiger::ornament::layout(&mut native, request.donor, request.kin, ornaments.len())?;
    let sockets = crate::tiger::shadowkeep::sockets(&mut native, request.donor)?;
    drop(native);
    let recipe: Value = serde_json::from_slice(&fs::read(request.recipe)?)?;
    layout.append_after(
        recipe["overrides"]["socket_columns"]
            .as_array()
            .map_or(0, Vec::len),
    )?;
    report["layout"] = layout.report.clone();
    let namespace = recipe["namespace"].as_str().context("Recipe namespace")?;
    let owner = profile::hash(&recipe["identity"], "item_hash")?;
    let mut ordinals = BTreeSet::from([request.ordinal]);
    let mut converted = Vec::new();
    let total = ornaments.len();
    for (index, (ornament, name)) in ornaments.iter().enumerate() {
        crate::cancellation::check()?;
        progress(format!(
            "Converting ornament {} of {total}: {name}…",
            index + 1
        ));
        let choice = converted.len() + 1;
        let plug = layout.plugs[converted.len()];
        let roles = crate::tiger::ornament::roles(namespace, layout.lane, choice);
        let ordinal = free_ordinal(&roles, &ordinals);
        let link = json!({"source_weapon":request.source,"source_socket":socket,
            "target_weapon":owner,"target_socket":layout.lane,"choice_index":choice,
            "native_plug":plug,"native_socket_type":layout.socket_type,
            "plug_category":layout.category,"name":name,"source_item":ornament});
        let out = request.out.join(format!("{ornament:08X}"));
        let result = prepare(
            &Request {
                modern: request.modern,
                native: request.native,
                weapon: request.weapon,
                ornament: *ornament,
                target_item: crate::tiger::ornament::fnv(&format!("{roles}/private-plug")),
                link: Some(link),
                art_key: ART_KEYS + ordinal,
                dye_key_base: DYE_KEYS + ordinal * 16,
                ordinal,
                out: &out,
            },
            progress,
        );
        match result {
            Ok(graph) => {
                ordinals.insert(ordinal);
                converted.push((plug, name, graph));
            }
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => {
                omit(report, *ornament, name, &error);
                remove(&out);
            }
        }
    }
    report["converted"] = converted
        .iter()
        .enumerate()
        .map(|(index, (_, name, _))| json!({"choice":index + 1,"name":name}))
        .collect();
    if converted.is_empty() {
        return Ok(false);
    }
    let appearances = converted
        .iter()
        .map(|(plug, name, graph)| crate::tiger::ornament::Appearance {
            plug: *plug,
            name,
            graph,
        })
        .collect::<Vec<_>>();
    crate::tiger::ornament::attach(request.recipe, &layout, &sockets, &appearances)?;
    Ok(true)
}

fn omit(report: &mut Value, ornament: u32, name: &str, error: &anyhow::Error) {
    if let Some(omitted) = report["omitted"].as_array_mut() {
        omitted.push(json!({"ornament":format!("{ornament:08X}"),"name":name,
            "reason":format!("{error:#}")}));
    }
}

/// The source weapon's ornament socket, the one that offers Default Ornament, and the named
/// appearance plugs it offers in source order. A plug whose name does not read is left out.
fn source_ornaments(
    r: &mut Reader,
    weapon: u32,
    report: &mut Value,
) -> Result<(usize, Vec<(u32, String)>)> {
    let found = super::ornaments::gameplay_sockets(r, weapon)?;
    let reset = u64::from(crate::tiger::ornament::RESET);
    let sockets = found["sockets"].as_array().context("Source sockets")?;
    let lanes = sockets
        .iter()
        .filter(|socket| {
            socket["choices"]
                .as_array()
                .is_some_and(|choices| choices.iter().any(|choice| choice["hash"] == reset))
        })
        .collect::<Vec<_>>();
    let socket = match lanes[..] {
        [] => return Ok((0, Vec::new())),
        [socket] => socket,
        _ => anyhow::bail!("The source weapon has several ornament sockets"),
    };
    let index = socket["index"].as_u64().context("Source socket index")?;
    let mut plugs = socket["choices"]
        .as_array()
        .context("Source socket choices")?
        .iter()
        .filter(|choice| {
            choice["hash"] != reset
                && choice["art_indices"]
                    .as_array()
                    .is_some_and(|art| !art.is_empty())
        })
        .collect::<Vec<_>>();
    plugs.sort_by_key(|choice| {
        (
            choice["curated"].as_u64().is_none(),
            choice["curated"].as_u64(),
            choice["hash"].as_u64(),
        )
    });
    let mut ornaments = Vec::new();
    for plug in plugs {
        let hash = profile::hash(plug, "hash")?;
        let item = plug["index"].as_u64().context("Ornament item index")?;
        match super::localization::item_name(r, hash, usize::try_from(item)?, 0) {
            Ok(label) => match label["name"].as_str().map(str::trim) {
                Some(name) if !name.is_empty() => ornaments.push((hash, name.to_owned())),
                _ => omit(
                    report,
                    hash,
                    "",
                    &anyhow::anyhow!("The ornament has no name"),
                ),
            },
            Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
            Err(error) => omit(report, hash, "", &error),
        }
    }
    Ok((usize::try_from(index)?, ornaments))
}

/// A 16-bit art ordinal for the ornament choice `roles` names, derived from its roles and taken
/// by no other ornament of the import or by the weapon.
fn free_ordinal(roles: &str, taken: &BTreeSet<u32>) -> u32 {
    let mut ordinal = crate::tiger::ornament::fnv(&format!("{roles}/ordinal")) & 0xFFFF;
    while taken.contains(&ordinal) {
        ordinal = (ordinal + 1) & 0xFFFF;
    }
    ordinal
}
