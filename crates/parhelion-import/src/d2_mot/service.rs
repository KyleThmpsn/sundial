//! Source discovery and conservative automatic donor matching for the workbench.
mod cache;
mod emblems;
mod family;
mod gear;
mod shaders;
pub use crate::d2_mot::catalog::ScanProgress;
use crate::d2_mot::{
    GraphReference, batch, catalog, extract, profile,
    reader::{Reader, write_json},
    rig,
};
use anyhow::{Context, Result, ensure};
pub use cache::{package_stamp, scan_cached};
pub use family::Family;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Weapon {
    pub hash: u32,
    pub name: String,
    pub weapon_type: String,
    pub present_in_native: bool,
    /// The source item itself exists natively, so it can lend its model. An installed import
    /// makes `present_in_native` true through its authored identity, but never this.
    #[serde(default)]
    pub native_item: bool,
    #[serde(default)]
    pub dummy: bool,
    #[serde(default)]
    pub icon_index: Option<usize>,
    #[serde(default)]
    pub bucket_hash: Option<u32>,
    #[serde(default)]
    pub class_type: Option<u8>,
    /// Source presentation metadata for browsing, independent of the native donor.
    #[serde(default)]
    pub rarity: Option<u8>,
    #[serde(default)]
    pub ammo: Option<u16>,
    #[serde(default)]
    pub damage: Option<String>,
}

impl Weapon {
    pub fn is_shader(&self) -> bool {
        self.family() == Family::Shader
    }
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

/// The native weapon a source weapon type is built on when Shadowkeep has none of that type.
pub fn type_base(weapon_type: &str) -> Option<u32> {
    (weapon_type == "Glaive").then_some(super::glaive::BASE)
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
        "Item name cannot form a model folder"
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

/// A prior successful conversion is the first candidate, converted again against the
/// current packages before any other donor is matched.
fn previous_model_donor(output: &Path, source: u32) -> Option<u32> {
    let result: Value = serde_json::from_slice(&fs::read(output.join("result.json")).ok()?).ok()?;
    if let Some(hash) = result["donor_hash"].as_u64() {
        return u32::try_from(hash).ok();
    }
    let recipe = Path::new(result["recipe"].as_str()?);
    let candidate = recipe.parent()?.parent()?.file_name()?.to_str()?;
    let index: u32 = candidate.strip_prefix("candidate-")?.parse().ok()?;
    let profile = output
        .join(format!("candidate-{index}"))
        .join(format!("{source:08X}"))
        .join("profile.json");
    let profile: Value = serde_json::from_slice(&fs::read(profile).ok()?).ok()?;
    u32::try_from(profile["native_item"].as_u64()?).ok()
}

/// Prepare `weapon` and record when each step began in the weapon folder's `timings.json`,
/// whether or not it succeeds.
pub fn prepare_with_progress(
    weapon: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    let started = std::time::Instant::now();
    let mut steps = Vec::new();
    let prepare = if weapon.is_shader() {
        shaders::prepare
    } else if weapon.family() == Family::Emblem {
        emblems::prepare
    } else if weapon.family().is_model_gear() {
        gear::prepare
    } else {
        prepare_steps
    };
    let result = prepare(
        weapon,
        modern,
        native,
        donors,
        output,
        &mut |step: String| {
            steps.push((started.elapsed().as_secs_f64(), step.clone()));
            progress(step);
        },
    );
    steps.push((
        started.elapsed().as_secs_f64(),
        if result.is_ok() { "Done" } else { "Failed" }.to_owned(),
    ));
    // Timings describe the run. A failure to save them never fails the import.
    let rows = steps
        .windows(2)
        .map(
            |pair| json!({"step": pair[0].1, "start": pair[0].0, "seconds": pair[1].0 - pair[0].0}),
        )
        .collect::<Vec<_>>();
    if let Err(error) = write_json(
        &output.join("timings.json"),
        &json!({"total": started.elapsed().as_secs_f64(), "steps": rows}),
    ) {
        eprintln!("Import timings not saved: {error:#}");
    }
    result
}

/// The source export's format. Bump it whenever extraction writes different files or reports,
/// so an export made by an earlier importer is extracted again rather than reused.
const SOURCE_EXPORT: u32 = 6;

/// The newest finished export of `weapon` from the same modern packages and export format.
/// An export is only read once it is finished, by every donor attempt alike, so a later import
/// of the same weapon can start from it.
fn reusable_source(output: &Path, weapon: u32, stamp: &str) -> Option<(PathBuf, bool)> {
    let mut found = fs::read_dir(output)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let index: u32 = path
                .file_name()?
                .to_str()?
                .strip_prefix("source-")?
                .parse()
                .ok()?;
            Some((index, path))
        })
        .collect::<Vec<_>>();
    found.sort();
    found.into_iter().rev().find_map(|(_, path)| {
        let export: Value =
            serde_json::from_slice(&fs::read(path.join("export.json")).ok()?).ok()?;
        let complete = [
            "report.json",
            "rig.json",
            "gameplay.json",
            "source-manifest.json",
            "hud.json",
            "hud-icon.png",
        ]
        .iter()
        .all(|file| path.join(file).is_file());
        (export["version"] == SOURCE_EXPORT
            && export["weapon"] == weapon
            && export["packages"] == stamp
            && complete)
            .then(|| Some((path, export["exotic"].as_bool()?)))
            .flatten()
    })
}

