use super::*;
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 6;

/// Metadata changes, additions, removals and a different configured build invalidate the cache.
pub fn package_stamp(packages: &Path) -> Result<String> {
    let packages = packages.canonicalize()?;
    let mut files = Vec::new();
    for entry in fs::read_dir(&packages)? {
        crate::cancellation::check()?;
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pkg"))
        {
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?;
            files.push((entry.file_name(), metadata.len(), modified.as_nanos()));
        }
    }
    files.sort();
    ensure!(
        !files.is_empty(),
        "No packages found in {}",
        packages.display()
    );
    let mut hash = Sha256::new();
    hash.update(packages.to_string_lossy().as_bytes());
    for (name, length, modified) in files {
        hash.update(name.to_string_lossy().as_bytes());
        hash.update([0]);
        hash.update(length.to_le_bytes());
        hash.update(modified.to_le_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

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
    // The importer and model picker share this catalog. Serialize discovery and
    // publication so a second reader can reuse the first one's completed scan.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(output.join("catalog.lock"))
        .ok();
    if let Some(lock) = &lock {
        crate::cancellation::lock(lock)?;
    }
    let modern_stamp = package_stamp(&modern)?;
    let native_stamp = package_stamp(&native)?;
    let path = output.join("weapon-cache.json");
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
            let hashes = catalog::native_hashes(&native, &output)?;
            catalog::match_native(&mut cached.weapons, &hashes)?;
        }
        (cached.weapons, changed)
    } else {
        (
            scan_with_progress(&modern, &native, &output, &mut progress)?,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_stamp_ignores_reports_but_detects_package_additions_changes_and_removals() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("one.pkg");
        fs::write(&package, [1]).unwrap();
        let first = package_stamp(dir.path()).unwrap();
        fs::write(dir.path().join("report.json"), "{}").unwrap();
        assert_eq!(first, package_stamp(dir.path()).unwrap());
        fs::write(&package, [1, 2]).unwrap();
        let changed = package_stamp(dir.path()).unwrap();
        assert_ne!(first, changed);
        fs::write(dir.path().join("two.pkg"), [3]).unwrap();
        assert_ne!(changed, package_stamp(dir.path()).unwrap());
        fs::remove_file(dir.path().join("two.pkg")).unwrap();
        assert_eq!(changed, package_stamp(dir.path()).unwrap());
    }
}
