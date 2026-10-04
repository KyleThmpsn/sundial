//! Opt-in source shader import through the same service and staged build used by the UI.
#![cfg(feature = "d2-model-importer")]

use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe,
    build_and_stage_snapshot_with_progress,
};
use parhelion_import::d2_mot::{dyes, payload::Payload, reader::Reader, service};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};
use sundial::investment::InvestmentCatalog;

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_CLEAN_STOCK_PACKAGES and PARHELION_SHADER_OUTPUT"]
fn source_shader_survives_import_and_native_package_staging()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native = configured("PARHELION_CLEAN_STOCK_PACKAGES")?;
    let output = parhelion_import::d2_mot::reader::outside(
        &configured("PARHELION_SHADER_OUTPUT")?,
        modern.parent().ok_or("Modern root")?,
    )?;
    let output =
        parhelion_import::d2_mot::reader::outside(&output, native.parent().ok_or("Native root")?)?;
    fs::create_dir_all(&output)?;
    let hashes = if let Some(path) = env::var_os("PARHELION_SHADER_MATRIX") {
        let matrix: Value = serde_json::from_slice(&fs::read(path)?)?;
        matrix["shaders"]
            .as_array()
            .ok_or("Shader matrix rows")?
            .iter()
            .map(|row| {
                row["hash"]
                    .as_u64()
                    .ok_or("Shader matrix hash")
                    .and_then(|hash| u32::try_from(hash).map_err(|_| "Shader matrix hash overflow"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![
            env::var("PARHELION_SHADER_SOURCE")
                .ok()
                .map(|s| s.parse::<u32>())
                .transpose()?
                .unwrap_or(0xF8313A8C),
        ]
    };
    assert!(!hashes.is_empty(), "Shader matrix size");
    assert_eq!(
        hashes.iter().collect::<BTreeSet<_>>().len(),
        hashes.len(),
        "Duplicate matrix shader"
    );
    let matrix = hashes.len() > 1;
    let mut receipts = Vec::new();
    for hash in hashes {
        let result = std::panic::catch_unwind(|| verify_shader(hash, matrix))
            .map_err(|panic| {
                panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| {
                        panic
                            .downcast_ref::<&str>()
                            .map(|message| (*message).to_owned())
                    })
                    .unwrap_or_else(|| "Shader verification panicked".to_owned())
            })
            .and_then(|result| result.map_err(|error| error.to_string()));
        receipts.push(json!({"source_item":hash,"passed":result.is_ok(),
            "error":result.err()}));
    }
    fs::write(
        output.join("verified-shader-matrix.json"),
        serde_json::to_vec_pretty(&json!({
            "shaders":receipts,"gameplay_verified":false
        }))?,
    )?;
    assert!(
        receipts.iter().all(|r| r["passed"] == true),
        "Shader matrix failures: {receipts:?}"
    );
    Ok(())
}

#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn verify_shader(hash: u32, matrix: bool) -> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native = configured("PARHELION_CLEAN_STOCK_PACKAGES")?;
    let output = configured("PARHELION_SHADER_OUTPUT")?;
    let output = if matrix {
        output.join(format!("{hash:08X}"))
    } else {
        output
    };
    let output =
        parhelion_import::d2_mot::reader::outside(&output, modern.parent().ok_or("Modern root")?)?;
    let output =
        parhelion_import::d2_mot::reader::outside(&output, native.parent().ok_or("Native root")?)?;
    fs::create_dir_all(&output)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().ok_or("Native root")?,
        &output.join("catalog-cache.json"),
        true,
        |_| {},
    )?;
    let donors = json!({"shaders": catalog.shader_donors().iter().map(|d| json!({"hash":d.hash,"name":d.name})).collect::<Vec<_>>()});
    let items = service::scan_cached(&modern, &native, &output.join("catalog"), false, |_| {})?;
    let shader = items
        .iter()
        .find(|i| i.hash == hash && i.is_shader())
        .ok_or("Shader missing from importer catalog")?;
    assert!(
        !shader.dummy,
        "A shader has no weapon pattern and must still be selectable"
    );
    let path = service::prepare_with_progress(
        shader,
        &modern,
        &native,
        &donors,
        &output.join("import"),
        &mut |step| println!("{step}"),
    )?;
    let mut recipe = WeaponRecipe::load_json(&path)?;
    assert_eq!(recipe.kind, ItemKind::Shader);
    assert_eq!(recipe.name, shader.name);
    assert!(recipe.presentation_donor.is_none());
    assert!(
        recipe.overrides.icon_edit.imported_image.is_some(),
        "Source artwork must survive"
    );
    let graph = recipe
        .overrides
        .imported_graph
        .as_ref()
        .cloned()
        .ok_or("Imported dyes missing")?;
    let document: Value =
        serde_json::from_slice(&fs::read(graph.directory.join("asset-graph.json"))?)?;
    let lookups = document["render_lookups"]
        .as_array()
        .ok_or("Render lookup evidence missing")?;
    for role in [
        "specular_tint",
        "specular_lobe",
        "specular_lobe_3d",
        "iridescence",
    ] {
        assert!(
            lookups.iter().any(|lookup| lookup["name"] == role),
            "missing native render lookup {role}"
        );
    }
    assert!(
        document["render_lookups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|lookup| lookup["identical"] == true)
    );
    let source: Value = serde_json::from_slice(&fs::read(
        graph
            .directory
            .parent()
            .ok_or("Graph parent")?
            .join("source/dyes.json"),
    )?)?;
    // Edit the source recipe through the same sparse overrides the ordinary shader page saves.
    let materials = parhelion::shader::source_materials(&recipe)?;
    assert_eq!(materials.len(), source.as_array().unwrap().len());
    // Build two independently authored recipes using the same pinned assets. Renaming must
    // remain portable, and the edited copy must not replace the original's dye registrations.
    let original_recipe = WeaponRecipe::from_json_str(&recipe.to_json_pretty()?)?;
    let legacy = legacy_graph(&graph, &document, &output.join("legacy-assets"))?;
    let mut legacy_recipe = original_recipe.clone();
    legacy_recipe.overrides.imported_graph = Some(legacy.clone());
    legacy_recipe.rename_authored_item(format!("{} Legacy Copy", recipe.name))?;
    let legacy_recipe = WeaponRecipe::from_json_str(&legacy_recipe.to_json_pretty()?)?;
    recipe.rename_authored_item(format!("{} Edited Copy", recipe.name))?;
    recipe.overrides.dye_edits.push(parhelion::dye::DyeEdit {
        color: Some([170, 30, 80]),
        ..parhelion::dye::DyeEdit::new(
            Some(parhelion::dye::GearType::Armor),
            parhelion::dye::DyeChannel::Armor,
            parhelion::dye::DyeSurface::Primary,
        )
    });
    let writes = recipe.overrides.dye_edits[0].writes();
    let shared = output.join("edited-source.parhelion.json");
    recipe.save_json(&shared)?;
    let reopened = WeaponRecipe::load_json(&shared)?;
    assert_eq!(
        serde_json::from_str::<Value>(&recipe.to_json_pretty()?)?,
        serde_json::from_str::<Value>(&reopened.to_json_pretty()?)?,
    );
    recipe = reopened;
    let graph_directory = graph.directory.clone();
    let graph = parhelion_import::GraphReference::new(
        &graph_directory,
        original_recipe.identity.item_hash.parse_u32()?,
    )?;
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        recipes: vec![
            original_recipe.clone(),
            recipe.clone(),
            legacy_recipe.clone(),
        ],
    })?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |p| println!("{:?}", p.phase))?;
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|x| x == "pkg") {
            let target = packages.join(entry.file_name());
            if !target.exists() {
                fs::hard_link(entry.path(), target)?;
            }
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
        assert!(
            !target.exists(),
            "Staging must never overwrite a stock hard link"
        );
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
    let table = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    let target = recipe.identity.item_hash.parse_u32()?;
    let row = table
        .array(8, 24, None)?
        .into_iter()
        .find(|&r| table.u32(r).ok() == Some(target))
        .ok_or("Authored shader not registered")?;
    let definition = staged.tag(table.u32(row + 16)?, None)?;
    assert_eq!(definition.u8(0xB8)?, 14, "Must remain a native shader plug");
    let root = definition.pointer(0x88)?;
    let custom = definition.array(root + 0x28, 4, None)?;
    let default = definition.array(root + 0x38, 4, None)?;
    let encoded = |rows: &[usize]| {
        rows.iter()
            .map(|&r| definition.0[r..r + 4].to_vec())
            .collect::<Vec<_>>()
    };
    assert_eq!(encoded(&custom), encoded(&default));
    assert!(definition.array(root + 0x48, 4, None)?.is_empty());
    let channels = custom
        .iter()
        .map(|&r| definition.u16(r))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected = source
        .as_array()
        .ok_or("Source dyes")?
        .iter()
        .map(|d| d["channel"].as_u64().unwrap() as u16)
        .collect::<BTreeSet<_>>();
    assert_eq!(channels, expected, "Every source gear channel must survive");
    let readback = dyes::inspect(&mut staged, table.u32(row + 16)?, false)?;
    let original_target = original_recipe.identity.item_hash.parse_u32()?;
    let original_row = table
        .array(8, 24, None)?
        .into_iter()
        .find(|&r| table.u32(r).ok() == Some(original_target))
        .ok_or("Original shader was replaced by its copy")?;
    let original_readback = dyes::inspect(&mut staged, table.u32(original_row + 16)?, false)?;
    let legacy_target = legacy_recipe.identity.item_hash.parse_u32()?;
    let legacy_row = table
        .array(8, 24, None)?
        .into_iter()
        .find(|&r| table.u32(r).ok() == Some(legacy_target))
        .ok_or("Legacy shader missing")?;
    let legacy_readback = dyes::inspect(&mut staged, table.u32(legacy_row + 16)?, false)?;
    let mut animated_channels = Vec::new();
    let mut execution_channels = Vec::new();
    let global_tag = *staged
        .classes(0x8080858D)
        .first()
        .ok_or("Global channels")?;
    let global_payload = staged.tag(global_tag, None)?;
    let global_values = vector_values(&global_payload, 0x18)?;
    for original in source.as_array().unwrap() {
        let emitted = readback
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["channel"] == original["channel"])
            .ok_or("Missing channel")?;
        let converted = &emitted["found"][0];
        let unedited = &original_readback
            .as_array()
            .ok_or("Original shader readback")?
            .iter()
            .find(|d| d["channel"] == original["channel"])
            .ok_or("Original shader lost a channel")?["found"][0];
        assert_ne!(converted["scope"], unedited["scope"]);
        let scope_tag =
            u32::from_str_radix(converted["scope"].as_str().ok_or("Native scope")?, 16)?;
        let scope = staged.tag(scope_tag, None)?;
        let evidence = document["material_programs"]
            .as_array()
            .ok_or("Material program evidence")?
            .iter()
            .find(|p| p["channel"] == original["channel"])
            .ok_or("Channel program evidence")?;
        let program = scope
            .array(0x58, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| scope.u8(at))
            .collect::<Result<Vec<_>, _>>()?;
        let original_program = hex_bytes(
            evidence["conversion"]["native_program"]
                .as_str()
                .ok_or("Native program evidence")?,
        )?;
        let expression_constants = scope
            .array(0x68, 16, Some(0x80800090))?
            .into_iter()
            .flat_map(|at| scope.0[at..at + 16].iter().copied())
            .collect::<Vec<_>>();
        let original_constants = hex_bytes(
            evidence["conversion"]["expression_constants"]
                .as_str()
                .ok_or("Source expression constants")?,
        )?;
        let unedited_scope = staged.tag(
            u32::from_str_radix(unedited["scope"].as_str().ok_or("Original scope")?, 16)?,
            None,
        )?;
        let legacy_channel = &legacy_readback
            .as_array()
            .ok_or("Legacy shader readback")?
            .iter()
            .find(|d| d["channel"] == original["channel"])
            .ok_or("Legacy shader channel missing")?["found"][0];
        let legacy_scope = staged.tag(
            u32::from_str_radix(legacy_channel["scope"].as_str().ok_or("Legacy scope")?, 16)?,
            None,
        )?;
        let legacy_program = legacy_scope
            .array(0x58, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| legacy_scope.u8(at))
            .collect::<Result<Vec<_>, _>>()?;
        for (label, emitted_scope) in [("new", &unedited_scope), ("legacy", &legacy_scope)] {
            let initial = vector_values(emitted_scope, 0x88)?;
            for (source_vector, native_vector) in [(4, 25), (13, 26)] {
                assert_eq!(
                    json!(initial[native_vector][1]),
                    original["found"][0]["constants"][source_vector][3],
                    "The source emission selector must reach the native pixel shader"
                );
            }
            let emitted_buffer = staged.tag(emitted_scope.u32(0xBC)?, None)?;
            let emitted_data = staged.tag(staged.reference(emitted_scope.u32(0xBC)?)?, None)?;
            assert_eq!(emitted_buffer.u32(0)? as usize, initial.len() * 16);
            let inline = initial
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(
                emitted_data.0, inline,
                "Initial buffer and scope values disagree"
            );
            execution_channels.push(json!({
                "channel":format!("{}-{label}", original["channel"]),
                "program":emitted_scope.array(0x58, 1, Some(0x80800009))?.iter()
                    .map(|&at| format!("{:02x}", emitted_scope.0[at])).collect::<String>(),
                "constants":vector_values(emitted_scope, 0x68)?, "initial":initial,
                "globals":global_values,
                "reference":{
                    "program":evidence["conversion"]["native_program_before_components"],
                    "constants":hex_bytes(evidence["conversion"]["source_expression_constants"].as_str()
                        .ok_or("Source expression bytes")?)?.chunks_exact(16)
                        .map(|row| std::array::from_fn::<_, 4, _>(|lane|
                            f32::from_le_bytes(row[lane * 4..lane * 4 + 4].try_into().unwrap())))
                        .collect::<Vec<_>>(),
                    "initial":initial
                }
            }));
        }
        assert_eq!(
            legacy_program, original_program,
            "Repair must retain the live program"
        );
        let unedited_program = unedited_scope
            .array(0x58, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| unedited_scope.u8(at))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(unedited_program, original_program);
        assert!(
            expression_constants.starts_with(&original_constants),
            "Source expression constants must survive edits"
        );
        let edited = original["channel"] == 0;
        let (mut at, mut source_at) = (0, 0);
        while source_at < original_program.len() {
            if program.get(at) == original_program.get(source_at) {
                at += 1;
                source_at += 1;
                continue;
            }
            assert!(
                edited && original_program[source_at] == 0x43,
                "An edit must preserve every original instruction"
            );
            let mask = program
                .get(at..at + 6)
                .ok_or("Edited material mask is truncated")?;
            assert_eq!(
                [mask[0], mask[2], mask[3], mask[5]],
                [0x34, 0x03, 0x34, 0x01]
            );
            assert_eq!(mask[4], mask[1] + 1);
            let vector = original_program[source_at + 1] as usize;
            assert!(writes.iter().any(|(v, _, _)| *v == vector));
            for lane in 0..4 {
                let value = writes
                    .iter()
                    .find(|(v, c, _)| *v == vector && *c == lane)
                    .map(|(_, _, v)| *v);
                for (index, expected) in [
                    (mask[1], if value.is_some() { 0.0f32 } else { 1.0 }),
                    (mask[4], value.unwrap_or(0.0)),
                ] {
                    let offset = index as usize * 16 + lane * 4;
                    assert_eq!(
                        f32::from_le_bytes(expression_constants[offset..offset + 4].try_into()?),
                        expected
                    );
                }
            }
            at += 6;
        }
        assert_eq!(
            at,
            program.len(),
            "Only edited lanes may add expression instructions"
        );
        assert!(
            evidence["conversion"]["outputs"]
                .as_array()
                .ok_or("Program outputs")?
                .iter()
                .all(|o| o["translated"] == true)
        );
        assert_eq!(evidence["conversion"]["donor_program_retained"], false);
        let source_scope = Payload(fs::read(
            graph
                .directory
                .parent()
                .unwrap()
                .join("source/raw")
                .join(format!(
                    "{}.bin",
                    original["found"][0]["scope"]
                        .as_str()
                        .ok_or("Source scope")?
                )),
        )?);
        for emitted_scope in [&scope, &unedited_scope, &legacy_scope] {
            assert_eq!(
                &emitted_scope.0[0xA8..0xB8],
                &source_scope.0[0xA0..0xB0],
                "Native scope metadata must use native field offsets"
            );
            assert!(emitted_scope.0[0x98..0xA8].iter().all(|b| *b == 0));
            for (source_at, native_at) in [
                (0xD0, 0xD8),
                (0x158, 0x170),
                (0x1E0, 0x208),
                (0x268, 0x2A0),
                (0x2F0, 0x338),
            ] {
                for offset in [0, 0x18, 0x28, 0x38, 0x48] {
                    assert_eq!(
                        emitted_scope.u64(native_at + offset)?,
                        0,
                        "Unused native stages must have no resource arrays"
                    );
                }
                assert_eq!(
                    &emitted_scope.0[native_at + 0x78..native_at + 0x80],
                    &source_scope.0[source_at + 0x68..source_at + 0x70],
                    "Absent buffer bindings must use the native stage stride"
                );
            }
        }
        if !original_program.is_empty() {
            animated_channels.push(original["channel"].clone());
            assert_ne!(
                scope.u32(0xAC)? & 0x10,
                0,
                "A live numeric program needs writable native output storage"
            );
            assert_ne!(
                legacy_scope.u32(0xAC)? & 0x10,
                0,
                "Previously imported shaders must gain writable storage when authored"
            );
        }
        assert_eq!(
            scope.u32(0xB8)?,
            source_scope.u32(0xB0)?,
            "Source buffer slot must survive"
        );
        let header = hex_bytes(
            converted["buffer_header_bytes"]
                .as_str()
                .ok_or("Native allocation")?,
        )?;
        let source_header = hex_bytes(
            original["found"][0]["buffer_header_bytes"]
                .as_str()
                .ok_or("Source allocation")?,
        )?;
        assert_eq!(
            &header[4..],
            &source_header[4..],
            "Source buffer write mode must survive"
        );
        for (from, to) in [
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 9),
            (4, 3),
            (5, 10),
            (6, 11),
            (7, 12),
            (8, 17),
            (9, 18),
            (10, 19),
            (11, 20),
            (12, 13),
            (13, 4),
            (14, 14),
            (15, 15),
            (16, 16),
            (17, 21),
            (18, 22),
            (19, 23),
            (20, 24),
        ] {
            assert_eq!(
                unedited["constants"][to], original["found"][0]["constants"][from],
                "Editing a copy changed the original shader",
            );
            let mut expected = original["found"][0]["constants"][from].clone();
            if original["channel"] == 0 {
                for &(vector, lane, value) in &writes {
                    if vector == to {
                        expected[lane] = json!(value);
                    }
                }
            }
            assert_eq!(
                converted["constants"][to], expected,
                "Source material vector {from} changed"
            );
        }
        for index in 0..2 {
            let source_texture = &original["found"][0]["textures"][index];
            let header = Payload(hex_bytes(
                source_texture["header"].as_str().ok_or("Header")?,
            )?);
            let native_header = Payload(hex_bytes(
                converted["textures"][index]["header"]
                    .as_str()
                    .ok_or("Native texture header")?,
            )?);
            assert_eq!(
                native_header.u32(4)?,
                header.u32(4)?,
                "Source encoding and color space must survive"
            );
            assert_eq!(
                &native_header.0[14..22],
                &header.0[34..42],
                "Source dimensions and layers must survive"
            );
            assert_eq!(
                &native_header.0[22..24],
                &header.0[44..46],
                "Source pixel pitch and mip count must survive"
            );
            let source_root = graph.directory.parent().unwrap().join("source/raw");
            let mut bytes = Vec::new();
            let large = header.u32(60)?;
            if ![0, u32::MAX, 0x811C9DC5].contains(&large) {
                bytes.extend(fs::read(source_root.join(format!("{large:08X}.bin")))?);
            }
            bytes.extend(fs::read(source_root.join(format!(
                "{}.bin",
                source_texture["buffer"].as_str().ok_or("Buffer")?
            )))?);
            let tag = u32::from_str_radix(
                converted["textures"][index]["buffer"]
                    .as_str()
                    .ok_or("Native buffer")?,
                16,
            )?;
            assert_eq!(
                staged.tag(tag, None)?.0,
                bytes,
                "Detail texture mip bytes must remain identical"
            );
        }
    }
    if hash == 0xF8313A8C {
        assert_eq!(
            animated_channels.len(),
            channels.len(),
            "Animated sample coverage missing"
        );
    }
    legacy.validate_shader()?;
    graph.validate_shader()?;
    staged.finish()?;
    fs::write(
        output.join("staged-emission-execution.json"),
        serde_json::to_vec_pretty(&json!({"channels":execution_channels,
            "source_item":hash,"gameplay_verified":false}))?,
    )?;
    fs::write(
        output.join("verified-shader.json"),
        serde_json::to_vec_pretty(&json!({
            "source_item":hash,"item_hash":target,"original_item_hash":original_target,
            "independent_shader_copies":true,"channels":channels,"graph":document,
            "build_manifest":built.manifest_path,"source_material_vectors_preserved":true,
            "source_detail_mips_preserved":true,"source_material_programs_staged":true,
        "source_buffer_write_modes_preserved":true,"render_lookups_identical":true,
        "source_material_edits_staged":true,"portable_native_assets":true,
            "animated_channels":animated_channels,"native_writable_outputs_staged":true,
            "legacy_scope_repair_staged":true,"pinned_input_files_unchanged":true,
            "native_shader_registered":true,"gameplay_verified":false
        }))?,
    )?;
    println!(
        "Verified shader artifact: {}",
        output.join("verified-shader.json").display()
    );
    Ok(())
}