/// Extract the source weapon into a new export folder and mark it finished.
fn export_source(
    weapon: &Weapon,
    modern: &Path,
    output: &Path,
    stamp: &str,
    progress: &mut dyn FnMut(String),
) -> Result<(PathBuf, bool)> {
    let source = reserve_assets(output, "source")?;
    progress("Opening source packages…".into());
    let mut reader = Reader::new(modern, &source, true)?;
    let report = extract::extract_with_progress(
        &mut reader,
        weapon.hash,
        None,
        super::geometry::Detail::default(),
        progress,
    )?;
    let exotic = reader
        .tag(profile::hash(&report, "item_tag")?, Some(0x8080799D))?
        .u8(0xA0)?
        == 5;
    progress("Reading source skeleton…".into());
    let skeleton = rig::inspect_with_audio(&mut reader, profile::hash(&report, "item_tag")?, true)?;
    progress("Reading source HUD artwork…".into());
    super::hud::export(&mut reader, &report, &skeleton)?;
    write_json(&source.join("report.json"), &report)?;
    write_json(&source.join("rig.json"), &skeleton)?;
    reader.finish()?;
    // Written last: an interrupted export has no marker and is never reused.
    write_json(
        &source.join("export.json"),
        &json!({"version": SOURCE_EXPORT, "weapon": weapon.hash, "packages": stamp, "exotic": exotic}),
    )?;
    Ok((source, exotic))
}

/// What became of one donor attempt that did not end the import.
enum Attempt {
    Prepared(PathBuf),
    /// The donor cannot carry this weapon, for the reason shown to the user.
    Rejected(anyhow::Error),
}

/// The inputs every donor attempt for one weapon shares.
struct Inputs<'a> {
    weapon: &'a Weapon,
    modern: &'a Path,
    native: &'a Path,
    source: &'a Path,
    output: &'a Path,
}

/// Convert the weapon against one donor in a fresh candidate folder. A rejected attempt's
/// folder is removed, since nothing refers to it. An error returned here ends the import.
fn try_donor(
    inputs: &Inputs,
    donor: &Value,
    native_reader: &Reader,
    progress: &mut dyn FnMut(String),
) -> Result<Attempt> {
    let attempt = reserve_assets(inputs.output, "candidate")?;
    let result = convert_with_donor(inputs, donor, native_reader, &attempt, progress);
    if !matches!(result, Ok(Attempt::Prepared(_))) {
        // Removing an unused attempt only saves space, so a failure here is not an error.
        if let Err(error) = fs::remove_dir_all(&attempt) {
            eprintln!("Unused donor attempt {} kept: {error}", attempt.display());
        }
    }
    result
}

