//! Configured source first-person controllers through normal import, recipe reload, fresh
//! staging and a readback of the selected item's route to its clips.
//!
//! Expectations come from an independent source oracle: specialized action routes with
//! descriptor names, source clip names, frame counts and weights, the native actions without a
//! source route, and the source hand-target marker frames. The readback follows the staged
//! item to its runtime entity, then follows raw payload references through the attachment owner,
//! first-person entity, lookup, bank, states, pose layers, skeletons and markers.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{
    marathon,
    tiger::{
        animation::{clip, control},
        payload::Payload,
        reader::Reader,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
    time::Instant,
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn hex(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(
        value.as_str().context("Hexadecimal identity")?,
        16,
    )?)
}

fn item(reader: &mut Reader, hash: u32) -> Result<u32> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|entry| entry.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let table = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let rows = table
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .filter(|&at| table.u32(at).ok() == Some(hash))
        .collect::<Vec<_>>();
    ensure!(rows.len() == 1, "Item {hash:08X} is missing or ambiguous");
    table.u32(rows[0] + 16)
}

/// Component owners of an entity as (owner tag, payload, definition, class).
/// A component: its tag, its owner payload, the owner's definition and that definition's class.
type Owner = (u32, std::sync::Arc<Payload>, usize, u32);

fn owners(reader: &mut Reader, entity: u32) -> Result<Vec<Owner>> {
    let e = reader.tag(entity, Some(0x80809C0F))?;
    e.array(16, 12, Some(0x80809C04))?
        .into_iter()
        .map(|row| {
            let tag = e.u32(row)?;
            let owner = reader.tag(tag, Some(0x80809C36))?;
            let data = owner.pointer(24)?;
            Ok((tag, owner.clone(), data, owner.u32(data - 4)?))
        })
        .collect()
}

fn one(
    list: &[(u32, std::sync::Arc<Payload>, usize, u32)],
    class: u32,
) -> Result<(u32, std::sync::Arc<Payload>, usize)> {
    let found = list.iter().filter(|o| o.3 == class).collect::<Vec<_>>();
    ensure!(
        found.len() == 1,
        "Component {class:08X} is absent or ambiguous"
    );
    Ok((found[0].0, found[0].1.clone(), found[0].2))
}

/// The runtime entity and the first-person entities its attachment selects for the item's
/// content key. Rows keyed to other items sharing the attachment owner are not reachable.
fn first_person(reader: &mut Reader, item_tag: u32) -> Result<(u32, BTreeSet<u32>)> {
    let report = parhelion_import::tiger::rig::inspect(reader, item_tag, false)?;
    let runtime = hex(&report["runtime_entity"])?;
    let content = hex(&report["content_key"])?;
    let (_, attachment, data) = one(&owners(reader, runtime)?, 0x80804221)?;
    let entities =
        parhelion_import::tiger::rig::attachment_rows(&attachment, data, content, false)?
            .into_iter()
            .map(|row| attachment.u32(row + 0x78))
            .collect::<Result<BTreeSet<_>>>()?;
    Ok((runtime, entities))
}

fn stock(stock: &mut Reader, tag: u32) -> bool {
    stock.tag(tag, None).is_ok()
}

fn rows(p: &Payload, field: usize, stride: usize, class: u32) -> Result<Vec<u8>> {
    Ok(p.0[p.array_range(field, stride, Some(class))?].to_vec())
}

/// A state node's kind and payload offset, `None` for a node without a payload.
fn state_node(p: &Payload, row: usize) -> Result<(u64, Option<usize>)> {
    Ok((
        p.u64(row)?,
        if p.u64(row + 8)? == 0 {
            None
        } else {
            Some(p.pointer(row + 8)?)
        },
    ))
}

fn names(skeleton: &Payload) -> Result<Vec<u32>> {
    skeleton
        .array(skeleton.pointer(24)? + 0x80, 16, Some(0x80808A08))?
        .into_iter()
        .map(|row| skeleton.u32(row))
        .collect()
}

