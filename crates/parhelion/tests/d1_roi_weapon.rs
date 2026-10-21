//! Configured weapon import, portable reload, native staging and independent readback.
#![cfg(feature = "d2-model-importer")]
use parhelion::{
    BatchBuildRequest, BatchBuildSnapshot, WeaponRecipe, build_and_stage_snapshot_with_progress,
};
use parhelion_import::{
    d1_roi::{NativeOptions, Source},
    tiger::reader::{Reader, package_stamp},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
mod d1_roi_weapon {
    use super::*;
    pub mod readback;
}

fn configured(name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(
        env::var_os(name).ok_or_else(|| format!("Configure {name}"))?,
    ))
}

fn item(reader: &mut Reader, hash: u32) -> Result<(u32, Vec<u8>)> {
    let globals = reader
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|n| n.name == "investment_globals")
        .ok_or("Investment globals")?
        .hash
        .0;
    let global = reader.tag(globals, None)?;
    let root = reader.tag(global.u32(16)?, None)?;
    let table = reader.tag(root.u32(8 + 48 * 16)?, None)?;
    let row = table
        .array(8, 24, Some(0x80807BE8))?
        .into_iter()
        .find(|&at| table.u32(at).ok() == Some(hash))
        .ok_or("Item definition")?;
    let tag = table.u32(row + 16)?;
    Ok((tag, reader.tag(tag, Some(0x80807BEA))?.0.clone()))
}

fn view(native: &Path, output: &Path, run: &Path, files: &[String]) -> Result<PathBuf> {
    let root = output.join("view");
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    fs::create_dir_all(root.join("bin/x64"))?;
    let game = native.parent().ok_or("Game root")?;
    fs::hard_link(game.join("destiny2.exe"), root.join("destiny2.exe"))?;
    fs::copy(
        game.join("bin/x64/oo2core_3_win64.dll"),
        root.join("bin/x64/oo2core_3_win64.dll"),
    )?;
    for file in fs::read_dir(native)? {
        let file = file?;
        if file.path().extension().is_some_and(|s| s == "pkg") {
            fs::hard_link(file.path(), packages.join(file.file_name()))?;
        }
    }
    for name in files {
        let target = packages.join(name);
        if target.exists() {
            fs::remove_file(&target)?;
        }
        fs::copy(run.join(name), target)?;
    }
    Ok(packages)
}

