//! Configured D2 item import, portable authoring and native registration readback.
#![cfg(feature = "d2-model-importer")]

use base64::{Engine as _, engine::general_purpose::STANDARD};
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, ItemKind, WeaponRecipe,
    build_and_stage_snapshot_with_progress,
};
use parhelion_import::d2_mot::{payload::Payload, reader::Reader, service};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::PathBuf,
};
use sundial::investment::InvestmentCatalog;

#[test]
#[ignore = "Requires configured modern and clean native packages, PARHELION_GEAR_MATRIX and PARHELION_GEAR_OUTPUT"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn gear_imports_keep_their_family_and_art_variants_through_staging()
-> Result<(), Box<dyn std::error::Error>> {
    let configured = |key| {
        env::var_os(key)
            .map(PathBuf::from)
            .ok_or_else(|| format!("Set {key}"))
    };
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?;
    let native = configured("SUNDIAL_STOCK_PACKAGES")?;
    // Matrix entries are { "hash": <source u32>, "kind": "armor" | ... }.
    // Callers select sources from their package version, without personal paths or fixed items.
    let matrix: Vec<Value> =
        serde_json::from_slice(&fs::read(configured("PARHELION_GEAR_MATRIX")?)?)?;
    let kinds = matrix
        .iter()
        .map(|v| v["kind"].as_str())
        .collect::<BTreeSet<_>>();
    for kind in ["armor", "ghost_shell", "ship", "sparrow", "emblem"] {
        assert!(kinds.contains(&Some(kind)), "Matrix must include {kind}");
    }
    let output = parhelion_import::d2_mot::reader::outside(
        &configured("PARHELION_GEAR_OUTPUT")?,
        modern.parent().ok_or("Modern root")?,
    )?;
    let output =
        parhelion_import::d2_mot::reader::outside(&output, native.parent().ok_or("Native root")?)?;
    assert!(!output.exists(), "Use a fresh artifact directory");
    fs::create_dir_all(&output)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native.parent().ok_or("Native root")?,
        &output.join("catalog-cache.json"),
        true,
        |_| {},
    )?;
    let gear = [
        ItemKind::Armor,
        ItemKind::GhostShell,
        ItemKind::Ship,
        ItemKind::Sparrow,
    ]
    .into_iter()
    .flat_map(|kind| catalog.gear_donors(kind.bucket_hashes()))
    .map(|d| {
        json!({
            "hash": d.hash, "name": d.name, "bucket_hash": d.bucket_hash,
            "class_type": catalog.item_class_type(d.hash),
            "collection_backed": d.collection_backed
        })
    })
    .collect::<Vec<_>>();
    let emblems = catalog
        .gear_donors(ItemKind::Emblem.bucket_hashes())
        .into_iter()
        .map(|donor| json!({"hash":donor.hash,"name":donor.name}))
        .collect::<Vec<_>>();
    let donors = json!({"gear": gear,"emblems":emblems});
    let items = service::scan_cached(&modern, &native, &output.join("catalog"), false, |_| {})?;
    let mut recipes = Vec::new();
    let mut evidence = Vec::new();
    let mut armor_coverage = BTreeSet::new();
    let mut animated_kinds = BTreeSet::new();
    for entry in &matrix {
        let source = items
            .iter()
            .find(|i| Some(u64::from(i.hash)) == entry["hash"].as_u64())
            .ok_or("Source missing from importer discovery")?;
        assert!(
            !source.dummy,
            "Equippable gear must not require a weapon pattern"
        );
        let path = service::prepare_with_progress(
            source,
            &modern,
            &native,
            &donors,
            &output.join(format!("import-{:08X}", source.hash)),
            &mut |step| println!("{step}"),
        )?;
        let mut recipe = WeaponRecipe::load_json(&path)?;
        assert_eq!(serde_json::to_value(recipe.kind)?, entry["kind"]);
        assert_eq!(recipe.name, source.name);
        assert!(recipe.presentation_donor.is_none());
        assert!(recipe.overrides.icon_edit.imported_image.is_some());
        if recipe.kind == ItemKind::Armor {
            armor_coverage.insert((
                source.bucket_hash.ok_or("Armor slot")?,
                source.class_type.ok_or("Armor class")?,
            ));
        }
        let graph = if recipe.kind == ItemKind::Emblem {
            assert!(recipe.overrides.imported_graph.is_none());
            let nameplate = recipe
                .overrides
                .nameplate
                .as_mut()
                .ok_or("Nameplate missing")?;
            for part in parhelion::emblem::NameplatePart::ALL {
                assert!(matches!(
                    nameplate.part(part),
                    Some(parhelion::emblem::NameplateImage::Image { .. })
                ));
            }
            let colors = nameplate.colors.as_mut().ok_or("Source colors missing")?;
            // A normal authoring edit must reach the native container after sharing.
            colors[0][0] = parhelion::dye::DyeValue::new(0.25).ok_or("Finite color")?;
            Value::Null
        } else {
            let reference = recipe
                .overrides
                .imported_graph
                .as_ref()
                .ok_or("Source assets missing")?;
            let graph: Value =
                serde_json::from_slice(&fs::read(reference.directory.join("asset-graph.json"))?)?;
            assert!(
                !graph["gear_art"]["rows"]
                    .as_array()
                    .ok_or("Art variants missing")?
                    .is_empty()
            );
            assert!(
                !graph["nodes"]
                    .as_array()
                    .ok_or("Native assets missing")?
                    .is_empty()
            );
            if matches!(
                recipe.kind,
                ItemKind::GhostShell | ItemKind::Ship | ItemKind::Sparrow
            ) {
                assert!(
                    graph["equipment_animation"].is_object(),
                    "Equipment animation was not inspected"
                );
                if graph["equipment_animation"]["status"] == "linked" {
                    animated_kinds.insert(entry["kind"].as_str().ok_or("Kind")?.to_owned());
                }
            }
            graph
        };
        // The ordinary name control changes private identity. Saving and sharing must still work.
        recipe.rename_authored_item(format!("{} Coverage Copy", recipe.name))?;
        let portable = recipe.to_json_pretty()?;
        let document: Value = serde_json::from_str(&portable)?;
        if recipe.kind != ItemKind::Emblem {
            assert!(
                document["overrides"]["imported_graph"]
                    .get("directory")
                    .is_none()
            );
            assert!(document["overrides"]["imported_graph"]["embedded_assets"].is_object());
        }
        fs::write(
            output.join(format!("{:08X}.parhelion.json", source.hash)),
            &portable,
        )?;
        let reopened = WeaponRecipe::from_json_str(&portable)?;
        assert_eq!(reopened.identity, recipe.identity);
        evidence.push(
            json!({"source": source.hash, "kind": entry["kind"], "graph": graph,
            "nameplate":document["overrides"]["nameplate"]}),
        );
        recipes.push(reopened);
    }
    for bucket in ItemKind::Armor.bucket_hashes() {
        for class in 0..3 {
            assert!(
                armor_coverage.contains(&(u32::try_from(*bucket)?, class)),
                "Matrix must cover armor bucket {bucket}, class {class}"
            );
        }
    }
    for kind in ["ghost_shell", "sparrow"] {
        assert!(
            animated_kinds.contains(kind),
            "Matrix must exercise converted {kind} animations"
        );
    }
    let duplicate = recipes
        .iter()
        .position(|recipe| recipe.kind == ItemKind::GhostShell)
        .ok_or("Ghost recipe missing")?;
    let mut copy = recipes[duplicate].clone();
    copy.rename_authored_item(format!("{} Second Copy", copy.name))?;
    let shared = copy.to_json_pretty()?;
    fs::write(output.join("duplicate-ghost.parhelion.json"), &shared)?;
    recipes.push(WeaponRecipe::from_json_str(&shared)?);
    evidence.push(evidence[duplicate].clone());
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
        let destination = packages.join(&artifact.file_name);
        assert!(
            !destination.exists(),
            "Never replace a stock package hard link"
        );
        fs::copy(built.run_directory.join(&artifact.file_name), destination)?;
    }
    let mut staged = Reader::new(&packages, &output.join("readback"), false)?;
    let globals = staged
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .ok_or("Globals")?
        .hash
        .0;
    let globals = staged.tag(globals, None)?;
    let root = staged.tag(globals.u32(16)?, None)?;
    let items = staged.tag(root.u32(8 + 48 * 16)?, None)?;
    let metadata = staged.tag(globals.u32(16 + 66 * 16)?, None)?;
    let art_rows = metadata.array(8, 32, Some(0x80805DFB))?;
    let mut private_rows = BTreeSet::new();
    let mut private_keys = BTreeSet::new();
    for (recipe, result) in recipes.iter().zip(&mut evidence) {
        let hash = recipe.identity.item_hash.parse_u32()?;
        let row = items
            .array(8, 24, Some(0x80807BE8))?
            .into_iter()
            .find(|&r| items.u32(r).ok() == Some(hash))
            .ok_or("Authored item absent")?;
        let definition = staged.tag(items.u32(row + 16)?, Some(0x80807BEA))?;
        if recipe.kind == ItemKind::Emblem {
            assert_eq!(definition.u8(0xB8)?, 27);
            verify_nameplate(&mut staged, &globals, hash, &result["nameplate"])?;
            result["item_hash"] = json!(hash);
            result["native_registration_verified"] = json!(true);
            result["nameplate_pixels_and_colors_verified"] = json!(true);
            continue;
        }
        let source_rows = result["graph"]["gear_art"]["rows"]
            .as_array()
            .ok_or("Source rows")?;
        let emitted = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
        assert_eq!(
            source_rows.len(),
            emitted.len(),
            "Art variants were collapsed"
        );
        let mut assignments = BTreeMap::new();
        for (source, at) in source_rows.iter().zip(emitted) {
            assert_eq!(
                i64::from(definition.u8(at)? as i8),
                source["class"].as_i64().ok_or("Class")?
            );
            assert_eq!(
                u64::from(definition.u8(at + 1)?),
                source["flags"].as_u64().ok_or("Art flags")?
            );
            let index = definition.u16(at + 2)? as usize;
            assert!(
                private_rows.insert(index),
                "Different imported art rows share an allocation"
            );
            let offset = *art_rows.get(index).ok_or("Art index is unregistered")?;
            assert!(!registered_keys(&metadata, offset)?.is_empty());
            verify_art_layout(&metadata, offset, source, &mut assignments)?;
        }
        for &key in assignments.values() {
            assert!(
                private_keys.insert(key),
                "Imported items share an art identity"
            );
        }
        let expected_bucket = match recipe.kind {
            ItemKind::Armor => 3..=7,
            ItemKind::GhostShell => 8..=8,
            ItemKind::Sparrow => 9..=9,
            ItemKind::Ship => 10..=10,
            _ => return Err("Unexpected gear kind".into()),
        };
        assert!(expected_bucket.contains(&definition.u8(0xB8)?));
        assert_eq!(
            u64::from(definition.u8(0xBA)?),
            result["graph"]["source_rarity"]
                .as_u64()
                .ok_or("Source rarity")?,
            "Native template changed the imported rarity"
        );
        if recipe.kind == ItemKind::Armor && definition.u8(0xBA)? == 5 {
            let equipment = definition.pointer(0x10)?;
            assert_eq!(definition.u32(equipment + 0x10)?, 0x1ED94273);
            assert_eq!(definition.u32(equipment + 0x14)?, 0x2D5D6C45);
        }
        if matches!(
            recipe.kind,
            ItemKind::GhostShell | ItemKind::Ship | ItemKind::Sparrow
        ) {
            let rig =
                parhelion_import::d2_mot::rig::inspect(&mut staged, items.u32(row + 16)?, false)?;
            assert_eq!(
                u32::from_str_radix(rig["pattern_key"].as_str().ok_or("Pattern key")?, 16)?,
                recipe
                    .identity
                    .pattern_global_id_hash
                    .as_ref()
                    .ok_or("Authored equipment pattern identity is missing")?
                    .parse_u32()?,
                "Equipment retained the template's shared runtime identity"
            );
            verify_equipment_clips(&mut staged, &rig, recipe, &result["graph"])?;
            result["runtime_registration_verified"] = json!(true);
        }
        result["item_hash"] = json!(hash);
        result["native_bucket"] = json!(definition.u8(0xB8)?);
        result["native_registration_verified"] = json!(true);
    }
    staged.finish()?;
    let staged_catalog = InvestmentCatalog::load_with_cache_path(
        &view,
        &output.join("staged-catalog-cache.json"),
        true,
        |_| {},
    )?;
    for (recipe, result) in recipes.iter().zip(&mut evidence) {
        let paths = staged_catalog.item_collection_paths(recipe.identity.item_hash.parse_u32()?);
        assert!(
            !paths.is_empty(),
            "Imported item is missing from Collections"
        );
        if recipe.kind == ItemKind::Armor {
            assert_eq!(
                paths
                    .iter()
                    .any(|path| path.iter().any(|node| node == "Exotics")),
                result["graph"]["source_rarity"] == 5,
                "Armor Collections placement disagrees with its source rarity"
            );
        }
        result["collection_paths"] = json!(paths);
    }
    fs::write(
        output.join("coverage.json"),
        serde_json::to_vec_pretty(&json!({
            "items": evidence, "artifact_directory": output,
            "native_registration_verified": true, "gameplay_verified": false
        }))?,
    )?;
    Ok(())
}

