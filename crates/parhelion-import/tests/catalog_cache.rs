//! Opt-in package-to-cache workflow. The installed packages are read only.
use anyhow::{Context, Result, ensure};
use parhelion_import::d2_mot::{
    reader,
    service::{self, ScanProgress, Weapon},
};
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn configured(key: &str) -> Result<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .with_context(|| format!("Set {key}"))
}

fn scan(
    modern: &Path,
    native: &Path,
    output: &Path,
    refresh: bool,
) -> Result<(Vec<Weapon>, Value)> {
    let started = Instant::now();
    let mut phases = Vec::new();
    let items = service::scan_cached(modern, native, output, refresh, |phase| {
        let phase = match phase {
            ScanProgress::ReadingItems { .. } => "ReadingItems".to_owned(),
            other => format!("{other:?}"),
        };
        if phases.last() != Some(&phase) {
            phases.push(phase);
        }
    })?;
    let receipt =
        json!({"phases":phases,"seconds":started.elapsed().as_secs_f64(),"items":items.len()});
    Ok((items, receipt))
}

fn visited(receipt: &Value, phase: &str) -> bool {
    receipt["phases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry == phase)
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES, PARHELION_IMPORT_NATIVE_PACKAGES and PARHELION_CATALOG_OUTPUT"]
fn reopening_and_native_changes_reuse_the_source_item_catalog() -> Result<()> {
    let modern = configured("PARHELION_IMPORT_MODERN_PACKAGES")?.canonicalize()?;
    let native = configured("PARHELION_IMPORT_NATIVE_PACKAGES")?.canonicalize()?;
    let output = reader::outside(
        &configured("PARHELION_CATALOG_OUTPUT")?,
        modern.parent().context("Modern root")?,
    )?;
    let output = reader::outside(&output, native.parent().context("Native root")?)?;
    ensure!(!output.exists(), "Use a fresh catalog artifact directory");
    fs::create_dir_all(&output)?;

    // A private package view lets us change one copied file's metadata without
    // touching the installation. Every other package remains a read-only hard link.
    let view = output.join("native-view/packages");
    fs::create_dir_all(&view)?;
    let mut packages = fs::read_dir(&native)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pkg"))
        })
        .map(|entry| Ok((entry.metadata()?.len(), entry.path())))
        .collect::<std::io::Result<Vec<_>>>()?;
    packages.sort();
    ensure!(!packages.is_empty(), "Native packages are empty");
    for (index, (_, package)) in packages.iter().enumerate() {
        let destination = view.join(package.file_name().context("Package name")?);
        if index == 0 {
            fs::copy(package, &destination)?;
        } else {
            fs::hard_link(package, &destination)
                .context("Keep the artifact directory on the native package volume")?;
        }
    }
    let cache = output.join("catalog");
    let (original, cold) = scan(&modern, &view, &cache, false)?;
    ensure!(
        !original.is_empty() && visited(&cold, "ReadingItems"),
        "Cold scan did not read items"
    );
    let (reopened, warm) = scan(&modern, &view, &cache, false)?;
    ensure!(
        !visited(&warm, "ReadingItems")
            && !visited(&warm, "OpeningModernPackages")
            && !visited(&warm, "OpeningNativePackages"),
        "Warm scan reopened packages"
    );
    ensure!(
        serde_json::to_value(&original)? == serde_json::to_value(&reopened)?,
        "Cache changed item data"
    );

    let copied = view.join(packages[0].1.file_name().unwrap());
    let modified = fs::metadata(&copied)?.modified()?;
    fs::OpenOptions::new()
        .write(true)
        .open(&copied)?
        .set_modified(modified + Duration::from_secs(2))?;
    let (updated, changed_native) = scan(&modern, &view, &cache, false)?;
    ensure!(
        !visited(&changed_native, "ReadingItems")
            && !visited(&changed_native, "OpeningModernPackages")
            && visited(&changed_native, "OpeningNativePackages"),
        "Native change reread modern items or skipped native discovery"
    );
    ensure!(
        serde_json::to_value(&updated)? == serde_json::to_value(&original)?,
        "Native metadata change altered source items"
    );

    // Stale installed flags must be recomputed, never ORed into the cached values.
    let path = cache.join("weapon-cache.json");
    let mut saved: Value = serde_json::from_slice(&fs::read(&path)?)?;
    for row in saved["weapons"].as_array_mut().unwrap() {
        row["present_in_native"] = json!(!row["present_in_native"].as_bool().unwrap());
        row["native_item"] = json!(!row["native_item"].as_bool().unwrap());
    }
    saved["native"] = json!("previous-native-snapshot");
    fs::write(&path, serde_json::to_vec(&saved)?)?;
    let (rematched, flags) = scan(&modern, &view, &cache, false)?;
    ensure!(
        !visited(&flags, "ReadingItems"),
        "Presence refresh reread source items"
    );
    ensure!(
        serde_json::to_value(&rematched)? == serde_json::to_value(&original)?,
        "Stale presence or stock-donor flags survived"
    );

    saved["modern"] = json!("previous-source-snapshot");
    fs::write(&path, serde_json::to_vec(&saved)?)?;
    let (_, changed_source) = scan(&modern, &view, &cache, false)?;
    ensure!(
        visited(&changed_source, "ReadingItems"),
        "Source change reused stale items"
    );
    fs::write(&path, b"{interrupted")?;
    let (_, repaired) = scan(&modern, &view, &cache, false)?;
    ensure!(
        visited(&repaired, "ReadingItems"),
        "Corrupt cache was not rebuilt"
    );
    let (_, refreshed) = scan(&modern, &view, &cache, true)?;
    ensure!(
        visited(&refreshed, "ReadingItems"),
        "Explicit refresh did not rescan"
    );

    fs::write(
        output.join("verified-catalog-cache.json"),
        serde_json::to_vec_pretty(&json!({
            "modern":modern,"native":native,"cold":cold,"warm":warm,"native_changed":changed_native,
            "presence_recomputed":flags,"source_changed":changed_source,"corruption_repaired":repaired,
            "explicit_refresh":refreshed,"installed_packages_modified":false
        }))?,
    )?;
    Ok(())
}
