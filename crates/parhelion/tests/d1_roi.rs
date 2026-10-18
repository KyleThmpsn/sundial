//! Configured D1 source to portable recipe to native package acceptance.
#![cfg(feature = "d2-model-importer")]
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe,
    build_and_stage_snapshot_with_progress,
};
use parhelion_import::{
    d1_roi::{NativeOptions, Source},
    tiger::reader::{Reader, package_stamp},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf};
use sundial::investment::InvestmentCatalog;

fn inspect_source_plates(
    source: &Source,
    graph: &Value,
    directory: &std::path::Path,
) -> Result<Value, Box<dyn std::error::Error>> {
    use parhelion_import::tiger::payload::Payload;
    use std::collections::BTreeMap;
    let mut receipt = Vec::new();
    for node in graph["nodes"].as_array().ok_or("Graph assets")? {
        let symbol = node["symbol"].as_str().ok_or("Graph symbol")?;
        let Some(tag) = symbol.strip_prefix("plate-").filter(|tag| tag.len() == 8) else {
            continue;
        };
        let tag = u32::from_str_radix(tag, 16)?;
        let plate = source.read(tag)?;
        let kind = plate.u16(8)? as usize;
        let edge = [2048usize, 2048, 1024, 1024][plate.u16(10)? as usize];
        let format = [72, 83, 71][kind];
        let block = if kind == 1 { 16 } else { 8 };
        let header = Payload(fs::read(
            directory.join(node["file"].as_str().ok_or("Plate header")?),
        )?);
        assert_eq!(
            (
                header.u32(4)?,
                header.u16(14)? as usize,
                header.u16(16)? as usize,
                header.u8(23)?
            ),
            (format, edge, edge, 5)
        );
        let data_symbol = node["reference"].as_str().ok_or("Plate data reference")?;
        let data_node = graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["symbol"] == data_symbol)
            .ok_or("Plate data node")?;
        let emitted = fs::read(directory.join(data_node["file"].as_str().ok_or("Plate bytes")?))?;
        let mut expected = BTreeMap::new();
        for row in plate.array(16, 20, Some(0x80800163))? {
            let texture = source.read(plate.u32(row)?)?;
            if texture.u8(0x39)? != 5 {
                continue;
            }
            assert_eq!(texture.u32(0)?, format);
            assert_eq!(
                texture.u32(4)?,
                8,
                "The independent plate oracle requires linear source blocks"
            );
            let original = source.read(source.reference(plate.u32(row)?)?)?;
            let width = usize::from(texture.u16(0x30)?);
            let height = usize::from(texture.u16(0x32)?);
            assert_eq!(
                (width, height),
                (plate.u32(row + 12)? as usize, plate.u32(row + 16)? as usize)
            );
            let mut input = 0;
            let mut output = 0;
            for mip in 0..5 {
                let (w, h, pitch) = (width >> (mip + 2), height >> (mip + 2), edge >> (mip + 2));
                let (x, y) = (
                    plate.u32(row + 4)? as usize >> (mip + 2),
                    plate.u32(row + 8)? as usize >> (mip + 2),
                );
                for by in 0..h {
                    for bx in 0..w {
                        expected.insert(
                            output + ((y + by) * pitch + x + bx) * block,
                            original.0
                                [input + (by * w + bx) * block..input + (by * w + bx + 1) * block]
                                .to_vec(),
                        );
                    }
                }
                input +=
                    (width >> mip).max(1).div_ceil(4) * (height >> mip).max(1).div_ceil(4) * block;
                output += pitch * pitch * block;
            }
        }
        for (offset, bytes) in &expected {
            assert_eq!(
                emitted.get(*offset..*offset + block),
                Some(bytes.as_slice()),
                "Authored plate changed a source BC block at {tag:08X}/{offset:X}"
            );
        }
        assert!(
            !expected.is_empty(),
            "Required plate has no independently checked source blocks"
        );
        receipt.push(json!({"source":tag,"edge":edge,"format":format,"mips":5,"source_blocks":expected.len()}));
    }
    Ok(json!(receipt))
}

