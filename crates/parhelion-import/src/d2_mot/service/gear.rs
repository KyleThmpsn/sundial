//! Source gear preparation with separate art variants and native family registration.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::{BTreeMap, BTreeSet};
mod dyes;
mod graph;
mod models;
mod runtime;

fn load(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("Read {}", path.display()))?)
        .map_err(Into::into)
}

fn private_key(item: u32, role: &str) -> u32 {
    let hash = format!("parhelion/imported-gear/{item:08X}/{role}")
        .bytes()
        .fold(0x811C9DC5u32, |hash, byte| {
            hash.wrapping_mul(16777619) ^ u32::from(byte)
        });
    if [0, u32::MAX, 0x811C9DC5].contains(&hash) {
        hash ^ 0x10000
    } else {
        hash
    }
}

fn validate_donor(reader: &mut Reader, source: &Weapon, donor: u32) -> Result<()> {
    let tag = crate::d2_mot::assets::item::find_native(reader, donor)?;
    let item = reader.tag(tag, Some(0x80807BEA))?;
    let native_bucket = match item.u8(0xB8)? {
        3 => 3448274439,
        4 => 3551918588,
        5 => 14239492,
        6 => 20886954,
        7 => 1585787867,
        8 => 4023194814,
        9 => 2025709351,
        10 => 284967655,
        _ => anyhow::bail!("Native template is not equippable model gear"),
    };
    ensure!(
        source.bucket_hash == Some(native_bucket),
        "Native template uses another inventory slot"
    );
    // Gear authoring writes the source rarity and its unique-equip group. An Exotic
    // class item therefore does not need an Exotic class-item template in the older game.
    // Avoid importing a different armor Exotic's intrinsic behavior through the carrier.
    ensure!(
        source.family() != Family::Armor || source.hash == donor || item.u8(0xBA)? != 5,
        "An unrelated Exotic armor item cannot supply the native runtime template"
    );
    if source.family() == Family::Armor {
        let globals = reader
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|t| t.name == "investment_globals")
            .context("native globals")?
            .hash
            .0;
        let globals = reader.tag(globals, None)?;
        let strings = reader.tag(globals.u32(16 + 33 * 16)?, None)?;
        let row = strings
            .array(8, 24, Some(0x80805CDF))?
            .into_iter()
            .find(|&at| strings.u32(at).ok() == Some(donor))
            .context("native gear strings")?;
        let text = reader.tag(strings.u32(row + 16)?, None)?;
        let class = match text.u32(0xB8)? {
            0xD60E6BA5 => Some(0),
            0xD74EBFB3 => Some(1),
            0x82E15A90 => Some(2),
            _ => None,
        };
        ensure!(
            source.class_type.is_some() && source.class_type == class,
            "Native armor template uses another class"
        );
    }
    Ok(())
}

struct SourcePaths<'a> {
    modern: &'a Path,
    native: &'a Path,
    extracted: &'a Path,
}

struct Plan {
    template: Value,
    rig: Value,
    models: models::Plan,
}

fn plan(
    source: &Weapon,
    paths: &SourcePaths<'_>,
    donor: &Value,
    folder: &Path,
    item: u32,
    progress: &mut dyn FnMut(String),
) -> Result<Plan> {
    let SourcePaths {
        native,
        extracted: source_path,
        ..
    } = *paths;
    let donor_hash = profile::hash(donor, "hash")?;
    let native_path = folder.join("native");
    let mut reader = Reader::new(native, &native_path, false)?;
    validate_donor(&mut reader, source, donor_hash)?;
    let template = crate::d2_mot::shadowkeep::extract(&mut reader, donor_hash)?;
    write_json(&native_path.join("template-report.json"), &template)?;
    let native_rig = rig::art(&mut reader, &template, false)?;
    reader.finish()?;
    let models = models::plan(
        source_path,
        &native_path,
        native,
        &folder.join("models"),
        item,
        progress,
    )?;
    Ok(Plan {
        template,
        rig: native_rig,
        models,
    })
}

