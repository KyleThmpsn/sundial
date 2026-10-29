//! Configured source sight conversion through saved recipes and the staged item's selected art.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{GraphReference, marathon, tiger::reader::Reader};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        ensure!(
            !entry.file_type()?.is_symlink(),
            "Graph contains a symbolic link"
        );
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn close(actual: f32, expected: &Value) -> Result<()> {
    let expected = expected.as_f64().context("Independent source value")?;
    ensure!(
        (f64::from(actual) - expected).abs() < 0.00001,
        "Sight value {actual} differs from source {expected}"
    );
    Ok(())
}

fn verify(reader: &mut Reader, entity_tag: u32, expected: &Value) -> Result<Vec<Value>> {
    let entity = reader.tag(entity_tag, Some(0x80809C0F))?;
    let mut results = Vec::new();
    for (index, component) in entity
        .array(16, 12, Some(0x80809C04))?
        .into_iter()
        .enumerate()
    {
        let tag = entity.u32(component)?;
        let owner = reader.tag(tag, Some(0x80809C36))?;
        let data = owner.pointer(24)?;
        if owner.u32(data - 4)? != 0x8080393B {
            continue;
        }
        close(owner.f32(data + 0x80)?, &expected["angle"])?;
        let mut points = Vec::new();
        for (i, descriptor) in [0x198, 0x228].into_iter().enumerate() {
            ensure!(
                owner.u32(data + descriptor - 32)? == 1,
                "Sight still inherits donor points"
            );
            let rows = owner.array(data + descriptor, 64, Some(0x80809C00))?;
            ensure!(rows.len() == 1, "Sight has no unique explicit point");
            let row = rows[0];
            ensure!(owner.u64(row)? == 0, "Sight is bound to a donor bone");
            for lane in 0..4 {
                close(
                    owner.f32(row + 16 + lane * 4)?,
                    &expected["points"][i]["rotation"][lane],
                )?;
                close(
                    owner.f32(row + 32 + lane * 4)?,
                    &expected["points"][i]["position"][lane],
                )?;
            }
            let events = entity
                .array(32, 72, Some(0x80809BC9))?
                .into_iter()
                .filter(|&at| {
                    entity.u32(at + 8).ok() == Some(tag)
                        && entity.u32(at + 12).ok() == Some(0x80809BD7)
                        && entity.u64(at + 16).ok() == Some((data + descriptor - 64) as u64)
                })
                .collect::<Vec<_>>();
            ensure!(events.len() == 1, "Sight has no unique movement input");
            let at = events[0];
            ensure!(
                entity.u64(at + 24)? == index as u64
                    && entity.u64(at + 32)? == 0xAE7D0AED
                    && entity.u64(at + 40)? == u64::MAX
                    && entity.u64(at + 48)? == 0
                    && entity.u64(at + 56)? == 0x3FFFF
                    && entity.u64(at + 64)? == 0,
                "Sight still composes with donor aim movement"
            );
            points.push(json!({"row":row,"movement":at}));
        }
        results.push(json!({"entity":entity_tag,"optic":tag,"points":points}));
    }
    Ok(results)
}

fn verify_mechanisms(reader: &mut Reader, entity_tag: u32, expected: &Value) -> Result<Vec<Value>> {
    let entity = reader.tag(entity_tag, Some(0x80809C0F))?;
    let mut evidence = Vec::new();
    for row in entity.array(16, 12, Some(0x80809C04))? {
        let owner = reader.tag(entity.u32(row)?, Some(0x80809C36))?;
        let data = owner.pointer(24)?;
        if owner.u32(data - 4)? != 0x808072BD {
            continue;
        }
        let model = reader.tag(owner.u32(data + 0x1DC)?, Some(0x808073A5))?;
        for mesh in model.array(16, 136, Some(0x80807378))? {
            if model.array(mesh + 24, 32, Some(0x8080737E))?.is_empty() {
                continue;
            }
            let header = reader.tag(model.u32(mesh)?, None)?;
            let positions = reader.tag(reader.reference(model.u32(mesh)?)?, None)?;
            ensure!(
                header.u16(4)? == 16,
                "Weapon mechanisms have no skin stream"
            );
            let mut weights = BTreeMap::<u8, u64>::new();
            for vertex in positions.0.chunks_exact(16) {
                ensure!(
                    vertex[8..12].iter().map(|v| u16::from(*v)).sum::<u16>() == 255,
                    "Native vertex weights are not normalized"
                );
                for lane in 0..4 {
                    if vertex[8 + lane] != 0 {
                        *weights.entry(vertex[12 + lane]).or_default() +=
                            u64::from(vertex[8 + lane]);
                    }
                }
            }
            ensure!(
                model.u32(0x40)?
                    == expected["palette"].as_u64().context("Expected palette")? as u32,
                "Weapon palette differs from its equipment rig"
            );
            let actual = serde_json::to_value(&weights)?;
            ensure!(
                actual == expected["weights"],
                "Mechanism weights differ from source bindings: {actual}"
            );
            evidence.push(
                json!({"entity":entity_tag,"weights":actual,"vertices":positions.0.len()/16}),
            );
        }
    }
    Ok(evidence)
}

