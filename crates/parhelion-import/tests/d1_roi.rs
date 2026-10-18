//! Source intake E2E. Binary fixtures are authored independently of the importer.
use anyhow::{Context, Result, ensure};
use parhelion_import::d1_roi::{Source, packages_path};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};
use tiger_pkg::{DestinyVersion, TagHash, Version};

fn package(path: &Path, patch: u16, language: u16, block_patch: u16, payload: &[u8]) {
    let mut data = vec![0u8; 0x200 + payload.len()];
    data[..2].copy_from_slice(&24u16.to_le_bytes());
    data[2..4].copy_from_slice(&8u16.to_le_bytes());
    data[4..6].copy_from_slice(&0x812u16.to_le_bytes());
    data[0x20..0x22].copy_from_slice(&patch.to_le_bytes());
    data[0x22..0x24].copy_from_slice(&language.to_le_bytes());
    data[0x24..0x2b].copy_from_slice(b"fixture");
    data[0xb4..0xb8].copy_from_slice(&1u32.to_le_bytes());
    data[0xb8..0xbc].copy_from_slice(&0x140u32.to_le_bytes());
    data[0xd0..0xd4].copy_from_slice(&1u32.to_le_bytes());
    data[0xd4..0xd8].copy_from_slice(&0x160u32.to_le_bytes());
    let size = data.len() as u32;
    data[0x13c..0x140].copy_from_slice(&size.to_le_bytes());
    data[0x140..0x144].copy_from_slice(&0x80800734u32.to_le_bytes());
    data[0x144..0x148].copy_from_slice(&8u32.to_le_bytes());
    data[0x148..0x150].copy_from_slice(&((payload.len() as u64) << 28).to_le_bytes());
    data[0x160..0x164].copy_from_slice(&0x200u32.to_le_bytes());
    data[0x164..0x168].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    data[0x168..0x16a].copy_from_slice(&block_patch.to_le_bytes());
    data[0x16c..0x180].copy_from_slice(&sha1::Sha1::digest(payload));
    data[0x200..].copy_from_slice(payload);
    fs::write(path, data).unwrap();
}

#[test]
fn roi_intake_follows_english_patch_storage_and_refuses_incomplete_or_changed_sources() -> Result<()>
{
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("source");
    let packages = root.join("packages");
    fs::create_dir_all(&packages)?;
    let base = packages.join("xboxone_fixture_0812_en_0.pkg");
    let latest = packages.join("xboxone_fixture_0812_en_2.pkg");
    package(&base, 0, 1, 0, b"original source");
    package(&latest, 2, 1, 0, b"original source");
    let mut decoy = fs::read(&latest)?;
    decoy[0x200..].copy_from_slice(b"not the source!");
    fs::write(&latest, decoy)?;
    package(
        &packages.join("xboxone_fixture_0812_fr_3.pkg"),
        3,
        2,
        3,
        b"French version",
    );
    let original = fs::read(&base)?;
    ensure!(packages_path(&root)? == packages.canonicalize()?);
    let source = Source::open(&root, |_, _| {})?;
    // The newest English index refers into patch 0. A later French package must not win.
    ensure!(source.read(0x81824000)?.0 == b"original source");
    ensure!(source.report().package_count == 1);
    ensure!(source.report().excluded.len() == 1);
    ensure!(source.report().require_armor_tables().is_err());
    let output = temp.path().join("inspection");
    let artifact = source.write_inspection(&output)?;
    let report: serde_json::Value = serde_json::from_slice(&fs::read(&artifact)?)?;
    ensure!(report["source"] == "d1_roi");
    ensure!(report["native_conversion"] == false);
    ensure!(
        source.write_inspection(&output).is_err(),
        "Must keep prior evidence"
    );
    ensure!(source.write_inspection(&root.join("output")).is_err());
    ensure!(!root.join("output").exists());
    ensure!(fs::read(&base)? == original);

    fs::remove_file(&base)?;
    ensure!(
        Source::open(&root, |_, _| {}).is_err(),
        "Missing block patch must fail"
    );
    ensure!(source.write_inspection(&temp.path().join("stale")).is_err());
    ensure!(!temp.path().join("stale").exists());
    fs::write(&base, &original)?;
    fs::write(packages.join("broken.pkg"), [24, 0, 8])?;
    ensure!(
        Source::open(&root, |_, _| {}).is_err(),
        "Unreadable package must fail"
    );
    let empty = temp.path().join("empty");
    fs::create_dir(&empty)?;
    ensure!(packages_path(&empty).is_err());
    Ok(())
}

