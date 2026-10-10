//! Configured cloth import through sharing, staging and independent geometry readback.
//! Integration failure model: the solver can be serialized correctly while its
//! owner is missing callbacks, its render groups select the wrong draws, or its
//! output buffer overwrites another mesh. Require separate float cloth streams,
//! native definitions and solver payloads after the public package workflow.
#![cfg(feature = "d2-model-importer")]
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe,
    build_and_stage_snapshot_with_progress,
};
use parhelion_import::d2_mot::{payload::Payload, reader, reader::Reader, service, shadowkeep};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};
use sundial::investment::InvestmentCatalog;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn configured(name: &str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Set {name}").into())
}
fn load(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn raw(root: &Path, tag: u32) -> Result<Payload> {
    Ok(Payload(fs::read(root.join(format!("raw/{tag:08X}.bin")))?))
}
fn hash(value: &Value) -> Result<u32> {
    Ok(u32::from_str_radix(value.as_str().ok_or("Tag")?, 16)?)
}
fn graph_bytes(root: &Path, graph: &Value, symbol: &str) -> Result<Payload> {
    let node = graph["nodes"]
        .as_array()
        .ok_or("Nodes")?
        .iter()
        .find(|n| n["symbol"] == symbol)
        .ok_or_else(|| format!("Missing {symbol}"))?;
    Ok(Payload(fs::read(
        root.join(node["file"].as_str().ok_or("File")?),
    )?))
}

fn verify_cloth(
    model: &Payload,
    positions: &Payload,
    skin: &Payload,
    target: &Payload,
    native: &Payload,
    base: usize,
) -> Result<Value> {
    let count = positions.0.len() / 48;
    let mut maximum_error = 0f32;
    let mut blended = 0;
    for i in 0..count {
        let out = (base + i) * 16;
        for axis in 0..3 {
            let expected = positions.f32(i * 48 + axis * 4)? * model.f32(0x50 + axis * 4)?
                + model.f32(0x60 + axis * 4)?;
            let actual = target.i16(out + axis * 2)? as f32 / 32767. * native.f32(0x6C)?
                + native.f32(0x60 + axis * 4)?;
            maximum_error = maximum_error.max((expected - actual).abs());
            assert!(
                (expected - actual).abs()
                    <= (model.f32(0x6C)?.abs() + native.f32(0x6C)?.abs()) * 1.5 / 32767. + 1e-6,
                "Cloth rest geometry moved: {expected} -> {actual}"
            );
        }
        assert_eq!(
            &target.0[out + 8..out + 12],
            &skin.0[i * 8..i * 8 + 4],
            "Cloth skin weights changed"
        );
        if skin.0[i * 8..i * 8 + 4].iter().filter(|w| **w != 0).count() > 1 {
            blended += 1;
        }
        for bone in &target.0[out + 12..out + 16] {
            assert!(u32::from(*bone) < native.u32(0x40)?);
        }
    }
    assert!(
        blended > 0,
        "The configured cloth must exercise blended skinning"
    );
    Ok(json!({"vertices":count,"blended":blended,"maximum_position_error":maximum_error}))
}

fn verify_physical(
    graph_root: &Path,
    graph: &Value,
    prefix: &str,
    positions: &Payload,
    skin: &Payload,
) -> Result<()> {
    let model = graph_bytes(graph_root, graph, &format!("{prefix}-model"))?;
    let native_positions = graph_bytes(graph_root, graph, &format!("{prefix}-positions-data"))?;
    let native_skin = graph_bytes(graph_root, graph, &format!("{prefix}-skin-data"))?;
    assert_eq!(
        native_positions.0, positions.0,
        "The solver needs the source float vertex ordering"
    );
    assert_eq!(native_skin.0.len(), skin.0.len());
    for (a, b) in native_skin.0.chunks_exact(8).zip(skin.0.chunks_exact(8)) {
        assert_eq!(&a[..4], &b[..4], "Physical cloth skin weights changed");
    }
    for mesh in model.array(16, 136, Some(0x80807378))? {
        assert_eq!(
            model.u16(mesh + 88)?,
            18,
            "Physical cloth declaration differs"
        );
    }
    let solver = graph_bytes(graph_root, graph, &format!("{prefix}-solver"))?;
    assert_eq!(
        solver.u32(0)?,
        0x57E0E057,
        "Source TAG0 bytes were not translated"
    );
    let definition = graph_bytes(graph_root, graph, &format!("{prefix}-definition"))?;
    assert!(definition.0.len() >= 0x698);
    Ok(())
}

fn inspect_source_geometry(import: &Path, graph_root: &Path, graph: &Value) -> Result<Value> {
    let source = import.join("gear-0/source");
    let report = load(&source.join("report.json"))?;
    let manifest = load(&source.join("source-manifest.json"))?;
    let buffer = |tag| -> Result<Payload> {
        raw(
            &source,
            u32::try_from(
                manifest["tags"][format!("{tag:08X}")]["reference"]
                    .as_u64()
                    .ok_or("Buffer reference")?,
            )?,
        )
    };
    let mut seen = BTreeSet::new();
    let mut checks = Vec::new();
    for (part, art) in report["art_parts"]
        .as_array()
        .ok_or("Art parts")?
        .iter()
        .enumerate()
    {
        if !seen.insert(art["entity"].clone().to_string()) {
            continue;
        }
        let native = graph_bytes(graph_root, graph, &format!("part-{part}-model"))?;
        let target = graph_bytes(graph_root, graph, &format!("part-{part}-positions-data"))?;
        let header = graph_bytes(graph_root, graph, &format!("part-{part}-positions-header"))?;
        assert_eq!(
            header.u16(4)?,
            16,
            "Complete skin weights need the weighted native stream"
        );
        let mut base = 0;
        for entry in report["models"]
            .as_array()
            .ok_or("Source models")?
            .iter()
            .filter(|m| m["entity"] == art["entity"])
        {
            let model = raw(&source, hash(&entry["model"])?)?;
            let meshes = model.array(16, 128, Some(0x80806EC5))?;
            let mesh = meshes[entry["mesh_index"].as_u64().unwrap_or(0) as usize];
            let source_header = raw(&source, model.u32(mesh)?)?;
            let stride = source_header.u16(4)? as usize;
            let positions = buffer(model.u32(mesh)?)?;
            let count = positions.0.len() / stride;
            if entry["cloth"] == true {
                assert_eq!(stride, 48);
                let skin = buffer(model.u32(mesh + 8)?)?;
                let mut check = verify_cloth(&model, &positions, &skin, &target, &native, base)?;
                let model_index = report["models"]
                    .as_array()
                    .ok_or("Source models")?
                    .iter()
                    .filter(|m| m["entity"] == art["entity"])
                    .position(|m| {
                        m["model"] == entry["model"] && m["mesh_index"] == entry["mesh_index"]
                    })
                    .ok_or("Cloth model index")?;
                let prefix = format!("part-{part}-cloth-{model_index}");
                verify_physical(graph_root, graph, &prefix, &positions, &skin)?;
                check["native_solver"] = json!(true);
                check["model"] = entry["model"].clone();
                check["entity"] = entry["entity"].clone();
                checks.push(check);
            }
            base += count;
        }
        assert_eq!(
            target.0.len(),
            base * 16,
            "A source mesh was lost or duplicated"
        );
    }
    assert!(
        !checks.is_empty(),
        "The configured item has no cloth coverage"
    );
    Ok(json!(checks))
}

fn verify_staged_streams(reader: &mut Reader, model: &Payload, physical: bool) -> Result<()> {
    for mesh in model.array(16, 136, Some(0x80807378))? {
        assert_eq!(model.u16(mesh + 88)?, if physical { 18 } else { 28 });
        let header_tag = model.u32(mesh)?;
        let header = reader.tag(header_tag, None)?;
        assert_eq!(header.u16(4)?, if physical { 48 } else { 16 });
        let reference = reader.reference(header_tag)?;
        let data = reader.tag(reference, None)?;
        assert_eq!(data.0.len(), header.u32(0)? as usize);
        let skin = if physical {
            let skin_header = model.u32(mesh + 8)?;
            let skin_reference = reader.reference(skin_header)?;
            reader.tag(skin_reference, None)?.0.clone()
        } else {
            data.0.clone()
        };
        for vertex in skin.chunks_exact(if physical { 8 } else { 16 }) {
            assert_eq!(
                vertex[if physical { 0..4 } else { 8..12 }]
                    .iter()
                    .map(|v| u16::from(*v))
                    .sum::<u16>(),
                255
            );
        }
    }
    Ok(())
}

fn read_staged(
    packages: &Path,
    output: &Path,
    recipes: &[WeaponRecipe],
    receipts: &mut [Value],
) -> Result<Vec<Value>> {
    let mut reader = Reader::new(packages, &output.join("readback"), false)?;
    let mut preview = Vec::new();
    for (recipe, receipt) in recipes.iter().zip(receipts) {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let extracted = shadowkeep::extract(&mut reader, hash)?;
        let mut models = BTreeSet::new();
        for entry in extracted["models"].as_array().ok_or("Native models")? {
            let tag = self::hash(&entry["model"])?;
            if !models.insert(tag) {
                continue;
            }
            let mut preview_tag = tag;
            let mut stored_triangles = None;
            let model = reader.tag(tag, Some(0x808073A5))?;
            verify_staged_streams(&mut reader, &model, entry["cloth"] == true)?;
            if entry["cloth"] == true {
                let owner = reader.tag(self::hash(&entry["owner"])?, Some(0x80809C36))?;
                let resource = owner.pointer(24)?;
                assert_eq!(owner.u32(resource - 4)?, 0x80807286);
                let definition_tag = owner.u32(resource + 0x358)?;
                let definition = reader.tag(definition_tag, Some(0x8080727A))?;
                preview_tag = self::hash(&entry["owner"])?;
                let group = definition.u32(28)? as usize;
                assert!(group < 4);
                let mesh = model.array(16, 136, Some(0x80807378))?[0];
                let parts = model.array(mesh + 24, 32, Some(0x8080737E))?;
                let start = model.u16(mesh + 40)? as usize;
                let mut triangles = 0u64;
                for at in definition.array(0xA0 + group * 0x180, 4, Some(0x80800007))? {
                    let part = parts[start + definition.u32(at)? as usize];
                    triangles += u64::from(model.u32(part + 12)? / 3);
                }
                stored_triangles = Some(triangles);
                let solver_tag = definition.u32(0x690)?;
                let solver = reader.tag(solver_tag, None)?;
                assert_eq!(solver.u32(0)?, 0x57E0E057);
                receipt["cloth"].as_array_mut().ok_or("Cloth receipts")?.push(json!({
                    "model":tag,"definition":definition_tag,"solver":solver_tag,
                    "readback_solver":output.join(format!("readback/raw/{solver_tag:08X}.bin")),
                    "readback_entity":output.join(format!("readback/raw/{}.bin",entry["entity"].as_str().ok_or("Cloth entity")?)),
                    "readback_model":output.join(format!("readback/raw/{tag:08X}.bin")),
                    "readback_definition":output.join(format!("readback/raw/{definition_tag:08X}.bin"))}));
            }
            preview.push(json!({"packages":packages,"tag":preview_tag,"name":format!("{}-{tag:08X}",recipe.name),"weighted":true,"stored_triangles":stored_triangles}));
        }
        assert!(!models.is_empty());
        assert!(
            !receipt["cloth"]
                .as_array()
                .ok_or("Cloth receipts")?
                .is_empty(),
            "Staging lost physical cloth owners"
        );
        receipt["item"] = json!(hash);
        receipt["models"] = json!(models);
    }
    reader.finish()?;
    Ok(preview)
}

#[test]
#[ignore = "Requires configured modern/native packages, PARHELION_CLOTH_CASES and fresh PARHELION_CLOTH_OUTPUT"]
fn cloth_survives_import_sharing_and_staging_with_complete_skin_weights() -> Result<()> {
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = reader::outside(
        &configured("PARHELION_CLOTH_OUTPUT")?,
        modern.parent().ok_or("Modern root")?,
    )?;
    let output = reader::outside(&output, native.parent().ok_or("Native root")?)?;
    assert!(!output.exists(), "Use a fresh cloth artifact directory");
    let cases = load(&configured("PARHELION_CLOTH_CASES")?)?;
    let cases = cases.as_array().ok_or("Cloth cases")?;
    assert!(!cases.is_empty());
    fs::create_dir_all(&output)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().ok_or("Native root")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )?;
    let donors = json!({"gear":catalog.gear_donors(ItemKind::Armor.bucket_hashes()).into_iter().map(|d| json!({
        "hash":d.hash,"name":d.name,"bucket_hash":d.bucket_hash,"class_type":catalog.item_class_type(d.hash),"collection_backed":d.collection_backed
    })).collect::<Vec<_>>()});
    let items = service::scan_cached(
        &modern,
        &native,
        &output.join("source-catalog"),
        false,
        |_| {},
    )?;
    let mut recipes = Vec::new();
    let mut receipts = Vec::new();
    for case in cases {
        let item = items
            .iter()
            .find(|item| Some(u64::from(item.hash)) == case["hash"].as_u64())
            .ok_or("Configured source item missing")?;
        let import = output.join(format!("import-{:08X}", item.hash));
        let path = service::prepare_with_progress(
            item,
            &modern,
            &native,
            &donors,
            &import,
            &mut |phase| println!("{phase}"),
        )?;
        let mut recipe = WeaponRecipe::load_json(&path)?;
        assert_eq!(recipe.kind, ItemKind::Armor);
        let reference = recipe
            .overrides
            .imported_graph
            .as_ref()
            .ok_or("Imported graph")?;
        let graph = load(&reference.directory.join("asset-graph.json"))?;
        let checks = inspect_source_geometry(&import, &reference.directory, &graph)?;
        assert_eq!(
            checks.as_array().unwrap().len() as u64,
            case["cloth_models"]
                .as_u64()
                .ok_or("Expected cloth model count")?
        );
        assert!(
            graph["rendering"]
                .as_array()
                .ok_or("Rendering evidence")?
                .iter()
                .any(
                    |part| part["cloth"].as_array().is_some_and(|rows| !rows.is_empty()
                        && rows.iter().all(|row| row["simulation_converted"] == true))
                )
        );
        recipe.rename_authored_item(format!("{} Cloth Coverage", recipe.name))?;
        let portable = recipe.to_json_pretty()?;
        fs::write(
            output.join(format!("{:08X}.parhelion.json", item.hash)),
            &portable,
        )?;
        recipes.push(WeaponRecipe::from_json_str(&portable)?);
        receipts.push(json!({"source":item.hash,"name":item.name,"geometry":checks,"limitations":graph["limitations"],"cloth":[]}));
    }
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))?;
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|x| x == "pkg") {
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
    for file in &built.artifacts {
        let target = packages.join(&file.file_name);
        assert!(!target.exists(), "Staging must not replace a stock package");
        fs::copy(built.run_directory.join(&file.file_name), target)?;
    }
    let preview = read_staged(&packages, &output, &recipes, &mut receipts)?;
    fs::write(
        output.join("preview-cases.json"),
        serde_json::to_vec_pretty(&preview)?,
    )?;
    fs::write(
        output.join("cloth-import.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":receipts,"packages":packages,"installed":false,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}
