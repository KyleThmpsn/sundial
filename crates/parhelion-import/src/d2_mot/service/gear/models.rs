//! Convert each independent art entity before assembling class and body alternatives.
use super::*;
use crate::d2_mot::{mapping, native, rig_convert};

/// Export views own their JSON reports. Immutable raw assets can be hard linked.
fn view(from: &Path, to: &Path, report_name: &str, report: &Value, rig: &Value) -> Result<()> {
    fs::create_dir_all(to.join("raw"))?;
    for directory in [Path::new(""), Path::new("raw")] {
        for entry in fs::read_dir(from.join(directory))? {
            crate::cancellation::check()?;
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let destination = to.join(directory).join(entry.file_name());
            if entry.path().extension().is_some_and(|x| x == "json") {
                fs::copy(entry.path(), destination)?;
            } else {
                fs::hard_link(entry.path(), &destination)
                    .or_else(|_| fs::copy(entry.path(), &destination).map(|_| ()))?;
            }
        }
    }
    write_json(&to.join(report_name), report)?;
    write_json(&to.join("rig.json"), rig)
}

fn skeletons(rig: &Value, entity: &Value) -> Result<Value> {
    let mut result = rig.clone();
    for field in ["skeletons", "components"] {
        result[field] = json!(
            rig[field]
                .as_array()
                .context("art rig entries")?
                .iter()
                .filter(|row| &row["entity"] == entity || row["shared_runtime"] == true)
                .collect::<Vec<_>>()
        );
    }
    Ok(result)
}

fn rig_config(
    source: &Path,
    native: &Path,
    report: &Value,
    template: &Value,
) -> Result<Option<Value>> {
    if report["models"]
        .as_array()
        .context("source models")?
        .iter()
        .all(|model| model["rigid_bone_zero"] == true)
    {
        return Ok(None);
    }
    let from = load(&source.join("rig.json"))?;
    let to = load(&native.join("rig.json"))?;
    let mut matches = Vec::new();
    let mut errors = BTreeSet::new();
    for a in from["skeletons"].as_array().context("source skeletons")? {
        if a["class"] != "808081DE" {
            continue;
        }
        for b in to["skeletons"].as_array().context("native skeletons")? {
            if b["class"] != "80808546" {
                continue;
            }
            let config = json!({"source":source,"native":native,"source_geometry":source,
                "source_owner":a["owner"],"native_owner":b["owner"]});
            match rig_convert::compatible_map(&config, &report["item_tag"], &template["item_tag"]) {
                Ok(mapping) => matches.push((
                    mapping["native_bone_count"].as_u64().unwrap_or(u64::MAX),
                    config,
                )),
                Err(error) => {
                    errors.insert(format!("{error:#}"));
                }
            }
        }
    }
    matches.sort_by_key(|(count, _)| *count);
    matches
        .into_iter()
        .next()
        .map(|(_, config)| Some(config))
        .with_context(|| {
            format!(
                "No compatible art skeleton. {}",
                errors.into_iter().collect::<Vec<_>>().join(". ")
            )
        })
}

fn placement_match(source: &Value, native: &Value) -> bool {
    source["class"] == native["class"]
        && if source["single"].is_number() {
            source["single"] == native["single"]
        } else {
            source["selector"] == native["selector"] && source["position"] == native["position"]
        }
}