#[test]
#[ignore = "Requires PARHELION_MARATHON_PACKAGES, SUNDIAL_STOCK_PACKAGES, PARHELION_MARATHON_CONTROLLER_CASES and fresh PARHELION_MARATHON_CONTROLLER_OUTPUT"]
#[allow(clippy::cognitive_complexity)]
fn source_controllers_route_staged_first_person_actions() -> Result<()> {
    let source = configured("PARHELION_MARATHON_PACKAGES")?.canonicalize()?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?.canonicalize()?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_CONTROLLER_OUTPUT")?,
        source.parent().context("Source root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(
        !output.exists(),
        "Use a fresh controller artifact directory"
    );
    let cases_path = configured("PARHELION_MARATHON_CONTROLLER_CASES")?;
    let cases: Vec<Value> = serde_json::from_slice(&fs::read(&cases_path)?)?;
    ensure!(!cases.is_empty(), "Required controller corpus is empty");
    fs::create_dir_all(&output)?;
    // Preparation reads the stock view the build compiles against. Packages an earlier install
    // wrote into the stock folder carry Sundial's header signature and are left out.
    let prepared = output.join("stock-view");
    let prepared_packages = prepared.join("packages");
    fs::create_dir_all(&prepared_packages)?;
    fs::create_dir_all(prepared.join("bin/x64"))?;
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        prepared.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    let mut excluded = Vec::new();
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|e| e != "pkg") {
            continue;
        }
        let mut header = [0u8; 16];
        std::io::Read::read_exact(&mut fs::File::open(entry.path())?, &mut header)?;
        if u64::from_le_bytes(header[8..16].try_into()?) == 0x1236_7498_D1CB_BA35 {
            excluded.push(entry.file_name().to_string_lossy().into_owned());
            continue;
        }
        fs::hard_link(entry.path(), prepared_packages.join(entry.file_name()))?;
    }
    let mut timings = BTreeMap::new();
    let started = Instant::now();
    let mut recipes = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        ensure!(
            case["expected_controller"]["actions"]
                .as_object()
                .is_some_and(|a| !a.is_empty()),
            "Case {index} has no independent route expectations"
        );
        let entry =
            marathon::browse::browse(&source, &output.join(format!("browse-{index}")), |_, _| {})?
                .entries
                .into_iter()
                .find(|entry| Some(u64::from(entry.item)) == case["source_item"].as_u64())
                .context("Required source item is absent")?;
        // A case may list the importer's candidate donors in order. The import must then choose
        // the case's donor, the first candidate that plays the source's own reloads.
        let expected_donor = u32::try_from(case["donor"].as_u64().context("Donor")?)?;
        let candidates = match case.get("donor_candidates").and_then(Value::as_array) {
            Some(rows) => rows
                .iter()
                .map(|row| {
                    Ok((
                        u32::try_from(row["hash"].as_u64().context("Candidate hash")?)?,
                        row["name"].as_str().context("Candidate name")?.to_owned(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
            None => vec![(
                expected_donor,
                case["donor_name"]
                    .as_str()
                    .context("Donor name")?
                    .to_owned(),
            )],
        };
        let path = marathon::browse::import(
            &entry,
            &candidates,
            true,
            &source,
            &prepared_packages,
            &output.join(format!("import-{index}")),
            &mut |_| {},
        )?;
        let recipe = WeaponRecipe::load_json(&path)?;
        ensure!(
            recipe.donor.item_hash.parse_u32()? == expected_donor,
            "The import chose donor {} instead of {expected_donor:08X}",
            recipe.donor.item_hash
        );
        let saved = output.join(format!("recipe-{index}.parhelion.json"));
        fs::write(&saved, recipe.to_json_pretty()?)?;
        recipes.push(WeaponRecipe::load_json(&saved)?);
    }
    timings.insert("import_and_reload_seconds", started.elapsed().as_secs_f64());
    let started = Instant::now();
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    timings.insert("build_and_stage_seconds", started.elapsed().as_secs_f64());
    let started = Instant::now();
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    let manifest: Value = serde_json::from_slice(&fs::read(
        built.run_directory.join("parhelion-manifest.json"),
    )?)?;
    let ignored = manifest["ignored_authored_files"]
        .as_array()
        .context("Ignored authored files")?
        .iter()
        .map(|row| row.as_str().context("Ignored authored filename"))
        .collect::<Result<BTreeSet<_>>>()?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if ignored.contains(name.as_str())
            || built.artifacts.iter().any(|a| a.file_name == name)
            || entry.path().extension().is_none_or(|e| e != "pkg")
        {
            continue;
        }
        fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
    }
    for artifact in &built.artifacts {
        let destination = packages.join(&artifact.file_name);
        ensure!(!destination.exists(), "Never overwrite a stock link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let mut original = Reader::new(&prepared_packages, &output.join("stock"), false)?;
    let mut evidence = Vec::new();
    for (recipe, case) in recipes.iter().zip(&cases) {
        let expected = &case["expected_controller"];
        let graph: Value = serde_json::from_slice(&fs::read(
            recipe
                .overrides
                .imported_graph
                .as_ref()
                .context("Imported graph")?
                .directory
                .join("asset-graph.json"),
        )?)?;
        let reported = graph["animation"]["first_person"]["unsupported_actions"]
            .as_array()
            .context("Unsupported action report")?
            .iter()
            .map(|row| {
                row["action"]
                    .as_str()
                    .map(str::to_owned)
                    .context("Unsupported action")
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let predicted = expected["unsupported_actions"]
            .as_object()
            .context("Predicted unsupported actions")?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        ensure!(
            reported == predicted,
            "Unsupported actions differ from the source prediction: {reported:?} versus {predicted:?}"
        );
        let paired = graph["animation"]["first_person"]["weapon_local_pairings"]
            .as_array()
            .context("Weapon-local pairing report")?
            .iter()
            .map(|row| {
                Ok((
                    format!(
                        "{}:{}",
                        row["action"].as_str().context("Paired action")?,
                        row["source_descriptor"]
                    ),
                    (row["weapon_descriptor"].clone(), row["chosen_by"].clone()),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        // Where the oracle cannot decode a curve clip, it lists every weapon descriptor the
        // closing test could lead to, and the graph's choice must be one of them.
        let predictions = expected["weapon_local_pairings"]
            .as_object()
            .context("Predicted weapon-local pairings")?;
        ensure!(
            paired.keys().collect::<BTreeSet<_>>() == predictions.keys().collect::<BTreeSet<_>>(),
            "Weapon-local pairing keys differ from the source prediction: {paired:?} versus {predictions:?}"
        );
        for (key, (weapon, how)) in &paired {
            let row = &predictions[key];
            let agrees = match row.get("accept").and_then(Value::as_array) {
                Some(accept) => accept.contains(weapon),
                None => *weapon == row["weapon"] && *how == row["how"],
            };
            ensure!(
                agrees,
                "Weapon-local pairing {key} is {weapon} by {how}, predicted {row}"
            );
        }
        let item_tag = item(&mut staged, recipe.identity.item_hash.parse_u32()?)?;
        let (runtime, entities) = first_person(&mut staged, item_tag)?;
        ensure!(
            entities.len() == 1,
            "Attachment rows select more than one first-person entity"
        );
        let entity = *entities.first().unwrap();
        ensure!(
            !stock(&mut original, entity),
            "First-person entity is the donor's"
        );
        let fp = owners(&mut staged, entity)?;
        let (lookup_tag, lookup, data) = one(&fp, 0x8080344B)?;
        ensure!(
            !stock(&mut original, lookup_tag),
            "Lookup owner is the donor's"
        );
        let bank_tag = lookup.u32(data + 0x90)?;
        let dictionary_tag = lookup.u32(data + 0x94)?;
        let states_tag = lookup.u32(data + 0x9C)?;
        for (tag, what) in [(bank_tag, "bank"), (states_tag, "state table")] {
            ensure!(
                !stock(&mut original, tag),
                "Lookup still names the donor {what}"
            );
        }
        // The carrier's own tables, read from the stock donor.
        let donor_tag = item(
            &mut original,
            u32::try_from(case["donor"].as_u64().context("Donor")?)?,
        )?;
        let (_, stock_entities) = first_person(&mut original, donor_tag)?;
        ensure!(
            stock_entities.len() == 1,
            "The donor's first-person entity is ambiguous"
        );
        let stock_fp = owners(&mut original, *stock_entities.first().unwrap())?;
        let (_, stock_lookup, stock_data) = one(&stock_fp, 0x8080344B)?;
        let stock_bank = original.tag(stock_lookup.u32(stock_data + 0x90)?, Some(0x808036F6))?;
        let stock_states = original.tag(stock_lookup.u32(stock_data + 0x9C)?, Some(0x80803465))?;
        // Untranslated states read the carrier's dictionary.
        ensure!(
            staged.tag(dictionary_tag, None)?.0
                == original.tag(stock_lookup.u32(stock_data + 0x94)?, None)?.0,
            "Lookup dictionary differs from the carrier's"
        );
        let bank = staged.tag(bank_tag, Some(0x808036F6))?;
        let clip_tags = bank
            .array(8, 4, Some(0x80808F48))?
            .into_iter()
            .map(|at| bank.u32(at))
            .collect::<Result<Vec<_>>>()?;
        // The carrier's clips and descriptors keep their ordinals, which its look table, pose
        // layers and untranslated states address. Translated rows follow them.
        let carrier_clips = stock_bank.array(8, 4, Some(0x80808F48))?.len();
        let carrier_descriptors = stock_bank.array(0x68, 32, Some(0x80809002))?.len();
        for (field, stride, class, what) in [
            (8, 4, 0x80808F48, "clips"),
            (0x18, 4, 0x80800007, "clip auxiliary words"),
            (0x68, 32, 0x80809002, "descriptors"),
        ] {
            ensure!(
                rows(&bank, field, stride, class)?.starts_with(&rows(
                    &stock_bank,
                    field,
                    stride,
                    class
                )?),
                "Bank changes the carrier's {what}"
            );
        }
        ensure!(
            clip_tags.len() > carrier_clips,
            "Bank holds no translated clip"
        );
        for &tag in &clip_tags[carrier_clips..] {
            ensure!(
                !stock(&mut original, tag),
                "Translated clip {tag:08X} is a stock clip"
            );
        }
        let descriptors = bank
            .array(0x68, 32, Some(0x80809002))?
            .into_iter()
            .map(|at| {
                Ok((
                    bank.u32(at + 16)?,
                    usize::from(bank.u16(at + 24)?),
                    bank.u16(at + 26)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let states = staged.tag(states_tag, Some(0x80803465))?;
        let nodes = states.array(24, 16, Some(0x8080342F))?;
        let stock_nodes = stock_states.array(24, 16, Some(0x8080342F))?;
        let mut stock_names = BTreeMap::new();
        for row in stock_states.array(8, 8, Some(0x8080342E))? {
            stock_names.insert(
                stock_states.u32(row)?,
                stock_nodes[stock_states.u32(row + 4)? as usize],
            );
        }
        let mut routed = BTreeMap::new();
        let mut fallback = 0;
        let mut emitted_names = BTreeSet::new();
        for row in states.array(8, 8, Some(0x8080342E))? {
            let action = states.u32(row)?;
            emitted_names.insert(action);
            let node = nodes[states.u32(row + 4)? as usize];
            let (kind, at) = state_node(&states, node)?;
            let weights = match (kind, at) {
                (1, Some(at)) => states.array(at, 8, Some(0x80803439))?,
                _ => Vec::new(),
            };
            let chosen = weights
                .iter()
                .map(|&w| Ok(states.u32(w)? as usize))
                .collect::<Result<Vec<_>>>()?;
            if chosen.iter().all(|&d| d < carrier_descriptors) {
                // An untranslated action keeps the carrier's node in place.
                let stock_node = *stock_names
                    .get(&action)
                    .with_context(|| format!("State {action:08X} is not the carrier's"))?;
                ensure!(
                    state_node(&states, node)? == state_node(&stock_states, stock_node)?,
                    "Untranslated action {action:08X} leaves the carrier's state"
                );
                fallback += 1;
                continue;
            }
            ensure!(
                chosen.iter().all(|&d| d >= carrier_descriptors),
                "Translated action {action:08X} mixes in carrier descriptors"
            );
            let mut route = Vec::new();
            for weight in weights {
                let (name, clip_index, mode) = descriptors[states.u32(weight)? as usize];
                ensure!(
                    clip_index >= carrier_clips,
                    "Translated descriptor {name:08X} plays a carrier clip"
                );
                let clip_payload = staged.tag(clip_tags[clip_index], Some(0x80808F49))?;
                route.push(json!({"name":format!("{name:08X}"),"weight":states.f32(weight + 4)?,
                    "clip_name":format!("{:08X}",clip_payload.u32(0x120)?),"frames":clip_payload.u16(0x13C)?,
                    "composition":clip_payload.u8(0x181)?,"mode":mode,"clip":clip_tags[clip_index]}));
            }
            routed.insert(format!("{action:08X}"), route);
        }
        ensure!(
            emitted_names == stock_names.keys().copied().collect::<BTreeSet<_>>(),
            "State names differ from the carrier's"
        );
        let want = expected["actions"]
            .as_object()
            .context("Expected actions")?;
        ensure!(
            routed.keys().collect::<BTreeSet<_>>() == want.keys().collect::<BTreeSet<_>>(),
            "Translated action names differ from the source routes"
        );
        for (action, route) in &routed {
            let expected_route = want[action].as_array().context("Expected route")?;
            ensure!(
                route.len() == expected_route.len(),
                "Route {action} changes its choices"
            );
            for (have, wanted) in route.iter().zip(expected_route) {
                ensure!(
                    have["name"] == wanted["name"]
                        && have["clip_name"] == wanted["clip_name"]
                        && have["frames"] == wanted["frames"]
                        && have["composition"] == wanted["composition"]
                        && (have["weight"].as_f64().unwrap() - wanted["weight"].as_f64().unwrap())
                            .abs()
                            < 1e-6
                        && have["mode"] == json!(if wanted["composition"] == 0 { 1 } else { 2 }),
                    "Route {action} differs from its source specialization: {have} versus {wanted}"
                );
                // Every emitted binding is mode 3, and each row the oracle expects disabled reads
                // a constant zero primary weight.
                let clip_tag = u32::try_from(have["clip"].as_u64().context("Clip tag")?)?;
                let clip_payload = staged.tag(clip_tag, Some(0x80808F49))?;
                let disabled = wanted["disabled_rows"]
                    .as_array()
                    .context("Expected disabled rows")?
                    .iter()
                    .map(|v| v.as_u64().map(|v| v as usize).context("Disabled row"))
                    .collect::<Result<BTreeSet<_>>>()?;
                if clip_payload.u64(0x30)? == 0 {
                    ensure!(disabled.is_empty(), "Clip {clip_tag:08X} lost its bindings");
                    continue;
                }
                let bindings = clip_payload.pointer(0x30)?;
                let rows = clip_payload.array(bindings, 64, Some(0x80808F5C))?;
                for &row in &rows {
                    ensure!(
                        clip_payload.u8(row + 18)? == 3,
                        "Clip {clip_tag:08X} keeps an untranslated constraint mode"
                    );
                }
                // Uniform weight codecs carry one range and one bias per binding row.
                for field in [0x38, 0x40] {
                    if clip_payload.u64(field)? == 0 {
                        continue;
                    }
                    let weight = clip_payload.pointer(field)?;
                    if clip_payload.u32(weight - 4)? != 0x80808F7C {
                        continue;
                    }
                    for array in [0x10, 0x30, 0x40] {
                        ensure!(
                            clip_payload.u64(weight + array)? as usize == rows.len(),
                            "Clip {clip_tag:08X} weight codec at {field:X} lacks per-binding array {array:X}"
                        );
                    }
                }
                if !disabled.is_empty() {
                    let weight = clip_payload.pointer(0x38)?;
                    ensure!(
                        clip_payload.u32(weight - 4)? == 0x80808F7C,
                        "Disabled rows need a uniform weight codec"
                    );
                    let selectors = clip_payload.array(weight + 0x10, 2, Some(0x80808F82))?;
                    let biases = clip_payload.array(weight + 0x40, 4, Some(0x8080000F))?;
                    for &index in &disabled {
                        ensure!(
                            clip_payload.u16(selectors[index])? >> 14 == 2
                                && clip_payload.f32(biases[index])? == 0.,
                            "Disabled constraint row {index} of {clip_tag:08X} keeps a weight"
                        );
                    }
                }
            }
        }
        // Translated clips emit the carrier's event kinds only: named events its own clips use,
        // resource references to live stock tags and sequence selectors, inside the clip.
        let record_rows = |p: &Payload| -> Result<Vec<usize>> {
            if p.u64(0x160)? == 0 {
                return Ok(Vec::new());
            }
            let header = p.pointer(0x168)?;
            (0..p.u64(0x160)? as usize)
                .map(|row| p.pointer(header + 16 + row * 8))
                .collect()
        };
        let mut vocabulary = BTreeSet::new();
        for &tag in &clip_tags[..carrier_clips] {
            let clip_payload = original.tag(tag, Some(0x80808F49))?;
            for at in record_rows(&clip_payload)? {
                if clip_payload.u32(at - 4)? == 0x80809031 {
                    vocabulary.insert((clip_payload.u32(at + 4)?, clip_payload.u32(at + 8)?));
                }
            }
        }
        let mut events = 0;
        let mut cues = BTreeMap::new();
        for &tag in &clip_tags[carrier_clips..] {
            let clip_payload = staged.tag(tag, Some(0x80808F49))?;
            let frames = u32::from(clip_payload.u16(0x13C)?);
            let rows = record_rows(&clip_payload)?;
            for &at in &rows {
                let kind = clip_payload.u32(at)?;
                ensure!(
                    kind & 0xFFFF < frames,
                    "Clip {tag:08X} cues past its last frame"
                );
                match clip_payload.u32(at - 4)? {
                    0x80809031 => ensure!(
                        kind >> 16 == 5
                            && vocabulary
                                .contains(&(clip_payload.u32(at + 4)?, clip_payload.u32(at + 8)?)),
                        "Clip {tag:08X} names a cue the carrier never uses"
                    ),
                    0x80809033 => {
                        let resource = clip_payload.u32(at + 0x18)?;
                        ensure!(
                            kind >> 16 == 3 && stock(&mut original, resource),
                            "Clip {tag:08X} references resource {resource:08X} outside the stock packages"
                        );
                    }
                    0x8080903A => {
                        ensure!(kind >> 16 == 2, "Clip {tag:08X} has a malformed selector")
                    }
                    class => anyhow::bail!("Clip {tag:08X} emits untranslated event {class:08X}"),
                }
                events += 1;
            }
            cues.insert(tag, rows.len());
        }
        // A reload plays the carrier's foley, so its clip carries cues.
        for action in ["6D507AD8", "28F43BD2"] {
            if let Some(route) = routed.get(action) {
                for choice in route {
                    let clip_tag = u32::try_from(choice["clip"].as_u64().context("Clip tag")?)?;
                    ensure!(
                        cues.get(&clip_tag).is_some_and(|&n| n > 0),
                        "Reload {action} clip {clip_tag:08X} carries no cues"
                    );
                }
            }
        }
        // The carrier's supplemental layers serve its own descriptors. A translated descriptor
        // dispatches to no supplemental pose or to the layer route the carrier gives one of the
        // actions that select it, checked below.
        let (pose_tag, pose, pose_data) = one(&fp, 0x80803640)?;
        let layers = staged.tag(pose.u32(pose_data + 0x11C)?, Some(0x808034EB))?;
        let (_, stock_pose, stock_pose_data) = one(&stock_fp, 0x80803640)?;
        let stock_layers =
            original.tag(stock_pose.u32(stock_pose_data + 0x11C)?, Some(0x808034EB))?;
        for (field, stride, class) in [
            (8, 24, 0x80803724),
            (24, 24, 0x80803721),
            (56, 32, 0x808034EF),
        ] {
            ensure!(
                rows(&layers, field, stride, class)? == rows(&stock_layers, field, stride, class)?,
                "Pose layer table changes the carrier's layers or routes"
            );
        }
        let dispatch = rows(&layers, 40, 2, 0x80800006)?;
        let stock_dispatch = rows(&stock_layers, 40, 2, 0x80800006)?;
        ensure!(
            dispatch.len() / 2 == descriptors.len() && dispatch.starts_with(&stock_dispatch),
            "Pose dispatch does not cover the bank or changes the carrier's entries"
        );
        // A translated descriptor dispatches to no supplemental pose or to a route the stock
        // carrier gives one of its selecting actions. A translated sprint loop takes the carrier's
        // route when the carrier has one, since source loops are held poses.
        let entry =
            |bytes: &[u8], at: usize| u16::from_le_bytes([bytes[at * 2], bytes[at * 2 + 1]]);
        let mut selecting = BTreeMap::<usize, BTreeSet<u32>>::new();
        for row in states.array(8, 8, Some(0x8080342E))? {
            let name = states.u32(row)?;
            let node = states.array(24, 16, Some(0x8080342F))?[states.u32(row + 4)? as usize];
            if states.u64(node)? != 1 {
                continue;
            }
            for weight in states.array(states.pointer(node + 8)?, 8, Some(0x80803439))? {
                selecting
                    .entry(states.u32(weight)? as usize)
                    .or_default()
                    .insert(name);
            }
        }
        let stock_routes = |action: u32| -> Result<BTreeSet<u16>> {
            if !stock_names.contains_key(&action) {
                return Ok(BTreeSet::new());
            }
            Ok(
                parhelion_import::tiger::animation::state::descriptors(&stock_states, action)?
                    .into_iter()
                    .map(|d| entry(&stock_dispatch, d))
                    .filter(|e| *e != u16::MAX)
                    .collect(),
            )
        };
        let mut layered = 0;
        for d in carrier_descriptors..descriptors.len() {
            let have = entry(&dispatch, d);
            let actions = selecting.get(&d).cloned().unwrap_or_default();
            if have != u16::MAX {
                let mut allowed = BTreeSet::new();
                for &action in &actions {
                    allowed.extend(stock_routes(action)?);
                }
                ensure!(
                    allowed.contains(&have),
                    "Translated descriptor {d} dispatches to layer route {have}, which no selecting action's carrier route uses"
                );
                layered += 1;
            }
            if actions.contains(&0xB9EE_E3BE) && !stock_routes(0xB9EE_E3BE)?.is_empty() {
                ensure!(
                    have != u16::MAX,
                    "The translated sprint loop has no supplemental layer"
                );
            }
        }
        ensure!(
            pose.u32(parhelion_import::tiger::animation::bank_field(&pose)?)? == bank_tag,
            "Pose controller {pose_tag:08X} names another bank"
        );
        // Source weapon bones live in both skeletons of the private pair.
        let (_, skeleton, _) = one(&fp, 0x80808546)?;
        let (_, controls, _) = one(&fp, 0x80808F8F)?;
        let runtime_owners = owners(&mut staged, runtime)?;
        let (_, equipment, _) = one(&runtime_owners, 0x80808546)?;
        let fp_names = names(&skeleton)?;
        let equipment_names = names(&equipment)?;
        let weapon = case["expected_weapon_bones"]
            .as_array()
            .context("Expected weapon bones")?;
        for bone in weapon {
            let name = hex(bone)?;
            ensure!(
                fp_names.contains(&name) && equipment_names.contains(&name),
                "Source weapon bone {name:08X} is absent from the private skeleton pair"
            );
        }
        // Hand targets come from the source placement.
        let (_, markers, _) = one(&runtime_owners, 0x80808507)?;
        let rows = parhelion_import::tiger::markers::read_native(&markers)?;
        for (name, frame) in expected["hand_targets"]
            .as_object()
            .context("Hand targets")?
        {
            let name = u32::from_str_radix(name, 16)?;
            let row = rows
                .iter()
                .find(|r| r.name == name)
                .context("Hand target marker is absent")?;
            for lane in 0..3 {
                ensure!(
                    (f64::from(row.position[lane]) - frame["position"][lane].as_f64().unwrap())
                        .abs()
                        < 1e-5,
                    "Hand target {name:08X} keeps a donor position"
                );
            }
        }
        // Targets the carrier lacks are appended from the source placement, bound to the root.
        for name in expected["appended_targets"]
            .as_array()
            .context("Appended hand targets")?
        {
            let name = hex(name)?;
            ensure!(
                rows.iter().any(|r| r.name == name && r.binding == [0, 0]),
                "Appended hand target {name:08X} is absent or bound to a bone"
            );
        }
        // Mechanism motion: decode each emitted reload clip and evaluate native FK. A reload the
        // oracle pairs with a weapon-local clip other than idle must move a weapon part relative
        // to the weapon root.
        let rig = control::Rig::read(&skeleton, &controls)?;
        let index = |n: u32| rig.names.iter().position(|x| *x == n);
        // The view reference keeps its source place in the idle state. The native rig rests it at
        // the origin and stock clips lift it to the eye, so a clip that leaves it at rest drops the
        // weapon and hands below the body.
        let view = &expected["view_reference"];
        let view_bone = index(hex(&view["bone"])?).context("Native view reference")?;
        let idle = routed
            .get("6FB760FF")
            .and_then(|route| route.iter().find(|r| r["composition"] == 0))
            .context("Absolute idle route")?;
        let idle_clip = staged.tag(
            u32::try_from(idle["clip"].as_u64().context("Idle clip")?)?,
            Some(0x80808F49),
        )?;
        let idle_objects = rig.objects(&clip::sample(&idle_clip, &rig.defaults, 0)?)?;
        let view_offset = (0..3)
            .map(|k| {
                (f64::from(idle_objects[view_bone].0[4 + k])
                    - view["position"][k].as_f64().unwrap())
                .powi(2)
            })
            .sum::<f64>()
            .sqrt();
        ensure!(
            view_offset < 0.25,
            "The view reference leaves its source place by {view_offset} m in the idle state"
        );
        // Native framing: the idle grip sits where the stock donor's idle holds its grip, in rig
        // space, from the donor's lowest absolute idle descriptor.
        let grip_name = 0xEF4B_AB19;
        let stock_rig = control::Rig::read(
            &one(&stock_fp, 0x80808546)?.1,
            &one(&stock_fp, 0x80808F8F)?.1,
        )?;
        let stock_rows = stock_bank.array(0x68, 32, Some(0x80809002))?;
        let stock_clip_rows = stock_bank.array(8, 4, Some(0x80808F48))?;
        let mut stock_grip = None;
        for descriptor in
            parhelion_import::tiger::animation::state::descriptors(&stock_states, 0x6FB7_60FF)?
        {
            let ordinal = stock_bank.u16(stock_rows[descriptor] + 24)?;
            if ordinal == u16::MAX {
                continue;
            }
            let stock_clip = original.tag(
                stock_bank.u32(stock_clip_rows[usize::from(ordinal)])?,
                Some(0x80808F49),
            )?;
            if stock_clip.u8(0x181)? != 0 {
                continue;
            }
            let (_, stock_controls_owner, _) = one(&stock_fp, 0x80808F8F)?;
            let values =
                clip::sample_common(&stock_clip, &stock_rig.defaults, &stock_controls_owner, 0)?;
            let objects = stock_rig.objects(&values)?;
            let at = stock_rig
                .names
                .iter()
                .position(|n| *n == grip_name)
                .context("Stock grip")?;
            stock_grip = Some([objects[at].0[4], objects[at].0[5], objects[at].0[6]]);
            break;
        }
        let stock_grip = stock_grip.context("Stock donor has no absolute idle")?;
        let staged_grip = idle_objects[index(grip_name).context("Native grip")?].0;
        let framing_error = (0..3)
            .map(|k| (staged_grip[4 + k] - stock_grip[k]).powi(2))
            .sum::<f32>()
            .sqrt();
        ensure!(
            framing_error < 0.01,
            "The idle grip sits {framing_error} m from the stock donor's idle grip"
        );
        let handle = index(hex(&weapon[0])?).context("Native weapon root")?;
        let parts = weapon[1..]
            .iter()
            .map(|bone| index(hex(bone)?).context("Native weapon part"))
            .collect::<Result<Vec<_>>>()?;
        // The graph's own pairing decides, since the oracle may accept idle where it cannot
        // decode a curve clip. A reload paired with anything but idle must move a part.
        let pairings = graph["animation"]["first_person"]["weapon_local_pairings"]
            .as_array()
            .context("Weapon-local pairing report")?
            .iter()
            .map(|row| {
                (
                    format!(
                        "{}:{}",
                        row["action"].as_str().unwrap_or(""),
                        row["source_descriptor"]
                    ),
                    row.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut motion = BTreeMap::new();
        for action in ["6D507AD8", "28F43BD2"] {
            let Some(route) = routed.get(action) else {
                continue;
            };
            let source_descriptor = &want[action][0]["descriptor"];
            let paired = pairings
                .get(&format!("{action}:{source_descriptor}"))
                .is_some_and(|p| p["chosen_by"] != "idle");
            let clip_tag = u32::try_from(route[0]["clip"].as_u64().unwrap())?;
            let clip_payload = staged.tag(clip_tag, Some(0x80808F49))?;
            let frames = usize::from(clip_payload.u16(0x13C)?);
            let mut start = BTreeMap::new();
            let mut travel = vec![0f32; parts.len()];
            let mut grip = 0f32;
            for frame in 0..frames {
                let values = clip::sample(&clip_payload, &rig.defaults, frame)?;
                let objects = rig.objects(&values)?;
                for (i, &part) in parts.iter().enumerate() {
                    let relative = objects[handle].inverse().then(objects[part]);
                    let here = [relative.0[4], relative.0[5], relative.0[6]];
                    let first = *start.entry(i).or_insert(here);
                    travel[i] = travel[i].max(
                        (0..3)
                            .map(|k| (here[k] - first[k]).powi(2))
                            .sum::<f32>()
                            .sqrt(),
                    );
                }
                let parent = rig.parents[handle].context("Weapon root parent")?;
                let held = objects[parent].inverse().then(objects[handle]);
                grip = grip.max(held.0[4..7].iter().map(|v| v * v).sum::<f32>().sqrt());
            }
            let (most, distance) = travel
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or((None, 0.), |(i, d)| (Some(&weapon[i + 1]), *d));
            ensure!(
                !paired || distance > 0.02,
                "Reload {action} leaves every weapon part fixed ({distance} m)"
            );
            motion.insert(action, json!({"clip":clip_tag,"frames":frames,"paired_weapon_clip":paired,
                "moving_part":most,"part_travel_metres":distance,"handle_offset_from_grip_metres":grip}));
        }
        // The stock donor keeps its own route.
        let donor_item = item(&mut staged, u32::try_from(case["donor"].as_u64().unwrap())?)?;
        let (_, staged_entities) = first_person(&mut staged, donor_item)?;
        ensure!(
            stock_entities == staged_entities,
            "The stock donor's first-person route changed"
        );
        evidence.push(json!({"item":recipe.identity.item_hash.parse_u32()?,"name":recipe.name,"runtime":runtime,
            "first_person":entity,"lookup":lookup_tag,"bank":bank_tag,"states":states_tag,"clips":clip_tags.len(),
            "carrier_clips":carrier_clips,"descriptors":descriptors.len(),"carrier_descriptors":carrier_descriptors,
            "routes":routed,"carrier_states":fallback,"events":events,"motion":motion,
            "weapon_bones":weapon.len(),"view_reference_offset_metres":view_offset,
            "layered_translated_descriptors":layered,
            "idle_grip":[staged_grip[4],staged_grip[5],staged_grip[6]],"stock_idle_grip":stock_grip,"framing_error_metres":framing_error}));
    }
    staged.finish()?;
    original.finish()?;
    timings.insert("readback_seconds", started.elapsed().as_secs_f64());
    let hashes = built
        .artifacts
        .iter()
        .map(|a| {
            Ok((
                a.file_name.clone(),
                hex::encode(Sha256::digest(fs::read(
                    built.run_directory.join(&a.file_name),
                )?)),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    fs::write(
        output.join("verified-controllers.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":cases_path,"cases_sha256":hex::encode(Sha256::digest(fs::read(&cases_path)?)),
            "items":evidence,"staged":built.run_directory,"packages":hashes,"timings":timings,
            "preparation_excluded_installed_packages":excluded,
            "converter_revision":parhelion_import::CONVERTER_REVISION,
            "limits":"Staged references, specialized routes, emitted events, native FK of decoded clips and stock isolation. Live playback, IK contact and gameplay are separate checks.",
            "gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