#[test]
#[ignore = "Requires PARHELION_MARATHON_PACKAGES, SUNDIAL_STOCK_PACKAGES, PARHELION_MARATHON_SIGHT_CASES and fresh PARHELION_MARATHON_SIGHT_OUTPUT"]
#[allow(clippy::cognitive_complexity)]
fn marathon_sights_survive_recipe_reload_and_selected_package_art() -> Result<()> {
    let source = configured("PARHELION_MARATHON_PACKAGES")?.canonicalize()?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?.canonicalize()?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_MARATHON_SIGHT_OUTPUT")?,
        source.parent().context("Source root")?,
    )?;
    let output =
        parhelion_import::tiger::reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh sight artifact directory");
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_MARATHON_SIGHT_CASES")?)?)?;
    ensure!(!cases.is_empty(), "Required sight corpus is empty");
    fs::create_dir_all(&output)?;
    let mut source_reader = Reader::marathon(&source, &output.join("source"))?;
    let mut native_reader = Reader::new(&native, &output.join("native"), false)?;
    let mut recipes = Vec::new();
    let mut originals = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut recipe = if let Some(item) = case.get("source_item") {
            let entry = marathon::browse::browse(
                &source,
                &output.join(format!("browse-{index}")),
                |_, _| {},
            )?
            .entries
            .into_iter()
            .find(|entry| Some(u64::from(entry.item)) == item.as_u64())
            .context("Required source item is absent")?;
            let path = marathon::browse::import(
                &entry,
                &[(
                    u32::try_from(case["donor"].as_u64().context("Donor")?)?,
                    case["donor_name"]
                        .as_str()
                        .context("Donor name")?
                        .to_owned(),
                )],
                marathon::browse::Options::default(),
                &source,
                &native,
                &output.join(format!("import-{index}")),
                &mut |_| {},
            )?;
            WeaponRecipe::load_json(&path)?
        } else {
            WeaponRecipe::load_json(Path::new(case["recipe"].as_str().context("Recipe")?))?
        };
        let item = recipe.identity.item_hash.parse_u32()?;
        let original = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Graph")?
            .clone();
        original.validate(item)?;
        let graph_path = output.join(format!("graph-{index}"));
        copy_tree(&original.directory, &graph_path)?;
        let mut graph: Value =
            serde_json::from_slice(&fs::read(graph_path.join("asset-graph.json"))?)?;
        if let Some(assembly) = case.get("source_assembly") {
            graph["source_assembly"] = assembly.clone();
        }
        marathon::author_sights(
            &mut source_reader,
            &mut native_reader,
            &graph_path,
            &mut graph,
        )?;
        if let Some(source_item) = case
            .get("source_item_identity")
            .or_else(|| case.get("source_item"))
            .and_then(Value::as_u64)
        {
            marathon::author_gameplay(
                &mut source_reader,
                &mut native_reader,
                &mut graph,
                u32::try_from(source_item)?,
            )?;
            let mut value = serde_json::to_value(&recipe)?;
            marathon::apply_gameplay(&mut value, &graph)?;
            recipe = serde_json::from_value(value)?;
        }
        fs::write(
            graph_path.join("asset-graph.json"),
            serde_json::to_vec_pretty(&graph)?,
        )?;
        let mut reference = GraphReference::new(&graph_path, item)?;
        reference.material_inputs = original.material_inputs.clone();
        reference.attachments = original.attachments.clone();
        recipe.overrides.imported_graph = Some(reference);
        if graph["generated_icon"].is_object() {
            let mut value = serde_json::to_value(&recipe)?;
            if let Some(icon) = value["overrides"]["icon_edit"].as_object_mut() {
                icon.remove("imported_image");
            }
            parhelion_import::artwork::apply(&mut value, &graph_path)?;
            recipe = serde_json::from_value(value)?;
        }
        let path = output.join(format!("recipe-{index}.parhelion.json"));
        fs::write(&path, recipe.to_json_pretty()?)?;
        recipes.push(WeaponRecipe::load_json(&path)?);
        originals.push((original, item));
    }
    source_reader.finish()?;
    native_reader.finish()?;
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
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    let manifest: Value = serde_json::from_slice(&fs::read(
        built.run_directory.join("parhelion-manifest.json"),
    )?)?;
    let source_files = manifest["source_artifacts"]
        .as_array()
        .context("Native source artifacts")?
        .iter()
        .map(|row| row["file_name"].as_str().context("Native source filename"))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(!source_files.is_empty(), "Native source corpus is empty");
    let ignored = manifest["ignored_authored_files"]
        .as_array()
        .context("Ignored authored files")?
        .iter()
        .map(|row| row.as_str().context("Ignored authored filename"))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        // Retain stock resources beyond the build's direct inputs. Exclude only
        // declared older authored overlays and destinations of the new outputs.
        if ignored.contains(entry.file_name().to_string_lossy().as_ref())
            || built
                .artifacts
                .iter()
                .any(|artifact| entry.file_name().to_string_lossy() == artifact.file_name)
        {
            continue;
        }
        if entry.path().extension().is_some_and(|e| e == "pkg") {
            fs::hard_link(entry.path(), packages.join(entry.file_name()))?;
        }
    }
    fs::copy(
        native
            .parent()
            .context("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for artifact in &built.artifacts {
        let destination = packages.join(&artifact.file_name);
        ensure!(!destination.exists(), "Never overwrite a stock link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let mut reader = Reader::new(&packages, &output.join("readback"), false)?;
    let names = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .map(|e| (e.name.clone(), e.hash.0))
        .collect::<BTreeMap<_, _>>();
    let globals = reader.tag(*names.get("investment_globals").context("Globals")?, None)?;
    let root = reader.tag(globals.u32(16)?, None)?;
    let items = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let metadata = reader.tag(globals.u32(16 + 66 * 16)?, None)?;
    let art_rows = metadata.array(8, 32, Some(0x80805DFB))?;
    let assets = reader.tag(*names.get("investment_assets").context("Assets")?, None)?;
    let assignments = reader.tag(assets.u32(0x20)?, None)?;
    let parents = assignments
        .array(8, 8, None)?
        .into_iter()
        .map(|at| Ok((assignments.u32(at)?, assignments.u32(at + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut evidence = Vec::new();
    for (recipe, case) in recipes.iter().zip(&cases) {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let row = items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .find(|&at| items.u32(at).ok() == Some(hash))
            .context("Emitted item absent")?;
        let item = reader.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        let mut sights = BTreeMap::new();
        let mut mechanisms = Vec::new();
        for selection in item.array(item.pointer(0x88)?, 4, Some(0x808077B5))? {
            let art = *art_rows
                .get(usize::from(item.u16(selection + 2)?))
                .context("Art row")?;
            for region in metadata.array(art + 16, 8, None)? {
                let slot = metadata.pointer(region)?;
                for key in metadata.array(slot + 8, 4, None)? {
                    let key = metadata.u32(key)?;
                    if [0, u32::MAX, 0x811C9DC5].contains(&key) {
                        continue;
                    }
                    let parent = reader.tag(*parents.get(&key).context("Art assignment")?, None)?;
                    if let Some(expected) = case.get("expected_motion") {
                        mechanisms.extend(verify_mechanisms(
                            &mut reader,
                            parent.u32(16)?,
                            expected,
                        )?);
                    }
                    for sight in verify(&mut reader, parent.u32(16)?, &case["expected"])? {
                        sights.insert(sight["entity"].as_u64().context("Sight entity")?, sight);
                    }
                }
            }
        }
        ensure!(
            sights.len() == 1,
            "Item does not select exactly one source sight"
        );
        ensure!(
            case.get("expected_motion").is_none() || !mechanisms.is_empty(),
            "Selected item has no articulated body"
        );
        evidence.push(json!({"item":hash,"name":recipe.name,"sights":sights.into_values().collect::<Vec<_>>(),"mechanisms":mechanisms}));
    }
    reader.finish()?;
    for (reference, item) in originals {
        reference.validate(item)?;
    }
    fs::write(
        output.join("verified-sights.json"),
        serde_json::to_vec_pretty(&json!({"cases":cases,"items":evidence,
        "staged_directory":built.run_directory,"original_graphs_unchanged":true,"gameplay_verified":false}))?,
    )?;
    Ok(())
}
