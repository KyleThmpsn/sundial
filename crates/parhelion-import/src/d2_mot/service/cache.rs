use super::*;

pub use crate::tiger::reader::package_stamp;

const SCHEMA: u32 = 9;

#[derive(Deserialize, Serialize)]
struct Cached {
    schema: u32,
    modern: String,
    native: String,
    weapons: Vec<Weapon>,
}

pub fn scan_cached(
    modern: &Path,
    native: &Path,
    output: &Path,
    refresh: bool,
    mut progress: impl FnMut(ScanProgress),
) -> Result<Vec<Weapon>> {
    progress(ScanProgress::CheckingCache);
    let modern = modern.canonicalize()?;
    let native = native.canonicalize()?;
    let output = super::super::reader::outside(output, modern.parent().unwrap_or(&modern))?;
    let output = super::super::reader::outside(&output, native.parent().unwrap_or(&native))?;
    fs::create_dir_all(&output)?;
    // The importer and model picker share this listing. Serialize discovery and
    // publication so a second reader can reuse the first one's completed scan.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(output.join("d2_mot.lock"))
        .ok();
    if let Some(lock) = &lock {
        crate::cancellation::lock(lock)?;
    }
    let modern_stamp = package_stamp(&modern)?;
    let native_stamp = package_stamp(&native)?;
    let path = output.join("d2_mot.json");
    // The package reads behind the listing are the importer's cache, `cache/d2_mot/catalog` beside
    // the catalog folder, which holds only listings.
    let work = output
        .with_file_name("cache")
        .join("d2_mot")
        .join("catalog");
    fs::create_dir_all(&work)?;
    // Earlier versions kept the listing as `weapon-cache.json` under `catalog.lock`, wrote a copy
    // of every scan to `catalog.json` that nothing read, and kept the reads' files in `output`,
    // then in its `d2_mot` folder.
    crate::cache::take_over(&output.join("weapon-cache.json"), &path);
    for former in ["catalog.lock", "catalog.json", "source-manifest.json"] {
        let _ = fs::remove_file(output.join(former));
    }
    for former in ["raw", "native", "d2_mot"] {
        let _ = fs::remove_dir_all(output.join(former));
    }
    let cached = (!refresh)
        .then(|| fs::read(&path).ok())
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Cached>(&bytes).ok())
        .filter(|cached| cached.schema == SCHEMA && cached.modern == modern_stamp);
    let (weapons, changed) = if let Some(mut cached) = cached {
        progress(ScanProgress::LoadingCachedItems);
        let changed = cached.native != native_stamp;
        if changed {
            // An install only changes which items exist in the target game.
            // Keep the costly source discovery and recompute both presence flags.
            progress(ScanProgress::OpeningNativePackages);
            let hashes = catalog::native_hashes(&native, &work)?;
            catalog::match_native(&mut cached.weapons, &hashes)?;
        }
        (cached.weapons, changed)
    } else {
        (
            scan_with_progress(&modern, &native, &work, &mut progress)?,
            true,
        )
    };
    // A cache hit can race an installation too. Never return mixed snapshots.
    ensure!(
        package_stamp(&modern)? == modern_stamp && package_stamp(&native)? == native_stamp,
        "Packages changed while reading items. Refresh after the installation finishes."
    );
    if changed {
        progress(ScanProgress::SavingCatalog);
        let cached = Cached {
            schema: SCHEMA,
            modern: modern_stamp.clone(),
            native: native_stamp.clone(),
            weapons: weapons.clone(),
        };
        // Persistence is an optimization. A failed write must not discard items
        // we successfully read. Replacing a complete temporary keeps old caches
        // usable after an interrupted save.
        let _ = save(&path, &cached);
        ensure!(
            package_stamp(&modern)? == modern_stamp && package_stamp(&native)? == native_stamp,
            "Packages changed while caching items. Refresh after the installation finishes."
        );
    }
    Ok(weapons)
}

fn save(path: &Path, cached: &Cached) -> Result<()> {
    use std::io::Write;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("Cache directory")?)?;
    temporary.write_all(&serde_json::to_vec(cached)?)?;
    temporary.persist(path)?;
    Ok(())
}