#[allow(clippy::cognitive_complexity)]
fn inspect_native_body(
    reader: &mut Reader,
    parent: u32,
) -> Result<Value, Box<dyn std::error::Error>> {
    let parent_data = reader.tag(parent, Some(0x8080744A))?;
    let entity = reader.tag(parent_data.u32(16)?, Some(0x80809C0F))?;
    let mut models = BTreeSet::new();
    let mut shaders = BTreeSet::new();
    let mut stages = Vec::new();
    for at in entity.array(16, 12, Some(0x80809C04))? {
        let owner_tag = entity.u32(at)?;
        let owner = reader.tag(owner_tag, Some(0x80809C36))?;
        let resource = owner.pointer(24)?;
        if !matches!(owner.u32(resource - 4)?, 0x808072BD | 0x80807286) {
            continue;
        }
        // Native inspection follows the instance's definition reference, which
        // can precede the sealed schema view selected by the file header.
        let instance = owner.pointer(16)?;
        assert_eq!(owner.u32(instance)?, owner_tag);
        let runtime_definition = usize::try_from(owner.u64(instance + 8)?)?;
        for definition in [runtime_definition, resource] {
            assert_eq!(
                owner.u32(definition + 0x248)?,
                u32::MAX,
                "Prebuilt armor textures must not retain a runtime donor atlas"
            );
        }
        let model_tag = owner.u32(resource + 0x1DC)?;
        models.insert(model_tag);
        let model = reader.tag(model_tag, Some(0x808073A5))?;
        for mesh in model.array(16, 136, Some(0x80807378))? {
            for part in model.array(mesh + 24, 32, Some(0x8080737E))? {
                let material = reader.tag(model.u32(part)?, Some(0x808071E8))?;
                for (offset, subtype, program_stage) in [
                    (0x48, 1, 1),
                    (0xE8, 4, 3),
                    (0x188, 5, 4),
                    (0x228, 2, 2),
                    (0x2C8, 0, 0),
                ] {
                    let shader = material.u32(offset)?;
                    if matches!(shader, 0 | u32::MAX) || !shaders.insert(shader) {
                        continue;
                    }
                    let header_entry = reader
                        .manager
                        .get_entry(tiger_pkg::TagHash(shader))
                        .ok_or("Shader header entry")?;
                    assert_eq!(
                        (header_entry.file_type, header_entry.file_subtype),
                        (33, subtype)
                    );
                    let data_tag = header_entry.reference;
                    let data_entry = reader
                        .manager
                        .get_entry(tiger_pkg::TagHash(data_tag))
                        .ok_or("Shader data entry")?;
                    assert_eq!(
                        (
                            data_entry.file_type,
                            data_entry.file_subtype,
                            data_entry.reference
                        ),
                        (41, subtype, shader)
                    );
                    let header = reader.tag(shader, None)?;
                    let data = reader.tag(data_tag, None)?;
                    assert_eq!(header.u32(8)? as usize, data.0.len());
                    let executable = (0..data.u32(28)?)
                        .map(|i| data.u32(32 + i as usize * 4))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .find(|offset| data.bytes::<4>(*offset as usize).ok() == Some(*b"SHEX"))
                        .ok_or("Shader executable chunk")?
                        as usize;
                    assert_eq!(data.u32(executable + 8)?, (program_stage << 16) | 0x50);
                    stages.push(json!({"header":shader,"data":data_tag,"subtype":subtype,"bytes":data.0.len()}));
                }
            }
        }
    }
    assert!(
        !models.is_empty() && !shaders.is_empty(),
        "Native body has no rendering graph"
    );
    Ok(json!({"parent":parent,"models":models,"shaders":stages}))
}