#[test]
#[ignore = "Requires source and native packages, XG, fresh output and PARHELION_D1_ROI_WEAPON_PAIRS as source:donor hexadecimal pairs"]
#[allow(clippy::cognitive_complexity)]
fn mounted_weapons_reload_and_stage_with_private_art_and_source_perk_contracts() -> Result<()> {
    let source_path = configured("PARHELION_D1_ROI_PACKAGES")?;
    let native = configured("PARHELION_NATIVE_PACKAGES")?.canonicalize()?;
    let output = configured("PARHELION_D1_ROI_OUTPUT")?;
    let output = parhelion_import::tiger::reader::outside(&output, &native)?;
    let output = parhelion_import::tiger::reader::outside(&output, &source_path)?;
    assert!(!output.exists(), "Use a fresh artifact directory");
    let pairs = env::var("PARHELION_D1_ROI_WEAPON_PAIRS")?
        .split(',')
        .map(|pair| {
            let (source, native) = pair.split_once(':').ok_or("Expected source:donor")?;
            Ok((
                u32::from_str_radix(source.trim(), 16)?,
                u32::from_str_radix(native.trim(), 16)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    assert!(!pairs.is_empty(), "Required weapon corpus is empty");
    fs::create_dir_all(&output)?;
    let source = Source::open(&source_path, |_, _| {})?;
    let options = NativeOptions {
        xg_library: Some(configured("PARHELION_D1_XG")?),
    };
    let stamp = package_stamp(&native)?;
    let mut stock = Reader::new(&native, &output.join("stock"), false)?;
    let mut unchanged = BTreeMap::new();
    let mut recipes = Vec::new();
    let mut receipts = Vec::new();
    for (hash, donor) in pairs {
        let folder = output.join(format!("item-{hash:08X}"));
        let path = source.prepare_weapon(hash, &native, donor, &folder, &options, &mut |s| {
            println!("{s}")
        })?;
        let recipe = WeaponRecipe::load_json(&path)?;
        let portable = recipe.to_json_pretty()?;
        fs::write(folder.join("portable.parhelion.json"), &portable)?;
        let reloaded = WeaponRecipe::from_json_str(&portable)?;
        assert_eq!(recipe.identity, reloaded.identity);
        let graph = reloaded
            .overrides
            .imported_graph
            .as_ref()
            .ok_or("Imported weapon graph")?;
        let graph: Value =
            serde_json::from_slice(&fs::read(graph.directory.join("asset-graph.json"))?)?;
        let source_report: Value =
            serde_json::from_slice(&fs::read(folder.join("source/weapon.json"))?)?;
        let source_keys = source_report["art"]["parts"]
            .as_array()
            .ok_or("Source parts")?
            .iter()
            .map(|p| p["assignment"].as_u64().ok_or("Source assignment"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        let converted_keys = graph["gear_art"]["parts"]
            .as_array()
            .ok_or("Converted parts")?
            .iter()
            .map(|p| {
                p["source_assignment"]
                    .as_u64()
                    .ok_or("Converted assignment")
            })
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        assert_eq!(
            source_keys, converted_keys,
            "Selectable source parts were lost"
        );
        let (tag, bytes) = item(&mut stock, donor)?;
        unchanged.insert(tag, bytes);
        for event in graph["audio"]["matched_events"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let mut tags = Vec::new();
            for field in ["owner", "parent", "group"] {
                tags.push(
                    event["native_group"][field]
                        .as_str()
                        .ok_or("Native audio group")?,
                );
            }
            for sound in event["native_sounds"].as_array().ok_or("Native sounds")? {
                for field in ["tag", "bank"] {
                    tags.push(sound[field].as_str().ok_or("Native audio asset")?);
                }
                for medium in sound["media"].as_array().ok_or("Native media")? {
                    tags.push(medium.as_str().ok_or("Native medium")?);
                }
            }
            for tag in tags {
                let tag = u32::from_str_radix(tag, 16)?;
                unchanged.insert(tag, stock.tag(tag, None)?.0.clone());
            }
        }
        for plug in &reloaded.overrides.socket_plug_variants {
            let (tag, bytes) = item(&mut stock, plug.source_plug_hash.parse_u32()?)?;
            unchanged.insert(tag, bytes);
        }
        receipts.push(json!({"source_item":hash,"item":recipe.identity.item_hash.parse_u32()?,
            "donor":donor,"graph":graph,"source_report":source_report,"source_directory":folder.join("source")}));
        recipes.push(reloaded);
    }
    let snapshot = BatchBuildSnapshot::new(BatchBuildRequest {
        package_directory: native.clone(),
        staging_root: output.join("build"),
        ignore_installed_authored_overlays: true,
        maps: Vec::new(),
        recipes: recipes.clone(),
    })?;
    let built = build_and_stage_snapshot_with_progress(&snapshot, |s| println!("{:?}", s.phase))?;
    let packages = view(
        &native,
        &output,
        &built.run_directory,
        &built
            .artifacts
            .iter()
            .map(|a| a.file_name.clone())
            .collect::<Vec<_>>(),
    )?;
    let mut readback = Reader::new(&packages, &output.join("readback"), false)?;
    for (tag, bytes) in unchanged {
        assert_eq!(
            readback.tag(tag, None)?.0,
            bytes,
            "Stock donor was modified"
        );
    }
    for (recipe, receipt) in recipes.iter().zip(&mut receipts) {
        let (item_tag, bytes) = item(&mut readback, recipe.identity.item_hash.parse_u32()?)?;
        receipt["named_audio_readback"] = d1_roi_weapon::readback::audio(
            &mut readback,
            &source,
            item_tag,
            &receipt["graph"],
            recipe
                .overrides
                .imported_graph
                .as_ref()
                .ok_or("Imported graph")?
                .directory
                .as_path(),
        )?;
        let definition = parhelion_import::tiger::payload::Payload(bytes);
        assert!(
            definition.u8(0xB8)? <= 2,
            "Imported item is outside the three weapon slots"
        );
        assert_eq!(definition.u8(0xBB)?, 1, "Imported weapon is not instanced");
        let global_tag = readback
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|n| n.name == "investment_globals")
            .ok_or("Globals")?
            .hash
            .0;
        let globals = readback.tag(global_tag, None)?;
        receipt["native_collections"] = d1_roi_weapon::readback::collections(
            &mut readback,
            &globals,
            recipe.identity.item_hash.parse_u32()?,
        )?;
        let art = readback.tag(globals.u32(16 + 66 * 16)?, None)?;
        let art_rows = art.array(8, 32, Some(0x80805DFB))?;
        let map = readback.tag(0x80EC3F61, None)?;
        let assignments = map
            .array(8, 8, None)?
            .into_iter()
            .map(|o| Ok((map.u32(o)?, map.u32(o + 4)?)))
            .collect::<std::result::Result<BTreeMap<_, _>, anyhow::Error>>()?;
        let mut selected = BTreeMap::new();
        let mut pair = |native: u32, source: &Value| -> Result<()> {
            let source = u32::try_from(source.as_u64().ok_or("Source assignment")?)?;
            if matches!(source, 0 | u32::MAX | 0x811C9DC5) {
                assert_eq!(native, source, "Empty source art choice changed");
            } else {
                assert!(!matches!(native, 0 | u32::MAX | 0x811C9DC5));
                if let Some(previous) = selected.insert(native, source) {
                    assert_eq!(
                        previous, source,
                        "Distinct source parts share an assignment"
                    );
                }
            }
            Ok(())
        };
        let selectors = definition.array(definition.pointer(0x88)?, 4, Some(0x808077B5))?;
        let source_rows = receipt["source_report"]["art"]["rows"]
            .as_array()
            .ok_or("Source art rows")?;
        assert_eq!(selectors.len(), source_rows.len());
        for (selector, source_row) in selectors.into_iter().zip(source_rows) {
            assert_eq!(
                i64::from(definition.u8(selector)? as i8),
                source_row["class"].as_i64().ok_or("Source class")?
            );
            assert_eq!(
                u64::from(definition.u8(selector + 1)?),
                source_row["flags"].as_u64().ok_or("Source art flags")?
            );
            let row = art_rows[usize::from(definition.u16(selector + 2)?)];
            for (index, at) in [row + 8, row + 12].into_iter().enumerate() {
                pair(art.u32(at)?, &source_row["singles"][index])?;
            }
            let slots = art.array(row + 16, 8, None)?;
            let source_slots = source_row["slots"].as_array().ok_or("Source art slots")?;
            assert_eq!(slots.len(), source_slots.len());
            let mut seen = BTreeSet::new();
            for at in slots {
                let slot = art.pointer(at)?;
                let selector = art.u64(slot)?;
                assert!(seen.insert(selector), "Repeated art placement selector");
                let source_slot = source_slots
                    .iter()
                    .find(|s| s["selector"].as_u64() == Some(selector))
                    .ok_or("Source placement selector")?;
                let choices = art.array(slot + 8, 4, None)?;
                let source_choices = source_slot["assignments"]
                    .as_array()
                    .ok_or("Source art choices")?;
                assert_eq!(choices.len(), source_choices.len());
                for (at, source) in choices.into_iter().zip(source_choices) {
                    pair(art.u32(at)?, source)?;
                }
            }
        }
        assert!(!selected.is_empty());
        let source_keys = receipt["source_report"]["art"]["parts"]
            .as_array()
            .ok_or("Source art parts")?
            .iter()
            .map(|p| p["assignment"].as_u64().ok_or("Source assignment"))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        assert_eq!(
            selected
                .values()
                .map(|v| u64::from(*v))
                .collect::<BTreeSet<_>>(),
            source_keys
        );
        let mut parents = Vec::new();
        for (key, source_key) in selected {
            let source_part = receipt["source_report"]["art"]["parts"]
                .as_array()
                .ok_or("Source parts")?
                .iter()
                .find(|p| p["assignment"].as_u64() == Some(u64::from(source_key)))
                .ok_or("Source part")?;
            let mut expected_markers = Vec::new();
            let source_directory = Path::new(
                receipt["source_directory"]
                    .as_str()
                    .ok_or("Source directory")?,
            );
            for component in source_part["components"]
                .as_array()
                .ok_or("Source components")?
            {
                if component["resource_class"].as_u64() != Some(0x808008FB) {
                    continue;
                }
                let owner = component["tag"]
                    .as_u64()
                    .or_else(|| component["owner"].as_u64())
                    .ok_or("Source marker owner")?;
                let data = parhelion_import::tiger::payload::Payload(fs::read(
                    source_directory.join(format!("{owner:08X}.bin")),
                )?);
                let layout = parhelion_import::tiger::markers::Layout {
                    row_class: 0x80800428,
                    ..parhelion_import::tiger::markers::NATIVE
                };
                expected_markers.extend(parhelion_import::tiger::markers::read(
                    &data,
                    data.pointer(24)? + 0xD8,
                    layout,
                )?);
            }
            let parent = *assignments.get(&key).ok_or("Private art assignment")?;
            let parent_data = readback.tag(parent, Some(0x8080744A))?;
            let entity = readback.tag(parent_data.u32(16)?, Some(0x80809C0F))?;
            let source_entity_tag = source_part["entity"].as_u64().ok_or("Source entity")?;
            let source_entity = parhelion_import::tiger::payload::Payload(fs::read(
                source_directory.join(format!("{source_entity_tag:08X}.bin")),
            )?);
            let optics = d1_roi_weapon::readback::optics(
                &mut readback,
                &entity,
                &source_entity,
                source_directory,
            )?;
            let mut models = Vec::new();
            let mut actual_markers = Vec::new();
            for at in entity.array(16, 12, Some(0x80809C04))? {
                let owner = readback.tag(entity.u32(at)?, Some(0x80809C36))?;
                let resource = owner.pointer(24)?;
                if owner.u32(resource - 4)? == 0x80808507 {
                    actual_markers.extend(parhelion_import::tiger::markers::read_native(&owner)?);
                }
                if owner.u32(resource - 4)? != 0x808072BD {
                    continue;
                }
                assert_eq!(
                    owner.u32(resource + 0x248)?,
                    u32::MAX,
                    "Untranslated donor atlas"
                );
                let model_tag = owner.u32(resource + 0x1DC)?;
                let model = readback.tag(model_tag, Some(0x808073A5))?;
                let meshes = model.array(16, 136, Some(0x80807378))?;
                assert!(!meshes.is_empty());
                for mesh in meshes {
                    assert!(!model.array(mesh + 24, 32, Some(0x8080737E))?.is_empty());
                    let header = model.u32(mesh)?;
                    assert!(
                        !readback
                            .tag(readback.reference(header)?, None)?
                            .0
                            .is_empty()
                    );
                }
                models.push(model_tag);
            }
            assert!(
                !models.is_empty(),
                "Selectable art contains no imported model"
            );
            let marker_values = |markers: Vec<parhelion_import::tiger::markers::Marker>| {
                markers
                    .into_iter()
                    .map(|m| (m.name, m.binding, m.orientation, m.position))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                marker_values(actual_markers),
                marker_values(expected_markers),
                "Named grip, aim or muzzle transform changed"
            );
            parents.push(json!({"assignment":key,"parent":parent,"models":models,"optics":optics}));
        }
        receipt["native_art"] = json!(parents);
        // Follow each staged choice through its plug, finished-perk
        // row and runtime assignment. Source bytes, not the authored program,
        // are the independent oracle for the supported scalar effects.
        let root = readback.tag(globals.u32(16)?, None)?;
        let items = readback.tag(root.u32(8 + 48 * 16)?, None)?;
        let item_rows = items.array(8, 24, Some(0x80807BE8))?;
        let finished = readback.tag(globals.u32(16 + 71 * 16)?, Some(0x80805C97))?;
        let finished_rows = finished.array(8, 24, Some(0x80805C9D))?;
        let runtime = readback.tag(0x80EC3F60, None)?;
        let runtime_rows = runtime.array(8, 8, None)?;
        let sockets = definition.array(definition.pointer(0x68)?, 80, Some(0x808077C4))?;
        let source_directory = Path::new(
            receipt["source_directory"]
                .as_str()
                .ok_or("Source directory")?,
        );
        let translated = receipt["graph"]["perk_translation"]["translated"]
            .as_array()
            .ok_or("Perk translation report")?;
        assert!(
            !translated.is_empty(),
            "Configured weapon has no supported source behavior"
        );
        let mut behaviors = Vec::new();
        for translated in translated {
            if let Some(kind) = translated["damage_type"].as_str() {
                let (source_mode, mode) = match kind {
                    "arc" => (2, 1),
                    "solar" => (1, 2),
                    "void" => (3, 3),
                    _ => return Err("Unsupported damage contract".into()),
                };
                for tag in translated["source_actions"]
                    .as_array()
                    .ok_or("Source damage actions")?
                {
                    let tag = tag.as_u64().ok_or("Source damage action tag")?;
                    let source = parhelion_import::tiger::payload::Payload(fs::read(
                        source_directory.join(format!("{tag:08X}.bin")),
                    )?);
                    let effects = source.array(0x38, 8, None)?;
                    assert_eq!(effects.len(), 1);
                    assert_eq!(
                        source.bytes::<3>(source.pointer(effects[0])?)?,
                        [5, 1, source_mode]
                    );
                }
                let perks =
                    definition.array(definition.pointer(0x70)? + 16, 24, Some(0x808077BC))?;
                let markers = perks
                    .iter()
                    .filter_map(|&at| match definition.u16(at).ok()? {
                        83 | 449 => Some(1),
                        84 | 450 => Some(2),
                        85 | 451 => Some(3),
                        _ => None,
                    })
                    .chain(sockets.iter().filter_map(|&at| {
                        (definition.u16(at).ok()? == 68)
                            .then(|| match definition.u16(at + 2).ok()? {
                                1505 => Some(1),
                                1506 => Some(2),
                                1507 => Some(3),
                                _ => None,
                            })
                            .flatten()
                    }))
                    .collect::<Vec<_>>();
                assert_eq!(
                    markers,
                    vec![mode],
                    "Native weapon damage differs from source talent"
                );
                behaviors.push(json!({"damage_type":kind,"native_damage_mode":mode}));
                continue;
            }
            let slot = usize::try_from(translated["socket"].as_u64().ok_or("Translated socket")?)?;
            let variant = recipe
                .overrides
                .socket_plug_variants
                .iter()
                .find(|p| {
                    usize::from(p.socket_index) == slot
                        && p.name.as_deref() == translated["name"].as_str()
                })
                .ok_or("Private variant")?;
            let mut choices = vec![definition.u16(sockets[slot] + 2)?];
            for member in definition.array(sockets[slot] + 0x40, 32, Some(0x80802E03))? {
                let index = definition.u16(member)?;
                if index != u16::MAX && !choices.contains(&index) {
                    choices.push(index);
                }
            }
            let plug_row = item_rows[usize::from(
                *choices
                    .get(usize::from(variant.choice_index))
                    .ok_or("Private choice is absent from the emitted socket")?,
            )];
            let plug_hash = items.u32(plug_row)?;
            let plug = readback.tag(items.u32(plug_row + 16)?, Some(0x80807BEA))?;
            assert_ne!(plug_hash, variant.source_plug_hash.parse_u32()?);
            let perks = plug.array(plug.pointer(0x70)? + 16, 24, Some(0x808077BC))?;
            let sources = translated["source_actions"]
                .as_array()
                .ok_or("Source actions")?;
            assert_eq!(
                perks.len(),
                sources.len(),
                "Private choice inherited donor effects"
            );
            // The private plug carries exactly the source deltas as equipped stat rows.
            let stats = plug.array(plug.pointer(0x70)?, 0x28, Some(0x80803033))?;
            let adjustments = translated["stat_adjustments"]
                .as_array()
                .ok_or("Stat adjustments")?;
            assert_eq!(
                stats.len(),
                adjustments.len(),
                "Private choice stats differ from the source adjustments"
            );
            let mut actual_stats = stats
                .into_iter()
                .map(|row| {
                    Ok((
                        u64::from(plug.u8(row)?),
                        i64::from(i32::from_le_bytes(plug.bytes::<4>(row + 4)?)),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut expected_stats = adjustments
                .iter()
                .map(|row| {
                    Ok((
                        row["definition_index"].as_u64().ok_or("Stat index")?,
                        row["value"].as_i64().ok_or("Stat value")?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            actual_stats.sort_unstable();
            expected_stats.sort_unstable();
            assert_eq!(
                actual_stats, expected_stats,
                "Private choice changed a source stat delta"
            );
            let equivalents = translated["stock_perk_plugs"]
                .as_array()
                .ok_or("Stock equivalents")?;
            if !equivalents.is_empty() {
                let node = receipt["source_report"]["talents"]["nodes"]
                    .as_array()
                    .ok_or("Source nodes")?
                    .iter()
                    .find(|n| n["index"] == translated["node"])
                    .ok_or("Source node")?;
                let step = node["steps"]
                    .as_array()
                    .ok_or("Source steps")?
                    .iter()
                    .find(|s| s["index"] == translated["step"])
                    .ok_or("Source step")?;
                let source_perks = step["variants"][0]["perks"]
                    .as_array()
                    .ok_or("Source perks")?;
                assert_eq!(equivalents.len(), source_perks.len());
                assert_eq!(perks.len(), source_perks.len());
                for (perk, source) in perks.iter().zip(source_perks) {
                    let native_plug = match source["hash"].as_u64().ok_or("Source perk identity")? {
                        0x4E9855BC => 0x2A1CBFD4,
                        0x5D32FF95 => 0x39169B67,
                        _ => return Err("Unverified stock perk equivalent".into()),
                    };
                    let (_, bytes) = item(&mut stock, native_plug)?;
                    let donor = parhelion_import::tiger::payload::Payload(bytes);
                    let donor_perks =
                        donor.array(donor.pointer(0x70)? + 16, 24, Some(0x808077BC))?;
                    assert_eq!(donor_perks.len(), 1);
                    let expected_key =
                        finished.u32(finished_rows[usize::from(donor.u16(donor_perks[0])?)] + 4)?;
                    let actual_key =
                        finished.u32(finished_rows[usize::from(plug.u16(*perk)?)] + 4)?;
                    assert_ne!(
                        actual_key, expected_key,
                        "Private choice reused the stock identity"
                    );
                    let assigned = runtime_rows
                        .iter()
                        .copied()
                        .filter(|&r| runtime.u32(r).ok() == Some(actual_key))
                        .collect::<Vec<_>>();
                    assert_eq!(assigned.len(), 1);
                    let tag = runtime.u32(assigned[0] + 4)?;
                    let expected = runtime_rows
                        .iter()
                        .copied()
                        .filter(|&r| runtime.u32(r).ok() == Some(expected_key))
                        .collect::<Vec<_>>();
                    assert_eq!(expected.len(), 1);
                    assert_eq!(
                        tag,
                        runtime.u32(expected[0] + 4)?,
                        "Private choice lost its native equivalent"
                    );
                    assert_eq!(
                        readback.tag(tag, None)?.0,
                        stock.tag(tag, None)?.0,
                        "Stock equivalent action changed"
                    );
                    behaviors.push(json!({"plug":plug_hash,"stock_equivalent":native_plug,"native_action":tag}));
                }
                continue;
            }
            for (perk, source_tag) in perks.into_iter().zip(sources) {
                let source_tag = source_tag.as_u64().ok_or("Source action tag")?;
                let source = parhelion_import::tiger::payload::Payload(fs::read(
                    source_directory.join(format!("{source_tag:08X}.bin")),
                )?);
                let key = finished.u32(finished_rows[usize::from(plug.u16(perk)?)] + 4)?;
                let assigned = runtime_rows
                    .iter()
                    .filter(|&&r| runtime.u32(r).ok() == Some(key))
                    .copied()
                    .collect::<Vec<_>>();
                assert_eq!(assigned.len(), 1);
                let action_tag = runtime.u32(assigned[0] + 4)?;
                let action = readback.tag(action_tag, Some(0x808040B5))?;
                let source_effects = source.array(0x38, 8, None)?;
                let effects = action.array(0x38, 8, Some(0x808040AC))?;
                assert_eq!(effects.len(), source_effects.len());
                for (source_at, at) in source_effects.into_iter().zip(&effects) {
                    let s = source.pointer(source_at)?;
                    let n = if source.u8(s)? == 9 {
                        let matching = effects
                            .iter()
                            .map(|&at| action.pointer(at))
                            .collect::<std::result::Result<Vec<_>, _>>()?
                            .into_iter()
                            .filter(|&at| {
                                action.u8(at).ok() == Some(10)
                                    && action.u8(at + 2).ok() == source.u8(s + 2).ok()
                                    && action.u32(at + 8).ok() == source.u32(s + 8).ok()
                            })
                            .collect::<Vec<_>>();
                        assert_eq!(
                            matching.len(),
                            1,
                            "Source property lacks a unique native write"
                        );
                        matching[0]
                    } else {
                        action.pointer(*at)?
                    };
                    match source.u8(s)? {
                        5 => {
                            assert_eq!(action.u8(n)?, 6);
                            assert_eq!(source.u8(s + 2)?, action.u8(n + 2)?);
                        }
                        9 => {
                            assert_eq!(action.u8(n)?, 10);
                            assert_eq!(source.u8(s + 2)?, action.u8(n + 2)?);
                            assert_eq!(source.u32(s + 8)?, action.u32(n + 8)?);
                            let code = action.array(n + 0x18, 1, Some(0x80800009))?;
                            assert_eq!(
                                code.into_iter()
                                    .map(|at| action.u8(at))
                                    .collect::<std::result::Result<Vec<_>, _>>()?,
                                [0x34, 0, 0x3E, 0]
                            );
                            let values = action.array(n + 0x28, 16, Some(0x80800090))?;
                            assert_eq!(values.len(), 1);
                            assert_eq!(source.bytes::<16>(s + 16)?, action.bytes::<16>(values[0])?);
                            assert_eq!(action.u8(n + 0x4A)?, 1);
                            assert_eq!(action.u32(n + 0x4C)?, 0);
                        }
                        14 => {
                            assert_eq!(action.u8(n)?, 15);
                            assert_eq!(source.u32(s + 8)?, action.u32(n + 0x6C)?);
                            assert_eq!(source.u8(s + 2)?, action.u8(n + 0x68)?);
                            assert_eq!(source.u8(s + 4)?, action.u8(n + 0x6A)?);
                            assert!(action.0[n + 0x70..n + 0x88].iter().all(|b| *b == 0));
                        }
                        24 | 25 => {
                            assert_eq!(action.u8(n)?, source.u8(s)? + 4);
                            let length = if source.u8(s)? == 24 { 24 } else { 16 };
                            assert_eq!(source.0[s + 1..s + length], action.0[n + 1..n + length]);
                        }
                        26 => {
                            assert_eq!(action.u8(n)?, 30);
                            assert_eq!(source.bytes::<11>(s + 1)?, action.bytes::<11>(n + 1)?);
                        }
                        _ => {
                            return Err(
                                "An unsupported source effect was presented as translated".into()
                            );
                        }
                    }
                }
                for group in [0x20, 0x48, 0x58] {
                    let source_conditions = source.array(group, 8, None)?;
                    let conditions = action.array(group, 8, Some(0x808040BA))?;
                    assert_eq!(conditions.len(), source_conditions.len());
                    for (s, n) in source_conditions.into_iter().zip(conditions) {
                        let s = source.pointer(s)?;
                        let n = action.pointer(n)?;
                        assert_eq!(source.bytes::<5>(s)?, action.bytes::<5>(n)?);
                        let kind = source.u8(s + 5)?;
                        assert_eq!(
                            action.u8(n + 5)?,
                            if matches!(kind, 17 | 18) {
                                kind - 1
                            } else {
                                kind
                            }
                        );
                        assert_eq!(source.u8(s + 6)?, action.u8(n + 6)?);
                        if source.u8(s + 5)? == 1 {
                            assert_eq!(source.u32(s + 8)?, action.u32(n + 8)?);
                        }
                    }
                }
                behaviors.push(
                    json!({"plug":plug_hash,"source_action":source_tag,"native_action":action_tag}),
                );
            }
        }
        receipt["native_behaviors"] = json!(behaviors);
    }
    readback.finish()?;
    stock.finish()?;
    assert_eq!(
        stamp,
        package_stamp(&native)?,
        "Offline preparation mutated mounted packages"
    );
    fs::write(
        output.join("weapon-readback.json"),
        serde_json::to_vec_pretty(&json!({
            "source_builds":source.report().builds,"source_stamp":source.report().stamp,
            "native_stamp":stamp,"weapons":receipts,"gameplay_verified":false
        }))?,
    )?;
    Ok(())
}