fn convert_with_donor(
    inputs: &Inputs,
    donor: &Value,
    native_reader: &Reader,
    attempt: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<Attempt> {
    let Inputs {
        weapon,
        modern,
        native,
        source,
        output,
    } = *inputs;
    fs::create_dir_all(attempt.join("recipes"))?;
    let row = json!({"hash":weapon.hash,"name":weapon.name,"native_item":donor["hash"],"native_donor":donor["name"]});
    let prepared = match batch::prepare_one_reusing(
        &row,
        weapon.hash & 0xffff,
        modern,
        native,
        attempt,
        Some(source),
        Some(native_reader),
        progress,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return Ok(Attempt::Rejected(error)),
    };
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
    let verified_profile =
        super::compatibility::apply(weapon.hash, profile::hash(donor, "hash")?, &mut document);
    progress("Matching source perks, stats and weapon properties…".into());
    let gameplay: Value = serde_json::from_slice(&fs::read(source.join("gameplay.json"))?)?;
    super::gameplay::apply(&gameplay, native, &attempt.join("gameplay"), &mut document)?;
    if super::glaive::is_glaive(&gameplay) {
        progress("Adding the glaive shield and intrinsic…".into());
        let prepared_folder = PathBuf::from(
            prepared["prepared"]
                .as_str()
                .context("Missing prepared folder")?,
        );
        let folder = PathBuf::from(
            prepared["folder"]
                .as_str()
                .context("Missing import folder")?,
        );
        let source_rig: Value = serde_json::from_slice(&fs::read(source.join("rig.json"))?)?;
        let native_rig: Value =
            serde_json::from_slice(&fs::read(folder.join("native").join("rig.json"))?)?;
        let kit = super::glaive::Inputs {
            modern,
            native,
            prepared: &prepared_folder,
            graph: &graph,
            work: &attempt.join("glaive"),
            source_rig: &source_rig,
            native_rig: &native_rig,
        };
        // Another donor may have the frame socket this one lacks.
        if let Err(error) = super::glaive::apply(&kit, &gameplay, &mut document, progress) {
            return Ok(Attempt::Rejected(
                error.context("glaive shield and intrinsic"),
            ));
        }
    }
    progress("Converting the hip-fire crosshair…".into());
    if let Err(error) =
        super::crosshair::apply(modern, native, source, &attempt.join("crosshair"), &graph)
    {
        crate::cancellation::check()?;
        super::crosshair::record_failure(&graph, &error)?;
    }
    progress("Verifying and saving recipe assets…".into());
    let item = profile::hash(&document["identity"], "item_hash")?;
    document["overrides"]["imported_graph"] =
        serde_json::to_value(GraphReference::new(&graph, item)?)?;
    write_json(&recipe, &document)?;
    let graph_manifest: Value = serde_json::from_slice(&fs::read(graph.join("asset-graph.json"))?)?;
    let audio = &graph_manifest["audio"];
    write_json(
        &output.join("result.json"),
        &json!({"recipe":recipe,"donor":donor["name"],"donor_hash":donor["hash"],"audio":audio,"source_audio_imported":false,"verified_profile":verified_profile,"gameplay_verified":false,"limitations":"Modern PCM media is prepared for named events with compatible native routing. Package authoring links those events through private Dawn soundbanks. Source-only and unnamed firing cues still need runtime routing. New conversions require an in-game test."}),
    )?;
    Ok(Attempt::Prepared(recipe))
}

/// Record a rejected donor. A limit of the source itself ends the import, because every
/// remaining donor would rediscover it, which is what turned such imports into long timeouts.
fn rejected(donor: &Value, error: &anyhow::Error, errors: &mut Vec<String>) -> Result<()> {
    crate::cancellation::check()?;
    if crate::cancellation::is_cancelled(error) {
        return Err(crate::cancellation::Cancelled.into());
    }
    let detail = format!("{}: {error:#}", donor["name"]);
    eprintln!("Donor conversion failed: {detail}");
    errors.push(detail);
    if super::is_source_limit(error) {
        anyhow::bail!(
            "Conversion needs importer support that no native donor supplies:\n{}",
            errors.join("\n")
        );
    }
    Ok(())
}

fn prepare_steps(
    weapon: &Weapon,
    modern: &Path,
    native: &Path,
    donors: &Value,
    output: &Path,
    progress: &mut dyn FnMut(String),
) -> Result<PathBuf> {
    // Every stage opens its own readers. Holding both package indexes for the whole weapon
    // lets each of them reuse the index rather than read every package header again.
    let _indexes = (
        super::reader::package_index(&modern.canonicalize()?, true)?,
        super::reader::package_index(&native.canonicalize()?, false)?,
    );
    let stamp = package_stamp(modern)?;
    let (source, source_exotic) = match reusable_source(output, weapon.hash, &stamp) {
        Some(found) => {
            progress("Reusing the saved source export…".into());
            found
        }
        None => export_source(weapon, modern, output, &stamp, progress)?,
    };
    let skeleton: Value = serde_json::from_slice(&fs::read(source.join("rig.json"))?)?;
    progress("Checking source material programs and blends…".into());
    super::native::effects::check_source_blends(&source)?;
    progress("Checking source vertex skinning…".into());
    super::rig_convert::check_source_skinning(&source)?;
    let catalog_path = source.join("donors.json");
    write_json(&catalog_path, donors)?;
    let matches_path = source.join("matches");
    let mut native_reader = Reader::discovery(native, &matches_path, false)?;
    let inputs = Inputs {
        weapon,
        modern,
        native,
        source: &source,
        output,
    };
    // Shadowkeep has no glaive, so a glaive is built on the glaive base without a donor search.
    let glaive = super::glaive::exported_glaive(&source)?;
    let preferred = if glaive {
        Some(super::glaive::BASE)
    } else {
        super::compatibility::profile(weapon.hash)
            .map(|p| p.model_donor)
            .or_else(|| previous_model_donor(output, weapon.hash))
    };
    let mut errors = Vec::new();
    // Converting checks a donor in full, and the donor that converted this weapon before
    // usually converts it again. The sweep over every compatible donor only runs if it fails.
    let first = preferred.and_then(|hash| {
        donors["weapons"].as_array()?.iter().find(|row| {
            row["hash"].as_u64() == Some(u64::from(hash))
                && batch::donor_candidate(row, &weapon.weapon_type, preferred)
        })
    });
    if let Some(donor) = first {
        progress(if glaive {
            "Building on the glaive base…".into()
        } else {
            format!(
                "Trying the previous donor: {}",
                donor["name"].as_str().unwrap_or("Weapon")
            )
        });
        match try_donor(&inputs, donor, &native_reader, progress)? {
            Attempt::Prepared(recipe) => return Ok(recipe),
            Attempt::Rejected(error) => rejected(donor, &error, &mut errors)?,
        }
    }
    // No other native weapon carries the glaive rig.
    ensure!(
        !glaive,
        "The glaive base could not carry this glaive: {}",
        if errors.is_empty() {
            "the base is not among the installed weapons".to_owned()
        } else {
            errors.join("\n")
        }
    );
    progress("Opening destination packages for donor matching…".into());
    let matches = batch::donors_with_reader(
        &source,
        &catalog_path,
        &weapon.weapon_type,
        native,
        &matches_path,
        &mut native_reader,
        progress,
    )?;
    native_reader.clear_cached_tags();
    let mut candidates = matches["matches"]
        .as_array()
        .context("Missing donor results")?
        .clone();
    let source_gameplay: Value = serde_json::from_slice(&fs::read(source.join("gameplay.json"))?)?;
    let source_bucket = source_gameplay["bucket"].as_u64();
    let source_rarity = source_gameplay["rarity"].as_u64();
    let source_content = skeleton["content_key"].as_str();
    candidates.sort_by_key(|row| {
        selection_priority(
            row,
            (weapon.hash, &weapon.name),
            source_exotic,
            source_bucket,
            source_rarity,
            source_content,
            preferred,
        )
    });
    candidates.dedup_by_key(|row| row["hash"].as_u64());
    // The previous donor was already converted and turned down.
    let tried = first.and_then(|donor| donor["hash"].as_u64());
    candidates.retain(|row| tried.is_none() || row["hash"].as_u64() != tried);
    if candidates.is_empty() && errors.is_empty() {
        anyhow::bail!(
            "No native {} donor has a compatible model skeleton: {}",
            weapon.weapon_type,
            rejections(&matches).join("; ")
        );
    }
    for (index, donor) in candidates.iter().enumerate() {
        progress(format!(
            "Trying donor {} of {}: {}",
            index + 1,
            candidates.len(),
            donor["name"].as_str().unwrap_or("Weapon")
        ));
        match try_donor(&inputs, donor, &native_reader, progress)? {
            Attempt::Prepared(recipe) => return Ok(recipe),
            Attempt::Rejected(error) => rejected(donor, &error, &mut errors)?,
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

fn selection_priority(
    row: &Value,
    source: (u32, &str),
    source_exotic: bool,
    source_bucket: Option<u64>,
    source_rarity: Option<u64>,
    source_content: Option<&str>,
    preferred: Option<u32>,
) -> (
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    u64,
    Option<u64>,
) {
    let (fallback, bones, hash) = donor_priority(row, source_exotic);
    let same_identity = hash == Some(u64::from(source.0))
        || row["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(source.1));
    (
        row["collection_backed"].as_bool() == Some(false),
        row["unrelated_collection_condition"].as_bool() == Some(true),
        !same_identity,
        source_content.is_none_or(|content| row["content_key"].as_str() != Some(content)),
        source_bucket.is_some_and(|bucket| row["bucket"].as_u64() != Some(bucket)),
        source_rarity.is_some_and(|rarity| row["rarity"].as_u64() != Some(rarity)),
        hash != preferred.map(u64::from),
        fallback,
        bones,
        hash,
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
    fn previous_success_is_found_without_following_saved_recipe_paths() {
        let root = tempfile::tempdir().unwrap();
        let candidate = root.path().join("candidate-7");
        let profile = candidate.join("0000002A").join("profile.json");
        fs::create_dir_all(profile.parent().unwrap()).unwrap();
        write_json(&profile, &json!({"native_item":77})).unwrap();
        write_json(
            &root.path().join("result.json"),
            &json!({"recipe":candidate.join("recipes").join("weapon.json")}),
        )
        .unwrap();
        assert_eq!(previous_model_donor(root.path(), 42), Some(77));
        assert_eq!(previous_model_donor(root.path(), 43), None);
        write_json(
            &root.path().join("result.json"),
            &json!({"donor_hash":91,"recipe":"outside/candidate-9/weapon.json"}),
        )
        .unwrap();
        assert_eq!(previous_model_donor(root.path(), 42), Some(91));
    }

    #[test]
    fn remembered_donor_precedes_smaller_unverified_candidates() {
        let mut candidates = [
            json!({"hash":1,"exotic":false,"mapping":{"native_bone_count":3}}),
            json!({"hash":2,"exotic":true,"mapping":{"native_bone_count":9}}),
        ];
        let preferred = Some(2);
        candidates.sort_by_key(|row| {
            selection_priority(row, (0, "Source"), false, None, None, None, preferred)
        });
        assert_eq!(candidates[0]["hash"], 2);
    }

    #[test]
    fn collection_backed_donor_can_cross_slot_and_rarity() {
        let mut candidates = [
            json!({"hash":1,"exotic":false,"bucket":20,"rarity":4,"collection_backed":false,"mapping":{"native_bone_count":1}}),
            json!({"hash":2,"exotic":false,"bucket":10,"rarity":2,"collection_backed":true,"mapping":{"native_bone_count":8}}),
        ];
        candidates.sort_by_key(|row| {
            selection_priority(row, (0, "Source"), false, Some(20), Some(4), None, Some(1))
        });
        assert_eq!(candidates[0]["hash"], 2);
    }

    #[test]
    fn simple_collection_donor_precedes_unrelated_progression_conditions() {
        let mut candidates = [
            json!({"hash":1,"exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"unrelated_collection_condition":true,"mapping":{"native_bone_count":15}}),
            json!({"hash":2,"exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"unrelated_collection_condition":false,"mapping":{"native_bone_count":15}}),
        ];
        candidates.sort_by_key(|row| {
            selection_priority(row, (0, "Source"), false, Some(20), Some(4), None, Some(1))
        });
        assert_eq!(candidates[0]["hash"], 2);
    }

    #[test]
    fn available_native_counterpart_precedes_other_safe_donors() {
        let mut candidates = [
            json!({"hash":1,"name":"Another Weapon","exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"mapping":{"native_bone_count":8}}),
            json!({"hash":2,"name":"Source Weapon","exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"mapping":{"native_bone_count":12}}),
        ];
        candidates.sort_by_key(|row| {
            selection_priority(
                row,
                (99, "source weapon"),
                false,
                Some(20),
                Some(4),
                None,
                None,
            )
        });
        assert_eq!(candidates[0]["hash"], 2);
        candidates[0]["unrelated_collection_condition"] = json!(true);
        candidates.sort_by_key(|row| {
            selection_priority(
                row,
                (99, "Source Weapon"),
                false,
                Some(20),
                Some(4),
                None,
                None,
            )
        });
        assert_eq!(candidates[0]["hash"], 1);
    }

    #[test]
    fn shared_content_group_precedes_smaller_unrelated_rig() {
        let mut candidates = [
            json!({"hash":1,"name":"Generic Auto","content_key":"611794E9","exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"mapping":{"native_bone_count":3}}),
            json!({"hash":2,"name":"Suros Auto","content_key":"A0EEEE21","exotic":false,"bucket":20,"rarity":4,"collection_backed":true,"mapping":{"native_bone_count":4}}),
        ];
        candidates.sort_by_key(|row| {
            selection_priority(
                row,
                (99, "Source"),
                false,
                Some(20),
                Some(4),
                Some("A0EEEE21"),
                None,
            )
        });
        assert_eq!(candidates[0]["hash"], 2);
    }

    #[test]
    fn only_a_finished_export_of_the_same_weapon_and_packages_is_reused() {
        let root = tempfile::tempdir().unwrap();
        let export = |marker: Option<Value>| {
            let source = reserve_assets(root.path(), "source").unwrap();
            for file in [
                "report.json",
                "rig.json",
                "gameplay.json",
                "source-manifest.json",
                "hud.json",
                "hud-icon.png",
            ] {
                fs::write(source.join(file), "{}").unwrap();
            }
            if let Some(marker) = marker {
                write_json(&source.join("export.json"), &marker).unwrap();
            }
            source
        };
        let marker = |weapon: u32, packages: &str, version: u32| json!({"version": version, "weapon": weapon, "packages": packages, "exotic": true});
        let finished = export(Some(marker(7, "build", SOURCE_EXPORT)));
        export(None);
        export(Some(marker(7, "other build", SOURCE_EXPORT)));
        export(Some(marker(8, "build", SOURCE_EXPORT)));
        export(Some(marker(7, "build", SOURCE_EXPORT + 1)));
        assert_eq!(
            reusable_source(root.path(), 7, "build"),
            Some((finished.clone(), true))
        );
        fs::remove_file(finished.join("rig.json")).unwrap();
        assert_eq!(reusable_source(root.path(), 7, "build"), None);
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
}
