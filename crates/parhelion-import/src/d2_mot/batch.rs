//! Offline preparation with explicit source items and verified animation donors.
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

pub(crate) fn identity(namespace: &str) -> Result<Value> {
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

pub(crate) fn prepare_one(
    row: &Value,
    ordinal: u32,
    modern: &Path,
    native: &Path,
    out: &Path,
) -> Result<Value> {
    prepare_one_reusing(row, ordinal, modern, native, out, None, &mut |_| {})
}

/// Shared location for searched carriers, keyed by the native package set so
/// a rebuilt or updated install starts a fresh index.
fn carrier_cache(native: &Path) -> Result<PathBuf> {
    let stamp = super::service::package_stamp(native)?;
    let directory = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("Local app data")?)
        .join("Sundial/parhelion/importer/cache/native-carriers");
    fs::create_dir_all(&directory)?;
    Ok(directory.join(format!("{stamp}.json")))
}

/// Add rendering templates from elsewhere in the packages for source models
/// the donor cannot carry. Only draw records and material shells come from
/// these models, so the gameplay item and animation donor stay as chosen.
fn extend_carriers(
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
    let mut found: Value = fs::read(&remembered)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| json!({}));
    let mut added = template["carrier_models"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for (tag, mesh) in missing {
        let model = super::mapping::source_model(source_path, tag)?;
        let key = super::mapping::contract_signature(source_path, &model, mesh, (true, stride))?;
        if found.get(&key).is_none() {
            progress("Searching packages for a compatible rendering template…".into());
            let carrier =
                super::mapping::search_carrier(reader, source_path, &model, mesh, (true, stride));
            found[&key] = match &carrier {
                Ok(carrier) => json!(format!("{carrier:08X}")),
                Err(error) => json!({ "unavailable": format!("{error:#}") }),
            };
            write_json(&remembered, &found)?;
            carrier.with_context(|| format!("source model {tag:08X}"))?;
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

pub(crate) fn prepare_one_reusing(
    row: &Value,
    ordinal: u32,
    modern: &Path,
    native: &Path,
    out: &Path,
    source_cache: Option<&Path>,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    let hash = profile::hash(row, "hash")?;
    let donor = profile::hash(row, "native_item")?;
    let folder = out.join(format!("{hash:08X}"));
    fs::create_dir(&folder)?;
    let source_path = source_cache.map_or_else(|| folder.join("source"), Path::to_owned);
    let (source, source_rig) = if source_cache.is_some() {
        (
            serde_json::from_slice::<Value>(&fs::read(source_path.join("report.json"))?)?,
            serde_json::from_slice::<Value>(&fs::read(source_path.join("rig.json"))?)?,
        )
    } else {
        progress("Extracting source models and textures…".into());
        let mut r = Reader::new(modern, &source_path, true)?;
        let source = extract::extract_with_progress(&mut r, hash, None, progress)?;
        write_json(&source_path.join("report.json"), &source)?;
        let source_rig = rig::inspect(&mut r, profile::hash(&source, "item_tag")?, true)?;
        r.finish()?;
        (source, source_rig)
    };
    ensure!(
        source["item_hash"].as_u64() == Some(u64::from(hash)) && source["name"] == row["name"],
        "source weapon identity differs"
    );
    progress("Extracting donor models and materials…".into());
    let native_path = folder.join("native");
    let mut r = Reader::new(native, &native_path, false)?;
    let template = shadowkeep::extract(&mut r, donor)?;
    write_json(&native_path.join("template-report.json"), &template)?;
    let native_rig = rig::inspect(&mut r, profile::hash(&template, "item_tag")?, false)?;
    r.finish()?;
    let from = source_rig["skeletons"]
        .as_array()
        .context("source skeletons")?
        .iter()
        .min_by_key(|s| s["bones"].as_array().map_or(usize::MAX, Vec::len))
        .context("source weapon skeleton")?;
    let mut matches = Vec::new();
    for to in native_rig["skeletons"]
        .as_array()
        .context("native skeletons")?
    {
        let config = json!({"source":source_path,"native":native_path,"source_geometry":source_path,"source_owner":from["owner"],"native_owner":to["owner"]});
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
    matches.sort_by_key(|(count, _, _)| *count);
    let (_, rig, mapping) = matches
        .first()
        .context("native donor lacks a matching source bone hierarchy")?;
    let namespace = super::compatibility::namespace(hash);
    let id = identity(&namespace)?;
    let use_rig = !(rigid_root(mapping) && has_rigid_carrier(&template)?);
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
    let mut profile = json!({"source_item":hash,"native_item":donor,"target_item":id["item_hash"],"art_key":0xE2A00000u32.checked_add(ordinal).context("art key overflow")?,"dye_key_base":0xE2B00000u32.checked_add(ordinal.checked_mul(16).context("dye key overflow")?).context("dye key overflow")?,"model_index":0,"plated":true,"convert_dyes":true});
    if use_rig {
        profile["rig"] = rig.clone();
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
    let animation = match rig_convert::animation::first_person::prepare(
        modern,
        native,
        &source_rig,
        &native_rig,
        &folder.join("animation"),
        &graph_dir,
    ) {
        Ok(section) => section,
        // A failed link keeps the donor's native animation rather than the import.
        Err(error) => {
            eprintln!("First-person animation stays native: {error:#}");
            json!({"first_person_status":"native","reason":format!("{error:#}"),"gameplay_verified":false})
        }
    };
    let graph_path = graph_dir.join("asset-graph.json");
    let mut graph: Value = serde_json::from_slice(&fs::read(&graph_path)?)?;
    graph["animation"] = animation.clone();
    write_json(&graph_path, &graph)?;
    result["animation"] = animation;
    progress("Saving weapon recipe and source lore…".into());
    let icon = STANDARD.encode(fs::read(source_path.join("item-icon.png"))?);
    let damage = row["damage"]
        .as_str()
        .filter(|v| ["kinetic", "arc", "solar", "void"].contains(v));
    // Unsupported source elements retain the native gameplay donor. Their
    // original element remains in the source plan and extraction metadata.
    let mut recipe = json!({"schema":1,"collection_placement":"sunrise_badge","namespace":namespace,"donor":{"item_hash":format!("0x{donor:08X}"),"expected_name":row["native_donor"]},"identity":id,"name":row["name"],"flavor":source["flavor"].as_str().unwrap_or(""),"source":"Source: Imported arsenal","overrides":{"icon_edit":{"imported_image":{"png_base64":icon}},"modern_damage_type":damage}});
    if let Some(lore) = source["lore"]["text"].as_str() {
        recipe["overrides"]["lore"] = json!(lore);
    } else {
        recipe["overrides"]["remove_lore"] = json!(true);
    }
    if source_cache.is_none() {
        let gameplay: Value =
            serde_json::from_slice(&fs::read(source_path.join("gameplay.json"))?)?;
        super::gameplay::apply(&gameplay, native, &folder.join("gameplay"), &mut recipe)?;
    }
    let recipe_path = out.join(format!("recipes/{hash:08X}.parhelion.json"));
    write_json(&recipe_path, &recipe)?;
    Ok(
        json!({"name":row["name"],"hash":hash,"native_donor":row["native_donor"],"native_item":donor,"folder":folder,"prepared":prepared,"recipe":recipe_path,"graph":result["graph"],"rendering_templates":template["carrier_models"],"status":"prepared","gameplay_verified":false}),
    )
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

#[cfg(test)]
mod automatic_tests {
    use super::*;
    #[test]
    fn rigid_carriers_require_every_used_bone_to_map_to_the_native_root() {
        assert!(rigid_root(
            &json!({"required_source_bones":[0],"bone_map":[0,1,2]})
        ));
        assert!(!rigid_root(
            &json!({"required_source_bones":[0,1],"bone_map":[0,1]})
        ));
        assert!(!rigid_root(
            &json!({"required_source_bones":[0],"bone_map":[1,0]})
        ));
        assert!(!rigid_root(&json!({})));
    }
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

pub fn donors_with_progress(
    source: &Path,
    catalog: &Path,
    kind: &str,
    native: &Path,
    out: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Value> {
    progress("Opening destination packages for donor matching…".into());
    let catalog: Value = serde_json::from_slice(&fs::read(catalog)?)?;
    let source_report: Value = serde_json::from_slice(&fs::read(source.join("report.json"))?)?;
    let gameplay = source
        .join("gameplay.json")
        .exists()
        .then(|| -> Result<Value> {
            Ok(serde_json::from_slice(&fs::read(
                source.join("gameplay.json"),
            )?)?)
        })
        .transpose()?;
    let preferred = super::compatibility::profile(profile::hash(&source_report, "item_hash")?)
        .map(|profile| profile.model_donor);
    let source_rig: Value = serde_json::from_slice(&fs::read(source.join("rig.json"))?)?;
    let from = source_rig["skeletons"]
        .as_array()
        .context("source skeletons")?
        .iter()
        .min_by_key(|s| s["bones"].as_array().map_or(usize::MAX, Vec::len))
        .context("source weapon skeleton")?;
    let mut r = Reader::new(native, out, false)?;
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
        .filter(|v| donor_candidate(v, kind, preferred))
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
                let source_hash = profile::hash(&source_report, "item_hash")?;
                let gameplay_hash = super::compatibility::profile(source_hash)
                    .filter(|p| p.model_donor == hash)
                    .map(|p| profile::hash(&p.donor, "item_hash"))
                    .transpose()?
                    .unwrap_or(hash);
                let gameplay_item = *table
                    .get(&gameplay_hash)
                    .context("native gameplay donor missing")?;
                super::gameplay::check_donor(
                    gameplay,
                    r.tag(gameplay_item, Some(0x80807BEA))?.as_ref(),
                )?;
            }
            let exotic = r.tag(item, Some(0x80807BEA))?.u8(0xBA)? == 5;
            let report = rig::inspect(&mut r, item, false)?;
            let folder = out.join(format!("{hash:08X}"));
            fs::create_dir_all(&folder)?;
            write_json(&folder.join("rig.json"), &report)?;
            let mut reasons = Vec::new();
            for to in report["skeletons"].as_array().context("native skeletons")? {
                let config = json!({"source":source,"source_geometry":source,"native":folder,"source_owner":from["owner"],"native_owner":to["owner"]});
                match rig_convert::compatible_map(
                    &config,
                    &source_rig["item_tag"],
                    &report["item_tag"],
                ) {
                    Ok(mapping) => {
                        matches.push(
                            json!({"name":row["name"],"hash":hash,"exotic":exotic,"rig":config,"mapping":mapping}),
                        );
                        eprintln!("Matched native rig {}", row["name"]);
                    }
                    // Recording why each skeleton was turned down keeps an empty
                    // candidate list diagnosable instead of silently unexplained.
                    Err(error) => reasons.push(format!("{error:#}")),
                }
            }
            ensure!(
                reasons.len()
                    < report["skeletons"]
                        .as_array()
                        .context("native skeletons")?
                        .len(),
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

fn donor_candidate(row: &Value, kind: &str, preferred: Option<u32>) -> bool {
    row["present_in_native"].as_bool() == Some(true)
        && (row["weapon_type"].as_str() == Some(kind)
            || preferred.is_some_and(|hash| row["hash"].as_u64() == Some(u64::from(hash))))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn proven_cross_type_model_donors_still_require_native_availability() {
        let mut donor =
            json!({"hash":0x5F67B9A2u32,"weapon_type":"Machine Gun","present_in_native":true});
        assert!(donor_candidate(&donor, "Trace Rifle", Some(0x5F67B9A2)));
        assert!(!donor_candidate(&donor, "Trace Rifle", None));
        assert!(!donor_candidate(&donor, "Trace Rifle", Some(1)));
        assert!(donor_candidate(&donor, "Machine Gun", None));
        donor["present_in_native"] = json!(false);
        assert!(!donor_candidate(&donor, "Trace Rifle", Some(0x5F67B9A2)));
    }
    #[test]
    fn identity_matches_existing_imported_chroma_recipe() {
        let id = identity("parhelion.bulk.42bdcc00").unwrap();
        assert_eq!(id["item_hash"], "0x39A0CE76");
        assert_eq!(id.as_object().unwrap().len(), 12);
    }
}
