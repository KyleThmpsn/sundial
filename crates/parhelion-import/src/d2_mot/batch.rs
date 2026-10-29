//! Offline preparation with explicit source items and verified animation donors.
mod rig_selection;
use crate::d2_mot::{
    extract, profile,
    reader::{Reader, outside, write_json},
    rig, rig_convert, shadowkeep,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The namespace an import of the source item `source`, named `name`, takes.
pub(crate) fn namespace(source: u32, name: &str) -> String {
    crate::graph::import_namespace("d2_mot", name, source)
}

pub(crate) fn prepare_one(
    row: &Value,
    ordinal: u32,
    modern: &Path,
    native: &Path,
    out: &Path,
) -> Result<Value> {
    prepare_one_reusing(row, ordinal, modern, native, out, None, None, &mut |_| {})
}

/// Shared location for searched carriers, keyed by the native package set so
/// a rebuilt or updated install starts a fresh index.
fn carrier_cache(native: &Path) -> Result<Option<crate::cache::Generation>> {
    let stamp = super::service::package_stamp(native)?;
    Ok(crate::cache::importer_cache("d2_mot", "native-carriers")
        .and_then(|root| crate::cache::Generation::file(&root, &stamp)))
}

/// Add rendering templates from elsewhere in the packages for source models
/// the donor cannot carry. Only draw records and material shells come from
/// these models, so the gameplay item and animation donor stay as chosen.
pub(crate) fn extend_carriers(
    source_path: &Path,
    native: &Path,
    reader: &mut Reader,
    source: &Value,
    stride: i16,
    template: &mut Value,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    let native_path = reader.output.clone();
    let missing = super::mapping::uncarried(source_path, &native_path, source, template, stride)?;
    ensure!(!missing.is_empty(), "no source model needs a new carrier");
    // A carrier depends on the source material contracts and the native
    // packages, never on which weapon asked, so the answer is remembered once
    // per package set and shared by every import rather than per weapon.
    let remembered = carrier_cache(native)?;
    let lock = remembered
        .as_ref()
        .map(crate::cache::Generation::writer)
        .transpose()?
        .flatten();
    let remembered = remembered.filter(|_| lock.is_some());
    let mut found: Value = remembered
        .as_ref()
        .and_then(|generation| fs::read(generation.path()).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let mut added = template["carrier_models"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for (tag, mesh) in missing {
        let model = super::mapping::source_model(source_path, tag)?;
        let key = super::mapping::contract_signature(source_path, &model, mesh, (true, stride))?;
        // Retry legacy negative observations rather than making transient failures permanent.
        if found
            .get(&key)
            .and_then(Value::as_str)
            .and_then(|tag| u32::from_str_radix(tag, 16).ok())
            .is_none()
        {
            progress("Searching packages for a compatible rendering template…".into());
            let carrier =
                super::mapping::search_carrier(reader, source_path, &model, mesh, (true, stride))
                    .with_context(|| format!("source model {tag:08X}"))?;
            crate::cancellation::check()?;
            found[&key] = json!(format!("{carrier:08X}"));
            if let Some(generation) = &remembered {
                let _ = crate::cache::write_json(generation.path(), &found);
            }
        }
        let carrier = found[&key]
            .as_str()
            .with_context(|| format!("source model {tag:08X}: {}", found[&key]["unavailable"]))?;
        eprintln!("Rendering template {carrier} carries source model {tag:08X}");
        let mut entry = shadowkeep::carrier_model(reader, u32::from_str_radix(carrier, 16)?)?;
        // A rendering template has no runtime graph of its own. Its draws run
        // inside the donor's owner and entity, which still supply animation
        // and residency, so package emission keeps using those. The body slot
        // hosts the assembled import, so its model is the host when known.
        let host = super::mapping::host_model(template)
            .context("native donor model with a runtime graph")?;
        entry["owner"] = host["owner"].clone();
        entry["entity"] = host["entity"].clone();
        added.push(entry);
    }
    // Keep the donor and added carriers in one export. A fresh reader here
    // would overwrite the manifest with only the carrier's newly read tags,
    // hiding the donor's still-present channel banks and runtime components.
    reader.finish()?;
    template["carrier_models"] = json!(added);
    write_json(&native_path.join("template-report.json"), template)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_one_reusing(
    row: &Value,
    ordinal: u32,
    modern: &Path,
    native: &Path,
    out: &Path,
    source_cache: Option<&Path>,
    shared_native: Option<&Reader>,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let hash = profile::hash(row, "hash")?;
    let folder = out.join(format!("{hash:08X}"));
    prepare_in(
        row,
        ordinal,
        (modern, native),
        (out, &folder),
        source_cache,
        shared_native,
        progress,
    )
}

/// [`prepare_one_reusing`] with the weapon's working folder named by the caller instead of by
/// its hash, for an import that has a conversion folder of its own. The recipe still goes in
/// `out`'s `recipes` folder.
pub(crate) fn prepare_in(
    row: &Value,
    ordinal: u32,
    (modern, native): (&Path, &Path),
    (out, folder): (&Path, &Path),
    source_cache: Option<&Path>,
    shared_native: Option<&Reader>,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let hash = profile::hash(row, "hash")?;
    let donor = profile::hash(row, "native_item")?;
    fs::create_dir(folder)?;
    let source_path = source_cache.map_or_else(|| folder.join("source"), Path::to_owned);
    let (source, source_rig) = if source_cache.is_some() {
        (
            serde_json::from_slice::<Value>(&fs::read(source_path.join("report.json"))?)?,
            serde_json::from_slice::<Value>(&fs::read(source_path.join("rig.json"))?)?,
        )
    } else {
        progress("Extracting source models and textures…".into());
        let mut r = Reader::new(modern, &source_path, true)?;
        let source = extract::extract_with_progress(
            &mut r,
            hash,
            None,
            super::geometry::Detail::parse(row["detail"].as_str())?,
            progress,
        )?;
        write_json(&source_path.join("report.json"), &source)?;
        let source_rig =
            rig::inspect_with_audio(&mut r, profile::hash(&source, "item_tag")?, true)?;
        super::hud::export(&mut r, &source, &source_rig)?;
        r.finish()?;
        (source, source_rig)
    };
    // A row without a name, such as an ornament's stock weapon, names its weapon by hash alone.
    ensure!(
        source["item_hash"].as_u64() == Some(u64::from(hash))
            && (row["name"].is_null() || source["name"] == row["name"]),
        "source weapon identity differs"
    );
    let glaive = super::glaive::exported_glaive(&source_path)?;
    progress("Extracting donor models and materials…".into());
    let native_path = folder.join("native");
    let mut r = if let Some(reader) = shared_native {
        reader.shared_export(&native_path)?
    } else {
        Reader::new(native, &native_path, false)?
    };
    let template = shadowkeep::extract(&mut r, donor)?;
    write_json(&native_path.join("template-report.json"), &template)?;
    let native_rig = rig::inspect_with_audio(&mut r, profile::hash(&template, "item_tag")?, false)?;
    r.finish()?;
    let (source_rig_choice, rig_selection) = if row["source_owned_rig"] == false && !glaive {
        (
            None,
            json!({"mode":"native","reason":"native rig requested"}),
        )
    } else {
        progress("Converting source skeletons, controls and animation…".into());
        match rig_selection::Source::prepare(
            row,
            donor,
            glaive,
            modern,
            native,
            &source_path,
            &native_path,
            &source_rig,
            &native_rig,
            folder,
        ) {
            Ok(prepared) => (Some(prepared), json!({"mode":"source"})),
            Err(error) => {
                crate::cancellation::check()?;
                ensure!(
                    row["source_owned_rig"] != true && !glaive,
                    "required source rig: {error:#}"
                );
                eprintln!("Source rig cannot be carried by this native format: {error:#}");
                progress("Checking a compatible native rig fallback…".into());
                (None, json!({"mode":"native","reason":format!("{error:#}")}))
            }
        }
    };
    let source_owned = source_rig_choice.is_some();
    let matches = matching_rigs(
        source_rig_choice.as_ref(),
        &source_rig,
        &native_rig,
        &source_path,
        &native_path,
        &source,
        &template,
    )?;
    let (_, rig, mapping) = matches.first().with_context(|| {
        format!(
            "native donor lacks a matching source bone hierarchy, source rig unavailable: {}",
            rig_selection["reason"]
        )
    })?;
    // A reimport names the saved recipe's namespace, so the item keeps its identity.
    let namespace = row["namespace"].as_str().map_or_else(
        || namespace(hash, row["name"].as_str().unwrap_or_default()),
        str::to_owned,
    );
    let id = crate::graph::import_identity(&namespace)?;
    let use_rig = source_owned || !(rigid_root(mapping) && has_rigid_carrier(&template)?);
    let mut profile = json!({"source_item":hash,"native_item":donor,"target_item":id["item_hash"],"art_key":0xE2A00000u32.checked_add(ordinal).context("art key overflow")?,"dye_key_base":0xE2B00000u32.checked_add(ordinal.checked_mul(16).context("dye key overflow")?).context("dye key overflow")?,"model_index":0,"plated":true,"convert_dyes":true});
    if use_rig {
        profile["rig"] = rig.clone();
    }
    // An ornament converts on its weapon's base: this profile, the rig match and the native
    // export beside it. Its own models find their carriers and channel host, so the checks of
    // the weapon's own models below do not apply to it.
    if row["base_only"] == true {
        write_json(&folder.join("profile.json"), &profile)?;
        return Ok(json!({"hash":hash,"native_item":donor,"folder":folder,"status":"base"}));
    }
    progress("Checking all model parts against donor materials…".into());
    let stride = if use_rig { 24 } else { 20 };
    let mut template = template;
    if let Err(error) =
        super::mapping::check_carriers(&source_path, &native_path, &source, &template, stride)
    {
        extend_carriers(
            &source_path,
            native,
            &mut r,
            &source,
            stride,
            &mut template,
            progress,
        )
        .with_context(|| format!("{error:#}"))?;
        super::mapping::check_carriers(&source_path, &native_path, &source, &template, stride)?;
    }
    drop(r);
    progress("Checking donor material controls…".into());
    let owner = super::mapping::primary_carrier_owner(
        &source_path,
        &native_path,
        &source,
        &template,
        stride,
    )?;
    if let Err(error) = super::native::effects::check_channels(&source_path, &native_path, owner) {
        // The body slot owner is preferred as host, but its channel bank must be
        // adaptable. Otherwise the carrier and host revert to assets-table order.
        ensure!(
            super::mapping::body_host(&template).is_some(),
            "source channel adaptation: {error:#}"
        );
        eprintln!("Body slot owner {owner:08X} channels are not adaptable: {error:#}");
        super::mapping::use_table_order(&mut template, &format!("{error:#}"))?;
        write_json(&native_path.join("template-report.json"), &template)?;
        super::mapping::check_carriers(&source_path, &native_path, &source, &template, stride)?;
        let owner = super::mapping::primary_carrier_owner(
            &source_path,
            &native_path,
            &source,
            &template,
            stride,
        )?;
        super::native::effects::check_channels(&source_path, &native_path, owner)
            .context("source channel adaptation")?;
    }
    write_json(&folder.join("profile.json"), &profile)?;
    let prepared = folder.join("prepared");
    let mut result = profile::prepare_reusing(
        &folder.join("profile.json"),
        modern,
        native,
        &prepared,
        Some(profile::Extracted {
            source: &source_path,
            native: &native_path,
        }),
        progress,
    )?;
    let assembled = super::native::automatic::build(
        &prepared,
        &source_path,
        &native_path,
        &folder.join("assembled"),
        progress,
    )?;
    result["graph"] = assembled["graph"].clone();
    progress("Linking source first-person animation clips…".into());
    let graph_dir = PathBuf::from(assembled["graph"].as_str().context("graph path")?);
    let mut animation = if let Some(prepared) = source_rig_choice {
        prepared.publish(&graph_dir)?
    } else {
        match rig_convert::animation::first_person::prepare_with_rig(
            modern,
            native,
            &source_rig,
            &native_rig,
            &folder.join("animation"),
            &graph_dir,
            None,
        ) {
            Ok(section) => section,
            // A failed link keeps the donor's native animation rather than the import.
            Err(error) => {
                crate::cancellation::check()?;
                eprintln!("First-person animation stays native: {error:#}");
                json!({"first_person_status":"native","reason":format!("{error:#}"),"gameplay_verified":false})
            }
        }
    };
    animation["rig_selection"] = rig_selection;
    let graph_path = graph_dir.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
    graph["pattern_global_id_hash"] = id["pattern_global_id_hash"].clone();
    graph["animation"] = animation.clone();
    let audio = super::audio::prepare(modern, native, &graph_dir, &source_rig, &native_rig);
    graph["audio"] = audio.clone();
    progress("Placing source sight and barrel markers…".into());
    let mut kept = Reader::new(native, &folder.join("kept"), false)?;
    super::kept_parts::build(
        &mut kept,
        &source_path,
        &template,
        &graph_dir,
        &mut graph,
        ordinal,
    )
    .context("kept donor parts")?;
    super::markers::author(
        &mut kept,
        &source_path,
        &template,
        &graph_dir,
        &mut graph,
        super::artwork::native_selector,
    )
    .context("source part markers")?;
    kept.finish()?;
    write_json(&graph_path, &graph)?;
    result["animation"] = animation;
    result["audio"] = audio;
    progress("Saving weapon recipe and source lore…".into());
    let icon = STANDARD.encode(fs::read(source_path.join("item-icon.png"))?);
    let hud = STANDARD.encode(fs::read(source_path.join("hud-icon.png"))?);
    let damage = row["damage"]
        .as_str()
        .filter(|v| ["kinetic", "arc", "solar", "void"].contains(v));
    // Unsupported source elements retain the native gameplay donor. Their
    // original element remains in the source plan and extraction metadata.
    let mut recipe = json!({"schema":1,"collection_placement":"sunrise_badge","namespace":namespace,"donor":{"item_hash":format!("0x{donor:08X}"),"expected_name":row["native_donor"]},"identity":id,"name":row["name"],"flavor":source["flavor"].as_str().unwrap_or(""),"source":"Source: Pulled through the timeline.","overrides":{"icon_edit":{"imported_image":{"png_base64":icon}},"modern_damage_type":damage}});
    recipe["overrides"]["hud_icon"] = json!({"png_base64":hud});
    if let Some(lore) = source["lore"]["text"].as_str() {
        recipe["overrides"]["lore"] = json!(lore);
    } else {
        recipe["overrides"]["remove_lore"] = json!(true);
    }
    if source_cache.is_none() {
        let gameplay: Value =
            serde_json::from_slice(&fs::read(source_path.join("gameplay.json"))?)?;
        super::gameplay::apply(&gameplay, native, &folder.join("gameplay"), &mut recipe)?;
        if super::glaive::is_glaive(&gameplay) {
            progress("Adding the glaive shield and intrinsic…".into());
            let inputs = super::glaive::Inputs {
                modern,
                native,
                prepared: &prepared,
                graph: &graph_dir,
                work: &folder.join("glaive"),
                source_rig: &source_rig,
                native_rig: &native_rig,
            };
            result["glaive"] = super::glaive::apply(&inputs, &gameplay, &mut recipe, progress)?;
        }
        progress("Converting the hip-fire crosshair…".into());
        result["crosshair"] = match super::crosshair::apply(
            modern,
            native,
            &source_path,
            &folder.join("crosshair"),
            &graph_dir,
        ) {
            Ok(record) => record,
            Err(error) => {
                crate::cancellation::check()?;
                super::crosshair::record_failure(&graph_dir, &error)?;
                json!({"status":"unconverted","reason":format!("{error:#}")})
            }
        };
    }
    let recipe_path = out.join(format!("recipes/{hash:08X}.parhelion.json"));
    write_json(&recipe_path, &recipe)?;
    Ok(
        json!({"name":row["name"],"hash":hash,"native_donor":row["native_donor"],"native_item":donor,"folder":folder,"prepared":prepared,"recipe":recipe_path,"graph":result["graph"],"glaive":result["glaive"],"crosshair":result["crosshair"],"rendering_templates":template["carrier_models"],"status":"prepared","gameplay_verified":false}),
    )
}

fn matching_rigs(
    source_rig_choice: Option<&rig_selection::Source>,
    source_rig: &Value,
    native_rig: &Value,
    source_path: &Path,
    native_path: &Path,
    source: &Value,
    template: &Value,
) -> Result<Vec<(u64, Value, Value)>> {
    let source_owned = source_rig_choice.is_some();
    let from = source_rig["skeletons"]
        .as_array()
        .context("source skeletons")?
        .iter()
        .min_by_key(|s| s["bones"].as_array().map_or(usize::MAX, Vec::len))
        .context("source weapon skeleton")?;
    let mut matches = Vec::new();
    if let Some(prepared) = &source_rig_choice {
        matches.push((
            prepared.mapping["native_bone_count"]
                .as_u64()
                .context("source bone count")?,
            prepared.config.clone(),
            prepared.mapping.clone(),
        ));
    } else {
        for to in native_rig["skeletons"]
            .as_array()
            .context("native skeletons")?
        {
            let config = json!({"source":source_path,"native":native_path,"source_geometry":source_path,"source_owner":from["owner"],"native_owner":to["owner"],"source_owned":source_owned});
            if let Ok(mapping) =
                rig_convert::compatible_map(&config, &source["item_tag"], &template["item_tag"])
            {
                matches.push((
                    mapping["native_bone_count"]
                        .as_u64()
                        .context("native bone count")?,
                    config,
                    mapping,
                ));
            }
        }
    }
    matches.sort_by_key(|(count, _, _)| *count);
    Ok(matches)
}

fn rigid_root(mapping: &Value) -> bool {
    mapping["required_source_bones"] == json!([0]) && mapping["bone_map"][0] == 0
}

fn has_rigid_carrier(template: &Value) -> Result<bool> {
    for model in template["models"].as_array().context("native models")? {
        for mesh in model["meshes"].as_array().context("native meshes")? {
            let mut position = false;
            let mut attributes = false;
            for buffer in mesh["buffers"].as_array().context("native buffers")? {
                let bytes = super::payload::Payload(hex::decode(
                    buffer["bytes"].as_str().context("native buffer header")?,
                )?);
                if bytes.i16(6)? == 0 {
                    position |= buffer["offset"] == 0 && bytes.i16(4)? == 8;
                    attributes |= buffer["offset"] == 4 && bytes.i16(4)? == 20;
                }
            }
            if position && attributes {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn prepare(plan: &Path, modern: &Path, native: &Path, out: &Path) -> Result<Value> {
    let plan: Value = serde_json::from_slice(&fs::read(plan)?)?;
    let rows = plan["items"].as_array().context("batch items")?;
    ensure!(
        !rows.is_empty() && rows.len() <= 256,
        "batch requires 1..256 items"
    );
    let out = outside(
        &outside(out, modern.parent().context("modern parent")?)?,
        native.parent().context("native parent")?,
    )?;
    ensure!(!out.exists(), "batch output already exists");
    let mut seen = BTreeSet::new();
    for row in rows {
        ensure!(
            seen.insert(profile::hash(row, "hash")?),
            "duplicate source item"
        );
        profile::hash(row, "native_item")?;
        ensure!(
            row["name"].is_string() && row["native_donor"].is_string(),
            "source and donor names required"
        );
        ensure!(
            ["kinetic", "arc", "solar", "void", "stasis", "strand"]
                .contains(&row["damage"].as_str().context("source damage")?),
            "invalid damage type"
        );
    }
    fs::create_dir_all(out.join("recipes"))?;
    write_json(&out.join("plan.json"), &plan)?;
    let mut results = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        eprintln!("Preparing {}/{} {}", i + 1, rows.len(), row["name"]);
        let ordinal = row
            .get("ordinal")
            .map(|v| v.as_u64().context("batch ordinal"))
            .transpose()?
            .unwrap_or(i as u64);
        let result = match prepare_one(row, u32::try_from(ordinal)?, modern, native, &out) {
            Ok(value) => value,
            Err(error) => {
                json!({"name":row["name"],"hash":row["hash"],"native_donor":row["native_donor"],"status":"unsupported","reason":format!("{error:#}")})
            }
        };
        eprintln!(
            "{}: {} {}",
            row["name"],
            result["status"],
            result.get("reason").unwrap_or(&Value::Null)
        );
        results.push(result);
        write_json(&out.join("results.json"), &json!(results))?;
    }
    Ok(json!({"results":results,"installed":false,"implementation":"Rust"}))
}

pub fn donors(
    source: &Path,
    catalog: &Path,
    kind: &str,
    native: &Path,
    out: &Path,
) -> Result<Value> {
    donors_with_progress(source, catalog, kind, native, out, &mut |_| {})
}

/// The format of the cached native rig reports. A change starts a new generation beside the old
/// ones, which retention then removes.
const NATIVE_RIG_SCHEMA: u32 = 1;

fn native_rig_cache(native: &Path) -> Option<crate::cache::Generation> {
    let stamp = super::service::package_stamp(native).ok()?;
    // Earlier builds kept a folder per format.
    let _ = fs::remove_dir_all(crate::cache::importer_folder()?.join("cache/native-rigs-v1"));
    crate::cache::Generation::directory(
        &crate::cache::importer_cache("d2_mot", "native-rigs")?,
        &format!("{NATIVE_RIG_SCHEMA}-{stamp}"),
    )
}

fn cached_rig(path: &Path, item: u32) -> Option<Value> {
    let report: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (report["item_tag"] == format!("{item:08X}") && report["skeletons"].is_array())
        .then_some(report)
}

fn native_rig(reader: &mut Reader, item: u32, cache: Option<&Path>) -> Result<Value> {
    let path = cache.map(|root| root.join(format!("{item:08X}.json")));
    if let Some(report) = path.as_deref().and_then(|path| cached_rig(path, item)) {
        return Ok(report);
    }
    let report = rig::inspect(reader, item, false)?;
    if let Some(path) = path {
        // Concurrent imports may discover the same rig. Publish only complete JSON.
        let _ = crate::cache::write_json(&path, &report);
    }
    Ok(report)
}

pub fn donors_with_progress(
    source: &Path,
    catalog: &Path,
    kind: &str,
    native: &Path,
    out: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    progress("Opening destination packages for donor matching…".into());
    let mut reader = Reader::discovery(native, out, false)?;
    donors_with_reader(source, catalog, kind, native, out, &mut reader, progress)
}

pub(crate) fn donors_with_reader(
    source: &Path,
    catalog: &Path,
    kind: &str,
    native: &Path,
    out: &Path,
    r: &mut Reader,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let catalog: Value = serde_json::from_slice(&fs::read(catalog)?)?;
    let gameplay = source
        .join("gameplay.json")
        .exists()
        .then(|| -> Result<Value> {
            Ok(serde_json::from_slice(&fs::read(
                source.join("gameplay.json"),
            )?)?)
        })
        .transpose()?;
    let source_rig: Value = serde_json::from_slice(&fs::read(source.join("rig.json"))?)?;
    let from = source_rig["skeletons"]
        .as_array()
        .context("source skeletons")?
        .iter()
        .min_by_key(|s| s["bones"].as_array().map_or(usize::MAX, Vec::len))
        .context("source weapon skeleton")?;
    let required_bones = rig_convert::used_bones(source, &source_rig["item_tag"])?;
    let rig_cache = native_rig_cache(native);
    let globals_tag = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(globals_tag, None)?;
    let root = r.tag(globals.u32(16)?, Some(0x80807D84))?;
    let items = r.tag(root.u32(8 + 48 * 16)?, None)?;
    let table = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .map(|a| Ok((items.u32(a)?, items.u32(a + 16)?)))
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let mut matches = Vec::new();
    let mut rejected = Vec::new();
    let candidates = catalog["weapons"]
        .as_array()
        .context("weapon catalog")?
        .iter()
        .filter(|v| donor_candidate(v, kind, None))
        .collect::<Vec<_>>();
    for (index, row) in candidates.iter().enumerate() {
        progress(format!(
            "Checking donor {} of {}: {}",
            index + 1,
            candidates.len(),
            row["name"].as_str().unwrap_or("Weapon")
        ));
        let hash = profile::hash(row, "hash")?;
        let result = (|| -> Result<()> {
            let item = *table.get(&hash).context("native catalog item missing")?;
            if let Some(gameplay) = &gameplay {
                super::gameplay::check_donor(gameplay, r.tag(item, Some(0x80807BEA))?.as_ref())?;
            }
            let rarity = r.tag(item, Some(0x80807BEA))?.u8(0xBA)?;
            let exotic = rarity == 5;
            let report = native_rig(
                r,
                item,
                rig_cache.as_ref().map(crate::cache::Generation::path),
            )?;
            let folder = out.join(format!("{hash:08X}"));
            fs::create_dir_all(&folder)?;
            write_json(&folder.join("rig.json"), &report)?;
            let mut reasons = Vec::new();
            let first_match = matches.len();
            for to in report["skeletons"].as_array().context("native skeletons")? {
                let config = json!({"source":source,"source_geometry":source,"native":folder,"source_owner":from["owner"],"native_owner":to["owner"]});
                match rig_convert::compatible_map_loaded(
                    &config,
                    &source_rig["item_tag"],
                    &report["item_tag"],
                    &source_rig,
                    &report,
                    Some(&required_bones),
                ) {
                    Ok(mapping) => {
                        matches.push(
                            json!({"name":row["name"],"hash":hash,"exotic":exotic,"rarity":rarity,"bucket":row["bucket_hash"],"content_key":report["content_key"],"collection_backed":row["collection_backed"],"unrelated_collection_condition":row["unrelated_collection_condition"],"rig":config,"mapping":mapping}),
                        );
                        eprintln!("Matched native rig {}", row["name"]);
                    }
                    // Recording why each skeleton was turned down keeps an empty
                    // candidate list diagnosable instead of silently unexplained.
                    Err(error) => reasons.push(format!("{error:#}")),
                }
            }
            if matches.len() == first_match {
                // Full payload, control and clip checks still run in prepare_one_reusing.
                // Discovery only establishes the shared attachment boundary here.
                match rig_convert::owned::validate(&source_rig, &report) {
                    Ok(pairs)
                        if pairs.iter().any(|(owner, _, fp)| {
                            !fp && from["owner"] == format!("{owner:08X}")
                        }) =>
                    {
                        matches.push(json!({"name":row["name"],"hash":hash,"exotic":exotic,"rarity":rarity,"bucket":row["bucket_hash"],"content_key":report["content_key"],"collection_backed":row["collection_backed"],"unrelated_collection_condition":row["unrelated_collection_condition"],"mapping":{"source_owned":true,"native_bone_count":from["bones"].as_array().context("source bones")?.len()}}));
                    }
                    Ok(_) => {
                        reasons.push("source geometry does not select the equipment rig".into())
                    }
                    Err(error) => reasons.push(format!("source-owned rig: {error:#}")),
                }
            }
            ensure!(
                matches.len() > first_match,
                "no compatible skeleton: {}",
                reasons.join("; ")
            );
            Ok(())
        })();
        if let Err(error) = result {
            rejected.push(json!({"name":row["name"],"hash":hash,"reason":format!("{error:#}")}));
        }
    }
    let result = json!({"matches":matches,"unavailable":rejected,"gameplay_verified":false});
    write_json(&out.join("matches.json"), &result)?;
    r.finish()?;
    Ok(result)
}

pub(crate) fn donor_candidate(row: &Value, kind: &str, preferred: Option<u32>) -> bool {
    row["present_in_native"].as_bool() == Some(true)
        && (row["weapon_type"].as_str() == Some(kind)
            || preferred.is_some_and(|hash| row["hash"].as_u64() == Some(u64::from(hash))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_cache_rejects_wrong_item_and_incomplete_report() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("rig.json");
        write_json(
            &path,
            &json!({"item_tag":"12345678","skeletons":[{"bones":[]}]}),
        )
        .unwrap();
        assert!(cached_rig(&path, 0x12345678).is_some());
        assert!(cached_rig(&path, 0x87654321).is_none());
        write_json(&path, &json!({"item_tag":"12345678"})).unwrap();
        assert!(cached_rig(&path, 0x12345678).is_none());
    }

    #[test]
    #[ignore = "Requires explicitly configured native packages, source export and donor"]
    fn configured_carrier_extension_preserves_donor_channel_banks() {
        let packages = std::env::var_os("PARHELION_IMPORT_NATIVE_PACKAGES")
            .expect("PARHELION_IMPORT_NATIVE_PACKAGES");
        let source = PathBuf::from(
            std::env::var_os("PARHELION_IMPORT_SOURCE_EXPORT")
                .expect("PARHELION_IMPORT_SOURCE_EXPORT"),
        );
        let donor =
            std::env::var("PARHELION_IMPORT_DONOR_HASH").expect("PARHELION_IMPORT_DONOR_HASH");
        let donor = u32::from_str_radix(donor.trim_start_matches("0x"), 16).unwrap();
        let stride: i16 = std::env::var("PARHELION_IMPORT_VERTEX_STRIDE")
            .expect("PARHELION_IMPORT_VERTEX_STRIDE")
            .parse()
            .unwrap();
        let output = tempfile::tempdir().unwrap();
        let mut reader = Reader::new(Path::new(&packages), output.path(), false).unwrap();
        let mut template = shadowkeep::extract(&mut reader, donor).unwrap();
        reader.finish().unwrap();
        let manifest = output.path().join("source-manifest.json");
        let before: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        let report: Value =
            serde_json::from_slice(&fs::read(source.join("report.json")).unwrap()).unwrap();
        extend_carriers(
            &source,
            Path::new(&packages),
            &mut reader,
            &report,
            stride,
            &mut template,
            &mut |_| {},
        )
        .unwrap();
        let after: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        for (tag, record) in before["tags"].as_object().unwrap() {
            assert_eq!(&after["tags"][tag], record, "lost donor tag {tag}");
        }
        assert!(!template["carrier_models"].as_array().unwrap().is_empty());
        let owner = super::super::mapping::primary_carrier_owner(
            &source,
            output.path(),
            &report,
            &template,
            stride,
        )
        .unwrap();
        super::super::native::effects::check_channels(&source, output.path(), owner).unwrap();
    }
}