#[test]
#[ignore = "Requires configured D1 and stock native packages, PARHELION_D1_ROI_ITEMS, PARHELION_D1_XG and fresh PARHELION_D1_NATIVE_OUTPUT"]
#[allow(clippy::cognitive_complexity)]
fn source_armor_and_ornaments_survive_portable_reload_and_native_staging()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let source_path = configured("PARHELION_D1_ROI_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    let output = parhelion_import::tiger::reader::outside(
        &configured("PARHELION_D1_NATIVE_OUTPUT")?,
        &native,
    )?;
    let output = parhelion_import::tiger::reader::outside(&output, &source_path)?;
    assert!(!output.exists(), "Use a fresh native artifact directory");
    let items = env::var("PARHELION_D1_ROI_ITEMS")?
        .split(',')
        .map(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(!items.is_empty(), "The required source corpus is empty");
    fs::create_dir_all(&output)?;
    let native_stamp = package_stamp(&native)?;
    let source = Source::open(&source_path, |_, _| {})?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().ok_or("Native root")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )?;
    let donors = json!({"gear":catalog.gear_donors(ItemKind::Armor.bucket_hashes()).into_iter().map(|d| json!({"hash":d.hash,"name":d.name,"bucket_hash":d.bucket_hash,"class_type":catalog.item_class_type(d.hash),"collection_backed":d.collection_backed})).collect::<Vec<_>>()});
    let options = NativeOptions {
        xg_library: Some(configured("PARHELION_D1_XG")?),
    };
    let mut stock = Reader::new(&native, &output.join("stock-readback"), false)?;
    let mut recipes = Vec::new();
    let mut evidence = Vec::new();
    for item in items {
        let folder = output.join(format!("item-{item:08X}"));
        let recipe_path =
            source.prepare_armor(item, &native, &donors, &folder, &options, &mut |step| {
                println!("{step}")
            })?;
        let recipe = WeaponRecipe::load_json(&recipe_path)?;
        assert_eq!(recipe.kind, ItemKind::Armor);
        let portable = recipe.to_json_pretty()?;
        fs::write(folder.join("portable.parhelion.json"), &portable)?;
        let reloaded = WeaponRecipe::from_json_str(&portable)?;
        assert_eq!(recipe.identity, reloaded.identity);
        let reference = reloaded
            .overrides
            .imported_graph
            .as_ref()
            .ok_or("Imported source art missing")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
        assert_eq!(graph["source"], "d1_roi");
        assert_eq!(graph["source_item"], item);
        let parts = graph["gear_art"]["parts"]
            .as_array()
            .ok_or("Body art missing")?;
        assert_eq!(
            parts.len(),
            2,
            "Both source body variants must remain selectable"
        );
        let ornaments = graph["source_ornaments"]
            .as_array()
            .ok_or("Source ornament trace missing")?;
        assert_eq!(
            reference.attachments.len(),
            ornaments.len(),
            "A linked source ornament was lost"
        );
        let mut targets = BTreeSet::new();
        let mut ornament_receipts = Vec::new();
        for attachment in &reference.attachments {
            let ornament: Value =
                serde_json::from_slice(&fs::read(attachment.directory.join("asset-graph.json"))?)?;
            assert_eq!(
                ornament["ornament"]["target_item"],
                recipe.identity.item_hash.parse_u32()?
            );
            assert!(
                targets.insert(
                    ornament["item_hash"]
                        .as_u64()
                        .ok_or("Private ornament identity")?
                )
            );
            assert_eq!(
                ornament["gear_art"]["parts"]
                    .as_array()
                    .ok_or("Ornament bodies")?
                    .len(),
                2
            );
            let plates = inspect_source_plates(&source, &ornament, &attachment.directory)?;
            ornament_receipts.push(json!({"item":ornament["item_hash"],"link":ornament["ornament"],"source_plate_readback":plates}));
        }
        let plates = inspect_source_plates(&source, &graph, &reference.directory)?;
        evidence.push(json!({"source":item,"item":recipe.identity.item_hash.parse_u32()?,"source_graph":graph,"ornaments":ornament_receipts,"source_plate_readback":plates}));
        recipes.push(reloaded);
    }
    let mut batch = recipes.clone();
    if let Some(path) = env::var_os("PARHELION_D1_ADDITIONAL_RECIPE") {
        batch.push(WeaponRecipe::load_json(PathBuf::from(path))?);
    }

    let globals = stock
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|tag| tag.name == "investment_globals")
        .ok_or("Stock globals")?
        .hash
        .0;
    let globals = stock.tag(globals, None)?;
    let root = stock.tag(globals.u32(16)?, None)?;
    let stock_items = stock.tag(root.u32(8 + 48 * 16)?, None)?;
    let stock_lookup = stock.tag(root.u32(8 + 50 * 16)?, None)?;
    let mut stock_definitions = std::collections::BTreeMap::new();
    for recipe in &recipes {
        let mut hashes = vec![recipe.donor.item_hash.parse_u32()?];
        hashes.extend(
            recipe
                .overrides
                .socket_plug_variants
                .iter()
                .map(|v| v.source_plug_hash.parse_u32())
                .collect::<Result<Vec<_>, _>>()?,
        );
        for hash in hashes {
            let row = stock_items
                .array(8, 24, Some(0x80807BE8))?
                .into_iter()
                .find(|&at| stock_items.u32(at).ok() == Some(hash))
                .ok_or("Stock donor definition")?;
            let tag = stock_items.u32(row + 16)?;
            stock_definitions.insert(tag, stock.tag(tag, Some(0x80807BEA))?.0.clone());
        }
    }
    stock.finish()?;
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: batch,
    })?;
    let built =
        build_and_stage_snapshot_with_progress(&snapshot, |step| println!("{:?}", step.phase))?;
    assert_eq!(
        native_stamp,
        package_stamp(&native)?,
        "Staging changed stock packages"
    );
    let view = output.join("view");
    let packages = view.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(view.join("bin/x64"))?;
    fs::hard_link(
        native.parent().ok_or("Native root")?.join("destiny2.exe"),
        view.join("destiny2.exe"),
    )?;
    for entry in fs::read_dir(&native)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|s| s == "pkg") {
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
        if target.exists() {
            // Remove this fresh view's hard link before copying a replacement package.
            // Writing through the link would modify the stock package itself.
            fs::remove_file(&target)?;
        }
        fs::copy(built.run_directory.join(&artifact.file_name), target)?;
    }
    let mut readback = Reader::new(&packages, &output.join("readback"), false)?;
    let global_tag = readback
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .ok_or("Native globals")?
        .hash
        .0;
    let globals = readback.tag(global_tag, None)?;
    let root = readback.tag(globals.u32(16)?, None)?;
    let table = readback.tag(root.u32(8 + 48 * 16)?, None)?;
    let lookup = readback.tag(root.u32(8 + 50 * 16)?, None)?;
    let art = readback.tag(globals.u32(16 + 66 * 16)?, None)?;
    let art_rows = art.array(8, 32, Some(0x80805DFB))?;
    let collectibles = readback.tag(root.u32(8 + 19 * 16)?, None)?;
    let assignments = readback.tag(0x80EC3F61, None)?;
    for (recipe, receipt) in recipes.iter().zip(&mut evidence) {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let row = table
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .find(|&r| table.u32(r).ok() == Some(hash))
            .ok_or("Imported armor is absent")?;
        let definition = readback.tag(table.u32(row + 16)?, Some(0x80807BEA))?;
        assert!((3..=7).contains(&definition.u8(0xB8)?));
        let selections = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
        assert!(!selections.is_empty());
        for selection in selections {
            let row = *art_rows
                .get(usize::from(definition.u16(selection + 2)?))
                .ok_or("Unregistered body art")?;
            let masculine = art.u32(row + 8)?;
            let feminine = art.u32(row + 12)?;
            assert!(![0, u32::MAX, 0x811C9DC5].contains(&masculine));
            assert!(![0, u32::MAX, 0x811C9DC5].contains(&feminine));
            assert_ne!(masculine, feminine, "Body assignments were collapsed");
            let mut bodies = Vec::new();
            for key in [masculine, feminine] {
                let at = assignments
                    .array(8, 8, None)?
                    .into_iter()
                    .find(|at| assignments.u32(*at).ok() == Some(key))
                    .ok_or("Body assignment was not emitted")?;
                bodies.push(inspect_native_body(
                    &mut readback,
                    assignments.u32(at + 4)?,
                )?);
            }
            receipt["native_bodies"] = json!(bodies);
        }
        let item_rows = table.array(8, 24, Some(0x80807BE8))?;
        for ornament in receipt["ornaments"]
            .as_array_mut()
            .ok_or("Ornament receipts")?
        {
            let target = ornament["item"].as_u64().ok_or("Private ornament item")? as u32;
            let link = &ornament["link"];
            let source_plug = link["native_plug"]
                .as_u64()
                .ok_or("Stock ornament identity")? as u32;
            for field in [8, 24] {
                let source_rows = stock_lookup
                    .array(field, 8, None)?
                    .into_iter()
                    .filter(|&row| stock_lookup.u32(row).ok() == Some(source_plug))
                    .count();
                let authored_rows = lookup
                    .array(field, 8, None)?
                    .into_iter()
                    .filter(|&row| lookup.u32(row).ok() == Some(target))
                    .collect::<Vec<_>>();
                assert_eq!(
                    authored_rows.len(),
                    source_rows,
                    "Ornament lost an item lookup family"
                );
                for row in authored_rows {
                    assert_eq!(
                        table.u32(item_rows[lookup.u16(row + 4)? as usize])?,
                        target,
                        "Ornament lookup resolves a different item"
                    );
                }
            }
            let sockets = definition.array(definition.pointer(0x68)?, 0x50, Some(0x808077C4))?;
            let socket = sockets[link["target_socket"].as_u64().ok_or("Ornament socket")? as usize];
            let initial = item_rows[definition.u16(socket + 2)? as usize];
            assert_eq!(
                table.u32(initial)? as u64,
                link["default_reset"].as_u64().ok_or("Reset")?
            );
            let reset = readback.tag(table.u32(initial + 16)?, Some(0x80807BEA))?;
            assert!(
                reset
                    .array(reset.pointer(0x88)?, 4, Some(0x808077B5))?
                    .is_empty(),
                "Reset retained alternate art"
            );
            let choices = definition.array(socket + 0x40, 0x20, Some(0x80802E03))?;
            let choice = choices
                .into_iter()
                .find(|&at| {
                    table
                        .u32(item_rows[definition.u16(at).unwrap() as usize])
                        .ok()
                        == Some(target)
                })
                .ok_or("Private ornament missing from actual socket")?;
            assert_eq!(
                definition.u64(choice + 8)?,
                0,
                "Ornament has an unlock condition"
            );
            assert_eq!(
                definition.u64(choice + 16)?,
                0,
                "Ornament has a secondary condition"
            );
            let plug = readback.tag(
                table.u32(item_rows[definition.u16(choice)? as usize] + 16)?,
                Some(0x80807BEA),
            )?;
            assert_eq!(plug.u8(0xB8)?, 13);
            let mut bodies = Vec::new();
            for selection in plug.array(plug.pointer(0x88)?, 4, Some(0x808077B5))? {
                let row = art_rows[plug.u16(selection + 2)? as usize];
                for key in [art.u32(row + 8)?, art.u32(row + 12)?] {
                    let at = assignments
                        .array(8, 8, None)?
                        .into_iter()
                        .find(|at| assignments.u32(*at).ok() == Some(key))
                        .ok_or("Ornament body assignment missing")?;
                    bodies.push(inspect_native_body(
                        &mut readback,
                        assignments.u32(at + 4)?,
                    )?);
                }
            }
            assert_eq!(bodies.len(), 2, "Ornament did not retain both bodies");
            ornament["native_bodies"] = json!(bodies);
        }
        receipt["native_definition"] = json!(table.u32(row + 16)?);
        let item_index = table
            .array(8, 24, Some(0x80807BE8))?
            .iter()
            .position(|at| *at == row)
            .ok_or("Native armor ordinal")?;
        let collectible = collectibles
            .array(8, 0xB8, None)?
            .into_iter()
            .find(|at| collectibles.u16(at + 0x2C).ok() == Some(item_index as u16))
            .ok_or("Native armor collectible")?;
        for field in [0x30, 0x40, 0x50, 0x60] {
            assert_eq!(
                collectibles.u64(collectible + field)?,
                0,
                "Imported armor retained an account condition"
            );
        }
        let condition = collectibles.array(collectible + 0x70, 8, Some(0x80807D31))?;
        assert_eq!(condition.len(), 1);
        assert_eq!(
            collectibles.u8(condition[0])?,
            11,
            "Collectible must use the native literal instruction"
        );
        assert_eq!(
            collectibles.u32(condition[0] + 4)?,
            1,
            "Collectible must be acquired without an account flag"
        );
        assert_eq!(
            collectibles.u16(collectible + 0x98)?,
            0,
            "Collections reclaim is disabled"
        );
    }
    for (tag, expected) in stock_definitions {
        assert_eq!(
            readback.tag(tag, Some(0x80807BEA))?.0,
            expected,
            "Staging mutated a stock armor or ornament donor"
        );
    }
    readback.finish()?;
    let staged = InvestmentCatalog::load_with_cache_path(
        &view,
        &output.join("staged-catalog.json"),
        true,
        |_| {},
    )?;
    for (recipe, receipt) in recipes.iter().zip(&evidence) {
        assert!(
            !staged
                .item_collection_paths(recipe.identity.item_hash.parse_u32()?)
                .is_empty(),
            "Imported armor is missing from Collections"
        );
        assert_eq!(
            staged.item_display_name(recipe.identity.item_hash.parse_u32()?),
            Some(recipe.name.as_str()),
            "Imported armor lost its display name"
        );
        for ornament in receipt["ornaments"].as_array().ok_or("Ornament receipts")? {
            assert_eq!(
                staged.item_display_name(ornament["item"].as_u64().ok_or("Ornament identity")? as u32),
                ornament["link"]["name"].as_str(),
                "Private ornament lost its authored choice label"
            );
        }
    }
    fs::write(
        output.join("native-readback.json"),
        serde_json::to_vec_pretty(
            &json!({"source":source.report(),"native_stamp":native_stamp,"profile":"release","source_revision":std::process::Command::new("git").args(["rev-parse","HEAD"]).output().ok().map(|o|String::from_utf8_lossy(&o.stdout).trim().to_owned()),"artifacts":built.artifacts,"items":evidence,"installed":false,"gameplay_verified":false}),
        )?,
    )?;
    Ok(())
}