#[test]
fn unpatchable_generations_keep_index_and_storage_together() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("source");
    fs::create_dir(&root)?;
    for (generation, build, payload) in [
        (3u16, 52553u32, b"old generation"),
        (4, 55707, b"new generation"),
    ] {
        let path = root.join(format!("xboxone_fixture_unp{generation}_0.pkg"));
        package(&path, 0, 0, 0, payload);
        let mut data = fs::read(&path)?;
        data[0x10..0x18].copy_from_slice(&(u64::from(build) * 100).to_le_bytes());
        data[0x18..0x1C].copy_from_slice(&build.to_le_bytes());
        fs::write(path, data)?;
    }
    let source = Source::open(&root, |_, _| {})?;
    ensure!(source.read(0x81824000)?.0 == b"new generation");
    let path = source.write_inspection(&temp.path().join("evidence"))?;
    let report: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        report["selected"][0]["path"]
            .as_str()
            .context("Selected package")?
            .ends_with("unp4_0.pkg")
    );
    package(
        &root.join("xboxone_unrelated_0.pkg"),
        0,
        0,
        0,
        b"wrong identity",
    );
    ensure!(Source::open(&root, |_, _| {}).is_err());
    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_D1_ROI_PACKAGES, PARHELION_D1_ROI_ITEMS and fresh PARHELION_D1_ROI_OUTPUT"]
