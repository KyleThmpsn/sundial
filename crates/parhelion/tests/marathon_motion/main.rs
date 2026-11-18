//! Fresh catalog imports through persisted recipes, staged native reload curves and arm clips.
//! Independent source expectations name the curves. The retained motion payloads are inputs to
//! original-instruction FK and limb replay, which is separate from this package readback.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{
    marathon,
    tiger::{
        payload::Payload,
        reader::{Reader, package_stamp},
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use sundial::package_authoring::{
    entity::{WEAPON_STAT_TRANSLATOR_COMPONENT_KEY, weapon_component_bindings},
    runtime,
};

mod equipment;
mod handling;
mod layers;
mod magazine;
mod reload;

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}
fn number(value: &Value) -> Result<u32> {
    Ok(u32::try_from(value.as_u64().context("Numeric identity")?)?)
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn stat_score(reader: &mut Reader, identity: u32, stat: u32) -> Result<i32> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = reader.tag(globals, None)?;
    let root = reader.tag(globals.u32(16)?, Some(0x80807D84))?;
    let definitions = reader.tag(root.u32(8 + 95 * 16)?, None)?;
    let definition = definitions
        .array(8, 32, Some(0x80807D09))?
        .iter()
        .position(|&r| definitions.u32(r).ok() == Some(stat))
        .context("Requested stat definition")?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let row = items
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .find(|&r| items.u32(r).ok() == Some(identity))
        .context("Authored item")?;
    let item = reader.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
    let resource = item.pointer(0x70)?;
    ensure!(
        item.u32(resource - 4)? == 0x808077B9,
        "Item stat resource differs"
    );
    let row = item
        .array(resource, 40, Some(0x80803033))?
        .into_iter()
        .find(|&r| item.u16(r).ok() == Some(definition as u16))
        .context("Authored stat")?;
    Ok(i32::from_le_bytes(item.bytes(row + 4)?))
}
type Owner = (u32, Arc<Payload>, usize, u32);
fn owners(reader: &mut Reader, entity: u32) -> Result<Vec<Owner>> {
    let entity = reader.tag(entity, Some(0x80809C0F))?;
    entity
        .array(16, 12, Some(0x80809C04))?
        .into_iter()
        .map(|row| {
            let tag = entity.u32(row)?;
            let owner = reader.tag(tag, Some(0x80809C36))?;
            let base = owner.pointer(24)?;
            Ok((tag, owner.clone(), base, owner.u32(base - 4)?))
        })
        .collect()
}
fn one(owners: &[Owner], class: u32) -> Result<(u32, Arc<Payload>, usize)> {
    let found = owners.iter().filter(|o| o.3 == class).collect::<Vec<_>>();
    ensure!(found.len() == 1, "Ambiguous component {class:08X}");
    Ok((found[0].0, found[0].1.clone(), found[0].2))
}
fn view(
    native: &Path,
    destination: &Path,
    replacements: &BTreeMap<String, PathBuf>,
) -> Result<PathBuf> {
    let packages = destination.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(destination.join("bin/x64"))?;
    fs::hard_link(
        native.parent().context("Native root")?.join("destiny2.exe"),
        destination.join("destiny2.exe"),
    )?;
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        destination.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for entry in fs::read_dir(native)? {
        let entry = entry?;
        if entry.path().extension().is_none_or(|e| e != "pkg") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if replacements.contains_key(&name) {
            continue;
        }
        let mut header = [0u8; 16];
        std::io::Read::read_exact(&mut fs::File::open(entry.path())?, &mut header)?;
        if u64::from_le_bytes(header[8..16].try_into()?) == 0x12367498D1CBBA35 {
            continue;
        }
        fs::hard_link(entry.path(), packages.join(name))?;
    }
    for (name, path) in replacements {
        fs::copy(path, packages.join(name))?;
    }
    Ok(packages)
}