/// Reproduce the old serialized layout as an independent E2E input. Its external
/// buffer is dynamic, but the scope's allocation bit occupies the wrong word.
fn legacy_graph(
    reference: &parhelion_import::GraphReference,
    document: &Value,
    output: &Path,
) -> Result<parhelion_import::GraphReference, Box<dyn std::error::Error>> {
    fs::create_dir_all(output)?;
    let mut legacy = document.clone();
    legacy
        .as_object_mut()
        .ok_or("Graph object")?
        .remove("shader_scope_layout");
    for node in document["nodes"].as_array().ok_or("Graph nodes")? {
        let file = node["file"].as_str().ok_or("Node file")?;
        let mut bytes = fs::read(reference.directory.join(file))?;
        if node["symbol"]
            .as_str()
            .is_some_and(|s| s.starts_with("dye-") && s.ends_with("-scope"))
        {
            let symbol = node["symbol"].as_str().ok_or("Scope symbol")?;
            let channel = symbol
                .strip_prefix("dye-")
                .and_then(|s| s.strip_suffix("-scope"))
                .ok_or("Scope channel")?
                .parse::<u64>()?;
            let conversion = &document["material_programs"]
                .as_array()
                .ok_or("Programs")?
                .iter()
                .find(|row| row["channel"] == channel)
                .ok_or("Channel program")?["conversion"];
            // Restore the original scalar/vector mapping as well as the old stage layout.
            // These are pre-translation source bytes recorded before the new conversion runs.
            replace_array(
                &mut bytes,
                0x58,
                1,
                0x80800009,
                &hex_bytes(
                    conversion["native_program_before_components"]
                        .as_str()
                        .ok_or("Old program")?,
                )?,
            )?;
            replace_array(
                &mut bytes,
                0x68,
                16,
                0x80800090,
                &hex_bytes(
                    conversion["source_expression_constants"]
                        .as_str()
                        .ok_or("Old constants")?,
                )?,
            )?;
            let rows = Payload(bytes.clone()).array(0x88, 16, Some(0x80800090))?;
            for vector in [25, 26] {
                bytes[rows[vector] + 4..rows[vector] + 8].fill(0);
            }
            let stages = [0xD8, 0x170, 0x208, 0x2A0, 0x338].map(|at| bytes[at..at + 0x98].to_vec());
            bytes.copy_within(0xA8..0xB8, 0x98);
            bytes[0xA8..0xB8].fill(0);
            bytes[0xC0..0xC8].fill(0);
            for (at, stage) in [0xC8, 0x150, 0x1D8, 0x260, 0x2E8].into_iter().zip(stages) {
                bytes[at..at + 0x88].fill(0);
                bytes[at + 0x10..at + 0x18].copy_from_slice(&stage[0x10..0x18]);
                bytes[at + 0x58..at + 0x68].copy_from_slice(&stage[0x68..0x78]);
                bytes[at + 0x78..at + 0x80].copy_from_slice(&stage[0x78..0x80]);
            }
        }
        let path = output.join(file);
        fs::create_dir_all(path.parent().ok_or("Node parent")?)?;
        fs::write(path, bytes)?;
    }
    if let Some(file) = document["source_icon_png"].as_str() {
        fs::copy(reference.directory.join(file), output.join(file))?;
    }
    fs::write(
        output.join("asset-graph.json"),
        serde_json::to_vec_pretty(&legacy)?,
    )?;
    Ok(parhelion_import::GraphReference::new(
        output,
        u32::try_from(document["item_hash"].as_u64().ok_or("Graph identity")?)?,
    )?)
}