fn carrier<'a>(part: &Value, template: &'a Value) -> Result<&'a Value> {
    let parents = template["parents"]
        .as_array()
        .context("native art parents")?;
    let positions = part["placements"]
        .as_array()
        .context("source art placements")?;
    let mut candidates = parents
        .iter()
        .filter(|parent| {
            template["models"].as_array().is_some_and(|models| {
                models
                    .iter()
                    .filter(|m| m["entity"] == parent["entity"])
                    .filter_map(|m| m["owner"].as_str())
                    .collect::<BTreeSet<_>>()
                    .len()
                    == 1
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|parent| {
        let matching = parent["placements"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|b| positions.iter().any(|a| placement_match(a, b)));
        (!matching, parent["assignment"].as_str().unwrap_or(""))
    });
    candidates
        .first()
        .copied()
        .context("Native gear has no single-owner model carrier")
}

/// A complete set of checked source views, before any geometry or shader work.
pub(super) struct Plan {
    parts: Vec<(Value, PathBuf)>,
}

impl Plan {
    pub(super) fn summary(&self) -> Value {
        json!({"art_parts":self.parts.iter().map(|(part, path)|
            json!({"source_entity":part["entity"],"assignment":part["assignment"],"directory":path}))
            .collect::<Vec<_>>()})
    }
}

pub(super) fn plan(
    source: &Path,
    native_root: &Path,
    native_packages: &Path,
    output: &Path,
    item: u32,
    progress: &mut dyn FnMut(String),
) -> Result<Plan> {
    let report = load(&source.join("report.json"))?;
    let template = load(&native_root.join("template-report.json"))?;
    let source_rig = load(&source.join("rig.json"))?;
    let native_rig = load(&native_root.join("rig.json"))?;
    let parts = report["art_parts"].as_array().context("source art parts")?;
    ensure!(!parts.is_empty(), "Source gear has no art assignments");
    let mut planned = BTreeMap::new();
    let mut results = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        crate::cancellation::check()?;
        ensure!(
            part["missing"] != true && part["entity_class"] == "80809AD8",
            "Source art assignment {} is not a supported model entity",
            part["assignment"]
        );
        if let Some(path) = planned.get(&part["entity"].to_string()) {
            results.push((part.clone(), PathBuf::clone(path)));
            continue;
        }
        let parent = carrier(part, &template)?;
        let mut selected = report.clone();
        selected["models"] = json!(
            report["models"]
                .as_array()
                .context("source models")?
                .iter()
                .filter(|model| model["entity"] == part["entity"])
                .collect::<Vec<_>>()
        );
        ensure!(
            !selected["models"].as_array().unwrap().is_empty(),
            "Source art assignment {} has no convertible geometry",
            part["assignment"]
        );
        selected["art_parts"] = json!([part]);
        selected["independent_art_entity"] = part["entity"].clone();
        let mut carrier = template.clone();
        carrier["models"] = json!(
            template["models"]
                .as_array()
                .context("native models")?
                .iter()
                .filter(|model| model["entity"] == parent["entity"])
                .collect::<Vec<_>>()
        );
        carrier["parents"] = json!([parent]);
        let folder = output.join(format!("part-{index}"));
        let from = folder.join("source");
        let to = folder.join("native");
        view(
            source,
            &from,
            "report.json",
            &selected,
            &skeletons(&source_rig, &part["entity"])?,
        )?;
        view(
            native_root,
            &to,
            "template-report.json",
            &carrier,
            &skeletons(&native_rig, &parent["entity"])?,
        )?;
        let rig = rig_config(&from, &to, &selected, &carrier)?;
        let stride = if rig.is_some() { 24 } else { 20 };
        if mapping::check_carriers(&from, &to, &selected, &carrier, stride).is_err() {
            let mut reader = Reader::new(native_packages, &to, false)?;
            // Re-read the selected donor so its provenance is retained when extending carriers.
            let full =
                crate::d2_mot::shadowkeep::extract(&mut reader, profile::hash(&template, "item")?)?;
            crate::d2_mot::rig::art(&mut reader, &full, false)?;
            batch::extend_carriers(
                &from,
                native_packages,
                &mut reader,
                &selected,
                stride,
                &mut carrier,
                progress,
            )?;
            mapping::check_carriers(&from, &to, &selected, &carrier, stride)?;
        }
        // Establish channel/provider compatibility before atlas assembly,
        // native shader decompilation and source shader conversion.
        let owner = mapping::primary_carrier_owner(&from, &to, &selected, &carrier, stride)?;
        native::effects::check_channels(&from, &to, owner).context("Source material controls")?;
        // Marker compatibility is independent of atlas and shader conversion.
        // Run the same checked rewrite used by the graph emitter in memory.
        let mut reader = Reader::discovery(native_packages, &to, false)?;
        let entity = reader.tag(profile::hash(parent, "entity")?, Some(0x80809C0F))?;
        crate::d2_mot::markers::carry(&mut reader, &from, &entity)
            .context("Source model markers")?;
        let mut profile = json!({"source_item":report["item_hash"],"native_item":profile::hash(&template,"item")?,
            "target_item":item,"art_key":private_key(item,&format!("part-{index}")),
            "model_index":0,"plated":true,"convert_dyes":false});
        if let Some(rig) = rig {
            profile["rig"] = rig;
        }
        write_json(&folder.join("profile.json"), &profile)?;
        planned.insert(part["entity"].to_string(), folder.clone());
        results.push((part.clone(), folder));
    }
    Ok(Plan { parts: results })
}

/// Convert only the selected plan. A failure here never starts another plan.
pub(super) fn convert(
    plan: &Plan,
    modern: &Path,
    native_packages: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Vec<(Value, PathBuf)>> {
    let mut converted = BTreeMap::new();
    let mut results = Vec::new();
    for (index, (part, folder)) in plan.parts.iter().enumerate() {
        crate::cancellation::check()?;
        if let Some(path) = converted.get(folder) {
            results.push((part.clone(), PathBuf::clone(path)));
            continue;
        }
        let from = folder.join("source");
        let to = folder.join("native");
        let prepared = folder.join("prepared");
        profile::prepare_reusing(
            &folder.join("profile.json"),
            modern,
            native_packages,
            &prepared,
            Some(profile::Extracted {
                source: &from,
                native: &to,
            }),
            progress,
        )?;
        // The rendering pass accepts a complete seed graph even when dyes are added later.
        let seed = load(&prepared.join("graph/asset-graph.json"))?;
        fs::create_dir_all(prepared.join("material-graph"))?;
        for node in seed["nodes"].as_array().context("seed nodes")? {
            let file = node["file"].as_str().context("seed payload")?;
            fs::copy(
                prepared.join("graph").join(file),
                prepared.join("material-graph").join(file),
            )?;
        }
        write_json(&prepared.join("material-graph/asset-graph.json"), &seed)?;
        progress(format!(
            "Converting Gear Model {} of {}...",
            index + 1,
            plan.parts.len()
        ));
        let result =
            native::automatic::build(&prepared, &from, &to, &folder.join("assembled"), progress)?;
        let graph = PathBuf::from(result["graph"].as_str().context("converted gear graph")?);
        converted.insert(folder.clone(), graph.clone());
        results.push((part.clone(), graph));
    }
    Ok(results)
}