#[allow(clippy::cognitive_complexity)]
fn mounted_roi_armor_exports_both_bodies_and_original_referenced_streams() -> Result<()> {
    let packages =
        env::var_os("PARHELION_D1_ROI_PACKAGES").context("Set PARHELION_D1_ROI_PACKAGES")?;
    let output = env::var_os("PARHELION_D1_ROI_OUTPUT").context("Set PARHELION_D1_ROI_OUTPUT")?;
    let requested = env::var("PARHELION_D1_ROI_ITEMS")
        .context("Set PARHELION_D1_ROI_ITEMS to comma-separated hexadecimal item hashes")?;
    let items = requested
        .split(',')
        .map(|hash| u32::from_str_radix(hash.trim().trim_start_matches("0x"), 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(!items.is_empty(), "Required armor corpus is empty");
    let output = Path::new(&output);
    ensure!(!output.exists(), "Use a fresh output directory");
    let source = Source::open(Path::new(&packages), |_, _| {})?;
    source.report().require_armor_tables()?;
    let catalog = source.armor()?;
    source.write_inspection(output)?;
    let mut exports = Vec::new();
    let mut slots = std::collections::BTreeSet::new();
    for hash in items {
        let item = catalog
            .iter()
            .find(|item| item.hash == hash)
            .context("Requested armor is missing")?;
        slots.insert(item.slot);
        let directory = output.join(format!("armor-{hash:08X}"));
        let path = source.export_armor(hash, &directory)?;
        let report: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        let bodies = report["bodies"].as_array().context("Body variants")?;
        ensure!(bodies.len() == 2, "Both body variants are required");
        for ornament in report["ornaments"]
            .as_array()
            .context("Source ornament links")?
        {
            let tag = ornament["grid"].as_u64().context("Talent grid")? as u32;
            let grid = fs::read(directory.join(format!("{tag:08X}.bin")))?;
            let appearance = ornament["appearance_offset"]
                .as_u64()
                .context("Appearance row")? as usize;
            let ordinal = u16::from_le_bytes(grid[appearance + 2..appearance + 4].try_into()?);
            let table = ornament["definition_table"]
                .as_u64()
                .context("Definition table")? as u32;
            let table = fs::read(directory.join(format!("{table:08X}.bin")))?;
            let rows = (16i64 + i64::from_le_bytes(table[16..24].try_into()?) + 16) as usize;
            let row = rows + usize::from(ordinal) * 24;
            let hash = u32::from_le_bytes(table[row..row + 4].try_into()?);
            ensure!(ornament["item"]["hash"] == hash && ornament["target_armor"] == item.hash);
            ensure!(
                ornament["bodies"]
                    .as_array()
                    .context("Ornament bodies")?
                    .len()
                    == 2
            );
        }
        ensure!(
            !report["dyes"].as_array().context("Armor dyes")?.is_empty(),
            "Required armor dye source is empty"
        );
        let mut readback = Vec::new();
        for asset in report["assets"].as_array().context("Exported assets")? {
            let file = directory.join(asset["file"].as_str().context("Asset file")?);
            let bytes = fs::read(&file)?;
            let package = DestinyVersion::DestinyRiseOfIron
                .open(asset["package"].as_str().context("Source package")?)?;
            let tag = asset["tag"].as_u64().context("Source tag")? as u32;
            ensure!(
                package.read_tag(TagHash(tag))? == bytes,
                "Export changed original tag {tag:08X}"
            );
            let entry = package
                .entry(TagHash(tag).entry_index() as usize)
                .context("Source entry")?;
            if entry.file_type == 32 && entry.file_subtype == 16 {
                let descriptor = fs::read(directory.join(format!("{:08X}.bin", entry.reference)))?;
                ensure!(
                    bytes == [0; 8] && descriptor.len() == 60,
                    "Unsupported Xbox sampler envelope"
                );
                ensure!(
                    descriptor[52..] == [255; 8],
                    "Unsupported Xbox sampler extension"
                );
            }
            if entry.file_type == 65 {
                let high_mips = directory.join(format!("{:08X}.bin", entry.reference));
                ensure!(
                    high_mips.is_file(),
                    "Streamed texture {tag:08X} lost its high mip payload"
                );
            }
            if entry.reference == 0x80801C32 {
                for stage in [0x28usize, 0xC8, 0x168, 0x208, 0x2A8] {
                    let count =
                        u64::from_le_bytes(bytes[stage + 16..stage + 24].try_into()?) as usize;
                    if count == 0 {
                        continue;
                    }
                    let pointer = stage + 24;
                    let start = (pointer as i64
                        + i64::from_le_bytes(bytes[pointer..pointer + 8].try_into()?)
                        + 16) as usize;
                    for row in 0..count {
                        let at = start + row * 8 + 4;
                        let texture = u32::from_le_bytes(bytes[at..at + 4].try_into()?);
                        if !matches!(texture, 0 | u32::MAX) {
                            ensure!(
                                directory.join(format!("{texture:08X}.bin")).is_file(),
                                "Material {tag:08X} lost texture {texture:08X}"
                            );
                        }
                    }
                }
            }
            readback.push(
                json!({"tag":tag,"sha256":hex::encode(Sha256::digest(&bytes)),"bytes":bytes.len()}),
            );
        }
        let ornament_bodies = report["ornaments"]
            .as_array()
            .context("Ornament links")?
            .iter()
            .flat_map(|ornament| ornament["bodies"].as_array().into_iter().flatten());
        for body in bodies.iter().chain(ornament_bodies) {
            let entity = body["entity"].as_u64().context("Body entity")? as u32;
            let entity = fs::read(directory.join(format!("{entity:08X}.bin")))?;
            let count = u64::from_le_bytes(entity[0x20..0x28].try_into()?);
            let start =
                (0x28i64 + i64::from_le_bytes(entity[0x28..0x30].try_into()?) + 16) as usize;
            let mut models = std::collections::BTreeSet::new();
            for index in 0..count as usize {
                let offset = start + index * 12;
                let tag = u32::from_le_bytes(entity[offset..offset + 4].try_into()?);
                let raw = fs::read(directory.join(format!("{tag:08X}.bin")))?;
                let offset = 24i64 + i64::from_le_bytes(raw[24..32].try_into()?);
                if offset <= 24 {
                    continue;
                }
                let offset = offset as usize;
                let class = u32::from_le_bytes(raw[offset - 4..offset].try_into()?);
                if class == 0x80801BF6 {
                    let solver =
                        u32::from_le_bytes(raw[offset + 0x8C0..offset + 0x8C4].try_into()?);
                    let solver = fs::read(directory.join(format!("{solver:08X}.bin")))?;
                    ensure!(
                        solver.starts_with(&[0x57, 0xE0, 0xE0, 0x57, 0x10, 0xC0, 0xC0, 0x10]),
                        "Cloth solver is not a Havok packfile"
                    );
                    ensure!(
                        solver[0x28..0x36] == *b"hk_2012.2.0-r1",
                        "Unexpected cloth SDK"
                    );
                }
                if matches!(class, 0x80801A9C | 0x80801BF6) {
                    models.insert(u32::from_le_bytes(
                        raw[offset + 0x1C4..offset + 0x1C8].try_into()?,
                    ));
                }
            }
            let exported = body["models"]
                .as_array()
                .context("Body models")?
                .iter()
                .map(|model| {
                    model["model"]
                        .as_u64()
                        .map(|tag| tag as u32)
                        .context("Model tag")
                })
                .collect::<Result<std::collections::BTreeSet<_>>>()?;
            ensure!(
                !models.is_empty() && models == exported,
                "Export must follow actual body component references, including cloth"
            );
        }
        ensure!(!readback.is_empty(), "Empty export");
        exports.push(json!({"item":hash,"name":item.name,"artifact":path,"readback":readback}));
    }
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "source":"d1_roi", "crate_version":env!("CARGO_PKG_VERSION"),
            "profile":"release", "native_conversion":false, "slots":slots, "exports":exports
        }))?,
    )?;
    Ok(())
}