/// Follow the selected runtime translator, then each destination's actual curve reference.
fn reload(reader: &mut Reader, entity: &[u8], key: u32, expected: &Value) -> Result<Value> {
    let bindings = weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
        .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        anyhow::bail!("Reload translator binding differs");
    };
    let owner = reader.tag(binding.owner_tag, Some(0x80809C36))?;
    let base = owner.pointer(24)?;
    let groups = owner.array(base + 0x48, 48, Some(0x80803975))?;
    let keys = owner.array(base + 0x238, 40, Some(0x808038C7))?;
    ensure!(
        !groups.is_empty() && groups.len() == keys.len(),
        "Reload groups differ"
    );
    let group = groups[keys
        .iter()
        .position(|&r| owner.u32(r + 16).ok() == Some(key))
        .unwrap_or(0)];
    let curves = owner.array(group, 56, Some(0x80803979))?;
    let routes = owner.array(group + 16, 80, Some(0x80803981))?;
    let mut readback = Vec::new();
    for want in expected
        .as_array()
        .context("Independent source reload curves")?
    {
        let input = number(&want["input"])?;
        let writers = routes
            .iter()
            .copied()
            .filter(|&r| owner.u32(r).ok() == Some(3) && owner.u32(r + 4).ok() == Some(input))
            .collect::<Vec<_>>();
        let [row] = writers.as_slice() else {
            anyhow::bail!("Reload has no unique writer");
        };
        ensure!(
            owner.u32(row + 12)? == 4
                && owner.bytes::<12>(row + 16)? == [0; 12]
                && owner.u32(row + 32)? == 0x7F7FFFFF,
            "Reload no longer reads a curve"
        );
        let channel = owner
            .u32(row + 28)?
            .checked_sub(0x1E00)
            .context("Reload channel")? as usize;
        let curve = *curves.get(channel / 32).context("Reload curve extent")?;
        let metadata = want["metadata"]
            .as_array()
            .context("Source curve metadata")?;
        ensure!(
            owner.u32(curve + 32)? == 9
                && owner.u32(curve + 36)? == 0
                && owner.u32(curve + 40)? == number(&metadata[3])?
                && owner.u32(curve + 44)? == number(&metadata[4])?
                && owner.u32(curve + 48)? == number(&metadata[5])?,
            "Source reload sampling domain was lost"
        );
        let samples = owner
            .array(curve + 16, 16, Some(0x80809F2A))?
            .into_iter()
            .map(|tier| {
                let cells = owner.array(tier, 4, Some(0x8080000F))?;
                owner.f32(*cells.get(channel % 32).context("Reload column")?)
            })
            .collect::<Result<Vec<_>>>()?;
        let expected = want["seconds"]
            .as_array()
            .context("Source reload samples")?
            .iter()
            .map(|x| x.as_f64().context("Source seconds").map(|x| x as f32))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            samples == expected,
            "Staged reload {input} differs from the independently decoded source curve"
        );
        ensure!(
            samples.iter().copied().reduce(f32::min) != samples.iter().copied().reduce(f32::max),
            "Required reload curve is constant"
        );
        readback
            .push(json!({"input":input,"channel":channel+0x1E00,"curve":curve,"seconds":samples}));
    }
    ensure!(readback.len() == 2, "Both reload clocks must be checked");
    Ok(json!({"owner":binding.owner_tag,"sha256":digest(&owner.0),"curves":readback}))
}

