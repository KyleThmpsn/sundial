//! Source discovery and conservative automatic donor matching for the workbench.
mod cache;
pub use crate::d2_mot::catalog::ScanProgress;
use crate::d2_mot::{
    GraphReference, batch, catalog, extract, profile,
    reader::{Reader, write_json},
    rig,
};
use anyhow::{Context, Result, ensure};
pub use cache::{package_stamp, scan_cached};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Weapon {
    pub hash: u32,
    pub name: String,
    pub weapon_type: String,
    pub present_in_native: bool,
    #[serde(default)]
    pub dummy: bool,
    #[serde(default)]
    pub icon_index: Option<usize>,
}

/// The stable authored identity also detects previous imports with a renamed weapon.
pub fn destination_hash(source: u32) -> Result<u32> {
    profile::hash(
        &batch::identity(&super::compatibility::namespace(source))?,
        "item_hash",
    )
}

/// The native model donor a retained compatibility profile names for a source weapon.
pub fn profile_donor(source: u32) -> Option<u32> {
    super::compatibility::profile(source).map(|profile| profile.model_donor)
}

/// Source weapons covered by the retained, gameplay-tested compatibility profiles.
pub fn known_weapons() -> impl Iterator<Item = (u32, &'static str)> {
    super::compatibility::known_weapons()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub modern_packages: Option<PathBuf>,
}

pub fn packages_path(path: &Path) -> Result<PathBuf> {
    let path = if path.join("packages").is_dir() {
        path.join("packages")
    } else {
        path.to_owned()
    };
    ensure!(
        path.is_dir(),
        "Choose the modern Destiny 2 installation or its packages folder"
    );
    path.canonicalize()
        .context("Open modern Destiny 2 packages")
}

pub fn model_directory(root: &Path, name: &str, hash: u32) -> Result<PathBuf> {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_end_matches('.');
    ensure!(
        !name.is_empty() && name != "." && name != "..",
        "Weapon name cannot form a model folder"
    );
    let reserved = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        ![
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
        ]
        .contains(&reserved.as_str()),
        "Reserved model folder name"
    );
    let parent = root.join(name);
    fs::create_dir_all(&parent)?;
    Ok(parent.join(format!("{hash:08X}")))
}