fn verify_equipment_clips(
    staged: &mut Reader,
    rig: &Value,
    recipe: &WeaponRecipe,
    graph: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let animation = &graph["equipment_animation"];
    if animation["status"] != "linked" {
        return Ok(());
    }
    let component = rig["components"]
        .as_array()
        .ok_or("Runtime components")?
        .iter()
        .find(|c| c["entity"] == rig["runtime_entity"] && c["class"] == "8080344B")
        .ok_or("Equipment lost its animation lookup")?;
    let owner_tag = u32::from_str_radix(component["owner"].as_str().ok_or("Lookup owner")?, 16)?;
    assert_ne!(
        u64::from(owner_tag),
        animation["lookup_owner"]
            .as_u64()
            .ok_or("Original lookup")?
    );
    let owner = staged.tag(owner_tag, Some(0x80809C36))?;
    let bank_tag = owner.u32(owner.pointer(24)? + 0x90)?;
    assert_ne!(
        u64::from(bank_tag),
        animation["bank"].as_u64().ok_or("Original bank")?
    );
    let bank = staged.tag(bank_tag, Some(0x808036F6))?;
    let mut actual = BTreeMap::new();
    for at in bank.array(8, 4, Some(0x80808F48))? {
        let clip = staged.tag(bank.u32(at)?, Some(0x80808F49))?;
        actual.entry(clip.u32(0x120)?).or_insert(clip);
    }
    let directory = &recipe
        .overrides
        .imported_graph
        .as_ref()
        .ok_or("Portable assets")?
        .directory;
    for clip in animation["clips"].as_array().ok_or("Converted clips")? {
        let name = u32::try_from(clip["name"].as_u64().ok_or("Clip name")?)?;
        let expected = fs::read(directory.join(clip["file"].as_str().ok_or("Clip payload")?))?;
        assert_eq!(
            actual.get(&name).ok_or("Converted clip is unreachable")?.0,
            expected
        );
    }
    Ok(())
}