#[test]
#[ignore = "Requires configured Marathon and stock packages, independent motion cases and a fresh output directory"]
fn source_reload_curves_and_motion_survive_staging() -> Result<()> {
    let source = configured("PARHELION_MARATHON_PACKAGES")?.canonicalize()?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?.canonicalize()?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_MOTION_OUTPUT")?,
        source.parent().context("Source root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh motion output");
    fs::create_dir_all(&output)?;
    let cases_path = configured("PARHELION_MARATHON_MOTION_CASES")?;
    let cases_bytes = fs::read(&cases_path)?;
    let cases: Vec<Value> = serde_json::from_slice(&cases_bytes)?;
    ensure!(!cases.is_empty(), "Required motion corpus is empty");
    let source_stamp = package_stamp(&source)?;
    let native_stamp = package_stamp(&native)?;
    let stock = view(&native, &output.join("stock-view"), &BTreeMap::new())?;
    let listing = marathon::browse::browse(&source, &output.join("listing"), |_, _| {})?;
    let mut recipes = Vec::new();
    let mut clocks = BTreeMap::new();
    let started = Instant::now();
    for (index, case) in cases.iter().enumerate() {
        let item = number(&case["source_item"])?;
        let entry = listing
            .entries
            .iter()
            .find(|e| e.item == item)
            .context("Source item absent")?;
        let donors = case["donors"]
            .as_array()
            .context("Native donor candidates")?
            .iter()
            .map(|d| {
                Ok((
                    number(&d["hash"])?,
                    d["name"].as_str().context("Donor name")?.to_owned(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let path = marathon::browse::import(
            entry,
            &donors,
            marathon::browse::Options {
                source_animation: true,
                ornaments: false,
            },
            &source,
            &stock,
            &output.join(format!("import-{index}")),
            &mut |m| println!("{}: {m}", entry.name),
        )?;
        let recipe = WeaponRecipe::load_json(&path)?;
        let saved = output.join(format!("recipe-{index}.parhelion.json"));
        fs::write(&saved, recipe.to_json_pretty()?)?;
        recipes.push(WeaponRecipe::load_json(&saved)?);
    }
    clocks.insert("import_seconds", started.elapsed().as_secs_f64());
    let started = Instant::now();
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: stock.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built =
        parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
            .map_err(anyhow::Error::msg)?;
    clocks.insert("stage_seconds", started.elapsed().as_secs_f64());
    let replacements = built
        .artifacts
        .iter()
        .map(|a| (a.file_name.clone(), built.run_directory.join(&a.file_name)))
        .collect();
    let packages = view(&stock, &output.join("view"), &replacements)?;
    let mut reader = Reader::new(&packages, &output.join("readback"), false)?;
    let mut original = Reader::new(&stock, &output.join("stock"), false)?;
    let manager = sundial::package_authoring::PackageManager::new(
        &packages,
        reader.manager.version,
        Some(reader.manager.platform),
    )
    .map_err(anyhow::Error::msg)?;
    let mut evidence = Vec::new();
    for (index, (recipe, case)) in recipes.iter().zip(&cases).enumerate() {
        let item = recipe.identity.item_hash.parse_u32()?;
        let score = stat_score(&mut reader, item, 0xF9A04987)?;
        ensure!(
            Some(i64::from(score)) == case["reload_stat"].as_i64(),
            "Authored Reload Speed no longer uses the source score"
        );
        let runtime = runtime::load_runtime_entity_for_item_with_manager(&manager, item)
            .map_err(anyhow::Error::msg)?;
        let reload = reload(
            &mut reader,
            &runtime.payload,
            runtime.weapon_translation_group_hash,
            &case["curves"],
        )?;
        let graph_dir = &recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?
            .directory;
        let graph: Value = serde_json::from_slice(&fs::read(graph_dir.join("asset-graph.json"))?)?;
        let fp = &graph["animation"]["first_person"];
        ensure!(
            fp["controller"] == "marathon_source",
            "Source controller missing"
        );
        let runtime_owners = owners(&mut reader, runtime.entity_tag)?;
        let (_, attachment, data) = one(&runtime_owners, 0x80804221)?;
        let content = runtime.weapon_content_group_hash;
        // Follow the authored item's selected content rows to its private first-person entity.
        let rows =
            parhelion_import::tiger::rig::attachment_rows(&attachment, data, content, false)?;
        let entities = rows
            .into_iter()
            .map(|r| attachment.u32(r + 0x78))
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            entities.len() == 1,
            "Selected first-person entity is ambiguous"
        );
        let first_person = *entities.first().context("First-person entity")?;
        let components = owners(&mut reader, first_person)?;
        let (_, lookup, base) = one(&components, 0x8080344B)?;
        let bank = reader.tag(lookup.u32(base + 0x90)?, Some(0x808036F6))?;
        let bank_clips = bank.array(8, 4, Some(0x80808F48))?;
        let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
        let native_clips = fp["source_clips"].as_array().context("Source clips")?;
        let first = descriptors
            .len()
            .checked_sub(
                fp["descriptors"]
                    .as_array()
                    .context("Added descriptors")?
                    .len(),
            )
            .context("Descriptor count")?;
        let replay = output.join(format!("motion-{index}"));
        fs::create_dir_all(&replay)?;
        let mut rigs = Vec::new();
        for (kind, class) in [("skeleton", 0x80808546), ("controls", 0x80808F8F)] {
            let (tag, payload, _) = one(&components, class)?;
            let file = format!("{kind}.bin");
            fs::write(replay.join(&file), &payload.0)?;
            rigs.push(json!({"kind":kind,"file":file,"tag":tag,"sha256":digest(&payload.0),"first_person":true}));
        }
        let source_bank = Payload(fs::read(
            graph_dir.join(
                fp["files"]["converted_bank"]
                    .as_str()
                    .context("Prepared bank")?,
            ),
        )?);
        let source_tags = source_bank.array(8, 4, Some(0x80808F48))?;
        let source_descriptors = source_bank.array(0x68, 32, Some(0x80809002))?;
        ensure!(
            source_descriptors.len() == descriptors.len(),
            "Staging changed the action descriptor population"
        );
        let mut retained = BTreeSet::new();
        let mut clips = Vec::new();
        for (descriptor_index, &descriptor) in descriptors.iter().enumerate().skip(first) {
            let prepared_descriptor = source_descriptors[descriptor_index];
            ensure!(
                bank.u32(descriptor + 20)? == source_bank.u32(prepared_descriptor + 20)?
                    && bank.u8(descriptor + 26)? == source_bank.u8(prepared_descriptor + 26)?,
                "Staging changed an action's duration or playback mode"
            );
            let ordinal = usize::from(bank.u16(descriptor + 24)?);
            let tag = bank.u32(*bank_clips.get(ordinal).context("Clip ordinal")?)?;
            if !retained.insert(tag) {
                continue;
            }
            let prepared_ordinal = usize::from(source_bank.u16(prepared_descriptor + 24)?);
            let placeholder = source_bank.u32(
                *source_tags
                    .get(prepared_ordinal)
                    .context("Prepared clip ordinal")?,
            )?;
            let source_clip = native_clips
                .iter()
                .find(|c| c["placeholder"].as_u64() == Some(u64::from(placeholder)))
                .context("Translated clip missing")?;
            let payload = reader.tag(tag, Some(0x80808F49))?;
            let prepared = graph_dir.join(source_clip["file"].as_str().context("Prepared clip")?);
            let expected = Payload(fs::read(&prepared)?);
            // Staging may recompress channels using the carrier's codec. The external replay
            // decodes both files, checks motion preservation and executes the staged arm solve.
            ensure!(
                payload.u16(0x13C)? == expected.u16(0x13C)?
                    && payload.u16(0x140)? == expected.u16(0x140)?
                    && payload.u8(0x181)? == expected.u8(0x181)?,
                "Staging changed clip timing, control coverage or composition"
            );
            let file = format!("{tag:08X}.bin");
            fs::write(replay.join(&file), &payload.0)?;
            clips.push(json!({"file":file,"tag":tag,"source":source_clip["source"],"sha256":digest(&payload.0),"frames":payload.u16(0x13C)?,"prepared":prepared,"prepared_sha256":digest(&expected.0)}));
        }
        ensure!(
            !clips.is_empty(),
            "No translated motion reached the package"
        );
        // Stock payload identity, not a report, establishes donor isolation.
        let donor = recipe.donor.item_hash.parse_u32()?;
        if let Some(expected) = case.get("reload_runtime") {
            ensure!(
                Some(u64::from(donor)) == expected["donor"].as_u64(),
                "Magazine reload selected a different firing donor"
            );
        }
        let donor_runtime = runtime::load_runtime_entity_for_item_with_manager(&manager, donor)
            .map_err(anyhow::Error::msg)?;
        let stock_payload = original.tag(donor_runtime.entity_tag, Some(0x80809C0F))?;
        ensure!(
            stock_payload.0 == donor_runtime.payload,
            "Stock donor runtime changed"
        );
        for (tag, _, _, _) in owners(&mut original, donor_runtime.entity_tag)? {
            ensure!(
                original.tag(tag, None)?.0 == reader.tag(tag, None)?.0,
                "Stock donor owner changed"
            );
        }
        let layers = layers::readback(
            &mut reader,
            &mut original,
            &components,
            &donor_runtime,
            &bank,
            first,
            &replay,
        )?;
        let reload_runtime = reload::readback(
            (&mut reader, &mut original, &manager),
            (
                &runtime.payload,
                runtime.weapon_translation_group_hash,
                runtime.weapon_content_group_hash,
            ),
            (
                &donor_runtime.payload,
                donor_runtime.weapon_content_group_hash,
            ),
            (&lookup, base),
            &case["reload_runtime"],
            &replay,
        )?;
        let magazine = magazine::readback(
            &mut reader,
            &mut original,
            &runtime,
            &donor_runtime,
            &case["magazine"],
        )
        .with_context(|| format!("{} magazine readback", recipe.name))?;
        let equipment = equipment::readback(
            &mut reader,
            &mut original,
            &runtime,
            &donor_runtime,
            &case["equipment_actions"],
            &replay,
        )?;
        let handling = handling::readback(
            (&mut reader, &mut original),
            (&runtime, &donor_runtime),
            item,
            case,
        )?;
        fs::write(replay.join("bank.bin"), &bank.0)?;
        let manifest = json!({"native_motion":true,"rigs":rigs,"clips":clips,"source_graph":graph_dir.join("asset-graph.json"),"item":item,"name":recipe.name,"first_person":first_person,
            "bank":{"file":"bank.bin","sha256":digest(&bank.0)},"layers":layers});
        fs::write(
            replay.join("motion.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        evidence.push(json!({"item":item,"name":recipe.name,"reload":reload,"reload_score":score,"handling":handling,"motion":replay.join("motion.json"),"donor":donor,"reload_runtime":reload_runtime,"magazine":magazine,"equipment":equipment}));
    }
    reader.finish()?;
    original.finish()?;
    ensure!(
        source_stamp == package_stamp(&source)? && native_stamp == package_stamp(&native)?,
        "Input packages changed during verification"
    );
    let hashes = built
        .artifacts
        .iter()
        .map(|a| {
            Ok((
                a.file_name.clone(),
                digest(&fs::read(built.run_directory.join(&a.file_name))?),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    fs::write(
        output.join("verified-motion.json"),
        serde_json::to_vec_pretty(
            &json!({"revision":env::var("SUNDIAL_TEST_REVISION")?,"test_binary_sha256":digest(&fs::read(env::current_exe()?)?),"cases_sha256":digest(&cases_bytes),"source_stamp":source_stamp,"native_stamp":native_stamp,"timings":clocks,"items":evidence,"packages":hashes,"limits":"Persisted import and native package readback. Original FK and limb replay consume the retained motion manifest separately. Live playback and gameplay are not established."}),
        )?,
    )?;
    Ok(())
}
