//! Package tables parsed once per directory listing and copied into each reader.
//!
//! Parsing opens every latest package and reads its entry, block, wide-hash and named-tag tables,
//! one record at a time. On a Shadowkeep installation that is about 2.8 million entries and takes
//! seconds of mostly system time, while previews, cards and jobs each open a reader of their own.
//! The upstream on-disk caches stay off, since they are not keyed by what the files hold.
use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};
use tiger_pkg::{
    GameVersion, PackagePlatform,
    manager::{PackagePath, TagLookupIndex},
};

/// Each entry costs about 20 bytes in every copy, so the least recently used tables go once the
/// kept ones pass about two installations' worth.
const RETAINED_ENTRIES: usize = 6_000_000;

static PARSED: Mutex<Vec<Parsed>> = Mutex::new(Vec::new());
/// One lock per directory being parsed, so readers opened together parse it once.
static PARSING: Mutex<Vec<(PathBuf, Arc<Mutex<()>>)>> = Mutex::new(Vec::new());

pub(super) struct Tables {
    pub package_dir: PathBuf,
    pub package_paths: HashMap<u16, PackagePath>,
    pub lookup: TagLookupIndex,
    pub platform: PackagePlatform,
}

impl Tables {
    fn copy(&self) -> Self {
        let lookup = &self.lookup;
        Self {
            package_dir: self.package_dir.clone(),
            package_paths: self.package_paths.clone(),
            lookup: TagLookupIndex {
                tag32_entries_by_pkg: lookup.tag32_entries_by_pkg.clone(),
                tag64_entries: lookup.tag64_entries.clone(),
                tag32_to_tag64: lookup.tag32_to_tag64.clone(),
                named_tags: lookup.named_tags.clone(),
                redaction_levels: lookup.redaction_levels.clone(),
            },
            platform: self.platform,
        }
    }
}

#[derive(PartialEq)]
struct Key {
    directory: PathBuf,
    version: GameVersion,
    platform: Option<PackagePlatform>,
}

/// Each package file's name, length and write time, which change when a file is added, removed
/// or replaced. The shared index caches use the same freshness key.
type Listing = Vec<(OsString, u64, Option<SystemTime>)>;

struct Parsed {
    key: Key,
    listing: Listing,
    entries: usize,
    tables: Arc<Tables>,
}

pub(super) fn read(
    directory: &Path,
    version: GameVersion,
    platform: Option<PackagePlatform>,
) -> Result<Tables, String> {
    let key = Key {
        directory: directory.to_path_buf(),
        version,
        platform,
    };
    // An unreadable directory is left for the parser to report as it always has.
    let listing = listing(directory).ok();
    if let Some(tables) = listing.as_ref().and_then(|listing| reuse(&key, listing)) {
        return Ok(tables);
    }
    let flight = flight(directory);
    let _parsing = flight.lock().unwrap_or_else(PoisonError::into_inner);
    // Another reader may have parsed this listing while this one waited.
    if let Some(tables) = listing.as_ref().and_then(|listing| reuse(&key, listing)) {
        return Ok(tables);
    }
    let metadata = tiger_pkg::PackageManager::new(directory, version, platform)
        .map_err(|e| format!("{e:#}"))?;
    let tables = Tables {
        package_dir: metadata.package_dir,
        package_paths: metadata.package_paths.into_iter().collect(),
        lookup: metadata.lookup,
        platform: metadata.platform,
    };
    // A file replaced during the parse may not match what was read, so only a listing that held
    // still is kept.
    if let Some(listing) = listing
        && self::listing(directory).ok().as_ref() == Some(&listing)
    {
        keep(key, listing, Arc::new(tables.copy()));
    }
    Ok(tables)
}

fn listing(directory: &Path) -> std::io::Result<Listing> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if Path::new(&name)
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("pkg"))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        files.push((name, metadata.len(), metadata.modified().ok()));
    }
    files.sort();
    Ok(files)
}

fn reuse(key: &Key, listing: &Listing) -> Option<Tables> {
    let tables = {
        let mut parsed = PARSED.lock().unwrap_or_else(PoisonError::into_inner);
        let at = parsed
            .iter()
            .position(|kept| kept.key == *key && kept.listing == *listing)?;
        let kept = parsed.remove(at);
        let tables = Arc::clone(&kept.tables);
        parsed.push(kept);
        tables
    };
    // Copied outside the lock, so readers of other directories don't wait on it.
    Some(tables.copy())
}

fn keep(key: Key, listing: Listing, tables: Arc<Tables>) {
    let entries = tables
        .lookup
        .tag32_entries_by_pkg
        .values()
        .map(Vec::len)
        .sum();
    let mut parsed = PARSED.lock().unwrap_or_else(PoisonError::into_inner);
    // A package view is a new folder each time its content changes, and the old one is removed.
    parsed.retain(|kept| kept.key != key && kept.key.directory.is_dir());
    parsed.push(Parsed {
        key,
        listing,
        entries,
        tables,
    });
    // Most recent last. The tables just parsed always stay.
    let mut retained: usize = parsed.iter().map(|entry| entry.entries).sum();
    while retained > RETAINED_ENTRIES && parsed.len() > 1 {
        retained -= parsed.remove(0).entries;
    }
}

fn flight(directory: &Path) -> Arc<Mutex<()>> {
    let mut parsing = PARSING.lock().unwrap_or_else(PoisonError::into_inner);
    // A lock only this list holds belongs to a parse that has finished.
    parsing.retain(|(path, lock)| path == directory || Arc::strong_count(lock) > 1);
    if let Some((_, lock)) = parsing.iter().find(|(path, _)| path == directory) {
        return Arc::clone(lock);
    }
    let lock = Arc::new(Mutex::new(()));
    parsing.push((directory.to_path_buf(), Arc::clone(&lock)));
    lock
}