fn registered_keys(data: &Payload, row: usize) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let mut keys = vec![data.u32(row + 8)?, data.u32(row + 12)?];
    for at in data.array(row + 16, 8, None)? {
        for entry in data.array(data.pointer(at)? + 8, 4, None)? {
            keys.push(data.u32(entry)?);
        }
    }
    keys.retain(|key| ![0, u32::MAX, 0x811C9DC5].contains(key));
    Ok(keys)
}

fn verify_art_layout(
    data: &Payload,
    row: usize,
    source: &Value,
    assignments: &mut BTreeMap<u32, u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut compare = |expected: &Value, actual: u32| -> Result<(), Box<dyn std::error::Error>> {
        let expected = u32::try_from(expected.as_u64().ok_or("Source art key")?)?;
        if [0, u32::MAX, 0x811C9DC5].contains(&expected) {
            assert_eq!(
                actual, expected,
                "An empty source placement acquired donor artwork"
            );
        } else {
            assert!(![0, u32::MAX, 0x811C9DC5].contains(&actual));
            if let Some(previous) = assignments.insert(expected, actual) {
                assert_eq!(
                    previous, actual,
                    "Shared source artwork acquired inconsistent identities"
                );
            }
        }
        Ok(())
    };
    for index in 0..2 {
        compare(&source["singles"][index], data.u32(row + 8 + index * 4)?)?;
    }
    let slots = data.array(row + 16, 8, None)?;
    let expected_slots = source["slots"].as_array().ok_or("Source selectors")?;
    assert_eq!(slots.len(), expected_slots.len(), "Selectors were dropped");
    for (at, expected) in slots.into_iter().zip(expected_slots) {
        let slot = data.pointer(at)?;
        assert_eq!(
            data.u64(slot)?,
            expected["selector"].as_u64().ok_or("Source selector")?
        );
        let entries = data.array(slot + 8, 4, None)?;
        let expected_entries = expected["assignments"]
            .as_array()
            .ok_or("Source alternatives")?;
        assert_eq!(
            entries.len(),
            expected_entries.len(),
            "Body alternatives were dropped"
        );
        for (at, expected) in entries.into_iter().zip(expected_entries) {
            compare(expected, data.u32(at)?)?;
        }
    }
    Ok(())
}

