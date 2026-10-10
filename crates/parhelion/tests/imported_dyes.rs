//! Rebuild existing portable imports and inspect their native dye precedence.
#![cfg(feature = "d2-model-importer")]

use parhelion::{BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe};
use parhelion_import::d2_mot::{payload::Payload, reader::Reader};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn configured(key: &str) -> Result<PathBuf> {
    Ok(env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Set {key}"))?)
}

fn layers(definition: &Payload) -> Result<[BTreeMap<u16, u16>; 3]> {
    let root = definition.pointer(0x88)?;
    let mut result = std::array::from_fn(|_| BTreeMap::new());
    for (layer, offset) in [0x28, 0x38, 0x48].into_iter().enumerate() {
        for row in definition.array(root + offset, 4, None)? {
            assert!(
                result[layer]
                    .insert(definition.u16(row)?, definition.u16(row + 2)?)
                    .is_none()
            );
        }
    }
    Ok(result)
}

#[test]
#[ignore = "Requires clean native packages, PARHELION_DYE_RECIPES (JSON path array) and PARHELION_DYE_OUTPUT"]
fn imported_colors_remain_defaults_when_shaders_are_equipped() -> Result<()> {
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = configured("PARHELION_DYE_OUTPUT")?;
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output)?;
    let paths: Vec<PathBuf> =
        serde_json::from_slice(&fs::read(configured("PARHELION_DYE_RECIPES")?)?)?;
    let mut recipes = Vec::new();
    let mut graphs = Vec::new();
    for path in paths {
        let recipe = WeaponRecipe::load_json(&path)?;
        let portable = recipe.to_json_pretty()?;
        fs::write(
            output.join(path.file_name().ok_or("Recipe filename")?),
            &portable,
        )?;
        let recipe = WeaponRecipe::from_json_str(&portable)?;
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .ok_or("Imported graph missing")?;
        graphs.push(serde_json::from_slice::<Value>(&fs::read(
            reference.directory.join("asset-graph.json"),
        )?)?);
        recipes.push(recipe);
    }
    assert!(recipes.iter().any(|r| r.kind == ItemKind::Weapon));
    assert!(recipes.iter().any(|r| r.kind == ItemKind::Armor));
    assert!(recipes.iter().any(|r| r.kind == ItemKind::Shader));
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })?;
    let built = parhelion::build_and_stage_snapshot_with_progress(&snapshot, |p| {
        println!("{:?}", p.phase)
    })?;
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
            .ok_or("Native root")?
            .join("bin/x64/oo2core_3_win64.dll"),
        view.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for artifact in &built.artifacts {
        let target = packages.join(&artifact.file_name);
        assert!(!target.exists(), "Never overwrite a stock hard link");
        fs::copy(built.run_directory.join(&artifact.file_name), target)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let globals_tag = staged
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .ok_or("Globals")?
        .hash
        .0;
    let globals = staged.tag(globals_tag, None)?;
    let root = staged.tag(globals.u32(16)?, None)?;
    let items = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    let dye_table = staged.tag(globals.u32(16 + 67 * 16)?, None)?;
    let dye_entries = dye_table.array(8, 8, None)?;
    let mut evidence = Vec::new();
    let mut gear_with_locked_source = false;
    for (recipe, graph) in recipes.iter().zip(&graphs) {
        let (item, locked_source) =
            inspect_recipe(&mut staged, &items, &dye_table, &dye_entries, recipe, graph)?;
        gear_with_locked_source |= locked_source;
        evidence.push(item);
    }
    assert!(
        gear_with_locked_source,
        "Include gear with source-locked colors"
    );
    staged.finish()?;
    fs::write(
        output.join("verified-dyes.json"),
        serde_json::to_vec_pretty(
            &json!({"items":evidence,"package_precedence_verified":true,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}

fn inspect_recipe(
    staged: &mut Reader,
    items: &Payload,
    dye_table: &Payload,
    dye_entries: &[usize],
    recipe: &WeaponRecipe,
    graph: &Value,
) -> Result<(Value, bool)> {
    let hash = recipe.identity.item_hash.parse_u32()?;
    let item = items
        .array(8, 24, None)?
        .into_iter()
        .find(|&r| items.u32(r).ok() == Some(hash))
        .ok_or("Authored item missing")?;
    let definition = staged.tag(items.u32(item + 16)?, None)?;
    let actual = layers(&definition)?;
    let dyes = graph["dyes"].as_array().ok_or("Source dyes")?;
    assert!(!dyes.is_empty(), "Coverage requires dye-bearing imports");
    if recipe.kind == ItemKind::Shader {
        assert!(!actual[0].is_empty(), "Shader lost its selectable colors");
        assert_eq!(actual[0], actual[1]);
        assert!(actual[2].is_empty());
        return Ok((
            json!({"name":recipe.name,"shader_control":true,"layers":actual}),
            false,
        ));
    }
    // Resolve source appearance independently before checking native allocations.
    let (expected, locked_source) = source_colors(graph, dyes)?;
    assert!(
        actual[0].is_empty(),
        "Imported base colors must be defaults"
    );
    assert!(
        actual[2].is_empty(),
        "Imported locks would override an equipped shader"
    );
    assert_eq!(
        actual[1].keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    assert!(!expected.is_empty(), "No effective source colors");
    for (&channel, &ordinal) in &expected {
        let at = *dye_entries
            .get(usize::from(actual[1][&channel]))
            .ok_or("Unregistered dye")?;
        let source = dyes.get(usize::from(ordinal)).ok_or("Source dye ordinal")?;
        assert_eq!(
            u64::from(dye_table.u32(at)?),
            source["manifest"].as_u64().ok_or("Source dye identity")?,
            "Source layer precedence or material identity changed"
        );
    }
    let inspected = parhelion_import::d2_mot::dyes::inspect(staged, items.u32(item + 16)?, false)?;
    for dye in inspected.as_array().ok_or("Native dye readback")? {
        assert!(
            !dye["found"]
                .as_array()
                .ok_or("Native dye assignment")?
                .is_empty(),
            "Base color has no material"
        );
    }
    Ok((
        json!({"name":recipe.name,"item_hash":hash,"kind":recipe.kind,"layers":actual,"expected_source_ordinals":expected,"resolved_dyes":inspected}),
        locked_source,
    ))
}

fn source_colors(graph: &Value, dyes: &[Value]) -> Result<(BTreeMap<u16, u16>, bool)> {
    let mut expected = BTreeMap::new();
    if let Some(source) = graph["dye_rows"].as_array() {
        let locked = source[2].as_array().is_some_and(|r| !r.is_empty());
        for layer in [1, 0, 2] {
            for row in source[layer].as_array().ok_or("Source dye layer")? {
                expected.insert(
                    row["channel"].as_u64().ok_or("Channel")? as u16,
                    row["dye"].as_u64().ok_or("Dye ordinal")? as u16,
                );
            }
        }
        return Ok((expected, locked));
    } else {
        for (ordinal, dye) in dyes.iter().enumerate() {
            expected.insert(
                dye["channel"].as_u64().ok_or("Channel")? as u16,
                u16::try_from(ordinal)?,
            );
        }
    }
    Ok((expected, false))
}