/// Reserve independent assets inside a stable weapon folder without replacing a saved graph.
pub fn reserve_assets(root: &Path, kind: &str) -> Result<PathBuf> {
    ensure!(
        !kind.is_empty() && kind.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "Invalid asset folder kind"
    );
    fs::create_dir_all(root)?;
    for index in 0..u32::MAX {
        let path = root.join(format!("{kind}-{index}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("No unused asset folder remains")
}

pub fn scan(modern: &Path, native: &Path, output: &Path) -> Result<Vec<Weapon>> {
    scan_with_progress(modern, native, output, |_| {})
}

pub fn scan_with_progress(
    modern: &Path,
    native: &Path,
    output: &Path,
    progress: impl FnMut(ScanProgress),
) -> Result<Vec<Weapon>> {
    let catalog = catalog::weapons_with_progress(modern, native, output, progress)?;
    let mut weapons: Vec<Weapon> = serde_json::from_value(catalog["weapons"].clone())?;
    weapons.sort_by_key(|weapon| (weapon.name.to_lowercase(), weapon.hash));
    Ok(weapons)
}

/// A successful result is a prepared recipe, never an installed package.
pub fn prepare(
    weapon: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
) -> Result<PathBuf> {
    prepare_with_progress(weapon, modern, native, donors, output, &mut |_| {})
}

/// Summarise why donor matching turned every candidate down. Distinct reasons
/// are far more useful than a repeated one, so the list is deduplicated.
fn rejections(matches: &Value) -> Vec<String> {
    let mut reasons = matches["unavailable"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| Some(row["reason"].as_str()?.to_owned()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    reasons.sort();
    reasons.dedup();
    reasons.truncate(3);
    if reasons.is_empty() {
        reasons.push("no native weapon of this type was examined".to_owned());
    }
    reasons
}

pub fn prepare_with_progress(
    weapon: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let source = reserve_assets(output, "source")?;
    progress("Opening source packages…".into());
    let mut reader = Reader::new(modern, &source, true)?;
    let report = extract::extract_with_progress(&mut reader, weapon.hash, None, progress)?;
    let source_exotic = reader
        .tag(profile::hash(&report, "item_tag")?, Some(0x8080799D))?
        .u8(0xA0)?
        == 5;
    progress("Reading source skeleton…".into());
    let skeleton = rig::inspect(&mut reader, profile::hash(&report, "item_tag")?, true)?;
    write_json(&source.join("report.json"), &report)?;
    write_json(&source.join("rig.json"), &skeleton)?;
    reader.finish()?;
    drop(reader);
    progress("Checking source material programs and blends…".into());
    super::native::effects::check_source_blends(&source)?;
    progress("Checking source vertex skinning…".into());
    super::rig_convert::check_source_skinning(&source)?;
    let catalog_path = source.join("donors.json");
    write_json(&catalog_path, donors)?;
    let matches = batch::donors_with_progress(
        &source,
        &catalog_path,
        &weapon.weapon_type,
        native,
        &source.join("matches"),
        progress,
    )?;
    let mut candidates = matches["matches"]
        .as_array()
        .context("Missing donor results")?
        .clone();
    let preferred = super::compatibility::profile(weapon.hash).map(|p| p.model_donor);
    candidates.sort_by_key(|row| {
        let (fallback, bones, hash) = donor_priority(row, source_exotic);
        (fallback, hash != preferred.map(u64::from), bones, hash)
    });
    candidates.dedup_by_key(|row| row["hash"].as_u64());
    if candidates.is_empty() {
        anyhow::bail!(
            "No native {} donor has a compatible model skeleton: {}",
            weapon.weapon_type,
            rejections(&matches).join("; ")
        );
    }
    let mut errors = Vec::new();
    for (index, donor) in candidates.iter().enumerate() {
        let attempt = reserve_assets(output, "candidate")?;
        fs::create_dir_all(attempt.join("recipes"))?;
        let row = json!({"hash":weapon.hash,"name":weapon.name,"native_item":donor["hash"],"native_donor":donor["name"]});
        progress(format!(
            "Trying donor {} of {}: {}",
            index + 1,
            candidates.len(),
            donor["name"].as_str().unwrap_or("Weapon")
        ));
        match batch::prepare_one_reusing(
            &row,
            weapon.hash & 0xffff,
            modern,
            native,
            &attempt,
            Some(&source),
            progress,
        ) {
            Ok(prepared) => {
                let recipe = PathBuf::from(
                    prepared["recipe"]
                        .as_str()
                        .context("Missing prepared recipe")?,
                );
                let graph = PathBuf::from(
                    prepared["graph"]
                        .as_str()
                        .context("Missing prepared graph")?,
                );
                let mut document: Value = serde_json::from_slice(&fs::read(&recipe)?)?;
                let verified_profile = super::compatibility::apply(
                    weapon.hash,
                    profile::hash(donor, "hash")?,
                    &mut document,
                );
                progress("Matching source perks, stats and weapon properties…".into());
                let gameplay: Value =
                    serde_json::from_slice(&fs::read(source.join("gameplay.json"))?)?;
                super::gameplay::apply(
                    &gameplay,
                    native,
                    &attempt.join("gameplay"),
                    &mut document,
                )?;
                progress("Verifying and saving recipe assets…".into());
                let item = profile::hash(&document["identity"], "item_hash")?;
                document["overrides"]["imported_graph"] =
                    serde_json::to_value(GraphReference::new(&graph, item)?)?;
                write_json(&recipe, &document)?;
                write_json(
                    &output.join("result.json"),
                    &json!({"recipe":recipe,"donor":donor["name"],"verified_profile":verified_profile,"gameplay_verified":false,"limitations":"Native donor animations. Source shader equations and supported rendering stages are converted. Procedural animations can retain validated static defaults. New conversions require an in-game test."}),
                )?;
                return Ok(recipe);
            }
            Err(error) => {
                let reason = format!("{error:#}");
                let detail = format!("{}: {reason}", donor["name"]);
                eprintln!("Donor conversion failed: {detail}");
                errors.push(detail);
                // A donor-specific rejection does not prove later donors fail.
                // The early channel preflight keeps these attempts inexpensive.
                // Sweeping the remaining donors would rediscover the same
                // limit, which is what turned these into long timeouts.
                if super::is_source_limit(&error) {
                    anyhow::bail!(
                        "Conversion needs importer support that no native donor supplies:
{}",
                        errors.join(
                            "
"
                        )
                    );
                }
            }
        }
    }
    anyhow::bail!(
        "Compatible skeletons were found, but model conversion failed:\n{}",
        errors.join("\n")
    )
}

fn donor_priority(row: &Value, source_exotic: bool) -> (bool, u64, Option<u64>) {
    (
        row["exotic"]
            .as_bool()
            .is_none_or(|exotic| exotic && !source_exotic),
        row["mapping"]["native_bone_count"]
            .as_u64()
            .unwrap_or(u64::MAX),
        row["hash"].as_u64(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_only_limits_are_distinguished_from_donor_failures() {
        // Bygones met the same plate limit on all 68 donors. Marking it lets
        // the loop report it once instead of sweeping the whole catalogue.
        let plain = anyhow::anyhow!("native donor has no object channel input");
        assert!(!crate::d2_mot::is_source_limit(&plain));
        let limited = crate::d2_mot::source_limit(anyhow::anyhow!(
            "multiple plate textures require composition"
        ));
        assert!(crate::d2_mot::is_source_limit(&limited));
        assert!(
            format!("{limited:#}").contains("multiple plate textures require composition"),
            "the underlying reason must survive marking"
        );
        assert!(crate::d2_mot::is_source_limit(
            &limited.context("source stage 1")
        ));
    }

    #[test]
    fn donor_rejections_are_summarised_without_repeats() {
        // Every native bow turned Pre Astyanax IV down for the same reason, so
        // the import error should state it once rather than fourteen times.
        let matches = serde_json::json!({"unavailable":[
            {"name":"A","reason":"no compatible skeleton: weighted source vertices require a skinning converter"},
            {"name":"B","reason":"no compatible skeleton: weighted source vertices require a skinning converter"},
            {"name":"C","reason":"native catalog item missing"},
        ]});
        let reasons = super::rejections(&matches);
        assert_eq!(reasons.len(), 2);
        assert!(reasons.iter().any(|r| r.contains("skinning converter")));
        assert_eq!(
            super::rejections(&serde_json::json!({})),
            ["no native weapon of this type was examined"]
        );
    }

    use super::*;

    #[test]
    fn ordinary_donors_precede_exotics_even_with_larger_skeletons() {
        let ordinary = json!({"hash":99,"exotic":false,"mapping":{"native_bone_count":12}});
        let exotic = json!({"hash":1,"exotic":true,"mapping":{"native_bone_count":8}});
        let unknown = json!({"hash":0,"mapping":{"native_bone_count":1}});
        assert!(donor_priority(&ordinary, false) < donor_priority(&exotic, false));
        assert!(donor_priority(&ordinary, false) < donor_priority(&unknown, false));
        // Exotic sources can use the closer exotic rig without a rarity penalty.
        assert!(donor_priority(&exotic, true) < donor_priority(&ordinary, true));
        assert!(donor_priority(&exotic, true) < donor_priority(&unknown, true));
    }

    #[test]
    fn model_cache_uses_stable_hash_and_preserves_previous_assets() {
        let root = tempfile::tempdir().unwrap();
        let first = model_directory(root.path(), "Half-Truths", 42).unwrap();
        let assets = reserve_assets(&first, "candidate").unwrap();
        fs::write(assets.join("model.bin"), [1, 2, 3]).unwrap();
        let second = model_directory(root.path(), "Half-Truths", 42).unwrap();
        assert_eq!(
            first.parent(),
            Some(root.path().join("Half-Truths").as_path())
        );
        assert_eq!(first, second);
        assert_eq!(first.file_name().unwrap(), "0000002A");
        let next = reserve_assets(&second, "candidate").unwrap();
        assert_ne!(assets, next);
        assert_eq!(fs::read(assets.join("model.bin")).unwrap(), [1, 2, 3]);
        assert!(reserve_assets(&second, "../escape").is_err());
        assert!(model_directory(root.path(), "..", 42).is_err());
        assert!(model_directory(root.path(), "CON", 42).is_err());
        assert!(
            model_directory(root.path(), "../Weapon", 42)
                .unwrap()
                .starts_with(root.path())
        );
    }

    #[test]
    fn source_configuration_round_trips_with_feature_disabled_by_default() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.enabled);
        let settings = Settings {
            enabled: true,
            modern_packages: Some(PathBuf::from("fixtures").join("modern").join("packages")),
        };
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(restored.enabled);
        assert_eq!(restored.modern_packages, settings.modern_packages);
    }
}
