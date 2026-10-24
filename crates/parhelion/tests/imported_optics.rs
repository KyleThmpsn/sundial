//! Refresh saved source optics, reopen the recipe and follow the staged item's actual art.
#![cfg(feature = "d2-model-importer")]

use anyhow::{Context, Result, ensure};
use parhelion::{BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe};
use parhelion_import::{
    GraphReference,
    tiger::{markers, payload::Payload, reader::Reader},
};
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
        let kind = entry.file_type()?;
        ensure!(!kind.is_symlink(), "Graph fixture contains a symbolic link");
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Sight {
    owner: u32,
    angle: [u8; 4],
    // Rotation, position, root binding and name in the original source row order.
    defaults: [Vec<u8>; 2],
}

fn source_sights(directory: &Path) -> Result<BTreeMap<(u64, usize), Sight>> {
    let report: Value = serde_json::from_slice(&fs::read(directory.join("report.json"))?)?;
    let mut result = BTreeMap::new();
    for part in report["art_parts"].as_array().context("Source art parts")? {
        if part["missing"] == true {
            continue;
        }
        let entity = Payload(fs::read(directory.join(format!(
            "raw/{}.bin",
            part["entity"].as_str().context("Source entity")?
        )))?);
        for component in entity.array(8, 12, Some(0x80809ACD))? {
            let tag = entity.u32(component)?;
            let path = directory.join(format!("raw/{tag:08X}.bin"));
            if !path.exists() {
                continue;
            }
            let owner = Payload(fs::read(path)?);
            let data = owner.pointer(24)?;
            if owner.u32(data - 4)? != 0x80802B93 {
                continue;
            }
            if owner.u64(data + 0x1B0)? == 0 && owner.u64(data + 0x250)? == 0 {
                continue;
            }
            let mut defaults = [Vec::new(), Vec::new()];
            for (i, descriptor) in [0x1B0, 0x250].into_iter().enumerate() {
                let rows = owner.array(data + descriptor, 48, Some(0x80809AC9))?;
                ensure!(
                    rows.len() == 1,
                    "Configure optics with one explicit default per input"
                );
                defaults[i] = owner.0[rows[0]..rows[0] + 44].to_vec();
                let connections = entity
                    .array(24, 56, Some(0x80809A8F))?
                    .into_iter()
                    .filter(|&at| {
                        entity.u32(at).ok() == Some(tag)
                            && entity.u32(at + 4).ok() == Some(0x80809A9D)
                            && entity.u64(at + 8).ok() == Some((data + descriptor - 64) as u64)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    connections.len() == 1,
                    "Source optic has no unique input connection"
                );
                let at = connections[0];
                ensure!(
                    entity.u64(at + 24)? == u64::MAX
                        && entity.u64(at + 32)? == 0
                        && entity.u32(at + 40)? == 0xAE7D0AED
                        && entity.u32(at + 44)? == 0x3FFFF
                        && entity.u64(at + 48)? == 0,
                    "Source fixture uses a different transform provider"
                );
            }
            let sight = Sight {
                owner: tag,
                angle: owner.bytes::<4>(data + 0x80)?,
                defaults,
            };
            for placement in part["placements"].as_array().context("Source placement")? {
                let selector = placement["selector"].as_u64().context("Selector")?;
                ensure!(
                    selector <= 16,
                    "Configure the unchanged native weapon art selector range"
                );
                let key = (
                    selector,
                    placement["position"].as_u64().context("Position")? as usize,
                );
                ensure!(
                    result.insert(key, sight.clone()).is_none(),
                    "Source optic placement overlaps"
                );
            }
        }
    }
    ensure!(!result.is_empty(), "Required source corpus has no optics");
    Ok(result)
}

fn verify_sight(reader: &mut Reader, entity_tag: u32, expected: &Sight) -> Result<Value> {
    let entity = reader.tag(entity_tag, Some(0x80809C0F))?;
    let mut optics = Vec::new();
    for component in entity.array(16, 12, Some(0x80809C04))? {
        let tag = entity.u32(component)?;
        let owner = reader.tag(tag, Some(0x80809C36))?;
        let data = owner.pointer(24)?;
        if owner.u32(data - 4)? == 0x8080393B {
            optics.push((component, tag, owner));
        }
    }
    ensure!(optics.len() == 1, "Selected art has no unique optic");
    let (component, tag, owner) = &optics[0];
    ensure!(
        *tag != expected.owner,
        "Modern owner leaked into native art"
    );
    let data = owner.pointer(24)?;
    ensure!(
        owner.bytes::<4>(data + 0x80)? == expected.angle,
        "Optic angle changed"
    );
    let components = entity.array(16, 12, Some(0x80809C04))?;
    let index = components
        .iter()
        .position(|at| at == component)
        .context("Component index")?;
    let mut links = Vec::new();
    for (i, descriptor) in [0x198, 0x228].into_iter().enumerate() {
        let rows = owner.array(data + descriptor, 64, Some(0x80809C00))?;
        ensure!(rows.len() == 1, "Native optic lost its default");
        let row = rows[0];
        let native = [
            &owner.0[row + 16..row + 48],
            &owner.0[row..row + 8],
            &owner.0[row + 48..row + 52],
        ]
        .concat();
        ensure!(
            native == expected.defaults[i],
            "Source aim transform or binding changed"
        );
        let events = entity
            .array(32, 72, Some(0x80809BC9))?
            .into_iter()
            .filter(|&at| {
                entity.u32(at + 8).ok() == Some(*tag)
                    && entity.u32(at + 12).ok() == Some(0x80809BD7)
                    && entity.u64(at + 16).ok() == Some((data + descriptor - 64) as u64)
            })
            .collect::<Vec<_>>();
        ensure!(
            events.len() == 1,
            "Native aim input is absent or multiply connected"
        );
        let at = events[0];
        ensure!(
            entity.u32(at + 24)? as usize == index,
            "Aim input names another component"
        );
        ensure!(
            entity.u64(at + 32)? == 0xAE7D0AED
                && entity.u64(at + 40)? == u64::MAX
                && entity.u64(at + 48)? == 0
                && entity.u64(at + 56)? == 0x3FFFF
                && entity.u64(at + 64)? == 0,
            "Aim input still uses a donor provider"
        );
        links.push(
            json!({"event":at,"input":data + descriptor - 64,"parent":0xAE7D0AEDu32,"mode":3}),
        );
    }
    Ok(json!({"entity":entity_tag,"optic":tag,"source":expected.owner,"connections":links}))
}

#[test]
#[ignore = "Requires clean SUNDIAL_STOCK_PACKAGES, PARHELION_OPTIC_CASES and fresh PARHELION_OPTIC_OUTPUT"]
fn source_aim_providers_follow_the_authored_parent_through_staging() -> Result<()> {
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_OPTIC_OUTPUT")?,
        native.parent().context("Native root")?,
    )?;
    ensure!(!output.exists(), "Use a fresh artifact directory");
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_OPTIC_CASES")?)?)?;
    ensure!(!cases.is_empty(), "Configure a required optic corpus");
    fs::create_dir_all(&output)?;
    let mut reader = Reader::new(&native, &output.join("stock"), false)?;
    let mut recipes = Vec::new();
    let mut expected = Vec::new();
    let mut original_refs = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let mut recipe =
            WeaponRecipe::load_json(Path::new(case["recipe"].as_str().context("Recipe path")?))?;
        let original = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?
            .clone();
        original.validate(recipe.identity.item_hash.parse_u32()?)?;
        let graph_path = output.join(format!("graph-{i}"));
        copy_tree(&original.directory, &graph_path)?;
        let mut graph: Value =
            serde_json::from_slice(&fs::read(graph_path.join("asset-graph.json"))?)?;
        let source = Path::new(case["source"].as_str().context("Prepared source")?);
        expected.push(source_sights(source)?);
        let template: Value = serde_json::from_slice(&fs::read(
            case["native_template"]
                .as_str()
                .context("Native template")?,
        )?)?;
        markers::author(
            &mut reader,
            source,
            &template,
            &graph_path,
            &mut graph,
            parhelion_import::d2_mot::artwork::native_selector,
        )?;
        fs::write(
            graph_path.join("asset-graph.json"),
            serde_json::to_vec_pretty(&graph)?,
        )?;
        recipe.overrides.imported_graph = Some(GraphReference::new(
            &graph_path,
            recipe.identity.item_hash.parse_u32()?,
        )?);
        let portable = recipe.to_json_pretty()?;
        fs::write(output.join(format!("recipe-{i}.parhelion.json")), &portable)?;
        recipes.push(WeaponRecipe::from_json_str(&portable)?);
        original_refs.push((original, recipe.identity.item_hash.parse_u32()?));
    }
    reader.finish()?;
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built = parhelion::build_and_stage_snapshot_with_progress(&snapshot, |progress| {
        println!("{:?}", progress.phase)
    })
    .map_err(anyhow::Error::msg)?;
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
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
        ensure!(!destination.exists(), "Never overwrite a stock hard link");
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let named = staged
        .manager
        .lookup
        .named_tags
        .iter()
        .map(|e| (e.name.clone(), e.hash.0))
        .collect::<BTreeMap<_, _>>();
    let globals = staged.tag(*named.get("investment_globals").context("Globals")?, None)?;
    let root = staged.tag(globals.u32(16)?, None)?;
    let items = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    let metadata = staged.tag(globals.u32(16 + 66 * 16)?, None)?;
    let art_rows = metadata.array(8, 32, Some(0x80805DFB))?;
    let assets = staged.tag(*named.get("investment_assets").context("Assets")?, None)?;
    let assignments = staged.tag(assets.u32(0x20)?, None)?;
    let parents = assignments
        .array(8, 8, None)?
        .into_iter()
        .map(|at| Ok((assignments.u32(at)?, assignments.u32(at + 4)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut evidence = Vec::new();
    for (recipe, sights) in recipes.iter().zip(&expected) {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let item = items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .find(|&at| items.u32(at).ok() == Some(hash))
            .context("Authored item absent")?;
        let definition = staged.tag(items.u32(item + 16)?, Some(0x80807BEA))?;
        let mut found = BTreeMap::new();
        for row in definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))? {
            let art = *art_rows
                .get(usize::from(definition.u16(row + 2)?))
                .context("Art row")?;
            for region in metadata.array(art + 16, 8, None)? {
                let slot = metadata.pointer(region)?;
                let selector = metadata.u64(slot)?;
                for (position, key_row) in
                    metadata.array(slot + 8, 4, None)?.into_iter().enumerate()
                {
                    let Some(sight) = sights.get(&(selector, position)) else {
                        continue;
                    };
                    let key = metadata.u32(key_row)?;
                    let parent =
                        staged.tag(*parents.get(&key).context("Scope assignment absent")?, None)?;
                    let result = verify_sight(&mut staged, parent.u32(16)?, sight)?;
                    ensure!(
                        found.insert((selector, position), result).is_none(),
                        "Scope is multiply selected"
                    );
                }
            }
        }
        ensure!(
            found.len() == sights.len(),
            "A source optic is unreachable from the emitted item"
        );
        evidence.push(json!({"item":hash,"name":recipe.name,"optics":found.into_values().collect::<Vec<_>>()}));
    }
    staged.finish()?;
    for (reference, hash) in original_refs {
        reference.validate(hash)?;
    }
    fs::write(
        output.join("verified-optics.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":cases,"items":evidence,"staged_directory":built.run_directory,
            "original_graphs_unchanged":true,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