fn convert(
    source: &Weapon,
    paths: &SourcePaths<'_>,
    donor: &Value,
    folder: &Path,
    item: u32,
    plan: &Plan,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let SourcePaths {
        modern,
        native,
        extracted: source_path,
    } = *paths;
    let donor_hash = profile::hash(donor, "hash")?;
    let gameplay = load(&source_path.join("gameplay.json"))?;
    let source_report = load(&source_path.join("report.json"))?;
    progress(format!("Converting {}...", source.name));
    crate::cancellation::check()?;
    let parts = models::convert(&plan.models, modern, native, progress)?;
    let output = folder.join("graph");
    let mut graph = graph::assemble(
        source,
        item,
        donor_hash,
        &parts,
        &source_report,
        &plan.template,
        &output,
    )?;
    dyes::append(
        modern,
        native,
        source.hash,
        donor_hash,
        folder,
        &output,
        &mut graph,
    )?;
    fs::copy(
        source_path.join("item-icon.png"),
        output.join("source-icon.png"),
    )?;
    graph["source_icon_png"] = json!("source-icon.png");
    graph["source_rarity"] = gameplay["rarity"].clone();
    let source_rig = load(&source_path.join("rig.json"))?;
    graph["source_components"] = source_rig["components"].clone();
    graph["limitations"] = json!([
        "Source geometry, art placements, textures and supported material programs are converted. Native equipment controls and physics remain in use. Verify rendering and behavior in game."
    ]);
    runtime::prepare(
        paths,
        &source_rig,
        &plan.rig,
        folder,
        &output,
        &mut graph,
        progress,
    )?;
    write_json(&output.join("asset-graph.json"), &graph)?;
    let namespace = crate::d2_mot::compatibility::namespace(source.hash);
    let reference = GraphReference::new(&output, item)?;
    let icon = STANDARD.encode(fs::read(source_path.join("item-icon.png"))?);
    let mut recipe = json!({"schema":1,"kind":source.family(),"collection_placement":"sunrise_badge",
        "namespace":namespace,"identity":batch::identity(&namespace)?,"name":source.name,"type_name":source.weapon_type,
        "donor":{"item_hash":format!("0x{donor_hash:08X}"),"expected_name":donor["name"]},
        "flavor":source_report["flavor"].as_str().unwrap_or(""),"source":"Source: Imported gear",
        "overrides":{"imported_graph":reference,"icon_edit":{"imported_image":{"png_base64":icon}}}});
    if let Some(lore) = source_report["lore"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
    {
        recipe["overrides"]["lore"] = json!(lore);
    } else {
        recipe["overrides"]["remove_lore"] = json!(true);
    }
    let mapping =
        crate::d2_mot::gameplay::apply(&gameplay, native, &folder.join("gameplay"), &mut recipe)?;
    let omitted = graph["rendering"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|part| {
            part["runtime_material_draws"]
                .as_array()
                .map_or(0, Vec::len)
        })
        .sum::<usize>();
    let fallbacks = mapping["fallbacks"].as_array().map_or(0, Vec::len);
    let base_glow = graph["rendering"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|part| {
            part["material_base_fallbacks"]
                .as_array()
                .map_or(0, Vec::len)
        })
        .sum::<usize>();
    let deferred = graph["rendering"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| {
            part["source_procedural_adapter"]["enabled"] == false
                || part["resting_source_procedures"]["enabled"] == false
        })
        .count();
    let limitations = graph["limitations"]
        .as_array_mut()
        .context("Import details")?;
    if omitted > 0 {
        limitations.push(json!(format!(
            "{omitted} source draws require runtime material overrides and could not be converted."
        )));
    }
    if fallbacks > 0 {
        limitations.push(json!(format!(
            "{fallbacks} source gameplay settings use native defaults or lack a native equivalent. The import report lists each setting.")));
    }
    if base_glow > 0 {
        limitations.push(json!(format!(
            "{base_glow} source materials use their authored base glow because a multiplier has no source binding. Bound glow adjustments remain active. The missing multiplier's behavior is unavailable."
        )));
    }
    if deferred > 0 {
        limitations.push(json!(format!("{deferred} source model parts retain static values for unsupported procedural controls. Texture animation supported by the material program remains active.")));
    }
    graph["gameplay_mapping"] = mapping;
    write_json(&output.join("asset-graph.json"), &graph)?;
    recipe["overrides"]["imported_graph"] =
        serde_json::to_value(GraphReference::new(&output, item)?)?;
    let path = folder.join("gear.parhelion.json");
    write_json(&path, &recipe)?;
    Ok(path)
}

pub(super) fn prepare(
    source: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    ensure!(source.family().is_model_gear(), "Item is not model gear");
    let output = crate::d2_mot::reader::outside(output, modern.parent().context("modern root")?)?;
    let output = crate::d2_mot::reader::outside(&output, native.parent().context("native root")?)?;
    let previous = previous_model_donor(&output, source.hash);
    let mut candidates = donors["gear"]
        .as_array()
        .context("No native gear templates loaded")?
        .iter()
        .filter(|donor| {
            source.accepts_gear_donor(
                donor["bucket_hash"].as_u64().unwrap_or(0),
                donor["class_type"]
                    .as_u64()
                    .and_then(|v| u8::try_from(v).ok()),
            )
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|donor| {
        (
            donor["hash"].as_u64() != previous.map(u64::from),
            donor["hash"].as_u64() != Some(u64::from(source.hash)),
            donor["collection_backed"] != true,
            donor["hash"].as_u64(),
        )
    });
    ensure!(
        !candidates.is_empty(),
        "No native {} template matches the source slot and class",
        source.family().label()
    );
    let root = reserve_assets(&output, "gear")?;
    let selection_path = root.join("selection.json");
    let mut selection = json!({"source_item":source.hash,"phase":"source",
        "conversions_started":0,"rejected":[],"gameplay_verified":false});
    write_json(&selection_path, &selection)?;
    let source_path = root.join("source");
    let mut reader = Reader::new(modern, &source_path, true)?;
    let report = extract::extract_with_progress(
        &mut reader,
        source.hash,
        None,
        crate::d2_mot::geometry::Detail::Full,
        progress,
    )?;
    ensure!(
        report["name"].as_str() == Some(source.name.as_str()),
        "Source name changed during import"
    );
    write_json(&source_path.join("report.json"), &report)?;
    rig::art(&mut reader, &report, true)?;
    reader.finish()?;
    progress("Checking Source Materials…".into());
    crate::d2_mot::native::effects::check_source_blends(&source_path)
        .map_err(crate::d2_mot::source_limit)?;
    // Keep one native package index for all structural compatibility checks.
    let _native_index = crate::d2_mot::reader::package_index(&native.canonicalize()?, false)?;
    let item = destination_hash(source.hash)?;
    let mut failures = Vec::new();
    let paths = SourcePaths {
        modern,
        native,
        extracted: &source_path,
    };
    let total = candidates.len();
    let mut selected = None;
    for (index, donor) in candidates.into_iter().enumerate() {
        crate::cancellation::check()?;
        progress(format!(
            "Checking Native Equipment {} of {total}…",
            index + 1
        ));
        crate::cancellation::check()?;
        let folder = root.join(format!("candidate-{index}"));
        selection["phase"] = json!("planning");
        selection["candidate"] = donor["hash"].clone();
        write_json(&selection_path, &selection)?;
        match plan(source, &paths, donor, &folder, item, progress) {
            Ok(plan) => {
                selected = Some((donor, folder, plan));
                break;
            }
            Err(error) => {
                crate::cancellation::check()?;
                if crate::cancellation::is_cancelled(&error) {
                    return Err(error);
                }
                failures.push(json!({"donor":donor["hash"],"reason":format!("{error:#}")}));
                write_json(&root.join("failures.json"), &json!(failures))?;
                selection["rejected"] = json!(failures);
                if crate::d2_mot::is_source_limit(&error) {
                    selection["phase"] = json!("unsupported");
                    write_json(&selection_path, &selection)?;
                    return Err(error)
                        .context("This source item needs additional material conversion support");
                }
                write_json(&selection_path, &selection)?;
            }
        }
    }
    if let Some((donor, folder, plan)) = selected {
        crate::cancellation::check()?;
        selection["phase"] = json!("converting");
        selection["selected"] = donor["hash"].clone();
        selection["models"] = plan.models.summary();
        selection["conversions_started"] = json!(1);
        write_json(&selection_path, &selection)?;
        // Selection is complete. A conversion failure cannot trigger a new
        // shader conversion against another item's equipment wrapper.
        let result = convert(source, &paths, donor, &folder, item, &plan, progress);
        crate::cancellation::check()?;
        match result {
            Ok(path) => {
                selection["phase"] = json!("prepared");
                write_json(&selection_path, &selection)?;
                write_json(
                    &output.join("result.json"),
                    &json!({"recipe":path,"source_item":source.hash,
                    "donor_hash":donor["hash"],"kind":source.family(),"attempts":failures,
                    "selection":selection_path,"gameplay_verified":false}),
                )?;
                progress(format!("{} recipe prepared.", source.family().label()));
                return Ok(path);
            }
            Err(error) => {
                if crate::cancellation::is_cancelled(&error) {
                    return Err(error);
                }
                selection["phase"] = json!("failed");
                selection["reason"] = json!(format!("{error:#}"));
                write_json(&selection_path, &selection)?;
                return Err(error).context("Converting source gear");
            }
        }
    }
    selection["phase"] = json!("unavailable");
    write_json(&selection_path, &selection)?;
    let reasons = failures
        .iter()
        .filter_map(|row| row["reason"].as_str())
        .collect::<BTreeSet<_>>();
    anyhow::bail!(
        "No compatible native {} template. {}",
        source.family().label(),
        reasons.into_iter().take(4).collect::<Vec<_>>().join(". ")
    )
}