fn verify_nameplate(
    reader: &mut Reader,
    globals: &Payload,
    hash: u32,
    expected: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let table = reader.tag(globals.u32(16 + 33 * 16)?, None)?;
    let row = table
        .array(8, 24, None)?
        .into_iter()
        .find(|&at| table.u32(at).ok() == Some(hash))
        .ok_or("Authored strings")?;
    let strings = reader.tag(table.u32(row + 16)?, None)?;
    let icons = reader.tag(globals.u32(16 + 75 * 16)?, None)?;
    let rows = icons.array(8, 24, None)?;
    let row = *rows
        .get(usize::from(strings.u16(0x82)?))
        .ok_or("Nameplate index")?;
    let container = reader.tag(icons.u32(row + 16)?, None)?;
    for color in 0..2 {
        for lane in 0..4 {
            assert_eq!(
                container.f32(0x30 + color * 16 + lane * 4)?,
                expected["colors"][color][lane]
                    .as_f64()
                    .ok_or("Nameplate color")? as f32
            );
        }
    }
    for (part, offset) in [("banner", 0x14), ("overlay", 0x20), ("background", 0x24)] {
        let encoded = expected[part]["image"]["png_base64"]
            .as_str()
            .ok_or("Embedded artwork")?;
        let image = image::load_from_memory(&STANDARD.decode(encoded)?)?.to_rgba8();
        let layer = reader.tag(container.u32(offset)?, None)?;
        let lanes = layer.array(layer.pointer(16)?, 16, None)?;
        assert_eq!(lanes.len(), 1);
        let frames = layer.array(lanes[0], 4, None)?;
        assert_eq!(frames.len(), 1);
        let texture_tag = layer.u32(frames[0])?;
        let header = reader.tag(texture_tag, None)?;
        let pixels = reader.tag(reader.reference(texture_tag)?, None)?;
        assert!(matches!(header.u32(4)?, 28 | 29));
        assert_eq!(
            (u32::from(header.u16(14)?), u32::from(header.u16(16)?)),
            image.dimensions()
        );
        assert_eq!(
            &pixels.0,
            image.as_raw(),
            "Nameplate pixels changed during sharing or staging"
        );
    }
    Ok(())
}