fn vector_values(
    payload: &Payload,
    at: usize,
) -> Result<Vec<[f32; 4]>, Box<dyn std::error::Error>> {
    payload
        .array(at, 16, Some(0x80800090))?
        .into_iter()
        .map(|at| {
            Ok([
                payload.f32(at)?,
                payload.f32(at + 4)?,
                payload.f32(at + 8)?,
                payload.f32(at + 12)?,
            ])
        })
        .collect()
}

fn replace_array(
    bytes: &mut Vec<u8>,
    at: usize,
    stride: usize,
    class: u32,
    rows: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(rows.len() % stride, 0);
    if rows.is_empty() {
        bytes[at..at + 16].fill(0);
        return Ok(());
    }
    while bytes.len() % 16 != 12 {
        bytes.push(0);
    }
    bytes.extend(0x80809FBDu32.to_le_bytes());
    let start = bytes.len();
    let count = u64::try_from(rows.len() / stride)?;
    bytes.extend(count.to_le_bytes());
    bytes.extend(u64::from(class).to_le_bytes());
    bytes.extend(rows);
    bytes[at..at + 8].copy_from_slice(&count.to_le_bytes());
    bytes[at + 8..at + 16]
        .copy_from_slice(&(i64::try_from(start)? - i64::try_from(at + 8)?).to_le_bytes());
    let len = u64::try_from(bytes.len())?;
    bytes[..8].copy_from_slice(&len.to_le_bytes());
    Ok(())
}

fn hex_bytes(value: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
        .collect()
}
