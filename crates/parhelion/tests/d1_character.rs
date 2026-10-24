//! Source-backed saved imports through refresh, portable reload and simultaneous staging.
#![cfg(feature = "d2-model-importer")]
use anyhow::{Context, Result, ensure};
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, build_and_stage_snapshot_with_progress,
};
use parhelion_import::{
    d1_roi::Source,
    tiger::{payload::Payload, reader::Reader},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("Set {name}"))
}

fn copy_graph(from: &std::path::Path, to: &std::path::Path) -> Result<()> {
    ensure!(!to.exists(), "Graph copy already exists");
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Prepared graph contains a symbolic link"
        );
        if kind.is_dir() {
            copy_graph(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "Requires D1_CHARACTER_PLAN, D1_CHARACTER_OUTPUT, D1_SOURCE_PACKAGES and SUNDIAL_STOCK_PACKAGES"]
fn source_holds_survive_reload_and_shared_bank_staging() -> Result<()> {
    let output = configured("D1_CHARACTER_OUTPUT")?;
    ensure!(!output.exists(), "Use a fresh artifact folder");
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let source = Source::open(&configured("D1_SOURCE_PACKAGES")?, |_, _| {})?;
    let plan: Vec<Value> = serde_json::from_slice(&fs::read(configured("D1_CHARACTER_PLAN")?)?)?;
    ensure!(
        plan.len() > 4,
        "The configured corpus must exercise profile capacity"
    );
    let mut recipes = Vec::new();
    let mut graphs = Vec::new();
    for (index, item) in plan.iter().enumerate() {
        let mut recipe =
            WeaponRecipe::load_json(PathBuf::from(item["file"].as_str().context("Recipe path")?))?;
        let hash = recipe.identity.item_hash.parse_u32()?;
        let original = recipe
            .overrides
            .imported_graph
            .as_ref()
            .context("Imported graph")?
            .clone();
        original.validate(hash)?;
        let copy = output.join(format!("item-{index}"));
        copy_graph(&original.directory, &copy)?;
        let reference = source.refresh_weapon_character(&copy, &native)?;
        original.validate(hash)?;
        fs::write(
            output.join(format!("reference-{index}.json")),
            serde_json::to_vec_pretty(&reference)?,
        )?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        ensure!(
            graph["animation"]["character"]["source_profiles"].is_object(),
            "Source profile evidence missing"
        );
        recipe.overrides.imported_graph = Some(reference);
        let portable = recipe.to_json_pretty()?;
        fs::write(output.join(format!("recipe-{index}.json")), &portable)?;
        recipes.push(WeaponRecipe::from_json_str(&portable)?);
        graphs.push(graph);
    }
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })
    .map_err(anyhow::Error::msg)?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))
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
        let target = packages.join(&artifact.file_name);
        ensure!(!target.exists(), "Staged package would replace stock");
        fs::copy(built.run_directory.join(&artifact.file_name), target)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let mut stock = Reader::discovery(&native, &output.join("stock"), false)?;
    let globals = staged
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .context("Globals")?
        .hash
        .0;
    let globals = staged.tag(globals, None)?;
    let root = staged.tag(globals.u32(16)?, None)?;
    let items = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    let mut results = Vec::new();
    let mut profiles = BTreeSet::new();
    for (recipe, graph) in recipes.iter().zip(&graphs) {
        let c = &graph["animation"]["character"];
        let number = |key: &str| -> Result<u32> {
            Ok(u32::try_from(c[key].as_u64().context("Character number")?)?)
        };
        let private = number("private")?;
        ensure!(profiles.insert(private), "Private profiles collide");
        let hash = recipe.identity.item_hash.parse_u32()?;
        let row = items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .find(|&r| items.u32(r).ok() == Some(hash))
            .context("Authored item")?;
        let runtime =
            parhelion_import::tiger::rig::inspect(&mut staged, items.u32(row + 16)?, false)?;
        let hex = |v: &Value| -> Result<u32> {
            Ok(u32::from_str_radix(
                v.as_str().context("Runtime hash")?,
                16,
            )?)
        };
        let component = runtime["components"]
            .as_array()
            .context("Runtime components")?
            .iter()
            .find(|c| c["class"] == "80804221")
            .context("Attachment")?;
        let owner = staged.tag(hex(&component["owner"])?, Some(0x80809C36))?;
        for row in parhelion_import::tiger::rig::attachment_rows(
            &owner,
            owner.pointer(24)?,
            hex(&runtime["content_key"])?,
            false,
        )? {
            ensure!(
                owner.u32(row + 0x6C)? == private,
                "Private hold not bound to the authored weapon"
            );
        }
        let first_person = runtime["components"]
            .as_array()
            .context("Runtime components")?
            .iter()
            .filter(|c| c["class"] == "8080344B" && c["entity"] != runtime["runtime_entity"])
            .collect::<Vec<_>>();
        let native_runtime = &graph["rendering"][0]["native_runtime"];
        let expected = native_runtime["components"]
            .as_array()
            .context("Native runtime components")?
            .iter()
            .filter(|c| c["class"] == "8080344B" && c["entity"] != native_runtime["runtime_entity"])
            .count();
        ensure!(
            expected <= 1 && first_person.len() == expected,
            "Staging changed the first-person controller population"
        );
        if let Some(first_person) = first_person.first() {
            let first_person = staged.tag(hex(&first_person["owner"])?, Some(0x80809C36))?;
            let parameters = staged.tag(
                first_person.u32(first_person.pointer(24)? + 0x94)?,
                Some(0x80808EE1),
            )?;
            let group = parameters.array(8, 32, Some(0x80808EE5))?[parameters.u16(30)? as usize];
            let names = parameters.array(group + 16, 24, Some(0x80808EE8))?;
            let find = |name| -> Result<usize> {
                names
                    .iter()
                    .copied()
                    .find(|&r| parameters.u32(r + 16).ok() == Some(name))
                    .context("First-person profile lookup")
            };
            let parent = find(u32::try_from(
                c["binding"]["parent"].as_u64().context("Profile parent")?,
            )?)?;
            let alias = find(private)?;
            ensure!(
                parameters.bytes::<16>(parent)? == parameters.bytes::<16>(alias)?
                    && parameters.bytes::<4>(parent + 20)? == parameters.bytes::<4>(alias + 20)?,
                "Third-person profile changes first-person selection"
            );
        }
        let bank = staged.tag(number("bank")?, Some(0x808036F6))?;
        let old = stock.tag(number("bank")?, Some(0x808036F6))?;
        let descriptors = bank.array(0x68, 32, Some(0x80809002))?;
        for (a, b) in old
            .array(0x68, 32, Some(0x80809002))?
            .iter()
            .zip(&descriptors)
        {
            ensure!(
                old.bytes::<32>(*a)? == bank.bytes::<32>(*b)?,
                "A stock body descriptor changed"
            );
        }
        let dictionary = staged.tag(number("parameters")?, Some(0x80808EE1))?;
        let old_dictionary = stock.tag(number("parameters")?, Some(0x80808EE1))?;
        let group = |p: &Payload| -> Result<usize> {
            Ok(p.array(8, 32, Some(0x80808EE5))?[p.u16(30)? as usize])
        };
        ensure!(
            dictionary.u64(group(&dictionary)?)? == old_dictionary.u64(group(&old_dictionary)?)?,
            "Private holds consumed hierarchy bits"
        );
        let layer = staged.tag(number("layers")?, Some(0x808034EB))?;
        let directory = &recipe.overrides.imported_graph.as_ref().unwrap().directory;
        let allocated: Value =
            serde_json::from_slice(&fs::read(directory.join("character-allocated.json"))?)?;
        let all_clips = bank
            .array(8, 4, Some(0x80808F48))?
            .into_iter()
            .map(|r| bank.u32(r))
            .collect::<Result<Vec<_>>>()?;
        let mut reached = BTreeSet::new();
        for d in allocated["descriptors"]
            .as_array()
            .context("Allocated body descriptors")?
        {
            let index = d.as_u64().context("Descriptor ordinal")? as usize;
            let alias = layer.u16(layer.array(40, 2, Some(0x80800006))?[index])?;
            ensure!(alias != u16::MAX, "D1 hold has no supplemental route");
            let route = layer.array(56, 32, Some(0x808034EF))?[alias as usize];
            let groups = layer.array(8, 24, Some(0x80803724))?;
            for edge in layer.array(route, 4, Some(0x808034F1))? {
                let group = groups[layer.u16(edge + 2)? as usize];
                let choice = layer.array(group, 16, Some(0x8080372E))?[layer.u16(edge)? as usize];
                for option in layer.array(choice, 64, Some(0x80803730))? {
                    for weight in layer.array(option, 8, Some(0x80803737))? {
                        reached.insert(all_clips[layer.u32(weight)? as usize]);
                    }
                }
            }
        }
        for (clip, tag) in c["clips"]
            .as_array()
            .context("Clips")?
            .iter()
            .zip(allocated["clips"].as_array().context("Allocated clips")?)
        {
            let tag = u32::try_from(tag.as_u64().context("Allocated clip")?)?;
            ensure!(
                staged.tag(tag, Some(0x80808F49))?.0
                    == fs::read(directory.join(clip["file"].as_str().context("Clip file")?))?,
                "Staged motion changed"
            );
            if clip.get("descriptor").is_none() {
                ensure!(
                    reached.contains(&tag),
                    "Source holding layer is unreachable"
                );
            }
        }
        results.push(json!({"item":hash,"runtime":runtime,"profile":private,"source":c["source_profiles"],"allocated":allocated}));
    }
    stock.finish()?;
    staged.finish()?;
    fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(
            &json!({"source_build":"56578.17.04.12.1251.evil","native_build":"86657.20.08.23.1800.d2_rc___release","items":results,"staged":built.run_directory,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}
