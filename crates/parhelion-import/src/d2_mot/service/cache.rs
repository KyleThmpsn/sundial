use super::*;
use sha2::{Digest, Sha256};

const SCHEMA: u32 = 2;

/// Metadata changes, additions, removals and a different configured build invalidate the cache.
pub fn package_stamp(packages: &Path) -> Result<String> {
    let packages = packages.canonicalize()?;
    let mut files = Vec::new();
    for entry in fs::read_dir(&packages)? {
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
    let modern_stamp = package_stamp(modern)?;
    let native_stamp = package_stamp(native)?;
    let path = output.join("weapon-cache.json");
    if !refresh
        && let Ok(bytes) = fs::read(&path)
        && let Ok(cached) = serde_json::from_slice::<Cached>(&bytes)
        && cached.schema == SCHEMA
        && cached.modern == modern_stamp
        && cached.native == native_stamp
    {
        return Ok(cached.weapons);
    }
    let weapons = scan_with_progress(modern, native, output, &mut progress)?;
    // Never publish a cache for packages changed while they were being read.
    if package_stamp(modern)? == modern_stamp && package_stamp(native)? == native_stamp {
        let cached = Cached {
            schema: SCHEMA,
            modern: modern_stamp,
            native: native_stamp,
            weapons: weapons.clone(),
        };
        fs::write(path, serde_json::to_vec(&cached)?)?;
    }
    Ok(weapons)
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
